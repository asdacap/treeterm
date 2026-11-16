//! The session pane's TTYs tab (`SessionTtyList.tsx`): it groups by the session's
//! workspaces as they load, and follows the connection across a reconnect.

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use treeterm_core::connection::ConnectionId;
use treeterm_core::local_daemon::LocalDaemonSpec;
use treeterm_core::pty::CreatePty;
use treeterm_model::active_processes::PtyGroup;
use treeterm_model::app_store::AppStore;
use treeterm_model::session_tty_list::{SessionTtyList, TtyListBody};
use treeterm_model::settings_store::SettingsStore;
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

    // Shown before the connection is up: it fails with why, as Electron's `getClient` does,
    // or, connected with the session still loading, waits for it.
    let list = cx.new(|cx| SessionTtyList::new(&app, id.clone(), cx));
    let body = list.read_with(cx, |l, cx| l.body(cx));
    assert!(
        body == TtyListBody::Failed(format!("Connection {id} is connecting"))
            || body == TtyListBody::Loading,
        "{body:?}"
    );

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
fn after_a_reconnect_the_list_refreshes_and_kills_on_the_new_connection(cx: &mut TestAppContext) {
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
    let list = cx.new(|cx| SessionTtyList::new(&app, id.clone(), cx));
    wait_until_app(cx, &list, |l, cx| {
        groups(l, cx) == Some(vec![group("Alpha", &[&pty])])
    });
    list.update(cx, |l, cx| l.select(&pty, cx));
    assert!(list.read_with(cx, |l, cx| {
        l.selected(cx).is_some_and(|(shown, _)| shown == pty)
    }));

    // The connection drops: listing fails with Electron's `getClient` error, and the viewer
    // closes.
    drop(proxy);
    let reconnecting = format!("Connection {id} is reconnecting: ");
    let down = |l: &SessionTtyList, cx: &gpui_kit::App| matches!(l.body(cx), TtyListBody::Failed(why) if why.starts_with(&reconnecting));
    wait_until_app(cx, &list, down);
    assert!(list.read_with(cx, |l, cx| l.selected(cx).is_none()));
    // Acting on it meanwhile does nothing.
    list.update(cx, |l, cx| l.kill(&pty, cx));
    list.update(cx, |l, cx| l.reload(cx));
    assert!(list.read_with(cx, down));

    // The next generation lists by itself; Refresh, Kill and Open use its connection.
    let _proxy = Proxy::start(&socket, h.daemon.socket_path());
    wait_until_app(cx, &list, |l, cx| {
        groups(l, cx) == Some(vec![group("Alpha", &[&pty])])
    });
    list.update(cx, |l, cx| l.reload(cx));
    assert_eq!(list.read_with(cx, |l, cx| l.body(cx)), TtyListBody::Loading);
    wait_until_app(cx, &list, |l, cx| {
        groups(l, cx) == Some(vec![group("Alpha", &[&pty])])
    });
    list.update(cx, |l, cx| l.select(&pty, cx));
    assert!(list.read_with(cx, |l, cx| {
        l.selected(cx).is_some_and(|(shown, _)| shown == pty)
    }));
    list.update(cx, |l, cx| l.deselect(cx));
    assert!(list.read_with(cx, |l, cx| l.selected(cx).is_none()));
    list.update(cx, |l, cx| l.kill(&pty, cx));
    wait_until_app(cx, &list, |l, cx| groups(l, cx) == Some(Vec::new()));
    let left = futures::executor::block_on(h.conn.list_pty_sessions())
        .unwrap()
        .unwrap();
    assert!(left.iter().all(|s| s.id != pty), "the PTY was killed");
}
