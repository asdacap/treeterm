//! SessionStore against a real in-process daemon: membership, body sync, and tabs.

use std::collections::HashSet;

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use treeterm_core::wire::workspace_file::{WorkspaceFile, WorkspaceStatus, WorktreeSettings};
use treeterm_model::session_store::{SessionState, SessionStore, WorkspaceEntry};
use treeterm_model::workspace_store::{TabContent, Workspace, WorkspaceStore};
use treeterm_proto::treeterm::WorkspaceRef;

mod common;
use common::{Harness, harness, wait_until, wait_until_app};

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
    let path = h.daemon.root().to_string_lossy().into_owned();
    let refs = ids
        .iter()
        .map(|id| WorkspaceRef { id: (*id).into(), path: path.clone() })
        .collect();
    futures::executor::block_on(h.conn.update_session(refs, None, "test-writer"))
        .unwrap()
        .unwrap();
}

fn open_store(cx: &mut TestAppContext, h: &Harness, client: &str) -> Entity<SessionStore> {
    let conn = h.conn.clone();
    let client = client.to_owned();
    cx.new(|cx| SessionStore::new(conn, client, Default::default(), cx))
}

fn loaded(s: &SessionStore, id: &str) -> bool {
    matches!(s.entry(id), Some(WorkspaceEntry::Loaded(_)))
}

fn ws(s: &SessionStore, id: &str) -> Entity<WorkspaceStore> {
    s.workspace_store(id).expect("workspace loaded").clone()
}

