//! Where the window lands once a connection's session is restored: the app store's
//! `ShowSessionPane` / `ShowSession` events (`startRemoteConnect`, `addRemoteSession`,
//! `local:connect`). Remotes run against a fake ssh host in front of an in-process daemon.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use gpui_kit::{AppContext as _, Entity, Subscription, TestAppContext};
use tempfile::TempDir;
use treeterm_core::connection::fake::{FakeResponse, FakeSpawner};
use treeterm_core::connection::{ConnectionId, OutdatedDaemon};
use treeterm_core::local_daemon::LocalDaemonSpec;
use treeterm_model::app_store::{AppEvent, AppStore, ConnState, SessionSlot};
use treeterm_model::session_store::{SessionState, SessionStore, WorkspaceEntry};
use treeterm_model::settings_store::SettingsStore;
use treeterm_proto::treeterm::WorkspaceRef;
use treeterm_testkit::fake_ssh::{Proxy, fake_ssh_response, remote_config, ssh_env};

mod common;
use common::{Harness, harness, wait_until, wait_until_app};

/// The daemon (in this process) runs `echo $HOME` with this process's environment: point
/// `HOME` at an empty directory, so the remote home workspace and the worktree registry
/// never reach the real home. Every test calls this before anything starts.
fn isolated_home() -> &'static Path {
    static HOME: LazyLock<TempDir> = LazyLock::new(|| {
        let home = tempfile::Builder::new()
            .prefix("tt-home")
            .tempdir_in("/tmp")
            .unwrap();
        // SAFETY: set once, before this test binary starts a daemon, a runtime or any other
        // thread that reads the environment.
        unsafe { std::env::set_var("HOME", home.path()) };
        home
    });
    HOME.path()
}

/// An app whose ssh host is `h`'s daemon and whose local daemon is `local` (or none).
struct App {
    app: Entity<AppStore>,
    events: Rc<RefCell<Vec<AppEvent>>>,
    _events: Subscription,
    /// Set: the host refuses ssh (the bootstrap fails).
    refuse: Arc<AtomicBool>,
    _tunnels: Arc<Mutex<Vec<Proxy>>>,
    _dir: TempDir,
}

fn app(cx: &mut TestAppContext, h: &Harness, local: Option<&Path>) -> App {
    let dir = tempfile::Builder::new()
        .prefix("tt")
        .tempdir_in("/tmp")
        .unwrap();
    let tunnels = Arc::new(Mutex::new(Vec::new()));
    let refuse = Arc::new(AtomicBool::new(false));
    let spawner = FakeSpawner::with_responder({
        let (socket, tunnels, refuse) = (
            h.daemon.socket_path().to_owned(),
            tunnels.clone(),
            refuse.clone(),
        );
        move |command| {
            if refuse.load(Ordering::SeqCst) {
                FakeResponse::Exit {
                    stdout: String::new(),
                    stderr: "ssh: connect to host example.com port 22: Connection refused"
                        .to_owned(),
                    code: 255,
                }
            } else {
                fake_ssh_response(command, &socket, &tunnels)
            }
        }
    });
    // Without a local daemon the local connection fails at once (nothing to start).
    let spec = LocalDaemonSpec {
        socket_path: local.map_or_else(|| dir.path().join("none.sock"), Path::to_owned),
        binary: dir.path().join("never-spawned"),
        log_path: dir.path().join("daemon.log"),
        pid_file: dir.path().join("daemon.pid"),
    };
    let settings = cx.new(|_| SettingsStore::in_memory(Default::default()));
    let (rt, ssh) = (h.rt.clone(), ssh_env(dir.path()));
    let app = cx.new(|cx| AppStore::with_ssh(rt, spec, Arc::new(spawner), ssh, settings, cx));
    let events = Rc::new(RefCell::new(Vec::new()));
    let subscription = cx.update(|cx| {
        let events = events.clone();
        cx.subscribe(&app, move |_, event: &AppEvent, _| {
            events.borrow_mut().push(event.clone())
        })
    });
    App {
        app,
        events,
        _events: subscription,
        refuse,
        _tunnels: tunnels,
        _dir: dir,
    }
}

impl App {
    fn events(&self) -> Vec<AppEvent> {
        self.events.borrow().clone()
    }

    fn connect(&self, cx: &mut TestAppContext) {
        self.app.update(cx, |a, cx| {
            a.connect_remote(remote_config(), OutdatedDaemon::Report, cx)
        });
    }

    /// The remote connection's session store, once restored.
    fn restored(&self, cx: &mut TestAppContext) -> Entity<SessionStore> {
        wait_until_app(cx, &self.app, |a, cx| {
            remote(a)
                .and_then(SessionSlot::session)
                .is_some_and(|s| matches!(s.read(cx).state(), SessionState::Live))
        });
        self.app.read_with(cx, |a, _| {
            remote(a).and_then(SessionSlot::session).unwrap().clone()
        })
    }

