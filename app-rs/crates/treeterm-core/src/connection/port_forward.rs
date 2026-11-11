//! One `ssh -L port:host:port` forward as a sans-IO state machine (port of Electron's
//! `portForward.ts`).
//!
//! The forward never touches a process itself: [`PortForward::start`] and friends return
//! [`ForwardEffect`]s for the owner to carry out, and the owner feeds the process's events
//! back with the generation they belong to. A forward is Active once its ssh has stayed up
//! for [`STABILIZATION`]; it reports Error when ssh ends.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::settings::{IdentityFile, PortForwardSpec, SshConnectionConfig};

use super::output::OutputLog;
use super::process::{ExitStatus, ProcessCommand, ProcessEvent};
use super::types::{PortForwardConfig, PortForwardInfo, PortForwardStatus};

/// How long ssh must stay up before the forward counts as active.
pub const STABILIZATION: Duration = Duration::from_secs(2);

/// Why a forward stopped working. Messages match Electron's.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PortForwardError {
    #[error("Port forward process exited (code {0})")]
    Exited(ExitStatus),
    #[error("Port forward error: {0}")]
    Failed(String),
}

/// What the owner must do with the forward's ssh process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardEffect {
    /// Start `command`, replacing (killing) any previous process; tag its events with
    /// `generation`.
    Spawn {
        generation: u64,
        command: ProcessCommand,
    },
    /// Kill the current process.
    Kill,
}

/// Something watchers of the forward should hear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardEvent {
    Status(PortForwardInfo),
    Output(String),
}

/// `ssh -L <local>:<host>:<port> … user@host cat`.
pub fn port_forward_args(ssh: &SshConnectionConfig, spec: &PortForwardSpec) -> Vec<String> {
    let mut args = vec![
        "-L".to_owned(),
        format!(
            "{}:{}:{}",
            spec.local_port, spec.remote_host, spec.remote_port
        ),
    ];
    args.extend(
        [
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "BatchMode=yes",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ServerAliveInterval=10",
            "-o",
            "ServerAliveCountMax=3",
            "-p",
        ]
        .map(str::to_owned),
    );
    args.push(ssh.port.to_string());
    match &ssh.identity_file {
        IdentityFile::Path(path) => args.extend(["-i".to_owned(), path.clone()]),
        IdentityFile::SshDefault => {}
    }
    args.push(format!("{}@{}", ssh.user, ssh.host));
    args.push("cat".to_owned());
    args
}

/// Where the forward's process is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Run {
    /// Created, not started.
    Idle,
    /// Running for less than [`STABILIZATION`].
    Starting {
        active_at: Instant,
    },
    Running,
    /// The process ended on its own.
    Failed(PortForwardError),
    /// Stopped by its owner.
    Stopped,
}

pub struct PortForward {
    config: PortForwardConfig,
    command: ProcessCommand,
    run: Run,
    /// Generation of the current process. Events of older ones are ignored: a killed
    /// process's late exit must not clobber its replacement.
    generation: u64,
    output: OutputLog,
    events: VecDeque<ForwardEvent>,
}

impl PortForward {
    pub fn new(ssh: &SshConnectionConfig, config: PortForwardConfig) -> Self {
        let command = ProcessCommand::new("ssh", port_forward_args(ssh, &config.spec));
        Self {
            config,
            command,
            run: Run::Idle,
            generation: 0,
            output: OutputLog::ssh(),
            events: VecDeque::new(),
        }
    }

    pub fn config(&self) -> &PortForwardConfig {
        &self.config
    }

    pub fn status(&self) -> PortForwardStatus {
        match &self.run {
            Run::Idle | Run::Starting { .. } => PortForwardStatus::Connecting,
            Run::Running => PortForwardStatus::Active,
            Run::Failed(error) => PortForwardStatus::Error(error.clone()),
            Run::Stopped => PortForwardStatus::Stopped,
        }
    }

    pub fn info(&self) -> PortForwardInfo {
        PortForwardInfo {
            config: self.config.clone(),
            status: self.status(),
        }
    }

