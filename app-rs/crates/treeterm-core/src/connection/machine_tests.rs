//! The reconnect policy, driven by made-up instants.

use super::*;
use crate::connection::process::ExitStatus;
use crate::connection::types::{Persistence, PortForwardStatus, StatusKind};
use crate::settings::{ConnectionLabel, Extra, IdentityFile, PortForwardSpec};

type Machine = ConnectionMachine<&'static str>;

fn ssh_config() -> SshConnectionConfig {
    SshConnectionConfig {
        id: "remote-1".to_owned(),
        host: "example.com".to_owned(),
        user: "test".to_owned(),
        port: 22,
        identity_file: IdentityFile::SshDefault,
        label: ConnectionLabel::UserAtHost,
        port_forwards: Vec::new(),
        extra: Extra::new(),
    }
}

fn local() -> Machine {
    ConnectionMachine::start(
        ConnectionId::new("local-1"),
        ConnectionTarget::Local,
        OutdatedDaemon::Report,
    )
}

fn remote(outdated: OutdatedDaemon) -> Machine {
    ConnectionMachine::start(
        ConnectionId::new("remote-1"),
        ConnectionTarget::Remote(Arc::new(ssh_config())),
        outdated,
    )
}

fn effects(machine: &mut Machine) -> Vec<Effect> {
    std::iter::from_fn(|| machine.poll_effect()).collect()
}

fn statuses(machine: &mut Machine) -> Vec<StatusKind> {
    std::iter::from_fn(|| machine.poll_event())
        .filter_map(|event| match event {
            MachineEvent::Status(info) => Some(info.status.kind()),
            MachineEvent::ForwardStatus(_)
            | MachineEvent::ForwardOutput { .. }
            | MachineEvent::ForwardRemoved(_) => None,
        })
        .collect()
}

/// The attempt the machine asked for, discarding other effects.
fn started_attempt(machine: &mut Machine) -> AttemptPlan {
    effects(machine)
        .into_iter()
        .find_map(|effect| match effect {
            Effect::StartAttempt(plan) => Some(plan),
            _ => None,
        })
        .expect("an attempt was started")
}

fn attempt_of(machine: &Machine) -> u32 {
    match machine.status() {
        ConnectionStatus::Reconnecting { attempt, .. } => attempt,
        other => panic!("not reconnecting: {:?}", other.kind()),
    }
}

