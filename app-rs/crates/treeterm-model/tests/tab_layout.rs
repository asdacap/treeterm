//! The tab layout (`metadata.layoutModel`, Electron's flex-layout model) against a real
//! in-process daemon: what the workspace shows, and what it saves when tabs come and go, are
//! selected, dragged or docked, and when a tabset is activated, maximized or resized.

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use serde_json::{Value, json};
use treeterm_core::layout::{Dock, DropTarget, Edge, Layout, LayoutNode, LayoutTabSet, Maximize, Orientation, Side};
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

/// `a b | (c / d)` as Electron's flex-layout writes it: the left tabset 30 wide showing `b`
/// and active, the right column 70 wide holding `c` over `d`, 40 and 60 high.
fn split_layout() -> String {
    let tab = |id: &str| json!({ "type": "tab", "id": id, "name": id, "component": "workspace-settings", "enableClose": true });
    json!({
        "global": { "splitterSize": 4, "splitterExtra": 4, "tabEnableRename": false, "tabEnableRenderOnDemand": false },
        "borders": [],
        "layout": { "type": "row", "id": "#root", "children": [
            { "type": "tabset", "id": "#left", "weight": 30, "selected": 1, "children": [tab("a"), tab("b")], "active": true },
            { "type": "row", "id": "#column", "weight": 70, "children": [
                { "type": "tabset", "id": "#top", "weight": 40, "children": [tab("c")] },
                { "type": "tabset", "id": "#bottom", "weight": 60, "children": [tab("d")] },
            ]},
        ]},
        "popouts": {}
    })
    .to_string()
}

fn open_split(cx: &mut TestAppContext, h: &Harness, store: &Entity<SessionStore>, active: &str) -> (String, Entity<WorkspaceStore>) {
    let metadata = json!({ "layoutModel": split_layout() });
    open_workspace(cx, h, store, tabs(&["a", "b", "c", "d"]), Some(active), metadata)
}

/// The tabset at `path` of a layout.
fn tabset(layout: &Layout, path: &[usize]) -> LayoutTabSet {
    layout.tree().tabsets().into_iter().find(|tabset| tabset.path == path).cloned().expect("a tabset there")
}

fn saved_model(h: &Harness, id: &str) -> Value {
    serde_json::from_str(file_on_disk(h, id)["metadata"]["layoutModel"].as_str().expect("saved")).unwrap()
}

#[gpui_kit::test]
fn a_split_layout_shows_its_tabsets_with_their_weights_and_selections(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h);
    let (id, workspace) = open_split(cx, &h, &store, "b");

    let tree = workspace.read_with(cx, |w, _| w.layout().tree());
    assert_eq!(tree.orientation, Orientation::Horizontal);
    assert_eq!(tree.children.iter().map(LayoutNode::weight).collect::<Vec<_>>(), [30., 70.]);
    let LayoutNode::Row(column) = &tree.children[1] else { panic!("the right column") };
    assert_eq!(column.orientation, Orientation::Vertical);
    assert_eq!(column.children.iter().map(LayoutNode::weight).collect::<Vec<_>>(), [40., 60.]);
    let shown: Vec<(Vec<usize>, Option<String>, bool, Maximize)> = tree
        .tabsets()
        .into_iter()
        .map(|tabset| (tabset.path.clone(), tabset.selected.clone(), tabset.active, tabset.maximize))
        .collect();
    assert_eq!(
        shown,
        [
            (vec![0], Some("b".to_owned()), true, Maximize::Restored),
            (vec![1, 0], Some("c".to_owned()), false, Maximize::Restored),
            (vec![1, 1], Some("d".to_owned()), false, Maximize::Restored),
        ]
    );
    assert_eq!(saved_model(&h, &id), serde_json::from_str::<Value>(&split_layout()).unwrap(), "shown, not rewritten");
}

#[gpui_kit::test]
fn the_active_tab_is_shown_in_its_tabset_which_keeps_or_takes_the_activation(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h);
    // An active tab its tabset does not show (another writer's `activeTabId`): shown there,
    // as `FlexLayoutPane` selects it.
    let (id, workspace) = open_split(cx, &h, &store, "a");
    workspace.read_with(cx, |w, _| {
        assert_eq!(tabset(&w.layout(), &[0]).selected.as_deref(), Some("a"));
        assert_eq!(w.shown_tab().as_deref(), Some("a"));
    });

    // A shortcut to a tab its (inactive) tabset already shows: the left tabset stays active.
    workspace.update(cx, |w, cx| w.set_active_tab("d", cx));
    wait_for_disk(cx, || file_on_disk(&h, &id)["activeTabId"] == "d");
    let saved = layout_on_disk(&h, &id).unwrap();
    assert!(tabset(&saved, &[0]).active);
    assert!(!tabset(&saved, &[1, 1]).active);
    assert_eq!(tabset(&saved, &[1, 1]).selected.as_deref(), Some("d"));

    // One its tabset does not show: selected there, and its tabset takes the activation.
    workspace.update(cx, |w, cx| w.select_tab("c", cx));
    wait_for_disk(cx, || file_on_disk(&h, &id)["activeTabId"] == "c");
    let shown = layout_on_disk(&h, &id).unwrap().tree().tabsets()[0].selected.clone().expect("a tab shown");
    let other = if shown == "a" { "b" } else { "a" };
    workspace.update(cx, |w, cx| w.set_active_tab(other, cx));
    wait_for_disk(cx, || file_on_disk(&h, &id)["activeTabId"] == other);
    let saved = layout_on_disk(&h, &id).unwrap();
    assert_eq!(saved.selected_tab().as_deref(), Some(other));
    assert!(tabset(&saved, &[0]).active);
}

