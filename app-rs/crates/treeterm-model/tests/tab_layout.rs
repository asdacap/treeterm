//! The tab layout (`metadata.layoutModel`, Electron's flex-layout model) against a real
//! in-process daemon: what the workspace shows, and what it saves when tabs come and go, are
//! selected or are dragged.

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use serde_json::{Value, json};
use treeterm_core::layout::{Layout, Side};
use treeterm_model::session_store::{SessionState, SessionStore, WorkspaceEntry};
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::workspace_store::WorkspaceStore;
use treeterm_proto::treeterm::WorkspaceRef;

mod common;
use common::{Harness, harness, wait_until, wait_until_app};

fn open_store(cx: &mut TestAppContext, h: &Harness) -> Entity<SessionStore> {
    let conn = h.conn.clone();
    let settings = cx.new(|_| SettingsStore::in_memory(Default::default()));
    cx.new(|cx| SessionStore::new(conn, "client-a".into(), settings, None, cx))
}

/// Settings tabs (no PTY, closable) with these ids, titled after them.
fn tabs(ids: &[&str]) -> Value {
    let tabs = ids
        .iter()
        .map(|id| (id.to_string(), json!({ "applicationId": "workspace-settings", "title": id, "state": {} })))
        .collect::<serde_json::Map<_, _>>();
    Value::Object(tabs)
}

/// A one-tabset layout as flex-layout writes it: `ids` in order, `selected` selected.
fn saved_layout(ids: &[&str], selected: usize) -> String {
    let children: Vec<Value> = ids
        .iter()
        .map(|id| json!({ "type": "tab", "id": id, "name": id, "component": "workspace-settings", "enableClose": true }))
        .collect();
    json!({
        "global": { "splitterSize": 4, "splitterExtra": 4, "tabEnableRename": false, "tabEnableRenderOnDemand": false },
        "borders": [],
        "layout": { "type": "row", "id": "#root", "children": [
            { "type": "tabset", "id": "#main", "selected": selected, "children": children, "active": true }
        ]},
        "popouts": {}
    })
    .to_string()
}

/// A loaded workspace with this body (`appStates`, `activeTabId`, `metadata`).
fn open_workspace(
    cx: &mut TestAppContext,
    h: &Harness,
    store: &Entity<SessionStore>,
    app_states: Value,
    active: Option<&str>,
    metadata: Value,
) -> (String, Entity<WorkspaceStore>) {
    let id = format!("ws-{}", uuid::Uuid::new_v4().simple());
    let dir = h.daemon.root().join(&id);
    std::fs::create_dir_all(&dir).unwrap();
    let mut body = json!({
        "name": "layout", "status": "active", "isGitRepo": false, "isWorktree": false,
        "appStates": app_states, "metadata": metadata, "createdAt": 1, "lastActivity": 1, "parentHash": ""
    });
    if let Some(active) = active {
        body["activeTabId"] = active.into();
    }
    std::fs::write(h.daemon.workspaces_dir().join(format!("{id}.json")), body.to_string()).unwrap();
    let refs = vec![WorkspaceRef { id: id.clone(), path: dir.to_string_lossy().into_owned() }];
    futures::executor::block_on(h.conn.update_session(refs, None, "test-writer")).unwrap().unwrap();
    wait_until(cx, store, |s| {
        matches!(s.state(), SessionState::Live) && matches!(s.entry(&id), Some(WorkspaceEntry::Loaded(_)))
    });
    let workspace = store.read_with(cx, |s, _| s.workspace_store(&id).unwrap().clone());
    (id, workspace)
}

fn file_on_disk(h: &Harness, id: &str) -> Value {
    let bytes = std::fs::read(h.daemon.workspaces_dir().join(format!("{id}.json"))).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// The layout saved on disk, if any.
fn layout_on_disk(h: &Harness, id: &str) -> Option<Layout> {
    file_on_disk(h, id)["metadata"]["layoutModel"].as_str().map(|json| Layout::parse(json).expect("a layout"))
}

/// Wait for a condition on the workspace file (written from tokio threads).
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
fn a_stale_active_tab_shows_the_tab_the_saved_layout_selects(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h);
    let metadata = json!({ "layoutModel": saved_layout(&["c", "a", "b"], 2) });
    let (id, workspace) = open_workspace(cx, &h, &store, tabs(&["a", "b", "c"]), Some("gone"), metadata);

    workspace.read_with(cx, |w, _| {
        assert_eq!(w.shown_tab().as_deref(), Some("b"));
        assert_eq!(w.layout().tab_order(), ["c", "a", "b"]);
        // Shown, not adopted: the body keeps the id it has.
        assert_eq!(w.file().active_tab_id.as_deref(), Some("gone"));
    });
    assert_eq!(file_on_disk(&h, &id)["activeTabId"], "gone");
}

#[gpui_kit::test]
fn a_stale_active_tab_without_a_saved_layout_shows_the_first_tab(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h);
    let (_, workspace) = open_workspace(cx, &h, &store, tabs(&["a", "b", "c"]), Some("gone"), json!({}));
    workspace.read_with(cx, |w, _| assert_eq!(w.shown_tab().as_deref(), Some("a")));

    // A valid active tab is the one shown, whatever the layout selects.
    workspace.update(cx, |w, cx| w.set_active_tab("c", cx));
    workspace.read_with(cx, |w, _| assert_eq!(w.shown_tab().as_deref(), Some("c")));
}