fn file_on_disk(h: &Harness, id: &str) -> serde_json::Value {
    let bytes = std::fs::read(h.daemon.workspaces_dir().join(format!("{id}.json"))).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn tabs_on_disk(h: &Harness, id: &str) -> Vec<String> {
    file_on_disk(h, id)["appStates"]
        .as_object()
        .map(|tabs| tabs.keys().cloned().collect())
        .unwrap_or_default()
}

/// Wait for a file condition (the daemon writes from tokio threads).
fn wait_for_disk(cx: &mut TestAppContext, done: impl Fn() -> bool) {
    for _ in 0..1000 {
        cx.run_until_parked();
        if done() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("disk condition not reached");
}

#[gpui_kit::test]
fn mirrors_refs_and_loads_workspace_bodies(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("root.json"), body("root", None)).unwrap();
    std::fs::write(h.daemon.workspaces_dir().join("child.json"), body("child", Some("root"))).unwrap();
    publish(&h, &["child", "root", "broken"]);

    let store = open_store(cx, &h, "client-a");
    wait_until(cx, &store, |s| {
        matches!(s.state(), SessionState::Live)
            && loaded(s, "root")
            && loaded(s, "child")
            && matches!(s.entry("broken"), Some(WorkspaceEntry::Error { .. }))
    });

    cx.update(|cx| {
        let s = store.read(cx);
        let rows: Vec<(String, usize)> = s
            .tree_rows(&HashSet::new(), cx)
            .into_iter()
            .map(|r| (r.id, r.depth))
            .collect();
        assert_eq!(rows, vec![("root".into(), 0), ("child".into(), 1), ("broken".into(), 0)]);
        match s.entry("broken") {
            Some(WorkspaceEntry::Error { error, .. }) => assert!(error.contains("missing"), "{error}"),
            _ => unreachable!(),
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

    let store = open_store(cx, &h, "client-a");
    wait_until(cx, &store, |s| loaded(s, "a"));

    std::fs::write(&path, body("second", None)).unwrap();
    wait_until_app(cx, &store, |s, cx| ws(s, "a").read(cx).file().name == "second");

    publish(&h, &["a"]);
    wait_until(cx, &store, |s| s.entry("b").is_none());
    wait_until_app(cx, &store, |s, cx| s.tree_rows(&HashSet::new(), cx).len() == 1);
}

#[gpui_kit::test]
fn adding_a_tab_creates_its_pty_and_persists_it(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("a.json"), body("a", None)).unwrap();
    publish(&h, &["a"]);
    let store = open_store(cx, &h, "client-a");
    wait_until(cx, &store, |s| loaded(s, "a"));

    let workspace = store.read_with(cx, |s, _| ws(s, "a"));
    let tab = workspace.update(cx, |w, cx| w.add_tab("terminal", cx)).unwrap();
    // The PTY id lands in the tab's state, and from there on disk.
    wait_for_disk(cx, || file_on_disk(&h, "a")["appStates"][&tab]["state"]["ptyId"].is_string());
    let disk = file_on_disk(&h, "a");
    assert_eq!(disk["activeTabId"], tab.as_str());
    assert_eq!(disk["appStates"][&tab]["title"], "Terminal 1");
    // Our own write's echo is not re-applied: the terminal stays attached.
    wait_until_app(cx, &workspace, |w, _| matches!(w.tab_content(&tab), Some(TabContent::Terminal(_))));

    workspace.update(cx, |w, cx| w.remove_tab(&tab, cx));
    wait_for_disk(cx, || tabs_on_disk(&h, "a").is_empty());
    assert!(file_on_disk(&h, "a").get("activeTabId").is_none(), "no null for an absent optional");
}

#[gpui_kit::test]
fn a_tab_whose_program_exits_is_closed(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("a.json"), body("a", None)).unwrap();
    publish(&h, &["a"]);
    let store = open_store(cx, &h, "client-a");
    wait_until(cx, &store, |s| loaded(s, "a"));
    let workspace = store.read_with(cx, |s, _| ws(s, "a"));
    let tab = workspace.update(cx, |w, cx| w.add_tab("terminal", cx)).unwrap();
    wait_until_app(cx, &workspace, |w, _| matches!(w.tab_content(&tab), Some(TabContent::Terminal(_))));
    let Some(TabContent::Terminal(terminal)) = workspace.read_with(cx, |w, _| w.tab_content(&tab)) else {
        unreachable!()
    };
    wait_until(cx, &terminal, |t| !t.screen_text().trim().is_empty());
    // Past the "exited immediately" window, so the exit closes the tab.
    cx.executor().advance_clock(std::time::Duration::from_secs(2));
    terminal.update(cx, |t, cx| {
        t.set_focused(true, cx);
        t.write_input(b"exit 0\r");
    });
    wait_for_disk(cx, || tabs_on_disk(&h, "a").is_empty());
}

#[gpui_kit::test]
fn concurrent_writers_both_keep_their_tabs(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("a.json"), body("a", None)).unwrap();
    publish(&h, &["a"]);
    // Two windows on one session.
    let first = open_store(cx, &h, "client-a");
    let second = open_store(cx, &h, "client-b");
    wait_until(cx, &first, |s| loaded(s, "a"));
    wait_until(cx, &second, |s| loaded(s, "a"));

    let wa = first.read_with(cx, |s, _| ws(s, "a"));
    let wb = second.read_with(cx, |s, _| ws(s, "a"));
    // Metadata edits from both, in the same tick: one write loses the CAS race.
    wa.update(cx, |w, cx| w.update_metadata("fromA", "1", cx));
    wb.update(cx, |w, cx| w.update_metadata("fromB", "1", cx));
    wait_for_disk(cx, || {
        let disk = file_on_disk(&h, "a");
        disk["metadata"]["fromA"] == "1" || disk["metadata"]["fromB"] == "1"
    });
    // The loser adopted the winner; its own edit was replaced, as in Electron. A later
    // edit from either side lands on top of the merged state.
    wb.update(cx, |w, cx| w.update_metadata("later", "1", cx));
    wait_for_disk(cx, || file_on_disk(&h, "a")["metadata"]["later"] == "1");
    wait_until_app(cx, &first, |s, cx| {
        ws(s, "a").read(cx).file().metadata.get("later").map(String::as_str) == Some("1")
    });
}

#[gpui_kit::test]
fn an_added_workspace_is_written_then_published(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish(&h, &[]);
    let first = open_store(cx, &h, "client-a");
    let second = open_store(cx, &h, "client-b");
    wait_until(cx, &first, |s| matches!(s.state(), SessionState::Live));

    let path = h.daemon.root().to_string_lossy().into_owned();
    first.update(cx, |s, cx| {
        let workspace = Workspace {
            id: "ws-new".into(),
            path: path.clone(),
            file: WorkspaceFile {
                name: "new".into(),
                parent_id: None,
                status: WorkspaceStatus::Active,
                is_git_repo: false,
                git_branch: None,
                git_root_path: None,
                is_worktree: false,
                is_detached: Some(false),
                app_states: Default::default(),
                active_tab_id: None,
                metadata: [("sortOrder".to_owned(), "0".to_owned())].into(),
                settings: WorktreeSettings::default(),
                favourite_paths: Vec::new(),
                created_at: 1,
                last_activity: 1,
                extra: Default::default(),
            },
        };
        s.add_workspace(workspace, cx);
    });
    // The other window learns of it through the session and loads its body.
    wait_until_app(cx, &second, |s, cx| {
        s.workspace_store("ws-new").is_some_and(|w| w.read(cx).file().name == "new")
    });
    assert_eq!(file_on_disk(&h, "ws-new")["parentHash"], "");
    first.read_with(cx, |s, _| assert!(s.version() >= 2));
}
