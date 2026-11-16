//! A lost connection and the reconnect (`handleReconnected`): the session store is kept, its
//! body writes deferred, and on the next generation it writes them before it is replaced.

use std::collections::HashSet;

use gpui_kit::{AppContext as _, Entity, Task, TestAppContext};
use treeterm_core::daemon::DaemonConnection;
use treeterm_core::local_daemon::LocalDaemonSpec;
use treeterm_model::app_store::{AppStore, ConnState, SessionSlot};
use treeterm_model::session_store::{ConnectionStatus, SessionStore, WorkspaceEntry};
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::workspace_store::WorkspaceStore;
use treeterm_proto::treeterm::WorkspaceRef;

mod common;
use common::{Harness, Proxy, harness, recv, wait_until, wait_until_app};

fn body(name: &str) -> String {
    serde_json::json!({
        "name": name, "status": "active", "isGitRepo": false, "isWorktree": false,
        "appStates": {}, "metadata": {}, "createdAt": 1, "lastActivity": 1, "parentHash": ""
    })
    .to_string()
}

/// One workspace, `a`, on disk and in the session.
fn publish_a(h: &Harness) {
    std::fs::write(h.daemon.workspaces_dir().join("a.json"), body("a")).unwrap();
    let path = h.daemon.root().to_string_lossy().into_owned();
    let refs = vec![WorkspaceRef {
        id: "a".into(),
        path,
    }];
    futures::executor::block_on(h.conn.update_session(refs, None, "test-writer"))
        .unwrap()
        .unwrap();
}

fn open_store(cx: &mut TestAppContext, conn: &DaemonConnection) -> Entity<SessionStore> {
    let conn = conn.clone();
    let settings = cx.new(|_| SettingsStore::in_memory(Default::default()));
    cx.new(|cx| SessionStore::new(conn, "client-a".to_owned(), settings, None, cx))
}

fn loaded(s: &SessionStore) -> bool {
    matches!(s.entry("a"), Some(WorkspaceEntry::Loaded(_)))
}

fn ws(s: &SessionStore) -> Entity<WorkspaceStore> {
    s.workspace_store("a").expect("workspace loaded").clone()
}

fn display_name_on_disk(h: &Harness) -> Option<String> {
    let bytes = std::fs::read(h.daemon.workspaces_dir().join("a.json")).unwrap();
    let file: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    file["metadata"]["displayName"].as_str().map(str::to_owned)
}

fn rename(cx: &mut TestAppContext, store: &Entity<SessionStore>) {
    let workspace = store.read_with(cx, |s, _| ws(s));
    workspace.update(cx, |w, cx| w.update_metadata("displayName", "Renamed", cx));
    cx.run_until_parked();
}

/// Hand `store` over to `conn` and pump the scheduler until the hand-over is done.
fn hand_over(cx: &mut TestAppContext, store: &Entity<SessionStore>, conn: DaemonConnection) {
    let task: Task<()> = store.update(cx, |s, cx| s.hand_over(conn, cx));
    let (done, finished) = futures::channel::oneshot::channel();
    cx.update(|cx| {
        cx.spawn(async move |_| {
            task.await;
            done.send(()).expect("the test waits for it");
        })
        .detach();
    });
    recv(cx, finished);
}