    /// A copy of the output scrollback.
    pub fn output(&self) -> Vec<String> {
        self.output.snapshot()
    }

    /// Spawn the forward's ssh.
    pub fn start(&mut self, now: Instant) -> ForwardEffect {
        let spec = &self.config.spec;
        let line = format!(
            "[portfwd] Starting port forward: localhost:{} -> {}:{}",
            spec.local_port, spec.remote_host, spec.remote_port
        );
        self.append(line);
        self.append(format!("[portfwd] $ {}", self.command.display()));
        self.generation += 1;
        self.run = Run::Starting {
            active_at: now + STABILIZATION,
        };
        ForwardEffect::Spawn {
            generation: self.generation,
            command: self.command.clone(),
        }
    }

    /// Kill the current ssh (if any) and spawn a fresh one. Used after the connection
    /// reconnects: the forward's own ssh session died with the network, or is about to.
    pub fn restart(&mut self, now: Instant) -> Vec<ForwardEffect> {
        let mut effects = self.kill().into_iter().collect::<Vec<_>>();
        self.append("[portfwd] Restarting port forward".to_owned());
        self.run = Run::Idle;
        self.emit_status();
        effects.push(self.start(now));
        effects
    }

    /// Kill the ssh process for good.
    pub fn stop(&mut self) -> Option<ForwardEffect> {
        let kill = self.kill();
        self.run = Run::Stopped;
        self.emit_status();
        kill
    }

    /// An event of the process spawned as `generation`.
    pub fn on_process_event(&mut self, generation: u64, event: ProcessEvent) {
        if !self.is_current(generation) {
            return;
        }
        match event {
            ProcessEvent::Stdout(line) => self.append(format!("[portfwd] {line}")),
            ProcessEvent::Stderr(line) => self.append(format!("[portfwd:err] {line}")),
            ProcessEvent::Exited(exit) => self.fail(PortForwardError::Exited(exit)),
            ProcessEvent::Failed(message) => self.fail(PortForwardError::Failed(message)),
        }
    }

    /// The process for `generation` could not be spawned.
    pub fn on_spawn_failed(&mut self, generation: u64, message: String) {
        if self.is_current(generation) {
            self.fail(PortForwardError::Failed(message));
        }
    }

    /// When [`Self::handle_timeout`] has something to do.
    pub fn poll_timeout(&self) -> Option<Instant> {
        match self.run {
            Run::Starting { active_at } => Some(active_at),
            Run::Idle | Run::Running | Run::Failed(_) | Run::Stopped => None,
        }
    }

    pub fn handle_timeout(&mut self, now: Instant) {
        if let Run::Starting { active_at } = self.run
            && now >= active_at
        {
            self.run = Run::Running;
            self.emit_status();
        }
    }

    pub fn poll_event(&mut self) -> Option<ForwardEvent> {
        self.events.pop_front()
    }

    fn is_current(&self, generation: u64) -> bool {
        generation == self.generation && matches!(self.run, Run::Starting { .. } | Run::Running)
    }

    /// Retire the current process (if one runs): its late events are ignored from now on.
    fn kill(&mut self) -> Option<ForwardEffect> {
        match self.run {
            Run::Starting { .. } | Run::Running => {
                self.generation += 1;
                Some(ForwardEffect::Kill)
            }
            Run::Idle | Run::Failed(_) | Run::Stopped => None,
        }
    }

    fn fail(&mut self, error: PortForwardError) {
        self.append(format!("[portfwd] {error}"));
        self.run = Run::Failed(error);
        self.emit_status();
    }

    fn append(&mut self, line: String) {
        tracing::debug!(forward = %self.config.id, "{line}");
        self.output.push(line.clone());
        self.events.push_back(ForwardEvent::Output(line));
    }

