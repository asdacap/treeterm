//! The core daemon client against a real in-process daemon.

use futures::StreamExt;
use treeterm_core::daemon::DaemonConnection;
use treeterm_core::local_daemon::{DaemonStart, LocalDaemonError, LocalDaemonSpec, ensure_running};
use treeterm_core::rt::Rt;
use treeterm_core::transport::ConnectError;
use treeterm_proto::treeterm::file_watch_event::State;
use treeterm_testkit::TestDaemon;

fn rt() -> Rt {
    Rt::new(tokio::runtime::Handle::current())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_watch_starts_with_the_current_session() {
    let daemon = TestDaemon::start();
    let conn = DaemonConnection::open(&rt(), daemon.socket_path()).await.unwrap().unwrap();

    let mut watch = conn.watch_session("listener-1");
    let first = watch.next().await.unwrap().unwrap();
    let session = first.session.unwrap();
    assert_eq!(session.version, 1);
    assert!(session.workspace_refs.is_empty());
    assert_eq!(session.workspace_data_dir, daemon.workspaces_dir().to_string_lossy());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_watch_reports_present_then_absent_files() {
    let daemon = TestDaemon::start();
    std::fs::write(daemon.workspaces_dir().join("ws-1.json"), b"{}").unwrap();
    let conn = DaemonConnection::open(&rt(), daemon.socket_path()).await.unwrap().unwrap();
    let scope = daemon.workspaces_dir().to_string_lossy().into_owned();

    let mut present = conn.watch_file(&scope, "ws-1.json");
    match present.next().await.unwrap().unwrap().state.unwrap() {
        State::Present(file) => {
            assert_eq!(file.content, b"{}");
            assert_eq!(file.sha256.len(), 64);
        }
        State::Absent(_) => panic!("expected the file to be present"),
    }

    let mut absent = conn.watch_file(&scope, "missing.json");
    assert!(matches!(
        absent.next().await.unwrap().unwrap().state.unwrap(),
        State::Absent(_)
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn opening_a_missing_socket_fails_loudly() {
    let missing = std::path::Path::new("/tmp/treeterm-no-such-daemon.sock");
    let result = DaemonConnection::open(&rt(), missing).await.unwrap();
    assert!(matches!(result, Err(ConnectError::SocketMissing(_))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ensure_running_reuses_a_live_daemon() {
    let daemon = TestDaemon::start();
    let spec = LocalDaemonSpec {
        socket_path: daemon.socket_path().to_owned(),
        binary: daemon.root().join("never-spawned"),
        log_path: daemon.root().join("daemon.log"),
        pid_file: daemon.root().join("daemon.pid"),
    };
    assert_eq!(ensure_running(&spec).await.unwrap(), DaemonStart::AlreadyRunning);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ensure_running_clears_stale_state_before_spawning() {
    let dir = tempfile::Builder::new().prefix("tt").tempdir_in("/tmp").unwrap();
    let spec = LocalDaemonSpec {
        socket_path: dir.path().join("d.sock"),
        binary: dir.path().join("no-such-daemon"),
        log_path: dir.path().join("daemon.log"),
        pid_file: dir.path().join("daemon.pid"),
    };
    std::fs::write(&spec.pid_file, "999999").unwrap();
    std::fs::write(&spec.socket_path, "").unwrap();
    let result = ensure_running(&spec).await;
    assert!(matches!(result, Err(LocalDaemonError::BinaryMissing(_))));
    assert!(!spec.pid_file.exists());
    assert!(!spec.socket_path.exists());
}
