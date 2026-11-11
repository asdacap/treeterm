//! Port of `src/main/connectionManager.test.ts`: the manager with its real drivers, a fake
//! dialer and an ssh host simulated through the fake spawner, on paused tokio time.
//!
//! Where the TS suite pokes mocks (`tunnelDisconnectCallback`, `grpcDisconnectCallback`),
//! these tests act on the fakes those mocks stand for: the tunnel's ssh process exits, the
//! daemon's session watch fails.

use std::time::Duration;

use futures::StreamExt;

use super::*;
use crate::connection::fake::FakeResponse;
use crate::connection::machine::HEARTBEAT_TIMEOUT;
use crate::connection::test_support::*;
use crate::connection::types::{ConnectionError, ConnectionErrorKind, PortForwardStatus};
use crate::settings::Extra;

fn pf_config(id: &str, port: u16) -> PortForwardConfig {
    PortForwardConfig {
        id: PortForwardId::new(id),
        connection_id: remote_id(),
        spec: PortForwardSpec {
            local_port: port,
            remote_host: "localhost".to_owned(),
            remote_port: 3000,
            extra: Extra::new(),
        },
        persistence: Persistence::Saved,
    }
}

fn kind(info: &ConnectionInfo<crate::connection::test_support::FakeDaemon>) -> StatusKind {
    info.status.kind()
}

fn status_of(harness: &Harness, id: &ConnectionId) -> StatusKind {
    harness.manager.connection(id).unwrap().status.kind()
}

fn error_of(harness: &Harness, id: &ConnectionId) -> String {
    harness
        .manager
        .connection(id)
        .unwrap()
        .status
        .error()
        .expect("an error")
        .to_string()
}

fn attempt_of(harness: &Harness, id: &ConnectionId) -> u32 {
    match harness.manager.connection(id).unwrap().status {
        ConnectionStatus::Reconnecting { attempt, .. } => attempt,
        other => panic!("not reconnecting: {}", other.kind()),
    }
}

fn daemon_of(harness: &Harness, id: &ConnectionId) -> FakeDaemon {
    harness.manager.client(id).unwrap()
}

fn kinds(updates: &mut Updates<ConnectionInfo<FakeDaemon>>) -> Vec<StatusKind> {
    updates.drain_ready().iter().map(kind).collect()
}

mod constructor {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn starts_with_no_connections() {
        let harness = Harness::new();
        assert!(harness.manager.list_connections().is_empty());
    }
}

mod connect_local {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn creates_a_connected_local_connection_with_auto_generated_id() {
        let (harness, local) = Harness::with_local().await;
        let info = harness.manager.connection(&local).unwrap();
        assert_eq!(info.status.kind(), StatusKind::Connected);
        assert_eq!(info.target, ConnectionTarget::Local);
        assert_eq!(
            uuid::Uuid::parse_str(local.as_str())
                .unwrap()
                .get_version_num(),
            4
        );
        assert_eq!(
            harness.dialer.dials(),
            vec![std::path::PathBuf::from(LOCAL_SOCKET)]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn creates_unique_ids_for_each_call() {
        let (mut harness, local) = Harness::with_local().await;
        let second = harness.manager.connect_local().await.unwrap();
        assert_ne!(second.id, local);
        assert_eq!(harness.manager.list_connections().len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_local_start_is_an_error_that_can_be_retried() {
        let mut harness = Harness::new();
        harness
            .dialer
            .fail_next("daemon executable not found at /x");
        let info = harness.manager.connect_local().await.unwrap();
        assert_eq!(kind(&info), StatusKind::Error);
        assert_eq!(
            info.status.error().unwrap().to_string(),
            "daemon executable not found at /x"
        );

        harness.manager.reconnect(&info.id);
        assert_eq!(status_of(&harness, &info.id), StatusKind::Reconnecting);
        advance(Duration::from_secs(1)).await;
        assert_eq!(status_of(&harness, &info.id), StatusKind::Connected);
    }
}

mod get_client {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn returns_client_for_existing_connection() {
        let (harness, local) = Harness::with_local().await;
        assert_eq!(harness.manager.client(&local).unwrap().serial, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn throws_for_missing_connection() {
        let (harness, _) = Harness::with_local().await;
        let error = harness
            .manager
            .client(&ConnectionId::new("nonexistent"))
            .unwrap_err();
        assert_eq!(error.to_string(), "Connection not found: nonexistent");
    }
}

mod list_connections {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn returns_all_connection_infos() {
        let (harness, _) = Harness::with_local().await;
        let connections = harness.manager.list_connections();
        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].target, ConnectionTarget::Local);
        assert_eq!(kind(&connections[0]), StatusKind::Connected);
    }
}

mod get_connection {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn returns_connection_info_for_existing_connection() {
        let (harness, local) = Harness::with_local().await;
        assert_eq!(status_of(&harness, &local), StatusKind::Connected);
    }

    #[tokio::test(start_paused = true)]
    async fn returns_undefined_for_missing_connection() {
        let (harness, _) = Harness::with_local().await;
        assert!(
            harness
                .manager
                .connection(&ConnectionId::new("nonexistent"))
                .is_none()
        );
    }
}

mod watch_bootstrap_output {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn sets_up_watcher_and_returns_scrollback() {
        let (harness, local) = Harness::with_local().await;
        let watch = harness
            .manager
            .watch_output(&local, OutputKind::Bootstrap)
            .unwrap();
        assert!(watch.initial.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn unsubscribe_removes_the_watcher() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        let watch = harness
            .manager
            .watch_output(&remote_id(), OutputKind::Bootstrap)
            .unwrap();
        drop(watch.updates);
        harness.manager.force_reconnect(&remote_id()).unwrap();
        advance(Duration::from_secs(1)).await;
        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Connected);
    }

    #[tokio::test(start_paused = true)]
    async fn creates_new_watcher_set_for_connection_without_existing_watchers() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        let mut first = harness
            .manager
            .watch_output(&remote_id(), OutputKind::Bootstrap)
            .unwrap();
        let mut second = harness
            .manager
            .watch_output(&remote_id(), OutputKind::Bootstrap)
            .unwrap();
        harness.manager.force_reconnect(&remote_id()).unwrap();
        advance(Duration::from_secs(1)).await;
        let first_lines = first.updates.drain_ready();
        assert!(!first_lines.is_empty());
        assert_eq!(first_lines, second.updates.drain_ready());
    }

    #[tokio::test(start_paused = true)]
    async fn returns_empty_scrollback_for_missing_connection() {
        let (harness, _) = Harness::with_local().await;
        let error = harness
            .manager
            .watch_output(&ConnectionId::new("nonexistent"), OutputKind::Bootstrap)
            .err()
            .unwrap();
        assert_eq!(error, ConnectionNotFound(ConnectionId::new("nonexistent")));
    }

    #[tokio::test(start_paused = true)]
    async fn get_bootstrap_output_returns_copy_of_buffer() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        let watch = harness
            .manager
            .watch_output(&remote_id(), OutputKind::Bootstrap)
            .unwrap();
        assert_eq!(
            watch.initial[0],
            "Bootstrapping remote daemon on test@example.com..."
        );
        let mut copy = watch.initial.clone();
        copy.push("extra".to_owned());
        let again = harness
            .manager
            .watch_output(&remote_id(), OutputKind::Bootstrap)
            .unwrap();
        assert_eq!(again.initial, watch.initial);
    }

    #[tokio::test(start_paused = true)]
    async fn scrollback_starts_fresh_with_each_tunnel() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        let before = harness
            .manager
            .watch_output(&remote_id(), OutputKind::Bootstrap)
            .unwrap();
        harness.manager.force_reconnect(&remote_id()).unwrap();
        advance(Duration::from_secs(1)).await;
        let after = harness
            .manager
            .watch_output(&remote_id(), OutputKind::Bootstrap)
            .unwrap();
        assert_eq!(after.initial.len(), before.initial.len());
    }
}

mod watch_connection_status {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn returns_initial_connection_info_and_unsubscribe() {
        let (harness, local) = Harness::with_local().await;
        let watch = harness.manager.watch_connection_status(&local).unwrap();
        assert_eq!(watch.initial.id, local);
        assert_eq!(kind(&watch.initial), StatusKind::Connected);
    }