#[gpui_kit::test]
fn a_tab_clicked_in_its_strip_is_selected_and_its_tabset_activated(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h);
    let (id, workspace) = open_split(cx, &h, &store, "b");

    // `d`, shown in its tabset already: the bottom tabset becomes the active one.
    workspace.update(cx, |w, cx| w.select_tab("d", cx));
    wait_for_disk(cx, || file_on_disk(&h, &id)["activeTabId"] == "d");
    let saved = layout_on_disk(&h, &id).unwrap();
    assert!(tabset(&saved, &[1, 1]).active);
    assert!(!tabset(&saved, &[0]).active);

    // `a`, not shown in the left tabset: selected there, and the left tabset active again.
    workspace.update(cx, |w, cx| w.select_tab("a", cx));
    wait_for_disk(cx, || file_on_disk(&h, &id)["activeTabId"] == "a");
    let left = tabset(&layout_on_disk(&h, &id).unwrap(), &[0]);
    assert_eq!((left.selected.as_deref(), left.active), (Some("a"), true));

    // Again, or a tab that is gone: nothing changes.
    let before = workspace.read_with(cx, |w, _| w.file().clone());
    workspace.update(cx, |w, cx| {
        w.select_tab("a", cx);
        w.select_tab("gone", cx);
    });
    workspace.read_with(cx, |w, _| assert_eq!(*w.file(), before));
}

#[gpui_kit::test]
fn an_activated_tabset_is_saved_and_takes_new_tabs(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h);
    let (id, workspace) = open_split(cx, &h, &store, "b");

    // A press in the top tabset: it is the active one, the active tab stays `b`.
    workspace.update(cx, |w, cx| w.activate_tabset(&[1, 0], cx));
    wait_for_disk(cx, || layout_on_disk(&h, &id).is_some_and(|saved| tabset(&saved, &[1, 0]).active));
    assert_eq!(file_on_disk(&h, &id)["activeTabId"], "b");
    assert!(!tabset(&layout_on_disk(&h, &id).unwrap(), &[0]).active);

    let tab = workspace.update(cx, |w, cx| w.add_tab("workspace-settings", cx)).unwrap();
    wait_for_disk(cx, || file_on_disk(&h, &id)["activeTabId"] == tab.as_str());
    let top = tabset(&layout_on_disk(&h, &id).unwrap(), &[1, 0]);
    assert_eq!(top.tabs, ["c", tab.as_str()]);
    assert_eq!(top.selected.as_deref(), Some(tab.as_str()));

    // A row, nothing there, or the active tabset again: nothing changes.
    let before = workspace.read_with(cx, |w, _| w.file().clone());
    workspace.update(cx, |w, cx| {
        w.activate_tabset(&[1], cx);
        w.activate_tabset(&[9], cx);
        w.activate_tabset(&[1, 0], cx);
    });
    workspace.read_with(cx, |w, _| assert_eq!(*w.file(), before));
}

#[gpui_kit::test]
fn a_maximized_tabset_is_saved_until_restored(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h);
    let (id, workspace) = open_split(cx, &h, &store, "b");

    workspace.update(cx, |w, cx| w.toggle_maximize(&[1, 1], cx));
    wait_for_disk(cx, || layout_on_disk(&h, &id).is_some_and(|saved| saved.tree().maximized().is_some()));
    let model = saved_model(&h, &id);
    assert_eq!(model["layout"]["children"][1]["children"][1]["maximized"], true);
    assert_eq!(model["layout"]["children"][1]["children"][1]["active"], true);
    workspace.read_with(cx, |w, _| assert_eq!(tabset(&w.layout(), &[1, 1]).maximize, Maximize::Maximized));

    workspace.update(cx, |w, cx| w.toggle_maximize(&[1, 1], cx));
    wait_for_disk(cx, || layout_on_disk(&h, &id).is_some_and(|saved| saved.tree().maximized().is_none()));
    assert!(tabset(&layout_on_disk(&h, &id).unwrap(), &[1, 1]).active);
}

#[gpui_kit::test]
fn the_only_tabset_cannot_be_maximized(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h);
    let (_, workspace) = open_workspace(cx, &h, &store, tabs(&["a", "b"]), Some("a"), json!({}));
    workspace.read_with(cx, |w, _| assert_eq!(tabset(&w.layout(), &[0]).maximize, Maximize::Unavailable));
    let before = workspace.read_with(cx, |w, _| w.file().clone());
    workspace.update(cx, |w, cx| w.toggle_maximize(&[0], cx));
    workspace.read_with(cx, |w, _| assert_eq!(*w.file(), before));
}