    fn emit_status(&mut self) {
        let info = self.info();
        self.events.push_back(ForwardEvent::Status(info));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::types::{ConnectionId, Persistence, PortForwardId};
    use crate::settings::{ConnectionLabel, Extra};

    fn ssh_config() -> SshConnectionConfig {
        SshConnectionConfig {
            id: "remote-1".to_owned(),
            host: "example.com".to_owned(),
            user: "admin".to_owned(),
            port: 22,
            identity_file: IdentityFile::SshDefault,
            label: ConnectionLabel::UserAtHost,
            port_forwards: Vec::new(),
            extra: Extra::new(),
        }
    }

    fn pf_config() -> PortForwardConfig {
        PortForwardConfig {
            id: PortForwardId::new("pf-1"),
            connection_id: ConnectionId::new("remote-1"),
            spec: PortForwardSpec {
                local_port: 8080,
                remote_host: "localhost".to_owned(),
                remote_port: 3000,
                extra: Extra::new(),
            },
            persistence: Persistence::Saved,
        }
    }

    fn statuses(pf: &mut PortForward) -> Vec<&'static str> {
        let mut statuses = Vec::new();
        while let Some(event) = pf.poll_event() {
            if let ForwardEvent::Status(info) = event {
                statuses.push(info.status.as_str());
            }
        }
        statuses
    }

    fn outputs(pf: &mut PortForward) -> Vec<String> {
        let mut lines = Vec::new();
        while let Some(event) = pf.poll_event() {
            if let ForwardEvent::Output(line) = event {
                lines.push(line);
            }
        }
        lines
    }

    fn started(now: Instant) -> (PortForward, u64) {
        let mut pf = PortForward::new(&ssh_config(), pf_config());
        let ForwardEffect::Spawn { generation, .. } = pf.start(now) else {
            panic!("start spawns");
        };
        (pf, generation)
    }

    mod constructor_and_to_info {
        use super::*;

        #[test]
        fn starts_with_connecting_status() {
            let pf = PortForward::new(&ssh_config(), pf_config());
            assert_eq!(
                pf.info(),
                PortForwardInfo {
                    config: pf_config(),
                    status: PortForwardStatus::Connecting
                }
            );
        }

        #[test]
        fn exposes_status_getter() {
            let pf = PortForward::new(&ssh_config(), pf_config());
            assert_eq!(pf.status().as_str(), "connecting");
        }
    }

    mod start {
        use super::*;

        #[test]
        fn spawns_ssh_with_correct_args() {
            let mut pf = PortForward::new(&ssh_config(), pf_config());
            let ForwardEffect::Spawn { command, .. } = pf.start(Instant::now()) else {
                panic!("start spawns");
            };
            assert_eq!(command.program, "ssh");
            let args = command.args.join(" ");
            assert!(args.starts_with("-L 8080:localhost:3000 "), "{args}");
            assert!(args.contains("-p 22 admin@example.com cat"), "{args}");
        }

        #[test]
        fn includes_identity_file_when_configured() {
            let ssh = SshConnectionConfig {
                identity_file: IdentityFile::Path("/home/admin/.ssh/id_rsa".to_owned()),
                ..ssh_config()
            };
            let mut pf = PortForward::new(&ssh, pf_config());
            let ForwardEffect::Spawn { command, .. } = pf.start(Instant::now()) else {
                panic!("start spawns");
            };
            let i = command.args.iter().position(|arg| arg == "-i").unwrap();
            assert_eq!(command.args[i + 1], "/home/admin/.ssh/id_rsa");
        }

        #[test]
        fn becomes_active_after_stabilization_timeout() {
            let now = Instant::now();
            let (mut pf, _) = started(now);
            assert_eq!(pf.status(), PortForwardStatus::Connecting);
            assert_eq!(pf.poll_timeout(), Some(now + STABILIZATION));
            pf.handle_timeout(now + Duration::from_millis(1999));
            assert_eq!(pf.status(), PortForwardStatus::Connecting);
            pf.handle_timeout(now + STABILIZATION);
            assert_eq!(pf.status(), PortForwardStatus::Active);
            assert_eq!(pf.poll_timeout(), None);
        }