    #[tokio::test(start_paused = true)]
    async fn returns_undefined_initial_for_missing_connection() {
        let (harness, _) = Harness::with_local().await;
        assert!(
            harness
                .manager
                .watch_connection_status(&ConnectionId::new("nonexistent"))
                .is_err()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn unsubscribe_removes_the_watcher() {
        let (harness, local) = Harness::with_local().await;
        let watch = harness.manager.watch_connection_status(&local).unwrap();
        drop(watch.updates);
        harness.manager.force_reconnect(&local).unwrap();
        assert_eq!(status_of(&harness, &local), StatusKind::Reconnecting);
    }
}

mod connect_remote {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn connects_successfully_and_lists_the_connection() {
        let (mut harness, _) = Harness::with_local().await;
        let info = harness.connect_remote().await;
        assert_eq!(kind(&info), StatusKind::Connected);
        assert_eq!(info.id, remote_id());
        assert_eq!(
            info.target,
            ConnectionTarget::Remote(std::sync::Arc::new(remote_config()))
        );
        assert_eq!(harness.manager.list_connections().len(), 2);
        // The daemon is reached through the tunnel's local end.
        let ConnectionStatus::Connected { daemon } = info.status else {
            unreachable!()
        };
        let tunnel = harness.latest(SpawnKind::Tunnel);
        assert_eq!(daemon.socket, forwarded_socket(tunnel.command()));
        assert!(daemon.socket.starts_with(harness.dir.path().join("s")));
    }

    #[tokio::test(start_paused = true)]
    async fn returns_existing_connection_if_already_connected() {
        let (mut harness, _) = Harness::with_local().await;
        let first = harness.connect_remote().await;
        let second = harness.connect_remote().await;
        assert_eq!(kind(&second), StatusKind::Connected);
        assert_eq!(harness.manager.client(&first.id).unwrap().serial, 2);
        assert_eq!(harness.spawned(SpawnKind::Bootstrap).len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn deduplicates_concurrent_connection_attempts() {
        let (mut harness, _) = Harness::with_local().await;
        let first = harness
            .manager
            .connect_remote(remote_config(), OutdatedDaemon::Report);
        let second = harness
            .manager
            .connect_remote(remote_config(), OutdatedDaemon::Report);
        let (first, second) = futures::join!(first, second);
        let (first, second) = (first.unwrap(), second.unwrap());
        assert_eq!(first.id, second.id);
        assert_eq!(kind(&first), kind(&second));
        assert_eq!(harness.spawned(SpawnKind::Bootstrap).len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn handles_connection_failure() {
        let (mut harness, _) = Harness::with_local().await;
        harness.host.next_bootstrap(FakeResponse::Exit {
            stdout: String::new(),
            stderr: "Permission denied (publickey)\n".to_owned(),
            code: 255,
        });
        let info = harness.connect_remote().await;
        assert_eq!(kind(&info), StatusKind::Error);
        assert_eq!(
            info.status.error().unwrap().to_string(),
            "SSH bootstrap failed (exit 255). See log output for details."
        );
        assert!(harness.spawned(SpawnKind::Tunnel).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn tears_down_the_tunnel_when_the_daemon_does_not_answer() {
        let (mut harness, _) = Harness::with_local().await;
        harness.dialer.fail_next("connection refused");
        let info = harness.connect_remote().await;
        assert_eq!(kind(&info), StatusKind::Error);
        let tunnel = harness.latest(SpawnKind::Tunnel);
        assert!(tunnel.is_killed());
        assert!(!forwarded_socket(tunnel.command()).exists());
    }

    #[tokio::test(start_paused = true)]
    async fn tags_a_generic_failure_with_error_kind_generic() {
        let (mut harness, _) = Harness::with_local().await;
        harness.host.next_bootstrap(FakeResponse::exit(1));
        let info = harness.connect_remote().await;
        assert_eq!(
            info.status.error().unwrap().kind(),
            ConnectionErrorKind::Generic
        );
    }

    #[tokio::test(start_paused = true)]
    async fn surfaces_a_daemon_hash_mismatch_as_a_typed_recoverable_error() {
        let (mut harness, _) = Harness::with_local().await;
        harness.host.set_remote_hash("remotehash9999");
        let dials = harness.dialer.dials().len();

        let info = harness.connect_remote().await;

        let Some(ConnectionError::DaemonHashMismatch {
            local_hash,
            remote_hash,
        }) = info.status.error()
        else {
            panic!("expected a hash mismatch");
        };
        assert_eq!(*local_hash, binary_hash());
        assert_eq!(remote_hash, "remotehash9999");
        assert_eq!(
            info.status.error().unwrap().kind(),
            ConnectionErrorKind::DaemonHashMismatch
        );
        assert!(
            info.status
                .error()
                .unwrap()
                .to_string()
                .ends_with("The remote daemon is outdated. Refresh it to re-upload (drops running processes), or connect anyway.")
        );
        // No tunnel and no gRPC client for a mismatch.
        assert!(harness.spawned(SpawnKind::Tunnel).is_empty());
        assert_eq!(harness.dialer.dials().len(), dials);
    }

    #[tokio::test(start_paused = true)]
    async fn connect_anyway_and_refresh_resolve_a_mismatch() {
        let (mut harness, _) = Harness::with_local().await;
        harness.host.set_remote_hash("remotehash9999");
        assert_eq!(kind(&harness.connect_remote().await), StatusKind::Error);

        let accepted = harness.connect_remote_with(OutdatedDaemon::Accept).await;
        assert_eq!(kind(&accepted), StatusKind::Connected);
        harness.manager.disconnect(&remote_id());

        let refreshed = harness.connect_remote_with(OutdatedDaemon::Refresh).await;
        assert_eq!(kind(&refreshed), StatusKind::Connected);
        assert_eq!(harness.spawned(SpawnKind::Scp).len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn emits_status_changes_during_connection() {
        let (mut harness, _) = Harness::with_local().await;
        let mut changes = harness.manager.status_changes();
        harness.connect_remote().await;
        let phases: Vec<String> = changes
            .drain_ready()
            .into_iter()
            .map(|info| match info.status {
                ConnectionStatus::Connecting { phase } => format!("connecting {phase:?}"),
                other => other.kind().to_string(),
            })
            .collect();
        assert_eq!(
            phases,
            vec![
                "connecting Bootstrap",
                "connecting Tunnel",
                "connecting Daemon",
                "connected"
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn notifies_per_connection_status_watchers() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        let mut watch = harness
            .manager
            .watch_connection_status(&remote_id())
            .unwrap();

        harness.latest(SpawnKind::Tunnel).exit(255);
        flush().await;

        assert_eq!(kinds(&mut watch.updates), vec![StatusKind::Reconnecting]);
    }

    #[tokio::test(start_paused = true)]
    async fn forwards_bootstrap_output_to_watchers() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        let mut watch = harness
            .manager
            .watch_output(&remote_id(), OutputKind::Bootstrap)
            .unwrap();
        harness.host.next_bootstrap(FakeResponse::stdout(format!(
            "hello from bootstrap\nTREETERM_OS:Linux\nTREETERM_ARCH:x86_64\nTREETERM_SOCKET:{REMOTE_SOCKET}\n"
        )));
        harness.manager.force_reconnect(&remote_id()).unwrap();
        advance(Duration::from_secs(1)).await;
        assert!(
            watch
                .updates
                .drain_ready()
                .contains(&"hello from bootstrap".to_owned())
        );
    }

    #[tokio::test(start_paused = true)]
    async fn forwards_tunnel_output_to_watchers() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        let mut watch = harness
            .manager
            .watch_output(&remote_id(), OutputKind::Tunnel)
            .unwrap();
        harness
            .latest(SpawnKind::Tunnel)
            .stdout("hello from tunnel\n");
        flush().await;
        assert_eq!(watch.updates.drain_ready(), vec!["hello from tunnel"]);
        assert!(
            watch
                .initial
                .last()
                .unwrap()
                .starts_with("Local socket ready: ")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn monitors_tunnel_disconnection_and_triggers_reconnect() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        let tunnel = harness.latest(SpawnKind::Tunnel);
        tunnel.stderr("client_loop: send disconnect: Broken pipe\n");
        tunnel.exit(255);
        flush().await;
        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Reconnecting);
        assert_eq!(
            error_of(&harness, &remote_id()),
            "SSH tunnel closed (exit 255): client_loop: send disconnect: Broken pipe"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn monitors_grpc_client_disconnection_and_triggers_reconnect() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        let daemon = daemon_of(&harness, &remote_id());
        harness
            .dialer
            .fail_watch(&daemon, "h2 protocol error: broken pipe");
        flush().await;
        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Reconnecting);
        assert_eq!(
            error_of(&harness, &remote_id()),
            "gRPC connection lost: h2 protocol error: broken pipe"
        );
        // The dead generation's tunnel goes with it.
        assert!(harness.latest(SpawnKind::Tunnel).is_killed());
    }
}

mod disconnect {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn no_ops_for_missing_connection() {
        let (mut harness, _) = Harness::with_local().await;
        harness
            .manager
            .disconnect(&ConnectionId::new("nonexistent"));
        assert_eq!(harness.manager.list_connections().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn disconnects_client_and_tunnel_and_removes_connection() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        let daemon = daemon_of(&harness, &remote_id());
        assert_eq!(harness.manager.list_connections().len(), 2);

        harness.manager.disconnect(&remote_id());
        flush().await;

        assert_eq!(harness.manager.list_connections().len(), 1);
        assert!(harness.manager.connection(&remote_id()).is_none());
        let tunnel = harness.latest(SpawnKind::Tunnel);
        assert!(tunnel.is_killed());
        assert!(!forwarded_socket(tunnel.command()).exists());
        assert!(harness.dialer.watch_dropped(&daemon));
    }

    #[tokio::test(start_paused = true)]
    async fn disconnects_local_connection() {
        let (mut harness, local) = Harness::with_local().await;
        harness.manager.disconnect(&local);
        assert!(harness.manager.list_connections().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn emits_disconnected_status() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        let mut changes = harness.manager.status_changes();
        let mut watch = harness
            .manager
            .watch_connection_status(&remote_id())
            .unwrap();

        harness.manager.disconnect(&remote_id());

        assert_eq!(kinds(&mut changes), vec![StatusKind::Disconnected]);
        assert_eq!(kinds(&mut watch.updates), vec![StatusKind::Disconnected]);
        // The connection is gone: its watch ends.
        assert!(watch.updates.next().await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn cleans_up_port_forwards_for_the_connection() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        harness
            .manager
            .add_port_forward(pf_config("pf-1", 8080))
            .unwrap();
        flush().await;
        assert_eq!(harness.manager.list_port_forwards(&remote_id()).len(), 1);

        harness.manager.disconnect(&remote_id());
        flush().await;

        assert!(harness.manager.list_port_forwards(&remote_id()).is_empty());
        assert!(harness.latest(SpawnKind::PortForward).is_killed());
    }
}

mod on_status_change {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn registers_listener_and_returns_unsubscribe() {
        let (harness, local) = Harness::with_local().await;
        let mut changes = harness.manager.status_changes();
        let dropped = harness.manager.status_changes();
        drop(dropped);
        harness.manager.force_reconnect(&local).unwrap();
        assert_eq!(kinds(&mut changes), vec![StatusKind::Reconnecting]);
    }
}

mod port_forwarding {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn add_port_forward_creates_and_starts_a_port_forward() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        let info = harness
            .manager
            .add_port_forward(pf_config("pf-1", 8080))
            .unwrap();
        assert_eq!(info.config.id, PortForwardId::new("pf-1"));
        assert_eq!(info.status, PortForwardStatus::Connecting);
        flush().await;
        let forward = harness.latest(SpawnKind::PortForward);
        assert_eq!(forward.args()[..2], ["-L", "8080:localhost:3000"]);

        advance(Duration::from_secs(2)).await;
        assert_eq!(
            harness.manager.list_port_forwards(&remote_id())[0].status,
            PortForwardStatus::Active
        );
    }

    #[tokio::test(start_paused = true)]
    async fn add_port_forward_throws_for_local_connection() {
        let (harness, local) = Harness::with_local().await;
        let config = PortForwardConfig {
            connection_id: local.clone(),
            ..pf_config("pf-1", 8080)
        };
        assert_eq!(
            harness.manager.add_port_forward(config).unwrap_err(),
            NotRemoteConnection(local)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn add_port_forward_throws_for_unknown_connection() {
        let (harness, _) = Harness::with_local().await;
        let config = PortForwardConfig {
            connection_id: ConnectionId::new("unknown"),
            ..pf_config("pf-1", 8080)
        };
        assert_eq!(
            harness
                .manager
                .add_port_forward(config)
                .unwrap_err()
                .to_string(),
            "Connection not found or not remote: unknown"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn list_port_forwards_returns_forwards_for_a_connection() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        harness
            .manager
            .add_port_forward(pf_config("pf-1", 8080))
            .unwrap();
        let list = harness.manager.list_port_forwards(&remote_id());
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].config.id, PortForwardId::new("pf-1"));
    }

    #[tokio::test(start_paused = true)]
    async fn list_port_forwards_returns_empty_for_connection_with_no_forwards() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        assert!(harness.manager.list_port_forwards(&remote_id()).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn remove_port_forward_stops_and_removes_the_forward() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        harness
            .manager
            .add_port_forward(pf_config("pf-1", 8080))
            .unwrap();
        flush().await;
        let mut status = harness
            .manager
            .watch_port_forward_status(&PortForwardId::new("pf-1"))
            .unwrap();

        harness
            .manager
            .remove_port_forward(&PortForwardId::new("pf-1"));
        flush().await;

        assert!(harness.latest(SpawnKind::PortForward).is_killed());
        assert!(harness.manager.list_port_forwards(&remote_id()).is_empty());
        let statuses: Vec<_> = status
            .updates
            .drain_ready()
            .into_iter()
            .map(|info| info.status)
            .collect();
        assert_eq!(statuses, vec![PortForwardStatus::Stopped]);
        assert!(status.updates.next().await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn remove_port_forward_no_ops_for_unknown_id() {
        let (harness, _) = Harness::with_local().await;
        harness
            .manager
            .remove_port_forward(&PortForwardId::new("unknown"));
    }

    #[tokio::test(start_paused = true)]
    async fn watch_port_forward_output_returns_scrollback_and_subscribes() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        harness
            .manager
            .add_port_forward(pf_config("pf-1", 8080))
            .unwrap();
        let watch = harness
            .manager
            .watch_port_forward_output(&PortForwardId::new("pf-1"))
            .unwrap();
        assert_eq!(
            watch.initial[0],
            "[portfwd] Starting port forward: localhost:8080 -> localhost:3000"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn watch_port_forward_output_returns_empty_for_unknown_forward() {
        let (harness, _) = Harness::with_local().await;
        assert!(
            harness
                .manager
                .watch_port_forward_output(&PortForwardId::new("unknown"))
                .is_err()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn watch_port_forward_status_returns_initial_info() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        harness
            .manager
            .add_port_forward(pf_config("pf-1", 8080))
            .unwrap();
        let watch = harness
            .manager
            .watch_port_forward_status(&PortForwardId::new("pf-1"))
            .unwrap();
        assert_eq!(watch.initial.config.id, PortForwardId::new("pf-1"));
        assert_eq!(watch.initial.status, PortForwardStatus::Connecting);
    }

    #[tokio::test(start_paused = true)]
    async fn watch_port_forward_status_returns_undefined_for_unknown_forward() {
        let (harness, _) = Harness::with_local().await;
        assert_eq!(
            harness
                .manager
                .watch_port_forward_status(&PortForwardId::new("unknown"))
                .err()
                .unwrap()
                .to_string(),
            "Port forward not found: unknown"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_forward_whose_ssh_cannot_start_reports_an_error() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        harness
            .spawner
            .respond(|_| FakeResponse::Refuse("spawn ssh ENOENT".to_owned()));
        harness
            .manager
            .add_port_forward(pf_config("pf-1", 8080))
            .unwrap();
        flush().await;
        assert_eq!(
            harness.manager.list_port_forwards(&remote_id())[0]
                .status
                .as_str(),
            "error"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn saved_forwards_start_on_request_and_are_listed_for_saving() {
        let (mut harness, _) = Harness::with_local().await;
        let spec = pf_config("x", 9000).spec;
        let config = SshConnectionConfig {
            port_forwards: vec![spec.clone()],
            ..remote_config()
        };
        harness
            .manager
            .connect_remote(config, OutdatedDaemon::Report)
            .await
            .unwrap();
        let started = harness
            .manager
            .start_saved_port_forwards(&remote_id())
            .unwrap();
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].config.persistence, Persistence::Saved);
        harness
            .manager
            .add_port_forward(PortForwardConfig {
                persistence: Persistence::Ephemeral,
                ..pf_config("tab", 9001)
            })
            .unwrap();
        assert_eq!(
            harness.manager.saved_port_forward_specs(&remote_id()),
            vec![spec]
        );
    }
}

mod port_forwards_across_reconnect {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn restarts_every_port_forward_once_the_connection_reconnects() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        harness
            .manager
            .add_port_forward(pf_config("pf-1", 8080))
            .unwrap();
        harness
            .manager
            .add_port_forward(pf_config("pf-2", 8081))
            .unwrap();
        flush().await;
        let originals = harness.spawned(SpawnKind::PortForward);
        assert_eq!(originals.len(), 2);

        let daemon = daemon_of(&harness, &remote_id());
        harness.dialer.fail_watch(&daemon, "gone");
        flush().await;
        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Reconnecting);
        assert_eq!(harness.spawned(SpawnKind::PortForward).len(), 2);
        assert!(originals.iter().all(|forward| !forward.is_killed()));

        advance(Duration::from_secs(1)).await;

        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Connected);
        assert!(originals.iter().all(|forward| forward.is_killed()));
        assert_eq!(harness.spawned(SpawnKind::PortForward).len(), 4);
        // Same forwards restarted in place: ids (and their watchers) stay valid.
        let ids: Vec<_> = harness
            .manager
            .list_port_forwards(&remote_id())
            .into_iter()
            .map(|info| info.config.id)
            .collect();
        assert_eq!(
            ids,
            vec![PortForwardId::new("pf-1"), PortForwardId::new("pf-2")]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn does_not_restart_port_forwards_on_a_failed_attempt_only_on_the_one_that_succeeds() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        harness
            .manager
            .add_port_forward(pf_config("pf-1", 8080))
            .unwrap();
        flush().await;
        harness.host.next_bootstrap(FakeResponse::Exit {
            stdout: String::new(),
            stderr: "ssh: connect to host example.com port 22: No route to host\n".to_owned(),
            code: 255,
        });

        harness.latest(SpawnKind::Tunnel).exit(255);
        advance(Duration::from_secs(1)).await;

        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Reconnecting);
        assert_eq!(harness.spawned(SpawnKind::PortForward).len(), 1);

        // Second attempt (2s backoff) succeeds.
        advance(Duration::from_secs(2)).await;

        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Connected);
        assert_eq!(harness.spawned(SpawnKind::PortForward).len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn does_not_restart_port_forwards_when_the_connection_is_removed_mid_attempt() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        harness
            .manager
            .add_port_forward(pf_config("pf-1", 8080))
            .unwrap();
        flush().await;
        let release = harness.dialer.gate_next();

        harness.manager.force_reconnect(&remote_id()).unwrap();
        advance(Duration::from_secs(1)).await;
        harness.manager.disconnect(&remote_id());
        flush().await;
        // The attempt was dropped with the connection: nobody waits on the gate any more.
        release.send(()).unwrap_err();
        flush().await;

        assert_eq!(harness.spawned(SpawnKind::PortForward).len(), 1);
        assert!(harness.latest(SpawnKind::PortForward).is_killed());
    }

    #[tokio::test(start_paused = true)]
    async fn stops_the_stale_connections_port_forwards_when_connect_remote_replaces_it() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        harness
            .manager
            .add_port_forward(pf_config("pf-1", 8080))
            .unwrap();
        flush().await;
        let stale = harness.latest(SpawnKind::PortForward);

        harness.latest(SpawnKind::Tunnel).exit(255);
        flush().await;
        harness.manager.cancel_reconnect(&remote_id());
        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Error);

        // Manual "Retry": a fresh connection takes over the same id.
        let info = harness.connect_remote().await;

        assert_eq!(kind(&info), StatusKind::Connected);
        assert!(stale.is_killed());
        assert!(harness.manager.list_port_forwards(&remote_id()).is_empty());
        // The replaced connection's reconnect loop is dead: no attempt fires later.
        let bootstraps = harness.spawned(SpawnKind::Bootstrap).len();
        advance(Duration::from_secs(30)).await;
        assert_eq!(harness.spawned(SpawnKind::Bootstrap).len(), bootstraps);
    }
}

mod disconnect_all {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn disconnects_all_connections_including_local() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        assert_eq!(harness.manager.list_connections().len(), 2);
        harness.manager.disconnect_all();
        assert!(harness.manager.list_connections().is_empty());
    }
}

mod get_ssh_tunnel {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn returns_tunnel_for_remote_connection() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        let upload = harness
            .manager
            .upload_file(
                &remote_id(),
                "/local/a.txt".into(),
                "/home/test/a.txt".to_owned(),
            )
            .unwrap();
        upload.await.unwrap().unwrap();
        assert_eq!(
            harness.latest(SpawnKind::Scp).args().last().unwrap(),
            "test@example.com:/home/test/a.txt"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn returns_undefined_for_local_connection() {
        let (harness, local) = Harness::with_local().await;
        assert!(
            harness
                .manager
                .upload_file(&local, "/a".into(), "/b".to_owned())
                .is_err()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn returns_undefined_for_missing_connection() {
        let (harness, _) = Harness::with_local().await;
        assert!(
            harness
                .manager
                .upload_file(
                    &ConnectionId::new("nonexistent"),
                    "/a".into(),
                    "/b".to_owned()
                )
                .is_err()
        );
    }
}

mod heartbeat_monitoring {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn starts_heartbeat_for_local_connection_on_connect_local() {
        let (harness, local) = Harness::with_local().await;
        flush().await;
        let listeners = harness.dialer.watch_listeners();
        assert_eq!(listeners.len(), 1);
        assert!(listeners[0].starts_with(&format!("heartbeat-{local}-")));
    }

    #[tokio::test(start_paused = true)]
    async fn starts_heartbeat_for_remote_connection_after_connect() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        flush().await;
        assert!(
            harness
                .dialer
                .watch_listeners()
                .iter()
                .any(|listener| listener.starts_with("heartbeat-remote-1-"))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn heartbeat_timeout_triggers_reconnecting_for_local_connection() {
        let (harness, local) = Harness::with_local().await;
        let mut changes = harness.manager.status_changes();
        advance(HEARTBEAT_TIMEOUT).await;
        assert_eq!(status_of(&harness, &local), StatusKind::Reconnecting);
        assert_eq!(
            error_of(&harness, &local),
            "Connection lost (no heartbeat from daemon)"
        );
        assert_eq!(kinds(&mut changes)[0], StatusKind::Reconnecting);
    }

    #[tokio::test(start_paused = true)]
    async fn heartbeat_timeout_triggers_reconnecting_for_remote_connection() {
        let (mut harness, local) = Harness::with_local().await;
        harness.connect_remote().await;
        // Keep the local connection alive; only the remote one goes quiet.
        let local_daemon = daemon_of(&harness, &local);
        advance(HEARTBEAT_TIMEOUT - Duration::from_secs(10)).await;
        harness.dialer.beat(&local_daemon);
        advance(Duration::from_secs(10)).await;
        assert_eq!(status_of(&harness, &local), StatusKind::Connected);
        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Reconnecting);
        assert_eq!(
            error_of(&harness, &remote_id()),
            "Connection lost (no heartbeat from daemon)"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn heartbeats_keep_the_connection_alive() {
        let (harness, local) = Harness::with_local().await;
        let daemon = daemon_of(&harness, &local);
        for _ in 0..10 {
            advance(Duration::from_secs(15)).await;
            harness.dialer.beat(&daemon);
            flush().await;
        }
        assert_eq!(status_of(&harness, &local), StatusKind::Connected);
    }

    #[tokio::test(start_paused = true)]
    async fn heartbeat_stream_error_triggers_reconnecting() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        flush().await;
        harness
            .dialer
            .fail_watch(&daemon_of(&harness, &remote_id()), "stream broken");
        flush().await;
        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Reconnecting);
    }

    #[tokio::test(start_paused = true)]
    async fn tunnel_disconnect_triggers_reconnecting() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        flush().await;
        let daemon = daemon_of(&harness, &remote_id());

        harness.latest(SpawnKind::Tunnel).exit(255);
        flush().await;

        assert!(harness.dialer.watch_dropped(&daemon));
        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Reconnecting);
    }

    #[tokio::test(start_paused = true)]
    async fn grpc_disconnect_triggers_reconnecting() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        flush().await;
        let daemon = daemon_of(&harness, &remote_id());

        harness.dialer.fail_watch(&daemon, "transport error");
        flush().await;

        assert!(harness.dialer.watch_dropped(&daemon));
        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Reconnecting);
    }

    #[tokio::test(start_paused = true)]
    async fn manual_disconnect_stops_heartbeat() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        flush().await;
        let daemon = daemon_of(&harness, &remote_id());
        harness.manager.disconnect(&remote_id());
        flush().await;
        assert!(harness.dialer.watch_dropped(&daemon));
    }
}

mod reconnect {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn keeps_the_remote_client_usable_when_reconnect_now_is_repeated_during_an_attempt() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        let release = harness.dialer.gate_next();

        harness.manager.force_reconnect(&remote_id()).unwrap();
        advance(Duration::from_secs(1)).await;
        let tunnels = harness.spawned(SpawnKind::Tunnel).len();
        harness.manager.reconnect_now(&remote_id());
        flush().await;

        release.send(()).unwrap();
        flush().await;

        assert_eq!(harness.spawned(SpawnKind::Tunnel).len(), tunnels);
        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Connected);
        assert!(harness.manager.client(&remote_id()).is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn carries_connect_anyway_into_reconnect_tunnels_but_never_replays_refresh_daemon() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote_with(OutdatedDaemon::Accept).await;
        harness.host.set_remote_hash("remotehash9999");
        harness.manager.force_reconnect(&remote_id()).unwrap();
        advance(Duration::from_secs(1)).await;
        // Still accepting the outdated daemon.
        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Connected);

        harness.manager.disconnect(&remote_id());
        harness.host.set_remote_hash(&binary_hash());
        harness.connect_remote_with(OutdatedDaemon::Refresh).await;
        let first = harness.latest(SpawnKind::Bootstrap);
        assert!(first.args().last().unwrap().contains("REFRESH_DAEMON=1"));
        harness.manager.force_reconnect(&remote_id()).unwrap();
        advance(Duration::from_secs(1)).await;
        let again = harness.latest(SpawnKind::Bootstrap);
        assert!(again.args().last().unwrap().contains("REFRESH_DAEMON=0"));
    }

    #[tokio::test(start_paused = true)]
    async fn a_reconnect_reports_a_mismatch_unless_it_was_accepted() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        harness.host.set_remote_hash("remotehash9999");
        harness.manager.force_reconnect(&remote_id()).unwrap();
        advance(Duration::from_secs(1)).await;
        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Reconnecting);
        assert_eq!(
            harness
                .manager
                .connection(&remote_id())
                .unwrap()
                .status
                .error()
                .unwrap()
                .kind(),
            ConnectionErrorKind::DaemonHashMismatch
        );
    }

    #[tokio::test(start_paused = true)]
    async fn does_not_schedule_another_attempt_when_the_tunnel_drops_during_reconnect() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        let old_tunnel = harness.latest(SpawnKind::Tunnel);
        harness.manager.force_reconnect(&remote_id()).unwrap();
        advance(Duration::from_millis(500)).await;
        // The old tunnel was dropped with its generation; its exit reaches nobody.
        assert!(old_tunnel.is_killed());
        old_tunnel.exit(255);
        advance(Duration::from_millis(500)).await;

        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Connected);
        assert!(harness.manager.client(&remote_id()).is_ok());
        let bootstraps = harness.spawned(SpawnKind::Bootstrap).len();
        advance(Duration::from_secs(30)).await;
        assert_eq!(harness.spawned(SpawnKind::Bootstrap).len(), bootstraps);
    }

    #[tokio::test(start_paused = true)]
    async fn successful_reconnect_restores_connected_status() {
        let (harness, local) = Harness::with_local().await;
        advance(HEARTBEAT_TIMEOUT).await;
        assert_eq!(status_of(&harness, &local), StatusKind::Reconnecting);
        assert_eq!(attempt_of(&harness, &local), 0);

        advance(Duration::from_secs(1)).await;

        assert_eq!(status_of(&harness, &local), StatusKind::Connected);
        assert_eq!(harness.manager.client(&local).unwrap().serial, 2);
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_reconnect_stops_retries_and_sets_error() {
        let (harness, local) = Harness::with_local().await;
        harness.dialer.fail_always("connection refused");
        advance(HEARTBEAT_TIMEOUT).await;
        assert_eq!(status_of(&harness, &local), StatusKind::Reconnecting);

        harness.manager.cancel_reconnect(&local);

        assert_eq!(status_of(&harness, &local), StatusKind::Error);
        let dials = harness.dialer.dials().len();
        advance(Duration::from_secs(120)).await;
        assert_eq!(harness.dialer.dials().len(), dials);
    }

    #[tokio::test(start_paused = true)]
    async fn reconnect_now_skips_backoff_delay() {
        let (harness, local) = Harness::with_local().await;
        advance(HEARTBEAT_TIMEOUT).await;
        assert_eq!(status_of(&harness, &local), StatusKind::Reconnecting);

        harness.manager.reconnect_now(&local);
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;

        assert_eq!(status_of(&harness, &local), StatusKind::Connected);
    }

    #[tokio::test(start_paused = true)]
    async fn reconnect_from_error_state_starts_reconnecting() {
        let (harness, local) = Harness::with_local().await;
        advance(HEARTBEAT_TIMEOUT).await;
        harness.manager.cancel_reconnect(&local);
        assert_eq!(status_of(&harness, &local), StatusKind::Error);

        harness.manager.reconnect(&local);

        assert_eq!(status_of(&harness, &local), StatusKind::Reconnecting);
    }

    #[tokio::test(start_paused = true)]
    async fn re_wires_the_transport_drop_handler_on_the_new_client_after_reconnect_local() {
        let (harness, local) = Harness::with_local().await;
        flush().await;
        let first = daemon_of(&harness, &local);
        assert!(harness.dialer.has_watch(&first));

        // Drop #1 on the original daemon connection.
        harness.dialer.fail_watch(&first, "gone");
        flush().await;
        assert_eq!(status_of(&harness, &local), StatusKind::Reconnecting);

        // A successful reconnect builds a fresh generation...
        advance(Duration::from_secs(1)).await;
        assert_eq!(status_of(&harness, &local), StatusKind::Connected);
        let second = daemon_of(&harness, &local);
        assert_ne!(second, first);
        // ...with its own watch,
        assert!(harness.dialer.has_watch(&second));

        // so drop #2 is detected promptly (no 50s heartbeat wait).
        harness.dialer.fail_watch(&second, "gone again");
        flush().await;
        assert_eq!(status_of(&harness, &local), StatusKind::Reconnecting);
    }

    #[tokio::test(start_paused = true)]
    async fn re_wires_transport_tunnel_drop_handlers_on_the_new_instances_after_reconnect_remote() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        flush().await;
        let first = daemon_of(&harness, &remote_id());

        harness.dialer.fail_watch(&first, "gone");
        flush().await;
        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Reconnecting);

        advance(Duration::from_secs(1)).await;
        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Connected);
        let second = daemon_of(&harness, &remote_id());
        assert_ne!(second, first);
        assert_ne!(
            second.socket, first.socket,
            "each tunnel gets its own socket"
        );
        assert!(harness.dialer.has_watch(&second));

        harness.dialer.fail_watch(&second, "gone again");
        flush().await;
        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Reconnecting);

        // Reconnect again, then the rebuilt tunnel's drop is detected too.
        advance(Duration::from_secs(1)).await;
        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Connected);
        harness.latest(SpawnKind::Tunnel).exit(255);
        flush().await;
        assert_eq!(status_of(&harness, &remote_id()), StatusKind::Reconnecting);
    }

