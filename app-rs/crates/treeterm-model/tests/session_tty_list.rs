//! The session pane's TTYs tab (`SessionTtyList.tsx`): it groups by the session's
//! workspaces as they load, lists only when shown, on Refresh and after a Kill, and acts
//! through the connection's daemon of the moment across a reconnect.

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use treeterm_core::connection::ConnectionId;
use treeterm_core::local_daemon::LocalDaemonSpec;
use treeterm_core::pty::CreatePty;
use treeterm_model::active_processes::PtyGroup;
use treeterm_model::app_store::AppStore;
use treeterm_model::session_tty_list::{SessionTtyList, TtyListBody, TtyViewer};
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::terminal::TerminalModel;
use treeterm_proto::treeterm::WorkspaceRef;

mod common;
use common::{Harness, Proxy, harness, wait_until_app};

/// One test at a time: PTY shells forked by one test must not hold another's pipes.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn root(h: &Harness) -> String {
    h.daemon.root().to_string_lossy().into_owned()
}

/// Workspace `a`, named "Alpha", at the daemon's root, on disk and in the session.
fn publish_a(h: &Harness) {
    let body = serde_json::json!({
        "name": "Alpha", "status": "active", "isGitRepo": false, "isWorktree": false,
        "appStates": {}, "metadata": {}, "createdAt": 1, "lastActivity": 1, "parentHash": ""
    });
    std::fs::write(h.daemon.workspaces_dir().join("a.json"), body.to_string()).unwrap();
    let refs = vec![WorkspaceRef {
        id: "a".into(),
        path: root(h),
    }];
    futures::executor::block_on(h.conn.update_session(refs, None, "test-writer"))
        .unwrap()
        .unwrap();
}

/// A shell in the daemon's root.
fn spawn_pty(h: &Harness) -> String {
    let create = CreatePty {
        cwd: root(h),
        ..CreatePty::default()
    };
    futures::executor::block_on(h.conn.create_pty(create))
        .unwrap()
        .unwrap()
}

/// The ids of the PTYs the daemon runs.
fn daemon_ptys(h: &Harness) -> Vec<String> {
    futures::executor::block_on(h.conn.list_pty_sessions())
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect()
}

/// The app on the local daemon at `socket` (never spawned: it is already running).
fn app_on(
    cx: &mut TestAppContext,
    h: &Harness,
    socket: &std::path::Path,
    dir: &std::path::Path,
) -> Entity<AppStore> {
    let spec = LocalDaemonSpec {
        socket_path: socket.to_owned(),
        binary: dir.join("never-spawned"),
        log_path: dir.join("daemon.log"),
        pid_file: dir.join("daemon.pid"),
    };
    let settings = cx.new(|_| SettingsStore::in_memory(Default::default()));
    let rt = h.rt.clone();
    cx.new(|cx| AppStore::new(rt, spec, settings, cx))
}

fn local_id(cx: &mut TestAppContext, app: &Entity<AppStore>) -> ConnectionId {
    app.read_with(cx, |a, _| {
        a.sessions()
            .next()
            .expect("the local connection")
            .id
            .clone()
    })
}

/// Wait until connection `id` has a daemon (`up`) or has none.
fn wait_connection(cx: &mut TestAppContext, app: &Entity<AppStore>, id: &ConnectionId, up: bool) {
    wait_until_app(cx, app, |a, _| a.manager().client(id).is_ok() == up);
}

fn group(label: &str, ptys: &[&str]) -> (String, Vec<String>) {
    (
        label.to_owned(),
        ptys.iter().map(|id| (*id).to_owned()).collect(),
    )
}

/// The body's groups as `(label, PTY ids)`, or `None` until it has loaded.
fn groups(list: &SessionTtyList, cx: &gpui_kit::App) -> Option<Vec<(String, Vec<String>)>> {
    let TtyListBody::Loaded(groups) = list.body(cx) else {
        return None;
    };
    let shape = |g: &PtyGroup| {
        (
            g.label.clone(),
            g.sessions.iter().map(|s| s.id.clone()).collect(),
        )
    };
    Some(groups.iter().map(shape).collect())
}

/// The viewer's PTY and its live terminal, if one is attached.
fn viewer_terminal(
    list: &Entity<SessionTtyList>,
    cx: &mut TestAppContext,
) -> Option<(String, Entity<TerminalModel>)> {
    list.read_with(cx, |l, _| match l.viewer()? {
        (id, TtyViewer::Terminal(terminal)) => Some((id.to_owned(), terminal.clone())),
        (_, TtyViewer::NoDaemon(_)) => None,
    })
}

