//! The connection manager against a real in-process daemon.
//!
//! Transport drops are simulated with a Unix-socket proxy between the client and the daemon:
//! cutting it closes every proxied connection, as a crashed daemon or a dead tunnel would.
//! Remote connections run against a fake ssh (no ssh in tests) whose "tunnel" is such a
//! proxy from the forwarded local socket to the daemon.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinSet;
use treeterm_core::connection::dialer::GrpcDialer;
use treeterm_core::connection::fake::{FakeResponse, FakeSpawner};
use treeterm_core::connection::output::Updates;
use treeterm_core::connection::process::{ProcessCommand, TokioSpawner};
use treeterm_core::connection::ssh::DaemonBinaries;
use treeterm_core::connection::{
    ConnectionInfo, ConnectionManager, ConnectionStatus, OutdatedDaemon, OutputKind,
    SshEnvironment, StatusKind,
};
use treeterm_core::daemon::DaemonConnection;
use treeterm_core::local_daemon::LocalDaemonSpec;
use treeterm_core::pty::CreatePty;
use treeterm_core::rt::Rt;
use treeterm_core::settings::{ConnectionLabel, Extra, IdentityFile, SshConnectionConfig};
use treeterm_testkit::TestDaemon;

fn rt() -> Rt {
    Rt::new(tokio::runtime::Handle::current())
}

/// Forwards connections on `listen` to `target` until dropped; dropping it cuts them all.
struct Proxy {
    task: tokio::task::JoinHandle<()>,
}

impl Proxy {
    fn start(listen: &Path, target: &Path) -> Self {
        match std::fs::remove_file(listen) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("cannot clear {listen:?}: {error}"),
        }
        let listener = UnixListener::bind(listen).expect("bind proxy");
        let target = target.to_owned();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                let (mut inbound, _) = listener.accept().await.expect("proxy accept");
                let target = target.clone();
                connections.spawn(async move {
                    let mut outbound = UnixStream::connect(&target).await.expect("proxy dial");
                    // Ends when either side closes; nothing to report then.
                    if tokio::io::copy_bidirectional(&mut inbound, &mut outbound)
                        .await
                        .is_err()
                    {}
                });
            }
        });
        Self { task }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn ssh_env(dir: &Path) -> SshEnvironment {
    SshEnvironment {
        daemon_binaries: DaemonBinaries::new(dir.to_owned()),
        forward_socket_dir: dir.join("s"),
    }
}

/// The next status of `id` that satisfies `done`, failing after 10 s.
async fn wait_for<C: Clone>(
    updates: &mut Updates<ConnectionInfo<C>>,
    mut done: impl FnMut(&ConnectionInfo<C>) -> bool,
) -> ConnectionInfo<C> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let info = updates.next().await.expect("status stream ended");
            if done(&info) {
                return info;
            }
        }
    })
    .await
    .expect("timed out waiting for a status")
}