/// A local connection, connected at `t0`, effects drained.
fn connected(t0: Instant) -> Machine {
    let mut machine = local();
    let plan = started_attempt(&mut machine);
    assert_eq!(
        machine.attempt_finished(plan.id, Ok("daemon-1"), t0),
        AttemptVerdict::Applied
    );
    effects(&mut machine);
    while machine.poll_event().is_some() {}
    machine
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

fn pf_config(id: &str, port: u16) -> PortForwardConfig {
    PortForwardConfig {
        id: PortForwardId::new(id),
        connection_id: ConnectionId::new("remote-1"),
        spec: PortForwardSpec {
            local_port: port,
            remote_host: "localhost".to_owned(),
            remote_port: 3000,
            extra: Extra::new(),
        },
        persistence: Persistence::Saved,
    }
}

#[test]
fn backoff_doubles_from_one_second_up_to_thirty() {
    let delays: Vec<u64> = (0..8).map(|n| reconnect_delay(n).as_secs()).collect();
    assert_eq!(delays, vec![1, 2, 4, 8, 16, 30, 30, 30]);
    assert_eq!(reconnect_delay(u32::MAX), RECONNECT_MAX_DELAY);
}

#[test]
fn first_connect_starts_an_attempt_and_reports_connecting() {
    let mut machine = local();
    assert!(matches!(
        machine.status(),
        ConnectionStatus::Connecting {
            phase: ConnectPhase::Daemon
        }
    ));
    assert_eq!(started_attempt(&mut machine).target, AttemptTarget::Local);
    assert_eq!(statuses(&mut machine), vec![StatusKind::Connecting]);

    let mut machine = remote(OutdatedDaemon::Refresh);
    assert!(matches!(
        machine.status(),
        ConnectionStatus::Connecting {
            phase: ConnectPhase::Bootstrap
        }
    ));
    assert_eq!(
        started_attempt(&mut machine).target,
        AttemptTarget::Remote {
            config: Arc::new(ssh_config()),
            outdated: OutdatedDaemon::Refresh
        }
    );
}

#[test]
fn connect_phases_are_reported_for_the_attempt_in_flight_only() {
    let mut machine = remote(OutdatedDaemon::Report);
    let plan = started_attempt(&mut machine);
    statuses(&mut machine);

    machine.connect_phase(plan.id, ConnectPhase::Tunnel);
    machine.connect_phase(AttemptId(999), ConnectPhase::Daemon);
    machine.connect_phase(plan.id, ConnectPhase::Tunnel);

    assert!(matches!(
        machine.status(),
        ConnectionStatus::Connecting {
            phase: ConnectPhase::Tunnel
        }
    ));
    assert_eq!(statuses(&mut machine), vec![StatusKind::Connecting]);
}

#[test]
fn a_successful_first_connect_arms_the_heartbeat() {
    let t0 = Instant::now();
    let mut machine = local();
    let plan = started_attempt(&mut machine);
    assert_eq!(
        machine.attempt_finished(plan.id, Ok("daemon-1"), t0),
        AttemptVerdict::Applied
    );
    assert!(matches!(
        machine.status(),
        ConnectionStatus::Connected { daemon: "daemon-1" }
    ));
    assert_eq!(effects(&mut machine), vec![Effect::WatchHeartbeat]);
    assert_eq!(machine.poll_timeout(), Some(t0 + HEARTBEAT_TIMEOUT));
}

#[test]
fn a_failed_first_connect_shows_the_error_and_does_not_retry() {
    let t0 = Instant::now();
    let mut machine = local();
    let plan = started_attempt(&mut machine);
    assert_eq!(
        machine.attempt_finished(plan.id, Err(ConnectionError::HeartbeatLost), t0),
        AttemptVerdict::Applied
    );
    assert_eq!(machine.status().kind(), StatusKind::Error);
    assert_eq!(machine.poll_timeout(), None);
    assert!(effects(&mut machine).is_empty());
}

#[test]
fn heartbeats_push_the_deadline_back() {
    let t0 = Instant::now();
    let mut machine = connected(t0);
    machine.heartbeat(t0 + secs(40));
    machine.handle_timeout(t0 + secs(50));
    assert_eq!(machine.status().kind(), StatusKind::Connected);
    assert_eq!(machine.poll_timeout(), Some(t0 + secs(90)));
}

#[test]
fn fifty_seconds_of_silence_starts_reconnecting() {
    let t0 = Instant::now();
    let mut machine = connected(t0);
    machine.handle_timeout(t0 + secs(49));
    assert_eq!(machine.status().kind(), StatusKind::Connected);

    machine.handle_timeout(t0 + secs(50));

    let ConnectionStatus::Reconnecting { attempt, error } = machine.status() else {
        panic!("expected reconnecting");
    };
    assert_eq!(attempt, 0);
    assert_eq!(
        error.to_string(),
        "Connection lost (no heartbeat from daemon)"
    );
    assert_eq!(effects(&mut machine), vec![Effect::DropTransport]);
    assert_eq!(machine.poll_timeout(), Some(t0 + secs(51)));
}

#[test]
fn a_failed_session_watch_starts_reconnecting() {
    let t0 = Instant::now();
    let mut machine = connected(t0);
    machine.heartbeat_failed("transport error".to_owned(), t0);
    assert_eq!(
        machine.status().error().unwrap().to_string(),
        "gRPC connection lost: transport error"
    );
    assert_eq!(effects(&mut machine), vec![Effect::DropTransport]);
}

#[test]
fn a_closed_tunnel_starts_reconnecting() {
    let t0 = Instant::now();
    let mut machine = connected(t0);
    machine.tunnel_closed(
        TunnelEnd::Closed {
            exit: ExitStatus::Code(255),
            stderr: vec!["Broken pipe".to_owned()],
        },
        t0,
    );
    assert_eq!(
        machine.status().error().unwrap().to_string(),
        "SSH tunnel closed (exit 255): Broken pipe"
    );
}

#[test]
fn losses_while_not_connected_are_ignored() {
    let t0 = Instant::now();
    let mut machine = local();
    machine.heartbeat_failed("late".to_owned(), t0);
    machine.tunnel_closed(TunnelEnd::Failed("late".to_owned()), t0);
    machine.heartbeat(t0);
    assert_eq!(machine.status().kind(), StatusKind::Connecting);
}

#[test]
fn attempts_back_off_without_limit() {
    let t0 = Instant::now();
    let mut machine = connected(t0);
    machine.heartbeat_failed("gone".to_owned(), t0);
    effects(&mut machine);

    let mut now = t0;
    let mut waits = Vec::new();
    for n in 1..=20u32 {
        let due = machine.poll_timeout().unwrap();
        waits.push((due - now).as_secs());
        machine.handle_timeout(due - Duration::from_millis(1));
        assert!(
            effects(&mut machine).is_empty(),
            "attempt {n} started early"
        );
        machine.handle_timeout(due);
        let plan = started_attempt(&mut machine);
        assert_eq!(attempt_of(&machine), n);
        assert_eq!(
            machine.poll_timeout(),
            None,
            "no timer while an attempt runs"
        );
        now = due + secs(3);
        assert_eq!(
            machine.attempt_finished(plan.id, Err(ConnectionError::HeartbeatLost), now),
            AttemptVerdict::Applied
        );
        assert_eq!(attempt_of(&machine), n);
    }
    assert_eq!(waits[..8], [1, 2, 4, 8, 16, 30, 30, 30]);
    assert!(waits[8..].iter().all(|wait| *wait == 30));
}

#[test]
fn first_attempt_after_one_second_then_two_four_eight() {
    let t0 = Instant::now();
    let mut machine = connected(t0);
    machine.handle_timeout(t0 + HEARTBEAT_TIMEOUT);
    let lost = t0 + HEARTBEAT_TIMEOUT;
    assert_eq!(machine.poll_timeout(), Some(lost + secs(1)));
    effects(&mut machine);

    let mut now = lost + secs(1);
    for delay in [2, 4, 8] {
        machine.handle_timeout(now);
        let plan = started_attempt(&mut machine);
        assert_eq!(
            machine.attempt_finished(plan.id, Err(ConnectionError::HeartbeatLost), now),
            AttemptVerdict::Applied
        );
        assert_eq!(machine.poll_timeout(), Some(now + secs(delay)));
        now += secs(delay);
    }
}

#[test]
fn a_successful_reconnect_resets_attempts_and_rearms_the_heartbeat() {
    let t0 = Instant::now();
    let mut machine = connected(t0);
    machine.heartbeat_failed("gone".to_owned(), t0);
    machine.handle_timeout(t0 + secs(1));
    let plan = started_attempt(&mut machine);
    statuses(&mut machine);

    assert_eq!(
        machine.attempt_finished(plan.id, Ok("daemon-2"), t0 + secs(2)),
        AttemptVerdict::Applied
    );

    assert!(matches!(
        machine.status(),
        ConnectionStatus::Connected { daemon: "daemon-2" }
    ));
    assert_eq!(effects(&mut machine), vec![Effect::WatchHeartbeat]);
    assert_eq!(statuses(&mut machine), vec![StatusKind::Connected]);
    assert_eq!(
        machine.poll_timeout(),
        Some(t0 + secs(2) + HEARTBEAT_TIMEOUT)
    );

    // The next loss starts counting from zero again.
    machine.heartbeat_failed("gone again".to_owned(), t0 + secs(3));
    assert_eq!(attempt_of(&machine), 0);
    assert_eq!(machine.poll_timeout(), Some(t0 + secs(4)));
}

#[test]
fn reconnect_now_skips_the_backoff() {
    let t0 = Instant::now();
    let mut machine = connected(t0);
    machine.heartbeat_failed("gone".to_owned(), t0);
    effects(&mut machine);

    machine.reconnect_now();

    started_attempt(&mut machine);
    assert_eq!(attempt_of(&machine), 1);
}

#[test]
fn reconnect_now_during_an_attempt_is_a_no_op() {
    let t0 = Instant::now();
    let mut machine = connected(t0);
    machine.heartbeat_failed("gone".to_owned(), t0);
    machine.handle_timeout(t0 + secs(1));
    let plan = started_attempt(&mut machine);

    machine.reconnect_now();

    assert!(effects(&mut machine).is_empty());
    assert_eq!(attempt_of(&machine), 1);
    assert_eq!(
        machine.attempt_finished(plan.id, Ok("daemon-2"), t0 + secs(2)),
        AttemptVerdict::Applied
    );
    assert_eq!(machine.status().kind(), StatusKind::Connected);
}

#[test]
fn cancel_shows_the_last_error_and_abandons_the_attempt_in_flight() {
    let t0 = Instant::now();
    let mut machine = connected(t0);
    machine.handle_timeout(t0 + HEARTBEAT_TIMEOUT);
    machine.reconnect_now();
    let plan = started_attempt(&mut machine);

    machine.cancel_reconnect();

    assert_eq!(effects(&mut machine), vec![Effect::CancelAttempt]);
    let ConnectionStatus::Error { error } = machine.status() else {
        panic!("expected an error");
    };
    assert_eq!(
        error.to_string(),
        "Connection lost (no heartbeat from daemon)"
    );
    assert_eq!(machine.poll_timeout(), None);
    // Had the result arrived anyway, it would be dropped.
    assert_eq!(
        machine.attempt_finished(plan.id, Ok("late"), t0 + secs(60)),
        AttemptVerdict::Stale
    );
    assert_eq!(machine.status().kind(), StatusKind::Error);
}

#[test]
fn cancel_while_waiting_cancels_nothing_in_flight() {
    let t0 = Instant::now();
    let mut machine = connected(t0);
    machine.heartbeat_failed("gone".to_owned(), t0);
    effects(&mut machine);
    machine.cancel_reconnect();
    assert!(effects(&mut machine).is_empty());
    assert_eq!(machine.status().kind(), StatusKind::Error);
}

#[test]
fn cancelling_a_forced_reconnect_says_so() {
    let t0 = Instant::now();
    let mut machine = connected(t0);
    machine.force_reconnect(t0);
    assert_eq!(
        machine.status().error().unwrap().to_string(),
        "Reconnecting..."
    );
    machine.cancel_reconnect();
    assert_eq!(
        machine.status().error().unwrap().to_string(),
        "Reconnection cancelled"
    );
}

#[test]
fn cancel_when_not_reconnecting_is_a_no_op() {
    let t0 = Instant::now();
    let mut machine = connected(t0);
    machine.cancel_reconnect();
    assert_eq!(machine.status().kind(), StatusKind::Connected);
}

#[test]
fn reconnect_restarts_the_loop_from_the_error_state_only() {
    let t0 = Instant::now();
    let mut machine = connected(t0);
    machine.reconnect(t0);
    assert_eq!(machine.status().kind(), StatusKind::Connected);

    machine.handle_timeout(t0 + HEARTBEAT_TIMEOUT);
    machine.cancel_reconnect();
    effects(&mut machine);

    machine.reconnect(t0 + secs(100));

    assert_eq!(attempt_of(&machine), 0);
    assert_eq!(
        machine.status().error().unwrap().to_string(),
        "Connection lost (no heartbeat from daemon)"
    );
    assert_eq!(machine.poll_timeout(), Some(t0 + secs(101)));
}

#[test]
fn force_reconnect_from_connected_drops_the_transport_and_waits_a_second() {
    let t0 = Instant::now();
    let mut machine = connected(t0);
    machine.force_reconnect(t0);
    assert_eq!(effects(&mut machine), vec![Effect::DropTransport]);
    assert_eq!(attempt_of(&machine), 0);
    machine.handle_timeout(t0 + secs(1));
    assert_eq!(started_attempt(&mut machine).target, AttemptTarget::Local);
}

#[test]
fn force_reconnect_while_reconnecting_is_a_no_op() {
    let t0 = Instant::now();
    let mut machine = connected(t0);
    machine.heartbeat_failed("gone".to_owned(), t0);
    effects(&mut machine);
    machine.force_reconnect(t0 + secs(10));
    assert!(effects(&mut machine).is_empty());
    assert_eq!(machine.poll_timeout(), Some(t0 + secs(1)));
}

#[test]
fn force_reconnect_during_the_first_connect_abandons_it() {
    let t0 = Instant::now();
    let mut machine = local();
    let first = started_attempt(&mut machine);
    machine.force_reconnect(t0);
    assert_eq!(
        effects(&mut machine),
        vec![Effect::CancelAttempt, Effect::DropTransport]
    );
    assert_eq!(
        machine.attempt_finished(first.id, Ok("late"), t0),
        AttemptVerdict::Stale
    );
}

#[test]
fn reconnects_carry_connect_anyway_but_never_refresh() {
    let t0 = Instant::now();
    for (first, again) in [
        (OutdatedDaemon::Accept, OutdatedDaemon::Accept),
        (OutdatedDaemon::Refresh, OutdatedDaemon::Report),
        (OutdatedDaemon::Report, OutdatedDaemon::Report),
    ] {
        let mut machine = remote(first);
        let plan = started_attempt(&mut machine);
        assert_eq!(
            machine.attempt_finished(plan.id, Ok("daemon-1"), t0),
            AttemptVerdict::Applied
        );
        machine.force_reconnect(t0);
        machine.reconnect_now();
        let AttemptTarget::Remote { outdated, .. } = started_attempt(&mut machine).target else {
            panic!("remote attempt");
        };
        assert_eq!(outdated, again, "first connect with {first:?}");
    }
}

#[test]
fn disconnect_stops_attempts_transport_and_forwards() {
    let t0 = Instant::now();
    let mut machine = remote(OutdatedDaemon::Report);
    let plan = started_attempt(&mut machine);
    assert_eq!(
        machine.attempt_finished(plan.id, Ok("daemon-1"), t0),
        AttemptVerdict::Applied
    );
    machine
        .add_port_forward(pf_config("pf-1", 8080), t0)
        .unwrap();
    effects(&mut machine);
    while machine.poll_event().is_some() {}

    machine.disconnect();

    assert_eq!(
        effects(&mut machine),
        vec![
            Effect::CancelAttempt,
            Effect::DropTransport,
            Effect::KillForward {
                id: PortForwardId::new("pf-1")
            }
        ]
    );
    let events: Vec<String> = std::iter::from_fn(|| machine.poll_event())
        .map(|event| match event {
            MachineEvent::Status(info) => format!("status {}", info.status.kind()),
            MachineEvent::ForwardStatus(info) => format!("forward {}", info.status.as_str()),
            MachineEvent::ForwardOutput { line, .. } => format!("output {line}"),
            MachineEvent::ForwardRemoved(id) => format!("removed {id}"),
        })
        .collect();
    assert_eq!(
        events,
        vec!["forward stopped", "removed pf-1", "status disconnected"]
    );
    assert!(machine.port_forwards().is_empty());
    assert_eq!(machine.poll_timeout(), None);
    machine.force_reconnect(t0);
    assert_eq!(machine.status().kind(), StatusKind::Disconnected);
}

#[test]
fn a_disconnect_mid_attempt_does_not_resurrect_the_connection() {
    let t0 = Instant::now();
    let mut machine = connected(t0);
    machine.handle_timeout(t0 + HEARTBEAT_TIMEOUT);
    machine.handle_timeout(t0 + HEARTBEAT_TIMEOUT + secs(1));
    let plan = started_attempt(&mut machine);

    machine.disconnect();

    assert_eq!(
        machine.attempt_finished(plan.id, Ok("late"), t0 + secs(60)),
        AttemptVerdict::Stale
    );
    assert_eq!(machine.status().kind(), StatusKind::Disconnected);
    assert_eq!(machine.poll_timeout(), None);
}

#[test]
fn port_forwards_need_a_remote_connection() {
    let mut machine = local();
    let error = machine
        .add_port_forward(pf_config("pf-1", 8080), Instant::now())
        .unwrap_err();
    assert_eq!(error.to_string(), "Connection is not remote: local-1");
}

#[test]
fn port_forwards_become_active_through_the_machine_clock() {
    let t0 = Instant::now();
    let mut machine = remote(OutdatedDaemon::Report);
    effects(&mut machine);
    let info = machine
        .add_port_forward(pf_config("pf-1", 8080), t0)
        .unwrap();
    assert_eq!(info.status, PortForwardStatus::Connecting);
    let Some(Effect::SpawnForward { id, command, .. }) = effects(&mut machine).pop() else {
        panic!("spawns the forward");
    };
    assert_eq!(id, PortForwardId::new("pf-1"));
    assert_eq!(command.args[..2], ["-L", "8080:localhost:3000"]);
    assert_eq!(machine.poll_timeout(), Some(t0 + secs(2)));

    machine.handle_timeout(t0 + secs(2));

    assert_eq!(machine.port_forwards()[0].status, PortForwardStatus::Active);
}

#[test]
fn port_forwards_restart_only_after_a_successful_reconnect() {
    let t0 = Instant::now();
    let mut machine = remote(OutdatedDaemon::Report);
    let plan = started_attempt(&mut machine);
    assert_eq!(
        machine.attempt_finished(plan.id, Ok("daemon-1"), t0),
        AttemptVerdict::Applied
    );
    machine
        .add_port_forward(pf_config("pf-1", 8080), t0)
        .unwrap();
    machine
        .add_port_forward(pf_config("pf-2", 8081), t0)
        .unwrap();
    effects(&mut machine);

    machine.heartbeat_failed("gone".to_owned(), t0);
    machine.handle_timeout(t0 + secs(1));
    let failed = started_attempt(&mut machine);
    assert_eq!(
        machine.attempt_finished(failed.id, Err(ConnectionError::HeartbeatLost), t0 + secs(1)),
        AttemptVerdict::Applied
    );
    assert!(effects(&mut machine).is_empty());

    machine.handle_timeout(t0 + secs(3));
    let next = started_attempt(&mut machine);
    assert_eq!(
        machine.attempt_finished(next.id, Ok("daemon-2"), t0 + secs(4)),
        AttemptVerdict::Applied
    );

    let restarted: Vec<Effect> = effects(&mut machine);
    assert_eq!(restarted[0], Effect::WatchHeartbeat);
    let kinds: Vec<String> = restarted[1..]
        .iter()
        .map(|effect| match effect {
            Effect::KillForward { id } => format!("kill {id}"),
            Effect::SpawnForward { id, .. } => format!("spawn {id}"),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        vec!["kill pf-1", "spawn pf-1", "kill pf-2", "spawn pf-2"]
    );
}

#[test]
fn removing_a_forward_stops_it_and_reports_it_gone() {
    let t0 = Instant::now();
    let mut machine = remote(OutdatedDaemon::Report);
    machine
        .add_port_forward(pf_config("pf-1", 8080), t0)
        .unwrap();
    effects(&mut machine);
    while machine.poll_event().is_some() {}

    assert!(machine.remove_port_forward(&PortForwardId::new("pf-1")));
    assert!(!machine.remove_port_forward(&PortForwardId::new("pf-1")));

    assert_eq!(
        effects(&mut machine),
        vec![Effect::KillForward {
            id: PortForwardId::new("pf-1")
        }]
    );
    assert!(matches!(
        machine.poll_event(),
        Some(MachineEvent::ForwardStatus(PortForwardInfo {
            status: PortForwardStatus::Stopped,
            ..
        }))
    ));
    assert!(matches!(
        machine.poll_event(),
        Some(MachineEvent::ForwardRemoved(_))
    ));
}

#[test]
fn forward_process_events_reach_the_forward() {
    let t0 = Instant::now();
    let mut machine = remote(OutdatedDaemon::Report);
    machine
        .add_port_forward(pf_config("pf-1", 8080), t0)
        .unwrap();
    let Some(Effect::SpawnForward { generation, .. }) = effects(&mut machine).pop() else {
        panic!("spawns the forward");
    };
    let id = PortForwardId::new("pf-1");
    machine.port_forward_event(&id, generation, ProcessEvent::Exited(ExitStatus::Code(255)));
    assert_eq!(machine.port_forwards()[0].status.as_str(), "error");

    machine.port_forward_spawn_failed(&PortForwardId::new("unknown"), 1, "x".to_owned());
    assert_eq!(machine.port_forwards().len(), 1);
}
