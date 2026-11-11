//! SessionStore against a real in-process daemon.

use std::collections::HashSet;
use std::time::Duration;

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use treeterm_core::daemon::DaemonConnection;
use treeterm_core::rt::Rt;
use treeterm_model::session_store::{SessionState, SessionStore, WorkspaceEntry};
use treeterm_proto::treeterm::WorkspaceRef;
use treeterm_testkit::TestDaemon;

struct Harness {
    _tokio: tokio::runtime::Runtime,
    daemon: TestDaemon,
    conn: DaemonConnection,
}

fn harness() -> Harness {
    let tokio = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let daemon = {
        let _enter = tokio.enter();
        TestDaemon::start()
    };
    let rt = Rt::new(tokio.handle().clone());
    let conn = futures::executor::block_on(DaemonConnection::open(&rt, daemon.socket_path()))
        .unwrap()
        .unwrap();
    Harness { _tokio: tokio, daemon, conn }
}

fn body(name: &str, parent: Option<&str>) -> String {
    let mut json = serde_json::json!({
        "name": name, "status": "active", "isGitRepo": true, "isWorktree": parent.is_some(),
        "appStates": {}, "metadata": {}, "createdAt": 1, "lastActivity": 1, "parentHash": ""
    });
    if let Some(parent) = parent {
        json["parentId"] = parent.into();
    }
    json.to_string()
}

fn publish(h: &Harness, ids: &[&str]) {
    let refs = ids
        .iter()
        .map(|id| WorkspaceRef { id: (*id).into(), path: format!("/w/{id}") })
        .collect();
    futures::executor::block_on(h.conn.update_session(refs, None, "test-writer"))
        .unwrap()
        .unwrap();
}

/// Daemon events arrive from tokio threads in real time; pump the GPUI test scheduler
/// until `done` holds.
fn wait_until(cx: &mut TestAppContext, store: &Entity<SessionStore>, done: impl Fn(&SessionStore) -> bool) {
    for _ in 0..500 {
        cx.run_until_parked();
        if store.read_with(cx, |s, _| done(s)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("condition not reached");
}

#[gpui_kit::test]
fn mirrors_refs_and_loads_workspace_bodies(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("root.json"), body("root", None)).unwrap();
    std::fs::write(h.daemon.workspaces_dir().join("child.json"), body("child", Some("root"))).unwrap();
    publish(&h, &["child", "root", "broken"]);

    let conn = h.conn.clone();
    let store = cx.new(|cx| SessionStore::new(conn, "client-a".into(), cx));
    wait_until(cx, &store, |s| {
        matches!(s.state(), SessionState::Live(_))
            && matches!(s.workspace("root"), WorkspaceEntry::Loaded(_))
            && matches!(s.workspace("child"), WorkspaceEntry::Loaded(_))
            && matches!(s.workspace("broken"), WorkspaceEntry::Error(_))
    });

    store.read_with(cx, |s, _| {
        let rows: Vec<(String, usize)> = s
            .tree_rows(&HashSet::new())
            .into_iter()
            .map(|r| (r.id, r.depth))
            .collect();
        assert_eq!(
            rows,
            vec![("root".into(), 0), ("child".into(), 1), ("broken".into(), 0)]
        );
        match s.workspace("broken") {
            WorkspaceEntry::Error(reason) => assert!(reason.contains("missing"), "{reason}"),
            WorkspaceEntry::Loading | WorkspaceEntry::Loaded(_) => unreachable!(),
        }
    });
}

#[gpui_kit::test]
fn follows_ref_removal_and_body_changes(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let path = h.daemon.workspaces_dir().join("a.json");
    std::fs::write(&path, body("first", None)).unwrap();
    publish(&h, &["a", "b"]);

    let conn = h.conn.clone();
    let store = cx.new(|cx| SessionStore::new(conn, "client-a".into(), cx));
    wait_until(cx, &store, |s| {
        matches!(s.state(), SessionState::Live(_)) && matches!(s.workspace("a"), WorkspaceEntry::Loaded(_))
    });

    std::fs::write(&path, body("second", None)).unwrap();
    wait_until(cx, &store, |s| match s.workspace("a") {
        WorkspaceEntry::Loaded(file) => file.name == "second",
        WorkspaceEntry::Loading | WorkspaceEntry::Error(_) => false,
    });

    publish(&h, &["a"]);
    wait_until(cx, &store, |s| s.tree_rows(&HashSet::new()).len() == 1);
}
