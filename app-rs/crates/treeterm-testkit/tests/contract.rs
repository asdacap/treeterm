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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn body_writes_are_compare_and_swap() {
    use treeterm_core::wire::stable_json::sha256_hex;
    use treeterm_core::workspace_io::{BodyWrite, Expected};
    let daemon = TestDaemon::start();
    let conn = DaemonConnection::open(&rt(), daemon.socket_path()).await.unwrap().unwrap();
    let dir = daemon.workspaces_dir().to_string_lossy().into_owned();

    let first = conn.write_body(&dir, "w.json", "one".into(), Expected::Absent).await.unwrap().unwrap();
    assert_eq!(first, BodyWrite::Written);
    let again = conn.write_body(&dir, "w.json", "two".into(), Expected::Absent).await.unwrap().unwrap();
    assert_eq!(again, BodyWrite::Conflict);
    let stale = conn
        .write_body(&dir, "w.json", "two".into(), Expected::Sha(sha256_hex("zzz")))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stale, BodyWrite::Conflict);
    let next = conn
        .write_body(&dir, "w.json", "two".into(), Expected::Sha(sha256_hex("one")))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next, BodyWrite::Written);
    assert_eq!(std::fs::read_to_string(daemon.workspaces_dir().join("w.json")).unwrap(), "two");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_session_lock_is_held_per_connection() {
    use std::time::Duration;
    use treeterm_core::daemon::SessionLockResult;
    let daemon = TestDaemon::start();
    let a = DaemonConnection::open(&rt(), daemon.socket_path()).await.unwrap().unwrap();
    let b = DaemonConnection::open(&rt(), daemon.socket_path()).await.unwrap().unwrap();

    let locked = a.lock_session(Duration::from_secs(60)).await.unwrap().unwrap();
    assert!(matches!(locked, SessionLockResult::Acquired(ref s) if s.lock.is_some()));
    let other = b.lock_session(Duration::from_secs(60)).await.unwrap().unwrap();
    assert!(matches!(other, SessionLockResult::HeldElsewhere(_)));
    // Another connection's update is rejected while the lock is held.
    let rejected = b.update_session(vec![], None, "b").await.unwrap().unwrap();
    assert_eq!(rejected.version, match locked { SessionLockResult::Acquired(s) => s.version, _ => unreachable!() });

    let unlocked = a.unlock_session().await.unwrap().unwrap();
    assert!(unlocked.lock.is_none());
    assert!(matches!(
        b.lock_session(Duration::from_secs(60)).await.unwrap().unwrap(),
        SessionLockResult::Acquired(_)
    ));
    assert!(b.force_unlock_session().await.unwrap().unwrap().lock.is_none());
}