    #[tokio::test(start_paused = true)]
    async fn a_disconnect_mid_attempt_does_not_resurrect_the_connection() {
        let (mut harness, local) = Harness::with_local().await;
        let release = harness.dialer.gate_next();

        // Heartbeat timeout starts the loop; the first attempt blocks while dialing.
        advance(HEARTBEAT_TIMEOUT).await;
        advance(Duration::from_secs(1)).await;
        assert_eq!(status_of(&harness, &local), StatusKind::Reconnecting);
        let watches = harness.dialer.watch_listeners().len();

        // The session is removed while that attempt is still in flight.
        harness.manager.disconnect(&local);
        assert!(harness.manager.connection(&local).is_none());

        // The attempt was dropped with the connection: nobody waits on the gate any more.
        flush().await;
        assert!(release.send(()).is_err());
        advance(Duration::from_secs(60)).await;

        assert!(harness.manager.connection(&local).is_none());
        // No heartbeat is armed, so nothing schedules another attempt.
        assert_eq!(harness.dialer.watch_listeners().len(), watches);
        assert_eq!(harness.dialer.dials().len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn failed_reconnect_attempt_schedules_retry_with_backoff() {
        let (harness, local) = Harness::with_local().await;
        harness.dialer.fail_always("connection refused");
        advance(HEARTBEAT_TIMEOUT).await;
        assert_eq!(status_of(&harness, &local), StatusKind::Reconnecting);

        // First attempt after 1s — fails.
        advance(Duration::from_secs(1)).await;
        assert_eq!(status_of(&harness, &local), StatusKind::Reconnecting);
        assert_eq!(attempt_of(&harness, &local), 1);
        assert_eq!(error_of(&harness, &local), "connection refused");

        // Then 2s, 4s, 8s.
        for (wait, attempt) in [(2, 2), (4, 3), (8, 4)] {
            advance(Duration::from_secs(wait) - Duration::from_millis(10)).await;
            assert_eq!(attempt_of(&harness, &local), attempt - 1);
            advance(Duration::from_millis(10)).await;
            assert_eq!(attempt_of(&harness, &local), attempt);
        }

        harness.dialer.stop_failing();
        advance(Duration::from_secs(16)).await;
        assert_eq!(status_of(&harness, &local), StatusKind::Connected);
    }
}

mod daemon_output_buffer {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn trims_buffer_when_it_exceeds_500_lines() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        for _ in 0..300 {
            harness.manager.force_reconnect(&remote_id()).unwrap();
            harness.manager.reconnect_now(&remote_id());
            flush().await;
        }
        let watch = harness
            .manager
            .watch_output(&remote_id(), OutputKind::Daemon)
            .unwrap();
        assert!(watch.initial.len() <= 500);
        assert!(watch.initial.len() >= 250);
    }
}

mod watch_daemon_output {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn returns_empty_scrollback_for_missing_connection() {
        let (harness, _) = Harness::with_local().await;
        assert!(
            harness
                .manager
                .watch_output(&ConnectionId::new("nonexistent"), OutputKind::Daemon)
                .is_err()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn returns_scrollback_for_existing_connection() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        let watch = harness
            .manager
            .watch_output(&remote_id(), OutputKind::Daemon)
            .unwrap();
        assert_eq!(
            watch.initial,
            vec!["Connecting to daemon via gRPC...", "Connected to daemon"]
        );
    }
}

mod watch_tunnel_output {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn returns_empty_scrollback_for_missing_connection() {
        let (harness, _) = Harness::with_local().await;
        assert!(
            harness
                .manager
                .watch_output(&ConnectionId::new("nonexistent"), OutputKind::Tunnel)
                .is_err()
        );
    }
}

mod get_client_edge_cases {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn throws_when_connection_exists_but_is_not_connected() {
        let (harness, local) = Harness::with_local().await;
        advance(HEARTBEAT_TIMEOUT).await;
        assert_eq!(status_of(&harness, &local), StatusKind::Reconnecting);
        let error = harness.manager.client(&local).unwrap_err().to_string();
        assert!(error.contains("is reconnecting"), "{error}");
    }

