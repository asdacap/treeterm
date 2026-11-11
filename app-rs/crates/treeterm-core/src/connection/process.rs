//! Local child processes (ssh, scp) behind a trait, so connection logic runs against a fake.
//!
//! A [`Process`] is the stream of what its child printed, line by line, ending with how it
//! ended. Dropping it kills the child: there is no separate kill call.

use std::fmt;
use std::pin::Pin;
use std::process::Stdio;
use std::task::{Context, Poll};

use futures::stream::BoxStream;
use futures::{Stream, StreamExt};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

/// A program and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl ProcessCommand {
    pub fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
        }
    }

    /// `program arg1 arg2`, as Electron logs it (`$ ssh ${args.join(' ')}`).
    pub fn display(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// How a child exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    Code(i32),
    /// Killed by this signal (Node reports a `null` code).
    Signal(i32),
}

impl ExitStatus {
    pub fn success(self) -> bool {
        self == ExitStatus::Code(0)
    }
}

impl fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExitStatus::Code(code) => write!(f, "{code}"),
            ExitStatus::Signal(signal) => write!(f, "signal {signal}"),
        }
    }
}

impl From<std::process::ExitStatus> for ExitStatus {
    fn from(status: std::process::ExitStatus) -> Self {
        use std::os::unix::process::ExitStatusExt;
        match (status.code(), status.signal()) {
            (Some(code), _) => ExitStatus::Code(code),
            (None, Some(signal)) => ExitStatus::Signal(signal),
            // Neither code nor signal: stopped/continued, which `wait` never reports.
            (None, None) => ExitStatus::Code(-1),
        }
    }
}

/// One thing a child did. `Exited` or `Failed` is always last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessEvent {
    /// A non-empty stdout line, without its newline.
    Stdout(String),
    /// A non-empty stderr line, without its newline.
    Stderr(String),
    Exited(ExitStatus),
    /// Waiting on the child failed (Node's `error` event after spawn).
    Failed(String),
}

/// The child could not be started (missing program, no permission, …).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct SpawnError {
    pub program: String,
    pub message: String,
}

/// Starts child processes. Implemented by [`TokioSpawner`] and by
/// [`super::fake::FakeSpawner`] in tests.
pub trait ProcessSpawner: Send + Sync + 'static {
    /// Start `command` with piped stdin/stdout/stderr. Stdin stays open while the
    /// [`Process`] lives: `ssh … cat` tunnels exit as soon as it closes.
    fn spawn(&self, command: &ProcessCommand) -> Result<Process, SpawnError>;
}

/// A running child. Dropping it kills the child.
#[must_use = "dropping a Process kills it"]
pub struct Process {
    events: BoxStream<'static, ProcessEvent>,
}

impl Process {
    /// A process from its event stream. Dropping the stream must kill the child.
    pub fn from_events(events: impl Stream<Item = ProcessEvent> + Send + 'static) -> Self {
        Self {
            events: events.boxed(),
        }
    }
}

impl Stream for Process {
    type Item = ProcessEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<ProcessEvent>> {
        self.events.poll_next_unpin(cx)
    }
}

/// How a process run to completion ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessEnd {
    Exited(ExitStatus),
    Failed(String),
}

/// Everything a finished process printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    /// Stdout lines joined with `\n`, each terminated.
    pub stdout: String,
    /// Stderr lines joined with `\n`.
    pub stderr: String,
    pub end: ProcessEnd,
}

/// Drive `process` to its end, handing each line to `on_line` as it arrives.
pub(crate) async fn run_to_end(
    mut process: Process,
    mut on_line: impl FnMut(&ProcessEvent),
) -> Finished {
    let mut stdout = String::new();
    let mut stderr: Vec<String> = Vec::new();
    while let Some(event) = process.next().await {
        on_line(&event);
        match event {
            ProcessEvent::Stdout(line) => {
                stdout.push_str(&line);
                stdout.push('\n');
            }
            ProcessEvent::Stderr(line) => stderr.push(line),
            ProcessEvent::Exited(status) => {
                return Finished {
                    stdout,
                    stderr: stderr.join("\n"),
                    end: ProcessEnd::Exited(status),
                };
            }
            ProcessEvent::Failed(message) => {
                return Finished {
                    stdout,
                    stderr: stderr.join("\n"),
                    end: ProcessEnd::Failed(message),
                };
            }
        }
    }
    Finished {
        stdout,
        stderr: stderr.join("\n"),
        end: ProcessEnd::Failed("process output ended without an exit status".to_owned()),
    }
}

/// Real children through `tokio::process`. Must be called on a tokio runtime.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioSpawner;

