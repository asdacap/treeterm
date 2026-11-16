//! "Window → Active Processes" against a real in-process daemon: the list follows the
//! daemon's PTYs on every row click, and processes are named after the session's
//! workspaces as they are now.

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use treeterm_core::pty::CreatePty;
use treeterm_core::settings::TerminalSettings;
use treeterm_model::active_processes::{
    ActiveProcesses, ProcessList, ViewerStatus, viewer_status, workspace_labels,
};
use treeterm_model::session_store::{SessionStore, WorkspaceEntry};
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::traffic::ConnectionTraffic;
use treeterm_proto::treeterm::WorkspaceRef;

mod common;
use common::{Harness, harness, refusing_daemon, wait_until, wait_until_app};

/// A shell with an empty config dir, so the user's dotfiles stay out of the test.
fn spawn_shell(h: &Harness) -> String {
    let mut env = HashMap::new();
    env.insert(
        "ZDOTDIR".to_owned(),
        h.daemon.root().to_string_lossy().into_owned(),
    );
    futures::executor::block_on(h.conn.create_pty(CreatePty {
        cwd: h.daemon.root().to_string_lossy().into_owned(),
        env,
        size: Some((80, 24)),
        ..CreatePty::default()
    }))
    .unwrap()
    .unwrap()
}

fn kill(h: &Harness, id: &str) {
    futures::executor::block_on(h.conn.kill_pty(id))
        .unwrap()
        .unwrap();
}

fn open(cx: &mut TestAppContext, h: &Harness) -> Entity<ActiveProcesses> {
    let conn = h.conn.clone();
    cx.new(|cx| ActiveProcesses::new(conn, TerminalSettings::default(), cx))
}

/// The listed PTY ids, sorted; `None` while not loaded.
fn listed(m: &ActiveProcesses) -> Option<Vec<String>> {
    match m.list() {
        ProcessList::Loaded(sessions) => {
            let mut ids: Vec<String> = sessions.iter().map(|s| s.id.clone()).collect();
            ids.sort();
            Some(ids)
        }
        ProcessList::Loading | ProcessList::Failed(_) => None,
    }
}

fn sorted(ids: &[&str]) -> Vec<String> {
    let mut ids: Vec<String> = ids.iter().map(|id| (*id).to_owned()).collect();
    ids.sort();
    ids
}

fn selected(cx: &mut TestAppContext, model: &Entity<ActiveProcesses>) -> Option<String> {
    model.read_with(cx, |m, _| m.selected().map(|(id, _)| id.to_owned()))
}

#[gpui_kit::test]
fn clicking_another_row_lists_again_keeping_the_old_list_meanwhile(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let a = spawn_shell(&h);
    let model = open(cx, &h);
    wait_until(cx, &model, |m| listed(m) == Some(sorted(&[&a])));

    // A PTY started while the dialog is open shows up on the next click.
    let b = spawn_shell(&h);
    model.update(cx, |m, cx| m.select_and_relist(&a, cx));
    assert_eq!(
        model.read_with(cx, |m, _| listed(m)),
        Some(sorted(&[&a])),
        "no Loading state"
    );
    wait_until(cx, &model, |m| listed(m) == Some(sorted(&[&a, &b])));
    assert_eq!(selected(cx, &model).as_deref(), Some(a.as_str()));

    // One that exited goes.
    kill(&h, &a);
    model.update(cx, |m, cx| m.select_and_relist(&b, cx));
    wait_until(cx, &model, |m| listed(m) == Some(sorted(&[&b])));
    assert_eq!(selected(cx, &model).as_deref(), Some(b.as_str()));
}

#[gpui_kit::test]
fn clicking_a_row_whose_process_exited_clears_the_selection(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let a = spawn_shell(&h);
    let b = spawn_shell(&h);
    let model = open(cx, &h);
    wait_until(cx, &model, |m| listed(m) == Some(sorted(&[&a, &b])));

    kill(&h, &b);
    model.update(cx, |m, cx| m.select_and_relist(&b, cx));
    assert_eq!(selected(cx, &model).as_deref(), Some(b.as_str()));
    wait_until(cx, &model, |m| listed(m) == Some(sorted(&[&a])));
    assert_eq!(selected(cx, &model), None);
}

#[gpui_kit::test]
fn clicking_the_selected_row_does_not_list_again(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let a = spawn_shell(&h);
    let b = spawn_shell(&h);
    let model = open(cx, &h);
    model.update(cx, |m, cx| m.select_and_relist(&a, cx));
    wait_until(cx, &model, |m| listed(m) == Some(sorted(&[&a, &b])));

    let c = spawn_shell(&h);
    model.update(cx, |m, cx| m.select_and_relist(&a, cx));
    for _ in 0..30 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        model.read_with(cx, |m, _| listed(m)),
        Some(sorted(&[&a, &b]))
    );

    // Another row does, and the daemon has the new PTY.
    model.update(cx, |m, cx| m.select_and_relist(&b, cx));
    wait_until(cx, &model, |m| listed(m) == Some(sorted(&[&a, &b, &c])));
}

/// What the viewer of the selected process shows.
fn status(m: &ActiveProcesses, cx: &gpui_kit::App) -> Option<ViewerStatus> {
    m.selected().map(|(_, terminal)| viewer_status(terminal.read(cx)))
}

#[gpui_kit::test]
fn the_viewer_attaches_then_shows_the_process(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let a = spawn_shell(&h);
    let model = open(cx, &h);
    assert_eq!(model.read_with(cx, status), None);

    model.update(cx, |m, cx| m.select(&a, cx));
    assert_eq!(model.read_with(cx, status), Some(ViewerStatus::Attaching));
    wait_until_app(cx, &model, |m, cx| status(m, cx) == Some(ViewerStatus::Ready));
}

#[gpui_kit::test]
fn a_viewer_whose_attach_fails_says_why(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (_daemon, conn) = refusing_daemon(tonic::Status::unavailable("Connection refused"));
    let model = cx.new(|cx| ActiveProcesses::new(conn, TerminalSettings::default(), cx));
    model.update(cx, |m, cx| m.select("p1", cx));
    assert_eq!(model.read_with(cx, status), Some(ViewerStatus::Attaching));
    let failed = ViewerStatus::Failed("14 UNAVAILABLE: Connection refused".to_owned());
    wait_until_app(cx, &model, |m, cx| status(m, cx) == Some(failed.clone()));
}

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
    let store = cx.new(|cx| {
        SessionStore::new(
            conn,
            "client-a".to_owned(),
            settings,
            None,
            cx.new(ConnectionTraffic::new),
            cx,
        )
    });
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
