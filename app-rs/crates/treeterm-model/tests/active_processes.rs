//! "Window → Active Processes" against a real in-process daemon: processes are named after
//! the session's workspaces as they are now.

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use treeterm_model::active_processes::workspace_labels;
use treeterm_model::session_store::{SessionStore, WorkspaceEntry};
use treeterm_model::settings_store::SettingsStore;
use treeterm_proto::treeterm::WorkspaceRef;

mod common;
use common::{Harness, harness, wait_until, wait_until_app};

/// A session member: `id`'s body is named `name`, its ref points at `<root>/<id>`.
fn member(h: &Harness, id: &str, name: &str) -> (WorkspaceRef, String) {
    let path = h.daemon.root().join(id);
    std::fs::create_dir_all(&path).unwrap();
    let body = serde_json::json!({
        "name": name, "status": "active", "isGitRepo": false, "isWorktree": false,
        "appStates": {}, "metadata": {}, "createdAt": 1, "lastActivity": 1, "parentHash": ""
    });
    std::fs::write(
        h.daemon.workspaces_dir().join(format!("{id}.json")),
        body.to_string(),
    )
    .unwrap();
    let path = path.to_string_lossy().into_owned();
    (
        WorkspaceRef {
            id: id.to_owned(),
            path: path.clone(),
        },
        path,
    )
}

fn publish(h: &Harness, refs: Vec<WorkspaceRef>) {
    futures::executor::block_on(h.conn.update_session(refs, None, "test-writer"))
        .unwrap()
        .unwrap();
}

fn open_session(cx: &mut TestAppContext, h: &Harness) -> Entity<SessionStore> {
    let conn = h.conn.clone();
    let settings = cx.new(|_| SettingsStore::in_memory(Default::default()));
    let store = cx.new(|cx| SessionStore::new(conn, "client-a".to_owned(), settings, None, cx));
    let github = std::sync::Arc::new(treeterm_core::github::UnavailableGitHub::new(
        "not in tests".to_owned(),
    ));
    store.update(cx, |s, _| s.set_github_api(github));
    store
}

fn labels(cx: &mut TestAppContext, session: &Entity<SessionStore>) -> Vec<(String, String)> {
    session.read_with(cx, workspace_labels)
}

#[gpui_kit::test]
fn processes_are_named_after_the_workspaces_the_session_has_now(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let (a, a_path) = member(&h, "a", "alpha");
    // A member whose body is missing has no name to give.
    let missing = WorkspaceRef {
        id: "missing".to_owned(),
        path: h.daemon.root().join("x").to_string_lossy().into_owned(),
    };
    publish(&h, vec![a.clone(), missing.clone()]);
    let session = open_session(cx, &h);
    wait_until(cx, &session, |s| {
        matches!(s.entry("a"), Some(WorkspaceEntry::Loaded(_)))
            && matches!(s.entry("missing"), Some(WorkspaceEntry::Error { .. }))
    });
    assert_eq!(labels(cx, &session), [(a_path.clone(), "alpha".to_owned())]);

    // A workspace added later names its processes too, in session order.
    let (b, b_path) = member(&h, "b", "beta");
    publish(&h, vec![a, missing, b]);
    wait_until_app(cx, &session, |s, cx| workspace_labels(s, cx).len() == 2);
    let expected = [
        (a_path.clone(), "alpha".to_owned()),
        (b_path, "beta".to_owned()),
    ];
    assert_eq!(labels(cx, &session), expected);

    // A loaded workspace keeps its name while an operation error is shown.
    std::fs::remove_file(h.daemon.workspaces_dir().join("a.json")).unwrap();
    wait_until(cx, &session, |s| {
        matches!(s.entry("a"), Some(WorkspaceEntry::OperationError { .. }))
    });
    assert_eq!(labels(cx, &session), expected);
}