    #[tokio::test(start_paused = true)]
    async fn includes_error_message_in_thrown_error_when_present() {
        let (harness, local) = Harness::with_local().await;
        advance(HEARTBEAT_TIMEOUT).await;
        assert_eq!(
            harness.manager.client(&local).unwrap_err().to_string(),
            format!(
                "Connection {local} is reconnecting: Connection lost (no heartbeat from daemon)"
            )
        );
    }
}

mod connect_remote_daemon_phase_failure {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn reports_daemon_phase_error_with_ssh_tunnel_ok_prefix() {
        let (mut harness, _) = Harness::with_local().await;
        harness.dialer.fail_next("daemon not running");
        let config = SshConnectionConfig {
            id: "daemon-fail".to_owned(),
            ..remote_config()
        };
        let info = harness
            .manager
            .connect_remote(config, OutdatedDaemon::Report)
            .await
            .unwrap();
        assert_eq!(kind(&info), StatusKind::Error);
        let error = info.status.error().unwrap().to_string();
        assert!(error.contains("SSH tunnel OK"), "{error}");
        assert!(error.contains("daemon not running"), "{error}");
        let log = harness
            .manager
            .watch_output(&ConnectionId::new("daemon-fail"), OutputKind::Daemon)
            .unwrap()
            .initial;
        assert_eq!(log.last().unwrap(), "Failed: daemon not running");
    }
}

mod port_forward_callbacks {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn forwards_output_events_to_watchers() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        harness
            .manager
            .add_port_forward(pf_config("pf-cb", 9090))
            .unwrap();
        flush().await;
        let mut watch = harness
            .manager
            .watch_port_forward_output(&PortForwardId::new("pf-cb"))
            .unwrap();