        #[test]
        fn collects_stdout_output() {
            let (mut pf, generation) = started(Instant::now());
            pf.on_process_event(generation, ProcessEvent::Stdout("tunnel ready".to_owned()));
            assert!(pf.output().contains(&"[portfwd] tunnel ready".to_owned()));
        }

        #[test]
        fn collects_stderr_output() {
            let (mut pf, generation) = started(Instant::now());
            pf.on_process_event(
                generation,
                ProcessEvent::Stderr("warning: something".to_owned()),
            );
            assert!(
                pf.output()
                    .contains(&"[portfwd:err] warning: something".to_owned())
            );
        }

        #[test]
        fn sets_error_status_on_process_close_with_non_zero_code() {
            let (mut pf, generation) = started(Instant::now());
            pf.on_process_event(generation, ProcessEvent::Exited(ExitStatus::Code(1)));
            assert_eq!(
                pf.status(),
                PortForwardStatus::Error(PortForwardError::Exited(ExitStatus::Code(1)))
            );
            assert_eq!(
                PortForwardError::Exited(ExitStatus::Code(1)).to_string(),
                "Port forward process exited (code 1)"
            );
        }

        #[test]
        fn sets_error_status_on_process_error() {
            let (mut pf, generation) = started(Instant::now());
            pf.on_process_event(generation, ProcessEvent::Failed("ENOENT".to_owned()));
            let PortForwardStatus::Error(error) = pf.status() else {
                panic!("expected an error");
            };
            assert!(error.to_string().contains("ENOENT"));
        }

        #[test]
        fn sets_error_status_when_ssh_cannot_be_spawned() {
            let (mut pf, generation) = started(Instant::now());
            pf.on_spawn_failed(generation, "spawn ssh ENOENT".to_owned());
            assert_eq!(
                pf.status(),
                PortForwardStatus::Error(PortForwardError::Failed("spawn ssh ENOENT".to_owned()))
            );
            assert_eq!(
                pf.output().last().unwrap(),
                "[portfwd] Port forward error: spawn ssh ENOENT"
            );
        }

        #[test]
        fn does_not_become_active_if_process_closes_before_stabilization() {
            let now = Instant::now();
            let (mut pf, generation) = started(now);
            pf.on_process_event(generation, ProcessEvent::Exited(ExitStatus::Code(255)));
            pf.handle_timeout(now + STABILIZATION);
            assert_eq!(pf.status().as_str(), "error");
        }
    }

    mod stop {
        use super::*;

        #[test]
        fn kills_the_process_and_sets_stopped_status() {
            let now = Instant::now();
            let (mut pf, _) = started(now);
            pf.handle_timeout(now + STABILIZATION);
            assert_eq!(pf.stop(), Some(ForwardEffect::Kill));
            assert_eq!(pf.status(), PortForwardStatus::Stopped);
        }

        #[test]
        fn does_not_transition_to_error_after_stop_on_process_close() {
            let (mut pf, generation) = started(Instant::now());
            assert_eq!(pf.stop(), Some(ForwardEffect::Kill));
            // Process close fires after kill - should not override stopped
            pf.on_process_event(generation, ProcessEvent::Exited(ExitStatus::Code(0)));
            pf.on_process_event(generation, ProcessEvent::Failed("late".to_owned()));
            assert_eq!(pf.status(), PortForwardStatus::Stopped);
        }
    }

    mod restart {
        use super::*;

        #[test]
        fn kills_the_old_process_spawns_a_new_one_and_becomes_active_again() {
            let now = Instant::now();
            let (mut pf, first) = started(now);
            pf.handle_timeout(now + STABILIZATION);
            assert_eq!(pf.status(), PortForwardStatus::Active);

            let later = now + Duration::from_secs(10);
            let effects = pf.restart(later);

            assert_eq!(effects.len(), 2);
            assert_eq!(effects[0], ForwardEffect::Kill);
            let ForwardEffect::Spawn { generation, .. } = &effects[1] else {
                panic!("restart spawns");
            };
            assert!(*generation > first);
            assert_eq!(pf.status(), PortForwardStatus::Connecting);
            pf.handle_timeout(later + STABILIZATION);
            assert_eq!(pf.status(), PortForwardStatus::Active);
        }