impl ProcessSpawner for TokioSpawner {
    fn spawn(&self, command: &ProcessCommand) -> Result<Process, SpawnError> {
        let mut child = tokio::process::Command::new(&command.program)
            .args(&command.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| SpawnError {
                program: command.program.clone(),
                message: error.to_string(),
            })?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");
        let output = futures::stream::select(
            lines(stdout).map(ProcessEvent::Stdout),
            lines(stderr).map(ProcessEvent::Stderr),
        );
        // The wait future owns the child (and its stdin), so dropping the stream kills it.
        let exit = futures::stream::once(async move {
            let status = child.wait().await;
            drop(stdin);
            match status {
                Ok(status) => ProcessEvent::Exited(status.into()),
                Err(error) => ProcessEvent::Failed(error.to_string()),
            }
        });
        Ok(Process::from_events(output.chain(exit)))
    }
}

/// Non-empty lines of `reader` (lossy UTF-8), ending at EOF or on a read error.
fn lines(reader: impl AsyncRead + Unpin + Send + 'static) -> impl Stream<Item = String> + Send {
    futures::stream::unfold(
        BufReader::new(reader).split(b'\n'),
        |mut segments| async move {
            loop {
                match segments.next_segment().await {
                    Ok(Some(segment)) if segment.is_empty() => continue,
                    Ok(Some(segment)) => {
                        return Some((String::from_utf8_lossy(&segment).into_owned(), segments));
                    }
                    Ok(None) => return None,
                    Err(error) => {
                        tracing::warn!(%error, "reading child output failed");
                        return None;
                    }
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn sh(script: &str) -> ProcessCommand {
        ProcessCommand::new("sh", vec!["-c".to_owned(), script.to_owned()])
    }

    #[tokio::test]
    async fn streams_lines_then_the_exit_status() {
        let process = TokioSpawner
            .spawn(&sh("printf 'one\\n\\ntwo\\n'; echo oops >&2; exit 3"))
            .unwrap();
        let events: Vec<ProcessEvent> = process.collect().await;
        let (stdout, rest): (Vec<_>, Vec<_>) = events
            .iter()
            .partition(|event| matches!(event, ProcessEvent::Stdout(_)));
        assert_eq!(
            stdout,
            vec![
                &ProcessEvent::Stdout("one".to_owned()),
                &ProcessEvent::Stdout("two".to_owned())
            ]
        );
        assert_eq!(
            rest,
            vec![
                &ProcessEvent::Stderr("oops".to_owned()),
                &ProcessEvent::Exited(ExitStatus::Code(3))
            ]
        );
    }

    #[tokio::test]
    async fn stdin_stays_open_while_the_process_lives() {
        // `cat` exits at stdin EOF; it must still be running.
        let mut process = TokioSpawner.spawn(&sh("cat")).unwrap();
        let next = tokio::time::timeout(Duration::from_millis(300), process.next()).await;
        assert!(next.is_err(), "cat ended early: {next:?}");
    }

    #[tokio::test]
    async fn dropping_the_process_kills_the_child() {
        let pid_file = tempfile::NamedTempFile::new().unwrap();
        let script = format!(
            "echo $$ > {}; echo ready; exec sleep 30",
            pid_file.path().display()
        );
        let mut process = TokioSpawner.spawn(&sh(&script)).unwrap();
        assert_eq!(
            process.next().await,
            Some(ProcessEvent::Stdout("ready".to_owned()))
        );
        let pid: libc::pid_t = std::fs::read_to_string(pid_file.path())
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        drop(process);
        let mut alive = true;
        for _ in 0..50 {
            // SAFETY: signal 0 only checks that the pid exists.
            alive = unsafe { libc::kill(pid, 0) } == 0;
            if !alive {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(!alive, "child {pid} survived its Process");
    }

    #[test]
    fn a_missing_program_is_a_spawn_error() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = runtime.enter();
        let error = TokioSpawner
            .spawn(&ProcessCommand::new(
                "/nonexistent/treeterm-no-such-program",
                vec![],
            ))
            .err()
            .unwrap();
        assert_eq!(error.program, "/nonexistent/treeterm-no-such-program");
        assert!(!error.message.is_empty());
    }

    #[test]
    fn signals_display_like_exit_codes_with_a_prefix() {
        assert_eq!(ExitStatus::Code(255).to_string(), "255");
        assert_eq!(ExitStatus::Signal(9).to_string(), "signal 9");
        assert!(ExitStatus::Code(0).success());
        assert!(!ExitStatus::Signal(15).success());
    }
}