fn connected_daemon(info: ConnectionInfo<DaemonConnection>) -> DaemonConnection {
    match info.status {
        ConnectionStatus::Connected { daemon } => daemon,
        other => panic!("not connected: {}", other.kind()),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_connection_serves_a_real_daemon_and_survives_a_dropped_transport() {
    let daemon = TestDaemon::start();
    let dir = tempfile::Builder::new()
        .prefix("tt")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = dir.path().join("p.sock");
    let proxy = Proxy::start(&socket, daemon.socket_path());
    let spec = LocalDaemonSpec {
        socket_path: socket.clone(),
        binary: dir.path().join("never-spawned"),
        log_path: dir.path().join("daemon.log"),
        pid_file: dir.path().join("daemon.pid"),
    };
    let mut manager = ConnectionManager::new(
        rt(),
        GrpcDialer::new(rt(), spec),
        Arc::new(TokioSpawner),
        ssh_env(dir.path()),
    );
    let mut changes = manager.status_changes();

    let first = connected_daemon(manager.connect_local().await.unwrap());
    let id = manager.list_connections()[0].id.clone();
    let pty = first
        .create_pty(CreatePty {
            cwd: std::env::temp_dir().to_string_lossy().into_owned(),
            ..CreatePty::default()
        })
        .await
        .unwrap()
        .unwrap();

    // The daemon goes away under the connection.
    drop(proxy);
    let lost = wait_for(&mut changes, |info| {
        info.status.kind() == StatusKind::Reconnecting
    })
    .await;
    let error = lost.status.error().unwrap().to_string();
    assert!(error.starts_with("gRPC connection lost: "), "{error}");

    // Attempts fail while it is gone (nothing to start: the binary does not exist)...
    let failed = wait_for(&mut changes, |info| {
        matches!(
            info.status,
            ConnectionStatus::Reconnecting { attempt: 1, .. }
        ) && info
            .status
            .error()
            .unwrap()
            .to_string()
            .starts_with("daemon executable not found")
    })
    .await;
    assert_eq!(failed.id, id);

    // ...and succeed once it is back, on a new generation that sees the same PTYs.
    let _proxy = Proxy::start(&socket, daemon.socket_path());
    manager.reconnect_now(&id);
    let second = connected_daemon(
        wait_for(&mut changes, |info| {
            info.status.kind() == StatusKind::Connected
        })
        .await,
    );
    let sessions = second.list_pty_sessions().await.unwrap().unwrap();
    assert!(
        sessions.iter().any(|session| session.id == pty),
        "{sessions:?}"
    );

    manager.disconnect(&id);
    assert_eq!(
        wait_for(&mut changes, |_| true).await.status.kind(),
        StatusKind::Disconnected
    );
    // Disconnecting is not killing: the PTY lives on in the daemon.
    let direct = DaemonConnection::open(&rt(), daemon.socket_path())
        .await
        .unwrap()
        .unwrap();
    let sessions = direct.list_pty_sessions().await.unwrap().unwrap();
    assert!(sessions.iter().any(|session| session.id == pty));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_reconnect_hands_out_a_new_generation() {
    let daemon = TestDaemon::start();
    let dir = tempfile::Builder::new()
        .prefix("tt")
        .tempdir_in("/tmp")
        .unwrap();
    let spec = LocalDaemonSpec {
        socket_path: daemon.socket_path().to_owned(),
        binary: dir.path().join("never-spawned"),
        log_path: dir.path().join("daemon.log"),
        pid_file: dir.path().join("daemon.pid"),
    };
    let mut manager = ConnectionManager::new(
        rt(),
        GrpcDialer::new(rt(), spec),
        Arc::new(TokioSpawner),
        ssh_env(dir.path()),
    );
    let first = manager.connect_local().await.unwrap();
    let mut watch = manager.watch_connection_status(&first.id).unwrap();

    manager.force_reconnect(&first.id).unwrap();

    assert_eq!(
        wait_for(&mut watch.updates, |_| true).await.status.kind(),
        StatusKind::Reconnecting
    );
    let second = connected_daemon(
        wait_for(&mut watch.updates, |info| {
            info.status.kind() == StatusKind::Connected
        })
        .await,
    );
    second.list_pty_sessions().await.unwrap().unwrap();
    // Heartbeats keep it connected (the daemon re-broadcasts every 15 s; the first event
    // arrives at once).
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        manager.connection(&first.id).unwrap().status.kind(),
        StatusKind::Connected
    );
}

/// A fake ssh host whose daemon is `daemon` and whose tunnels are proxies to it.
fn fake_ssh_host(daemon_socket: PathBuf, tunnels: Arc<Mutex<Vec<Proxy>>>) -> FakeSpawner {
    FakeSpawner::with_responder(move |command: &ProcessCommand| {
        let script = command.args.last().cloned().unwrap_or_default();
        if script.contains("TREETERM_OS") {
            return FakeResponse::stdout(format!(
                "TREETERM_OS:Linux\nTREETERM_ARCH:x86_64\nTREETERM_HOME:/home/test\nTREETERM_REMOTE_HASH:NONE\nTREETERM_SOCKET:{}\n",
                daemon_socket.display()
            ));
        }
        let forward = command
            .args
            .iter()
            .skip_while(|arg| *arg != "-L")
            .nth(1)
            .expect("only bootstraps and tunnels are expected");
        let (local, remote) = forward.split_once(':').unwrap();
        tunnels
            .lock()
            .unwrap()
            .push(Proxy::start(Path::new(local), Path::new(remote)));
        FakeResponse::Run
    })
}

fn remote_config() -> SshConnectionConfig {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_connection_reaches_the_daemon_through_its_tunnel_and_rebuilds_it() {
    let daemon = TestDaemon::start();
    let dir = tempfile::Builder::new()
        .prefix("tt")
        .tempdir_in("/tmp")
        .unwrap();
    let tunnels = Arc::new(Mutex::new(Vec::new()));
    let spawner = fake_ssh_host(daemon.socket_path().to_owned(), tunnels.clone());
    let spec = LocalDaemonSpec {
        socket_path: dir.path().join("unused.sock"),
        binary: dir.path().join("never-spawned"),
        log_path: dir.path().join("daemon.log"),
        pid_file: dir.path().join("daemon.pid"),
    };
    let mut manager = ConnectionManager::new(
        rt(),
        GrpcDialer::new(rt(), spec),
        Arc::new(spawner.clone()),
        ssh_env(dir.path()),
    );
    let mut changes = manager.status_changes();

    let info = manager
        .connect_remote(remote_config(), OutdatedDaemon::Report)
        .await
        .unwrap();
    let first = connected_daemon(info.clone());
    assert!(first.socket_path().starts_with(dir.path().join("s")));
    first.list_pty_sessions().await.unwrap().unwrap();
    let daemon_log = manager
        .watch_output(&info.id, OutputKind::Daemon)
        .unwrap()
        .initial;
    assert_eq!(
        daemon_log,
        vec!["Connecting to daemon via gRPC...", "Connected to daemon"]
    );

    // The tunnel dies: the connection notices without waiting for the heartbeat timeout.
    tunnels.lock().unwrap().clear();
    let lost = wait_for(&mut changes, |info| {
        info.status.kind() == StatusKind::Reconnecting
    })
    .await;
    assert!(
        lost.status
            .error()
            .unwrap()
            .to_string()
            .starts_with("gRPC connection lost"),
        "{:?}",
        lost.status.error()
    );
    let tunnel = spawner
        .processes()
        .into_iter()
        .find(|process| process.args().iter().any(|arg| arg == "-L"))
        .unwrap();
    // The status is reported before the driver drops the dead generation.
    for _ in 0..100 {
        if tunnel.is_killed() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(tunnel.is_killed(), "the dead generation's ssh is killed");

    let second = connected_daemon(
        wait_for(&mut changes, |info| {
            info.status.kind() == StatusKind::Connected
        })
        .await,
    );
    assert_ne!(second.socket_path(), first.socket_path());
    second.list_pty_sessions().await.unwrap().unwrap();

    manager.disconnect(&info.id);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !second.socket_path().exists(),
        "the tunnel socket is removed"
    );
}