#[gpui_kit::test]
fn a_dropped_splitter_saves_the_rows_new_weights(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h);
    let (id, workspace) = open_split(cx, &h, &store, "b");

    workspace.update(cx, |w, cx| w.resize_layout_row(&[1], &[25., 75.], cx));
    wait_for_disk(cx, || saved_model(&h, &id)["layout"]["children"][1]["children"][0]["weight"] == 25);
    let model = saved_model(&h, &id);
    assert_eq!(model["layout"]["children"][1]["children"][1]["weight"], 75);
    assert_eq!(model["layout"]["children"][0]["weight"], 30, "the other rows keep theirs");

    // Not a row, or the wrong count: nothing changes.
    let before = workspace.read_with(cx, |w, _| w.file().clone());
    workspace.update(cx, |w, cx| {
        w.resize_layout_row(&[0], &[50., 50.], cx);
        w.resize_layout_row(&[], &[100.], cx);
    });
    workspace.read_with(cx, |w, _| assert_eq!(*w.file(), before));
}

#[gpui_kit::test]
fn a_tabset_whose_last_tab_closes_goes_and_its_neighbour_takes_the_room(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h);
    let (id, workspace) = open_split(cx, &h, &store, "b");

    workspace.update(cx, |w, cx| w.remove_tab("c", cx));
    wait_for_disk(cx, || layout_on_disk(&h, &id).is_some_and(|saved| saved.tab_order() == ["a", "b", "d"]));
    let tree = layout_on_disk(&h, &id).unwrap().tree();
    // The column, left with the bottom tabset, is replaced by it, with the column's weight.
    assert_eq!(tree.tabsets().iter().map(|tabset| tabset.path.clone()).collect::<Vec<_>>(), [vec![0], vec![1]]);
    assert_eq!(tree.children.iter().map(LayoutNode::weight).collect::<Vec<_>>(), [30., 70.]);
}

#[gpui_kit::test]
fn a_docked_tab_splits_its_tabset_or_the_layout_and_is_saved_there_shown(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h);
    let (id, workspace) = open_workspace(cx, &h, &store, tabs(&["a", "b", "c"]), Some("a"), json!({}));
    let into = |path: &[usize], dock: Dock| DropTarget::TabSet { path: path.to_vec(), dock };
    let ids = |ids: &[&str]| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>();

    // On the right of the only tabset's content: a tabset of its own beside it, shown there.
    workspace.update(cx, |w, cx| w.dock_tab("c", &into(&[0], Dock::Edge(Edge::Right)), cx));
    wait_for_disk(cx, || file_on_disk(&h, &id)["activeTabId"] == "c");
    let shown: Vec<(Vec<String>, Option<String>, bool, f64)> = layout_on_disk(&h, &id)
        .unwrap()
        .tree()
        .tabsets()
        .into_iter()
        .map(|tabset| (tabset.tabs.clone(), tabset.selected.clone(), tabset.active, tabset.weight))
        .collect();
    assert_eq!(
        shown,
        [(ids(&["a", "b"]), Some("a".to_owned()), false, 50.), (ids(&["c"]), Some("c".to_owned()), true, 50.)]
    );
    workspace.read_with(cx, |w, _| assert_eq!(w.shown_tab().as_deref(), Some("c")));

    // Back on the middle of the left one: it joins its tabs, and the empty right one goes.
    workspace.update(cx, |w, cx| w.dock_tab("c", &into(&[0], Dock::Center), cx));
    wait_for_disk(cx, || layout_on_disk(&h, &id).is_some_and(|saved| saved.tree().tabsets().len() == 1));
    let only = tabset(&layout_on_disk(&h, &id).unwrap(), &[0]);
    assert_eq!((only.tabs, only.selected.as_deref()), (ids(&["a", "b", "c"]), Some("c")));

    // At the layout's bottom edge: a pane across the whole layout, under the rest.
    workspace.update(cx, |w, cx| w.dock_tab("b", &DropTarget::LayoutEdge(Edge::Bottom), cx));
    wait_for_disk(cx, || file_on_disk(&h, &id)["activeTabId"] == "b");
    let tree = layout_on_disk(&h, &id).unwrap().tree();
    let [LayoutNode::Row(column)] = tree.children.as_slice() else { panic!("one column: {tree:?}") };
    assert_eq!(column.orientation, Orientation::Vertical);
    let panes: Vec<(Vec<String>, f64, bool)> =
        column.tabsets().into_iter().map(|tabset| (tabset.tabs.clone(), tabset.weight, tabset.active)).collect();
    assert_eq!(panes, [(ids(&["a", "c"]), 75., false), (ids(&["b"]), 25., true)]);

    // A tab that is gone, or no tabset there: nothing changes.
    let before = workspace.read_with(cx, |w, _| w.file().clone());
    workspace.update(cx, |w, cx| {
        w.dock_tab("gone", &DropTarget::LayoutEdge(Edge::Top), cx);
        w.dock_tab("a", &into(&[0], Dock::Center), cx);
    });
    workspace.read_with(cx, |w, _| assert_eq!(*w.file(), before));
}