    /// Wait until `count` events arrived.
    fn wait_for_events(&self, cx: &mut TestAppContext, count: usize) {
        let events = self.events.clone();
        wait_until(cx, &self.app, move |_| events.borrow().len() >= count);
    }
}

fn remote_id() -> ConnectionId {
    ConnectionId::new(remote_config().id)
}

fn remote(app: &AppStore) -> Option<&SessionSlot> {
    app.session_slot(&remote_id())
}

fn local_id(cx: &mut TestAppContext, app: &Entity<AppStore>) -> ConnectionId {
    app.read_with(cx, |a, _| {
        a.sessions()
            .find(|slot| slot.is_local())
            .unwrap()
            .id
            .clone()
    })
}

/// Let anything still under way (a home lookup, a write) happen.
fn settle(cx: &mut TestAppContext) {
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        cx.run_until_parked();
    }
}

fn body(name: &str) -> String {
    serde_json::json!({
        "name": name, "status": "active", "isGitRepo": false, "isWorktree": false,
        "appStates": {}, "metadata": {}, "createdAt": 1, "lastActivity": 1, "parentHash": ""
    })
    .to_string()
}

/// Workspaces `a` and `b` in `h`'s session.
fn publish_a_and_b(h: &Harness) {
    let path = h.daemon.root().to_string_lossy().into_owned();
    let refs = ["a", "b"]
        .into_iter()
        .map(|id| {
            std::fs::write(
                h.daemon.workspaces_dir().join(format!("{id}.json")),
                body(id),
            )
            .unwrap();
            WorkspaceRef {
                id: id.into(),
                path: path.clone(),
            }
        })
        .collect();
    futures::executor::block_on(h.conn.update_session(refs, None, "test-writer"))
        .unwrap()
        .unwrap();
}

fn workspaces(cx: &mut TestAppContext, session: &Entity<SessionStore>) -> Vec<String> {
    session.read_with(cx, |s, _| s.workspace_ids().cloned().collect())
}

fn active(cx: &mut TestAppContext, session: &Entity<SessionStore>) -> Option<String> {
    session.read_with(cx, |s, _| s.active_workspace().map(str::to_owned))
}

fn path_of(cx: &mut TestAppContext, session: &Entity<SessionStore>, id: &str) -> PathBuf {
    let id = id.to_owned();
    wait_until_app(cx, session, |s, _| {
        matches!(s.entry(&id), Some(WorkspaceEntry::Loaded(_)))
    });
    session.read_with(cx, |s, cx| {
        PathBuf::from(&s.workspace_store(&id).unwrap().read(cx).workspace().path)
    })
}

#[gpui_kit::test]
fn a_remote_session_without_workspaces_opens_the_remote_home_and_shows_it(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let home = isolated_home();
    let h = harness();
    let app = app(cx, &h, None);

    app.connect(cx);
    let session = app.restored(cx);
    app.wait_for_events(cx, 1);
    settle(cx);

    assert_eq!(app.events(), vec![AppEvent::ShowSession(remote_id())]);
    let opened = workspaces(cx, &session);
    assert_eq!(opened.len(), 1, "{opened:?}");
    assert_eq!(active(cx, &session).as_ref(), Some(&opened[0]));
    assert_eq!(path_of(cx, &session, &opened[0]), home);
}

#[gpui_kit::test]
fn a_remote_session_with_workspaces_shows_its_first_one(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    publish_a_and_b(&h);
    let app = app(cx, &h, None);

    app.connect(cx);
    let session = app.restored(cx);
    app.wait_for_events(cx, 1);
    settle(cx);

    assert_eq!(app.events(), vec![AppEvent::ShowSession(remote_id())]);
    assert_eq!(
        workspaces(cx, &session),
        ["a", "b"],
        "no home workspace is added"
    );
    assert_eq!(active(cx, &session).as_deref(), Some("a"));
}

#[gpui_kit::test]
fn the_picker_shows_the_session_pane_while_connecting_then_the_session(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    publish_a_and_b(&h);
    let app = app(cx, &h, None);

    app.app.update(cx, |a, cx| {
        a.start_remote_connect(remote_config(), OutdatedDaemon::Report, cx)
    });
    assert_eq!(app.events(), vec![AppEvent::ShowSessionPane(remote_id())]);
    assert!(app.app.read_with(cx, |a, _| matches!(
        remote(a).unwrap().state,
        ConnState::Connecting(_)
    )));

    app.restored(cx);
    app.wait_for_events(cx, 2);
    assert_eq!(
        app.events(),
        vec![
            AppEvent::ShowSessionPane(remote_id()),
            AppEvent::ShowSession(remote_id())
        ]
    );
}