#[gpui_kit::test]
fn ptys_move_under_their_workspace_once_it_loads(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let _serial = serial();
    let h = harness();
    let pty = spawn_pty(&h);
    let dir = tempfile::Builder::new()
        .prefix("tt")
        .tempdir_in("/tmp")
        .unwrap();
    let app = app_on(cx, &h, h.daemon.socket_path(), dir.path());
    let id = local_id(cx, &app);

    // Shown before the connection is up, it fails with why, as Electron's `getClient` does,
    // and lists again only on Refresh; shown once connected, it lists at once.
    let list = cx.new(|cx| SessionTtyList::new(&app, id.clone(), cx));
    let body = list.read_with(cx, |l, cx| l.body(cx));
    let connecting = TtyListBody::Failed(format!("Connection {id} is connecting"));
    assert!(
        body == connecting || body == TtyListBody::Loading,
        "{body:?}"
    );
    if body == connecting {
        wait_connection(cx, &app, &id, true);
        assert_eq!(list.read_with(cx, |l, cx| l.body(cx)), connecting);
        list.update(cx, |l, cx| l.reload(cx));
    }

    // Connected, with no workspace yet: the shell is under "Unknown".
    wait_until_app(cx, &list, |l, cx| {
        groups(l, cx) == Some(vec![group("Unknown", &[&pty])])
    });

    // The workspace arrives and loads: the list regroups without a refresh.
    publish_a(&h);
    wait_until_app(cx, &list, |l, cx| {
        groups(l, cx) == Some(vec![group("Alpha", &[&pty])])
    });
}

#[gpui_kit::test]
fn a_dropped_connection_keeps_the_list_and_viewer_until_refresh(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let _serial = serial();
    let h = harness();
    publish_a(&h);
    let pty = spawn_pty(&h);
    // The app reaches the daemon through a proxy, so the test can take the daemon away.
    let dir = tempfile::Builder::new()
        .prefix("tt")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = dir.path().join("p.sock");
    let proxy = Proxy::start(&socket, h.daemon.socket_path());
    let app = app_on(cx, &h, &socket, dir.path());
    let id = local_id(cx, &app);
    wait_connection(cx, &app, &id, true);
    let list = cx.new(|cx| SessionTtyList::new(&app, id.clone(), cx));
    let listed = |l: &SessionTtyList, cx: &gpui_kit::App| {
        groups(l, cx) == Some(vec![group("Alpha", &[&pty])])
    };
    wait_until_app(cx, &list, listed);
    list.update(cx, |l, cx| l.select(&pty, cx));
    let (shown, viewer) = viewer_terminal(&list, cx).expect("a viewer attached");
    assert_eq!(shown, pty);

    // The connection drops: the list, grouped by the kept session's workspaces, and the
    // viewer stay.
    drop(proxy);
    wait_connection(cx, &app, &id, false);
    assert!(list.read_with(cx, listed));
    assert!(
        viewer_terminal(&list, cx) == Some((pty.clone(), viewer)),
        "the viewer stays"
    );

    // Meanwhile Open, Refresh and Kill fail with Electron's `getClient` error.
    let reconnecting = format!("Connection {id} is reconnecting: ");
    list.update(cx, |l, cx| l.deselect(cx));
    assert!(list.read_with(cx, |l, _| l.viewer().is_none()));
    list.update(cx, |l, cx| l.select(&pty, cx));
    assert!(list.read_with(cx, |l, _| matches!(
        l.viewer(),
        Some((shown, TtyViewer::NoDaemon(why))) if shown == pty && why.starts_with(&reconnecting)
    )));
    let failed = |l: &SessionTtyList, cx: &gpui_kit::App| matches!(l.body(cx), TtyListBody::Failed(why) if why.starts_with(&reconnecting));
    list.update(cx, |l, cx| l.reload(cx));
    assert!(list.read_with(cx, failed));
    list.update(cx, |l, cx| l.deselect(cx));
    list.update(cx, |l, cx| l.select(&pty, cx));
    list.update(cx, |l, cx| l.kill(&pty, cx));
    assert!(
        list.read_with(cx, |l, _| l.viewer().is_none()),
        "Kill closes its viewer"
    );
    assert!(list.read_with(cx, failed));
    assert!(daemon_ptys(&h).contains(&pty), "nothing was killed");

    // Connected again, it does not list by itself; Refresh, Open and Kill use the new
    // connection.
    let _proxy = Proxy::start(&socket, h.daemon.socket_path());
    wait_connection(cx, &app, &id, true);
    cx.run_until_parked();
    assert!(list.read_with(cx, failed));
    list.update(cx, |l, cx| l.reload(cx));
    assert_eq!(list.read_with(cx, |l, cx| l.body(cx)), TtyListBody::Loading);
    wait_until_app(cx, &list, listed);
    list.update(cx, |l, cx| l.select(&pty, cx));
    assert!(viewer_terminal(&list, cx).is_some_and(|(shown, _)| shown == pty));
    list.update(cx, |l, cx| l.kill(&pty, cx));
    assert!(
        list.read_with(cx, |l, _| l.viewer().is_none()),
        "Kill closes its viewer"
    );
    assert_eq!(list.read_with(cx, |l, cx| l.body(cx)), TtyListBody::Loading);
    wait_until_app(cx, &list, |l, cx| groups(l, cx) == Some(Vec::new()));
    assert!(!daemon_ptys(&h).contains(&pty), "the PTY was killed");
}