        harness
            .latest(SpawnKind::PortForward)
            .stdout("port forward line\n");
        flush().await;

        assert_eq!(
            watch.updates.drain_ready(),
            vec!["[portfwd] port forward line"]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn forwards_status_events_to_watchers() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        harness
            .manager
            .add_port_forward(pf_config("pf-status", 9091))
            .unwrap();
        let mut watch = harness
            .manager
            .watch_port_forward_status(&PortForwardId::new("pf-status"))
            .unwrap();

        advance(Duration::from_secs(2)).await;
        harness.latest(SpawnKind::PortForward).exit(1);
        flush().await;

        let statuses: Vec<_> = watch
            .updates
            .drain_ready()
            .into_iter()
            .map(|info| info.status)
            .collect();
        assert_eq!(statuses[0], PortForwardStatus::Active);
        assert_eq!(statuses[1].as_str(), "error");
    }

    #[tokio::test(start_paused = true)]
    async fn unsubscribes_output_listeners() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        harness
            .manager
            .add_port_forward(pf_config("pf-1", 8080))
            .unwrap();
        flush().await;
        let watch = harness
            .manager
            .watch_port_forward_output(&PortForwardId::new("pf-1"))
            .unwrap();
        drop(watch.updates);
        harness.latest(SpawnKind::PortForward).stdout("hello\n");
        flush().await;
        assert!(
            harness
                .manager
                .watch_port_forward_output(&PortForwardId::new("pf-1"))
                .unwrap()
                .initial
                .contains(&"[portfwd] hello".to_owned())
        );
    }