#[gpui_kit::test]
fn a_new_tab_is_saved_at_the_end_of_the_layout_selected(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h);
    let metadata = json!({ "layoutModel": saved_layout(&["b", "a"], 0) });
    let (id, workspace) = open_workspace(cx, &h, &store, tabs(&["a", "b"]), Some("b"), metadata);

    let tab = workspace.update(cx, |w, cx| w.add_tab("workspace-settings", cx)).unwrap();
    wait_for_disk(cx, || file_on_disk(&h, &id)["activeTabId"] == tab.as_str());
    let saved = layout_on_disk(&h, &id).expect("saved");
    assert_eq!(saved.tab_order(), ["b", "a", tab.as_str()]);
    assert_eq!(saved.selected_tab().as_deref(), Some(tab.as_str()));
    // What it does not use stays as Electron wrote it.
    let model: Value = serde_json::from_str(file_on_disk(&h, &id)["metadata"]["layoutModel"].as_str().unwrap()).unwrap();
    assert_eq!(model["layout"]["id"], "#root");
    assert_eq!(model["layout"]["children"][0]["id"], "#main");
    assert_eq!(model["global"]["splitterSize"], 4);
    assert_eq!(
        model["layout"]["children"][0]["children"][2],
        json!({ "type": "tab", "id": tab, "name": "Settings 3", "component": "workspace-settings", "enableClose": true })
    );
}

#[gpui_kit::test]
fn a_closed_tab_leaves_the_saved_layout_and_the_next_tab_is_selected(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h);
    // No saved layout yet: closing a tab saves Electron's default without it.
    let (id, workspace) = open_workspace(cx, &h, &store, tabs(&["a", "b", "c"]), Some("b"), json!({}));

    workspace.update(cx, |w, cx| w.remove_tab("b", cx));
    wait_for_disk(cx, || layout_on_disk(&h, &id).is_some());
    let saved = layout_on_disk(&h, &id).unwrap();
    assert_eq!(saved.tab_order(), ["a", "c"]);
    assert_eq!(file_on_disk(&h, &id)["activeTabId"], "c");
    assert_eq!(saved.selected_tab().as_deref(), Some("c"));

    // The last ones: an empty layout.
    workspace.update(cx, |w, cx| {
        w.remove_tab("a", cx);
        w.remove_tab("c", cx);
    });
    wait_for_disk(cx, || layout_on_disk(&h, &id).is_some_and(|saved| saved.tab_order().is_empty()));
    assert_eq!(layout_on_disk(&h, &id).unwrap().selected_tab(), None);
}

#[gpui_kit::test]
fn selecting_a_tab_saves_it_as_the_layouts_selection(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h);
    let metadata = json!({ "layoutModel": saved_layout(&["c", "b", "a"], 0) });
    let (id, workspace) = open_workspace(cx, &h, &store, tabs(&["a", "b", "c"]), Some("c"), metadata);

    workspace.update(cx, |w, cx| w.set_active_tab("a", cx));
    wait_for_disk(cx, || file_on_disk(&h, &id)["activeTabId"] == "a");
    let saved = layout_on_disk(&h, &id).unwrap();
    assert_eq!(saved.selected_tab().as_deref(), Some("a"));
    assert_eq!(saved.tab_order(), ["c", "b", "a"]);
}

#[gpui_kit::test]
fn a_dragged_tab_is_saved_in_its_new_place_and_shown(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h);
    let (id, workspace) = open_workspace(cx, &h, &store, tabs(&["a", "b", "c"]), Some("a"), json!({}));

    workspace.update(cx, |w, cx| w.move_tab("c", "a", Side::Before, cx));
    wait_for_disk(cx, || layout_on_disk(&h, &id).is_some());
    assert_eq!(layout_on_disk(&h, &id).unwrap().tab_order(), ["c", "a", "b"]);
    assert_eq!(file_on_disk(&h, &id)["activeTabId"], "c");
    workspace.read_with(cx, |w, _| {
        assert_eq!(w.layout().tab_order(), ["c", "a", "b"]);
        assert_eq!(w.shown_tab().as_deref(), Some("c"));
    });

    workspace.update(cx, |w, cx| w.move_tab("a", "b", Side::After, cx));
    wait_for_disk(cx, || layout_on_disk(&h, &id).is_some_and(|saved| saved.tab_order() == ["c", "b", "a"]));
    assert_eq!(file_on_disk(&h, &id)["activeTabId"], "a");

    // Onto a tab that is gone: nothing moves.
    let before = workspace.read_with(cx, |w, _| w.file().clone());
    workspace.update(cx, |w, cx| w.move_tab("a", "gone", Side::Before, cx));
    workspace.read_with(cx, |w, _| assert_eq!(*w.file(), before));
}

#[gpui_kit::test]
fn a_body_from_another_writer_is_shown_with_its_layout_reconciled_but_not_rewritten(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h);
    let (id, workspace) = open_workspace(cx, &h, &store, tabs(&["a"]), Some("a"), json!({}));

    // Another app adds c and closes a, its layout still listing a and not c.
    let path = h.daemon.workspaces_dir().join(format!("{id}.json"));
    let mut body = file_on_disk(&h, &id);
    body["appStates"] = tabs(&["b", "c"]);
    body["activeTabId"] = "c".into();
    body["metadata"]["layoutModel"] = saved_layout(&["b", "a"], 1).into();
    let written = body.to_string();
    std::fs::write(&path, &written).unwrap();
    wait_until_app(cx, &workspace, |w, _| w.file().app_states.contains_key("c"));

    workspace.read_with(cx, |w, _| {
        assert_eq!(w.layout().tab_order(), ["b", "c"]);
        assert_eq!(w.shown_tab().as_deref(), Some("c"));
    });
    for _ in 0..20 {
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), written, "the other writer's body stays as written");
}