#[gpui_kit::test]
fn connecting_again_to_a_live_session_shows_its_first_workspace_again(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    publish_a_and_b(&h);
    let app = app(cx, &h, None);
    app.connect(cx);
    let session = app.restored(cx);
    app.wait_for_events(cx, 1);
    session.update(cx, |s, cx| s.set_active_workspace("b".to_owned(), cx));

    // The manager keeps the healthy connection: the window lands at once, on its first one.
    app.app.update(cx, |a, cx| {
        a.start_remote_connect(remote_config(), OutdatedDaemon::Report, cx)
    });

    assert_eq!(
        app.events(),
        vec![
            AppEvent::ShowSession(remote_id()),
            AppEvent::ShowSessionPane(remote_id()),
            AppEvent::ShowSession(remote_id()),
        ]
    );
    assert_eq!(app.restored(cx), session, "the same session");
    assert_eq!(active(cx, &session).as_deref(), Some("a"));
}

#[gpui_kit::test]
fn a_reconnect_neither_lands_again_nor_opens_another_home_workspace(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    let app = app(cx, &h, None);
    app.connect(cx);
    let first = app.restored(cx);
    app.wait_for_events(cx, 1);
    // The home workspace is loaded and its ref written before the connection goes.
    wait_until(cx, &first, |s| s.workspace_ids().count() == 1);
    let home = workspaces(cx, &first).remove(0);
    path_of(cx, &first, &home);
    settle(cx);

    let forced = app
        .app
        .read_with(cx, |a, _| a.manager().force_reconnect(&remote_id()));
    forced.unwrap();
    wait_until_app(cx, &app.app, |a, cx| {
        remote(a)
            .and_then(SessionSlot::session)
            .is_some_and(|s| *s != first && matches!(s.read(cx).state(), SessionState::Live))
    });
    let second = app.restored(cx);
    settle(cx);

    assert_eq!(app.events(), vec![AppEvent::ShowSession(remote_id())]);
    assert_eq!(workspaces(cx, &second), [home]);
}

#[gpui_kit::test]
fn a_failed_connect_lands_nowhere_and_a_retry_shows_only_existing_workspaces(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    let app = app(cx, &h, None);
    app.refuse.store(true, Ordering::SeqCst);

    app.app.update(cx, |a, cx| {
        a.start_remote_connect(remote_config(), OutdatedDaemon::Report, cx)
    });
    wait_until(cx, &app.app, |a| {
        matches!(
            remote(a).map(|slot| &slot.state),
            Some(ConnState::Failed { .. })
        )
    });
    assert_eq!(app.events(), vec![AppEvent::ShowSessionPane(remote_id())]);

    // The sidebar's Reconnect is no connect call: the empty session gets no home workspace.
    app.refuse.store(false, Ordering::SeqCst);
    app.app.read_with(cx, |a, _| {
        a.reconnect(&remote_id());
        a.reconnect_now(&remote_id());
    });
    let session = app.restored(cx);
    settle(cx);

    assert_eq!(app.events(), vec![AppEvent::ShowSessionPane(remote_id())]);
    assert!(workspaces(cx, &session).is_empty());
}

#[gpui_kit::test]
fn a_failed_first_connect_retried_shows_its_first_workspace(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    publish_a_and_b(&h);
    let app = app(cx, &h, None);
    app.refuse.store(true, Ordering::SeqCst);
    app.connect(cx);
    wait_until(cx, &app.app, |a| {
        matches!(
            remote(a).map(|slot| &slot.state),
            Some(ConnState::Failed { .. })
        )
    });

    app.refuse.store(false, Ordering::SeqCst);
    app.app.read_with(cx, |a, _| {
        a.reconnect(&remote_id());
        a.reconnect_now(&remote_id());
    });
    let session = app.restored(cx);
    app.wait_for_events(cx, 1);

    assert_eq!(app.events(), vec![AppEvent::ShowSession(remote_id())]);
    assert_eq!(active(cx, &session).as_deref(), Some("a"));
}

#[gpui_kit::test]
fn the_local_session_shows_its_first_workspace_once_restored(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    publish_a_and_b(&h);
    let app = app(cx, &h, Some(h.daemon.socket_path()));
    let id = local_id(cx, &app.app);

    app.wait_for_events(cx, 1);

    assert_eq!(app.events(), vec![AppEvent::ShowSession(id)]);
    let session = app.app.read_with(cx, |a, _| a.local_session().unwrap());
    assert_eq!(active(cx, &session).as_deref(), Some("a"));
}

#[gpui_kit::test]
fn a_local_session_without_workspaces_lands_nowhere(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    let app = app(cx, &h, Some(h.daemon.socket_path()));

    wait_until_app(cx, &app.app, |a, cx| {
        a.local_session()
            .is_some_and(|s| matches!(s.read(cx).state(), SessionState::Live))
    });
    settle(cx);

    assert_eq!(app.events(), vec![]);
    let session = app.app.read_with(cx, |a, _| a.local_session().unwrap());
    assert!(
        workspaces(cx, &session).is_empty(),
        "no home workspace for a local session"
    );
}