/// A connection to the daemon whose transport is already gone.
fn dead_connection(h: &Harness, dir: &std::path::Path) -> DaemonConnection {
    let socket = dir.join("p.sock");
    let proxy = Proxy::start(&socket, h.daemon.socket_path());
    let conn = futures::executor::block_on(DaemonConnection::open(&h.rt, &socket))
        .unwrap()
        .unwrap();
    drop(proxy);
    for _ in 0..500 {
        if futures::executor::block_on(conn.list_pty_sessions())
            .unwrap()
            .is_err()
        {
            return conn;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the connection did not go down");
}

#[gpui_kit::test]
fn a_body_changed_while_disconnected_is_written_by_the_hand_over(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish_a(&h);
    let store = open_store(cx, &h.conn);
    wait_until(cx, &store, loaded);

    store.update(cx, |s, cx| {
        s.set_connection_status(ConnectionStatus::Disconnected, cx)
    });
    rename(cx, &store);
    std::thread::sleep(std::time::Duration::from_millis(100));
    cx.run_until_parked();
    assert_eq!(
        display_name_on_disk(&h),
        None,
        "deferred while disconnected"
    );
    assert!(
        store.read_with(cx, |s, _| loaded(s)),
        "no error on the workspace"
    );

    let next = futures::executor::block_on(DaemonConnection::open(&h.rt, h.daemon.socket_path()))
        .unwrap()
        .unwrap();
    hand_over(cx, &store, next.clone());
    assert_eq!(display_name_on_disk(&h).as_deref(), Some("Renamed"));

    // The store rebuilt on the new generation reads it from disk.
    let rebuilt = open_store(cx, &next);
    wait_until_app(cx, &rebuilt, |s, cx| {
        s.workspace_store("a").is_some_and(|w| {
            w.read(cx)
                .file()
                .metadata
                .get("displayName")
                .map(String::as_str)
                == Some("Renamed")
        })
    });
}

#[gpui_kit::test]
fn the_hand_over_resolves_at_once_when_nothing_is_dirty(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish_a(&h);
    let store = open_store(cx, &h.conn);
    wait_until(cx, &store, loaded);
    let before = std::fs::read(h.daemon.workspaces_dir().join("a.json")).unwrap();

    store.update(cx, |s, cx| {
        s.set_connection_status(ConnectionStatus::Disconnected, cx)
    });
    let next = futures::executor::block_on(DaemonConnection::open(&h.rt, h.daemon.socket_path()))
        .unwrap()
        .unwrap();
    hand_over(cx, &store, next);
    assert_eq!(
        std::fs::read(h.daemon.workspaces_dir().join("a.json")).unwrap(),
        before,
        "nothing written"
    );
}

#[gpui_kit::test]
fn an_operation_error_is_dropped_when_the_connection_goes_down(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish_a(&h);
    let store = open_store(cx, &h.conn);
    wait_until(cx, &store, loaded);

    std::fs::remove_file(h.daemon.workspaces_dir().join("a.json")).unwrap();
    wait_until(cx, &store, |s| {
        matches!(s.entry("a"), Some(WorkspaceEntry::OperationError { .. }))
    });
    // The connection's banner owns errors from now on.
    store.update(cx, |s, cx| {
        s.set_connection_status(ConnectionStatus::Disconnected, cx)
    });
    assert!(store.read_with(cx, |s, _| loaded(s)));
}

#[gpui_kit::test]
fn a_write_that_fails_during_the_hand_over_is_written_by_the_next_one(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish_a(&h);
    let store = open_store(cx, &h.conn);
    wait_until(cx, &store, loaded);
    store.update(cx, |s, cx| {
        s.set_connection_status(ConnectionStatus::Disconnected, cx)
    });
    rename(cx, &store);

    // The next generation is lost before the flush gets through: every round fails.
    let dir = tempfile::Builder::new()
        .prefix("tt")
        .tempdir_in("/tmp")
        .unwrap();
    let dead = dead_connection(&h, dir.path());
    hand_over(cx, &store, dead);
    assert_eq!(display_name_on_disk(&h), None);
    let error = store.read_with(cx, |s, _| match s.entry("a") {
        Some(WorkspaceEntry::OperationError { error, .. }) => error.clone(),
        _ => String::new(),
    });
    assert!(error.starts_with("Failed to save workspace: "), "{error}");

    // Lost again: the error goes with the connection, and the body waits for the next one.
    store.update(cx, |s, cx| {
        s.set_connection_status(ConnectionStatus::Disconnected, cx)
    });
    assert!(store.read_with(cx, |s, _| loaded(s)));
    let next = futures::executor::block_on(DaemonConnection::open(&h.rt, h.daemon.socket_path()))
        .unwrap()
        .unwrap();
    hand_over(cx, &store, next);
    assert_eq!(display_name_on_disk(&h).as_deref(), Some("Renamed"));
}

fn local(app: &AppStore) -> &SessionSlot {
    app.sessions().next().expect("the local connection")
}

#[gpui_kit::test]
fn a_reconnect_keeps_the_session_dimmed_and_writes_its_unsaved_bodies_before_the_rebuild(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let h = harness();
    publish_a(&h);
    // The app reaches the daemon through a proxy, so the test can take the daemon away.
    let dir = tempfile::Builder::new()
        .prefix("tt")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = dir.path().join("p.sock");
    let proxy = Proxy::start(&socket, h.daemon.socket_path());
    let spec = LocalDaemonSpec {
        socket_path: socket.clone(),
        binary: dir.path().join("never-spawned"),
        log_path: dir.path().join("daemon.log"),
        pid_file: dir.path().join("daemon.pid"),
    };
    let settings = cx.new(|_| SettingsStore::in_memory(Default::default()));
    let rt = h.rt.clone();
    let app = cx.new(|cx| AppStore::new(rt, spec, settings, cx));
    wait_until_app(cx, &app, |a, cx| {
        local(a).session().is_some_and(|s| loaded(s.read(cx)))
    });
    let (id, first) = app.read_with(cx, |a, _| {
        (local(a).id.clone(), local(a).session().unwrap().clone())
    });

    // The daemon goes away: the session stays on screen, inert, with its workspace.
    drop(proxy);
    wait_until(cx, &app, |a| {
        matches!(local(a).state, ConnState::Reconnecting { .. })
    });
    app.read_with(cx, |a, cx| {
        assert!(local(a).session().is_none());
        assert_eq!(local(a).shown_session(), Some(&first));
        assert_eq!(first.read(cx).tree_rows(&HashSet::new(), cx).len(), 1);
    });
    // An edit made meanwhile is deferred.
    rename(cx, &first);
    assert_eq!(display_name_on_disk(&h), None);
    assert!(
        first.read_with(cx, |s, _| loaded(s)),
        "no error on the workspace"
    );

    // Stopped, it is still kept; retried, it is still kept.
    app.read_with(cx, |a, _| a.cancel_reconnect(&id));
    wait_until(cx, &app, |a| {
        matches!(local(a).state, ConnState::Failed { .. })
    });
    assert_eq!(
        app.read_with(cx, |a, _| local(a).shown_session().cloned()),
        Some(first.clone())
    );
    app.read_with(cx, |a, _| a.reconnect(&id));
    wait_until(cx, &app, |a| {
        matches!(local(a).state, ConnState::Reconnecting { .. })
    });

    // The daemon is back: the next generation gets the edit before its store reads the disk.
    let _proxy = Proxy::start(&socket, h.daemon.socket_path());
    wait_until(cx, &app, |a| local(a).session().is_some());
    let second = app.read_with(cx, |a, _| local(a).session().unwrap().clone());
    assert_ne!(second, first);
    assert_eq!(display_name_on_disk(&h).as_deref(), Some("Renamed"));
    assert_eq!(
        app.read_with(cx, |a, _| local(a).shown_session().cloned()),
        Some(second.clone())
    );
    wait_until_app(cx, &second, |s, cx| {
        s.workspace_store("a").is_some_and(|w| {
            w.read(cx)
                .file()
                .metadata
                .get("displayName")
                .map(String::as_str)
                == Some("Renamed")
        })
    });
    // The kept store is gone: an edit to it is never written.
    let old = first.read_with(cx, |s, _| ws(s));
    old.update(cx, |w, cx| w.update_metadata("displayName", "Ghost", cx));
    std::thread::sleep(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    assert_eq!(display_name_on_disk(&h).as_deref(), Some("Renamed"));
}