    #[tokio::test(start_paused = true)]
    async fn unsubscribes_status_listeners() {
        let (mut harness, _) = Harness::with_local().await;
        harness.connect_remote().await;
        harness
            .manager
            .add_port_forward(pf_config("pf-1", 8080))
            .unwrap();
        let watch = harness
            .manager
            .watch_port_forward_status(&PortForwardId::new("pf-1"))
            .unwrap();
        drop(watch.updates);
        advance(Duration::from_secs(2)).await;
        assert_eq!(
            harness.manager.list_port_forwards(&remote_id())[0].status,
            PortForwardStatus::Active
        );
    }
}

mod force_reconnect {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn from_connected_state_enters_reconnecting() {
        let (harness, local) = Harness::with_local().await;
        assert_eq!(status_of(&harness, &local), StatusKind::Connected);
        harness.manager.force_reconnect(&local).unwrap();
        assert_eq!(status_of(&harness, &local), StatusKind::Reconnecting);
        assert_eq!(error_of(&harness, &local), "Reconnecting...");
    }

    #[tokio::test(start_paused = true)]
    async fn throws_for_unknown_connection() {
        let (harness, _) = Harness::with_local().await;
        assert!(
            harness
                .manager
                .force_reconnect(&ConnectionId::new("nonexistent"))
                .unwrap_err()
                .to_string()
                .starts_with("Connection not found")
        );
    }

    #[tokio::test(start_paused = true)]
    async fn is_no_op_when_already_reconnecting() {
        let (harness, local) = Harness::with_local().await;
        advance(HEARTBEAT_TIMEOUT).await;
        assert_eq!(status_of(&harness, &local), StatusKind::Reconnecting);
        harness.manager.force_reconnect(&local).unwrap();
        assert_eq!(status_of(&harness, &local), StatusKind::Reconnecting);
        assert_eq!(
            error_of(&harness, &local),
            "Connection lost (no heartbeat from daemon)"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn dropping_the_manager_kills_its_ssh_processes() {
    let (mut harness, _) = Harness::with_local().await;
    harness.connect_remote().await;
    harness
        .manager
        .add_port_forward(pf_config("pf-1", 8080))
        .unwrap();
    flush().await;
    let tunnel = harness.latest(SpawnKind::Tunnel);
    let forward = harness.latest(SpawnKind::PortForward);
    let Harness { manager, .. } = harness;
    drop(manager);
    flush().await;
    assert!(tunnel.is_killed());
    assert!(forward.is_killed());
}