        #[test]
        fn restarts_from_error_state_without_killing_anything_and_clears_the_error() {
            let (mut pf, generation) = started(Instant::now());
            pf.on_process_event(generation, ProcessEvent::Exited(ExitStatus::Code(255)));
            assert_eq!(
                pf.status(),
                PortForwardStatus::Error(PortForwardError::Exited(ExitStatus::Code(255)))
            );
            assert_eq!(
                PortForwardError::Exited(ExitStatus::Code(255)).to_string(),
                "Port forward process exited (code 255)"
            );

            let effects = pf.restart(Instant::now());

            assert!(matches!(effects.as_slice(), [ForwardEffect::Spawn { .. }]));
            assert_eq!(
                pf.info(),
                PortForwardInfo {
                    config: pf_config(),
                    status: PortForwardStatus::Connecting
                }
            );
        }

        #[test]
        fn ignores_a_late_close_or_error_from_the_replaced_process() {
            let now = Instant::now();
            let (mut pf, old) = started(now);
            let effects = pf.restart(now);
            let Some(ForwardEffect::Spawn {
                generation: new, ..
            }) = effects.last().cloned()
            else {
                panic!("restart spawns");
            };

            pf.on_process_event(old, ProcessEvent::Exited(ExitStatus::Code(255)));
            pf.on_process_event(old, ProcessEvent::Failed("late".to_owned()));
            assert_eq!(pf.status(), PortForwardStatus::Connecting);

            pf.handle_timeout(now + STABILIZATION);
            assert_eq!(pf.status(), PortForwardStatus::Active);

            // The new process is still the live one: its close is honoured.
            pf.on_process_event(new, ProcessEvent::Exited(ExitStatus::Code(1)));
            assert_eq!(pf.status().as_str(), "error");
        }

        #[test]
        fn notifies_status_listeners_on_restart() {
            let now = Instant::now();
            let (mut pf, _) = started(now);
            statuses(&mut pf);

            let _effects = pf.restart(now);
            pf.handle_timeout(now + STABILIZATION);

            assert_eq!(statuses(&mut pf), vec!["connecting", "active"]);
        }
    }

    mod output_buffer {
        use super::*;

        #[test]
        fn trims_buffer_when_exceeding_1000_lines() {
            let (mut pf, generation) = started(Instant::now()); // adds 2 lines
            for i in 0..999 {
                pf.on_process_event(generation, ProcessEvent::Stdout(format!("line-{i}")));
            }
            // Now at 1001 lines, trim fires: slices to last 500
            assert_eq!(pf.output().len(), 500);
        }

        #[test]
        fn returns_a_copy_of_the_buffer() {
            let (pf, _) = started(Instant::now());
            let mut a = pf.output();
            a.push("extra".to_owned());
            assert_ne!(a, pf.output());
        }
    }

    mod listeners {
        use super::*;

        #[test]
        fn notifies_output_listeners() {
            let (mut pf, generation) = started(Instant::now());
            outputs(&mut pf);
            pf.on_process_event(generation, ProcessEvent::Stdout("hello".to_owned()));
            assert_eq!(outputs(&mut pf), vec!["[portfwd] hello"]);
        }

        #[test]
        fn reports_start_lines_as_output() {
            let (mut pf, _) = started(Instant::now());
            let lines = outputs(&mut pf);
            assert_eq!(
                lines[0],
                "[portfwd] Starting port forward: localhost:8080 -> localhost:3000"
            );
            assert!(lines[1].starts_with("[portfwd] $ ssh -L 8080:localhost:3000 "));
        }

        #[test]
        fn notifies_status_listeners_on_status_change() {
            let now = Instant::now();
            let (mut pf, _) = started(now);
            pf.handle_timeout(now + STABILIZATION); // connecting -> active
            assert_eq!(statuses(&mut pf), vec!["active"]);
        }
    }
}
