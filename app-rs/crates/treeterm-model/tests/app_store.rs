//! Where the window lands once a connection's session is restored: the app store's
//! `ShowSessionPane` / `ShowSession` events (`startRemoteConnect`, `addRemoteSession`,
//! `local:connect`), and what the session pane shows of a connection as it is lost and
//! replaced. Remotes run against a fake ssh host in front of an in-process daemon.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use gpui_kit::{AppContext as _, Entity, Subscription, TestAppContext};
use tempfile::TempDir;
use treeterm_core::connection::fake::{FakeResponse, FakeSpawner};
use treeterm_core::connection::process::ProcessCommand;
use treeterm_core::connection::{ConnectionId, OutdatedDaemon, OutputKind, StatusKind};
use treeterm_core::local_daemon::LocalDaemonSpec;
use treeterm_core::settings::{Extra, PortForwardSpec, SshConnectionConfig};
use treeterm_core::traffic::TrafficRate;
use treeterm_model::app_store::{AppEvent, AppStore, ConnState, IdlePane, SessionSlot};
use treeterm_model::browser_page::{BrowserPageModel, BrowserPhase};
use treeterm_model::session_info::SessionInfo;
use treeterm_model::session_store::{SessionState, SessionStore, WorkspaceEntry};
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::system_monitor::{Metrics, POLL_INTERVAL, SystemMonitor};
use treeterm_model::tool_tabs::ssh_upload::{LinkStatus, SshUpload, UploadTarget};
use treeterm_model::traffic::{ConnectionTraffic, Reading, TrafficMeter};
use treeterm_model::web_app::RemoteHost;
use treeterm_proto::treeterm::WorkspaceRef;
use treeterm_testkit::fake_ssh::{Proxy, fake_ssh_response, remote_config, ssh_env};

mod common;
use common::{Harness, Proxy as DaemonProxy, harness, refusing_daemon, wait_until, wait_until_app};

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
    /// The ssh host's daemon socket, as its next bootstrap reports it.
    host_daemon: Arc<Mutex<PathBuf>>,
    /// The ssh tunnels to the host's daemon: clearing them cuts the connection.
    tunnels: Arc<Mutex<Vec<Proxy>>>,
    _dir: TempDir,
}

fn app(cx: &mut TestAppContext, h: &Harness, local: Option<&Path>) -> App {
    let dir = tempfile::Builder::new()
        .prefix("tt")
        .tempdir_in("/tmp")
        .unwrap();
    let tunnels = Arc::new(Mutex::new(Vec::new()));
    let refuse = Arc::new(AtomicBool::new(false));
    let host_daemon = Arc::new(Mutex::new(h.daemon.socket_path().to_owned()));
    let spawner = FakeSpawner::with_responder({
        let (socket, tunnels, refuse) = (host_daemon.clone(), tunnels.clone(), refuse.clone());
        move |command| {
            if refuse.load(Ordering::SeqCst) {
                FakeResponse::Exit {
                    stdout: String::new(),
                    stderr: "ssh: connect to host example.com port 22: Connection refused"
                        .to_owned(),
                    code: 255,
                }
            } else if is_port_forward(command) {
                FakeResponse::Run
            } else {
                fake_ssh_response(command, &socket.lock().unwrap(), &tunnels)
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
        host_daemon,
        tunnels,
        _dir: dir,
    }
}

/// A forward of a TCP port (`-L <port>:<host>:<port>`), not the daemon socket's tunnel.
fn is_port_forward(command: &ProcessCommand) -> bool {
    command
        .args
        .iter()
        .skip_while(|arg| *arg != "-L")
        .nth(1)
        .is_some_and(|forward| !forward.contains(".sock"))
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
    publish(h, &[("a", body("a")), ("b", body("b"))]);
}

/// These workspaces, as `(id, body)`, in `h`'s session.
fn publish(h: &Harness, workspaces: &[(&str, String)]) {
    let path = h.daemon.root().to_string_lossy().into_owned();
    let refs = workspaces
        .iter()
        .map(|(id, body)| {
            std::fs::write(
                h.daemon.workspaces_dir().join(format!("{id}.json")),
                body,
            )
            .unwrap();
            WorkspaceRef {
                id: (*id).into(),
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

fn idle(cx: &mut TestAppContext, app: &App) -> IdlePane {
    app.app.read_with(cx, |a, cx| a.idle_pane(cx))
}

fn local_state(cx: &mut TestAppContext, app: &App) -> ConnState {
    app.app.read_with(cx, |a, _| {
        a.sessions()
            .find(|slot| slot.is_local())
            .unwrap()
            .state
            .clone()
    })
}

fn wait_for_local(cx: &mut TestAppContext, app: &App, state: impl Fn(&ConnState) -> bool) {
    wait_until(cx, &app.app, |a| {
        a.sessions()
            .any(|slot| slot.is_local() && state(&slot.state))
    });
}

/// With no session shown, the window shows the local connection's start-up until its session
/// has loaded: the splash while it connects and loads its session, then why it failed, or
/// that it is retrying. Then App.tsx's "Select a workspace to get started", also while the
/// loaded session is kept over a lost or stopped connection, and once every connection is
/// disconnected.
#[gpui_kit::test]
fn with_no_session_shown_the_window_shows_the_start_up_until_the_local_session_loads(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    // The app reaches the daemon through a proxy, so the test can hold or take it away.
    let dir = tempfile::Builder::new()
        .prefix("tt")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = dir.path().join("p.sock");
    let silent = DaemonProxy::silent(&socket, h.daemon.socket_path());
    let app = app(cx, &h, Some(&socket));
    let id = local_id(cx, &app.app);
    assert_eq!(idle(cx, &app), IdlePane::Splash, "connecting");

    // Connected, its daemon does not answer: the session waits.
    wait_until(cx, &app.app, |a| a.local_session().is_some());
    assert_eq!(idle(cx, &app), IdlePane::Splash, "loading the session");

    // Lost before the session loaded: the connect stops at its error.
    drop(silent);
    wait_for_local(cx, &app, |state| matches!(state, ConnState::Failed { .. }));
    let ConnState::Failed { error, .. } = local_state(cx, &app) else {
        unreachable!("waited for it")
    };
    assert_eq!(idle(cx, &app), IdlePane::Failed { error });

    // Retried while no daemon is reachable.
    app.app.read_with(cx, |a, _| a.reconnect(&id));
    wait_for_local(cx, &app, |state| matches!(state, ConnState::Reconnecting { .. }));
    let ConnState::Reconnecting { attempt, error } = local_state(cx, &app) else {
        unreachable!("waited for it")
    };
    assert_eq!(idle(cx, &app), IdlePane::Reconnecting { attempt, error });

    let proxy = DaemonProxy::start(&socket, h.daemon.socket_path());
    wait_until_app(cx, &app.app, |a, cx| {
        a.local_session()
            .is_some_and(|s| matches!(s.read(cx).state(), SessionState::Live))
    });
    assert_eq!(idle(cx, &app), IdlePane::SelectWorkspace, "loaded");

    // The daemon goes away: the loaded session is kept, reconnecting and once stopped.
    drop(proxy);
    wait_for_local(cx, &app, |state| matches!(state, ConnState::Reconnecting { .. }));
    assert_eq!(idle(cx, &app), IdlePane::SelectWorkspace, "kept while reconnecting");
    app.app.read_with(cx, |a, _| a.cancel_reconnect(&id));
    wait_for_local(cx, &app, |state| matches!(state, ConnState::Failed { .. }));
    assert_eq!(idle(cx, &app), IdlePane::SelectWorkspace, "kept once stopped");

    app.app.update(cx, |a, cx| a.disconnect(&id, cx));
    assert_eq!(idle(cx, &app), IdlePane::SelectWorkspace, "no connection left");
}

fn unmerged(cx: &mut TestAppContext, app: &App) -> Vec<(String, Option<String>)> {
    app.app.read_with(cx, |a, cx| a.unmerged_sub_workspaces(cx))
}

/// Closing the app asks about the active worktrees of every connection's session, also of a
/// connection that is down: while it reconnects, while its next generation's session loads
/// and once reconnecting is stopped, as Electron's `onCloseConfirm` walks every session store.
/// Disconnected, they go with it.
#[gpui_kit::test]
fn the_unmerged_worktrees_of_a_lost_connection_count_until_it_is_disconnected(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    let mut kid: serde_json::Value = serde_json::from_str(&body("kid")).unwrap();
    kid["isWorktree"] = true.into();
    kid["parentId"] = "root".into();
    kid["gitBranch"] = "kid-branch".into();
    publish(&h, &[("root", body("root")), ("kid", kid.to_string())]);
    let dir = tempfile::Builder::new()
        .prefix("tt")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = dir.path().join("p.sock");
    let proxy = DaemonProxy::start(&socket, h.daemon.socket_path());
    let app = app(cx, &h, Some(&socket));
    let id = local_id(cx, &app.app);
    let kid = vec![("kid".to_owned(), Some("kid-branch".to_owned()))];
    wait_until_app(cx, &app.app, |a, cx| !a.unmerged_sub_workspaces(cx).is_empty());
    assert_eq!(unmerged(cx, &app), kid);

    drop(proxy);
    wait_for_local(cx, &app, |state| matches!(state, ConnState::Reconnecting { .. }));
    assert_eq!(unmerged(cx, &app), kid, "reconnecting");

    // The next generation connects, but its daemon does not answer: its store waits for the
    // session, and the last one still stands for the connection.
    let silent = DaemonProxy::silent(&socket, h.daemon.socket_path());
    wait_until(cx, &app.app, |a| a.local_session().is_some());
    assert_eq!(unmerged(cx, &app), kid, "while the next session loads");

    drop(silent);
    wait_for_local(cx, &app, |state| matches!(state, ConnState::Reconnecting { .. }));
    app.app.read_with(cx, |a, _| a.cancel_reconnect(&id));
    wait_for_local(cx, &app, |state| matches!(state, ConnState::Failed { .. }));
    assert_eq!(unmerged(cx, &app), kid, "reconnecting stopped");

    app.app.update(cx, |a, cx| a.disconnect(&id, cx));
    assert_eq!(unmerged(cx, &app), vec![], "disconnected");
}

/// Exit and Kill Daemon reaches the local daemon through a client of its own, also while the
/// local connection has failed (`quitAndKillDaemon`'s temporary client). With nothing on the
/// socket it fails with why; a daemon that drops the call as it exits has shut down.
#[gpui_kit::test]
fn exit_and_kill_daemon_reaches_the_local_daemon_while_its_connection_has_failed(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    let dir = tempfile::Builder::new()
        .prefix("tt")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = dir.path().join("p.sock");
    let app = app(cx, &h, Some(&socket));
    wait_for_local(cx, &app, |state| matches!(state, ConnState::Failed { .. }));
    let shut_down = |cx: &mut TestAppContext| {
        let shutdown = app.app.read_with(cx, |a, _| a.shutdown_daemon());
        futures::executor::block_on(shutdown).expect("the shutdown ran").map_err(|error| error.to_string())
    };

    assert_eq!(
        shut_down(cx),
        Err(format!("daemon is not running: Daemon socket not found at {}", socket.display()))
    );

    // A daemon answers on the socket now, while the connection stays failed.
    let (refusing, _conn) = refusing_daemon(tonic::Status::permission_denied("not you"));
    let proxy = DaemonProxy::start(&socket, refusing.socket_path());
    assert_eq!(shut_down(cx), Err("7 PERMISSION_DENIED: not you".to_owned()));
    drop(proxy);

    let (exiting, _conn) = refusing_daemon(tonic::Status::unavailable("Connection reset by peer"));
    let _proxy = DaemonProxy::start(&socket, exiting.socket_path());
    assert_eq!(shut_down(cx), Ok(()));
    assert!(matches!(local_state(cx, &app), ConnState::Failed { .. }));
}

/// [`remote_config`] with a saved port forward.
fn config_with_a_forward() -> SshConnectionConfig {
    SshConnectionConfig {
        port_forwards: vec![PortForwardSpec {
            local_port: 18080,
            remote_host: "localhost".to_owned(),
            remote_port: 3000,
            extra: Extra::new(),
        }],
        ..remote_config()
    }
}

fn remote_error(cx: &mut TestAppContext, app: &App) -> Option<String> {
    app.app.read_with(cx, |a, _| match &remote(a)?.state {
        ConnState::Failed { error, .. } => Some(error.clone()),
        ConnState::Connecting(_) | ConnState::Connected(_) | ConnState::Reconnecting { .. } => None,
    })
}

#[gpui_kit::test]
fn a_remote_whose_session_does_not_load_stops_at_electrons_error_without_a_session(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    publish_a_and_b(&h);
    let app = app(cx, &h, None);
    // The host runs a daemon without SessionWatch.
    let (old, _conn) = refusing_daemon(tonic::Status::unimplemented("no SessionWatch"));
    *app.host_daemon.lock().unwrap() = old.socket_path().to_owned();

    app.app.update(cx, |a, cx| {
        a.start_remote_connect(config_with_a_forward(), OutdatedDaemon::Report, cx)
    });
    wait_until(cx, &app.app, |a| {
        matches!(
            remote(a).map(|slot| &slot.state),
            Some(ConnState::Failed { .. })
        )
    });
    settle(cx);

    assert_eq!(
        remote_error(cx, &app).as_deref(),
        Some(
            "Remote daemon is outdated. Retry with 'Refresh remote daemon' checked. (12 UNIMPLEMENTED: no SessionWatch)"
        )
    );
    app.app.read_with(cx, |a, _| {
        let slot = remote(a).unwrap();
        assert!(
            matches!(
                slot.state,
                ConnState::Failed {
                    hash_mismatch: false,
                    ..
                }
            ),
            "no Daemon Mismatch actions"
        );
        assert!(slot.shown_session().is_none(), "no session is added");
        assert!(
            a.manager().list_port_forwards(&remote_id()).is_empty(),
            "the saved forwards wait for the session"
        );
    });
    assert_eq!(app.events(), vec![AppEvent::ShowSessionPane(remote_id())]);
    // It stops there: no reconnect loop.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    cx.run_until_parked();
    assert!(remote_error(cx, &app).is_some());

    // Retried (the session pane's Retry) on a daemon that serves the session: it lands on
    // its first workspace, and the saved forwards start.
    *app.host_daemon.lock().unwrap() = h.daemon.socket_path().to_owned();
    app.app.update(cx, |a, cx| {
        a.reconnect_outdated(&remote_id(), OutdatedDaemon::Report, cx)
    });
    let session = app.restored(cx);
    app.wait_for_events(cx, 2);
    assert_eq!(
        app.events(),
        vec![
            AppEvent::ShowSessionPane(remote_id()),
            AppEvent::ShowSession(remote_id())
        ]
    );
    assert_eq!(active(cx, &session).as_deref(), Some("a"));
    let forwards = app
        .app
        .read_with(cx, |a, _| a.manager().list_port_forwards(&remote_id()));
    assert_eq!(forwards.len(), 1);
    assert_eq!(forwards[0].config.spec.local_port, 18080);
}

/// The remote connection's forwards, as `(id, local port)`.
fn forwards(cx: &mut TestAppContext, app: &App) -> Vec<(String, u16)> {
    app.app.read_with(cx, |a, _| {
        a.manager()
            .list_port_forwards(&remote_id())
            .into_iter()
            .map(|info| (info.config.id.as_str().to_owned(), info.config.spec.local_port))
            .collect()
    })
}

/// Wait for a session store other than `old` to be restored on the remote connection.
fn restored_anew(cx: &mut TestAppContext, app: &App, old: &Entity<SessionStore>) -> Entity<SessionStore> {
    wait_until_app(cx, &app.app, |a, cx| {
        remote(a)
            .and_then(SessionSlot::session)
            .is_some_and(|s| s != old && matches!(s.read(cx).state(), SessionState::Live))
    });
    app.restored(cx)
}

#[gpui_kit::test]
fn a_retry_after_the_reconnects_were_stopped_starts_the_saved_forwards_again(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    let app = app(cx, &h, None);
    app.app.update(cx, |a, cx| {
        a.connect_remote(config_with_a_forward(), OutdatedDaemon::Report, cx)
    });
    let first = app.restored(cx);
    let started = forwards(cx, &app);
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].1, 18080);

    // A reconnect keeps the connection, which restarts its own forwards: none is added.
    let forced = app.app.read_with(cx, |a, _| a.manager().force_reconnect(&remote_id()));
    forced.unwrap();
    let second = restored_anew(cx, &app, &first);
    settle(cx);
    assert_eq!(forwards(cx, &app), started);

    // Lost, the host refuses, and the reconnect loop is stopped.
    app.refuse.store(true, Ordering::SeqCst);
    let forced = app.app.read_with(cx, |a, _| a.manager().force_reconnect(&remote_id()));
    forced.unwrap();
    app.app.read_with(cx, |a, _| a.reconnect_now(&remote_id()));
    wait_until(cx, &app.app, |a| {
        matches!(
            remote(a).map(|slot| &slot.state),
            Some(ConnState::Reconnecting { error, .. }) if error.starts_with("SSH bootstrap failed")
        )
    });
    app.app.read_with(cx, |a, _| a.cancel_reconnect(&remote_id()));
    wait_until(cx, &app.app, |a| {
        matches!(remote(a).map(|slot| &slot.state), Some(ConnState::Failed { .. }))
    });

    // The session pane's Retry: a new connect replaces the connection, and its forwards
    // with it. They start again once its session has loaded (Electron's `ssh:connect`).
    app.refuse.store(false, Ordering::SeqCst);
    app.app.update(cx, |a, cx| {
        a.reconnect_outdated(&remote_id(), OutdatedDaemon::Report, cx)
    });
    restored_anew(cx, &app, &second);
    let again = forwards(cx, &app);
    assert_eq!(again.len(), 1, "{again:?}");
    assert_eq!(again[0].1, 18080);
    assert_ne!(again[0].0, started[0].0, "a new forward");
}

#[gpui_kit::test]
fn the_sidebars_reconnect_after_a_session_that_did_not_load_stops_at_the_error_again(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    let app = app(cx, &h, None);
    let (broken, _conn) = refusing_daemon(tonic::Status::internal("boom"));
    *app.host_daemon.lock().unwrap() = broken.socket_path().to_owned();
    app.connect(cx);
    wait_until(cx, &app.app, |a| {
        matches!(
            remote(a).map(|slot| &slot.state),
            Some(ConnState::Failed { .. })
        )
    });
    assert_eq!(
        remote_error(cx, &app).as_deref(),
        Some("Connected but failed to load session: 13 INTERNAL: boom")
    );

    let changes = RefCell::new(app.app.read_with(cx, |a, _| a.manager().status_changes()));
    let kinds = RefCell::new(Vec::new());
    let record = || {
        let infos = changes.borrow_mut().drain_ready();
        kinds
            .borrow_mut()
            .extend(infos.iter().map(|info| info.status.kind()));
        kinds.borrow_mut().dedup();
    };
    app.app.read_with(cx, |a, _| {
        a.reconnect(&remote_id());
        a.reconnect_now(&remote_id());
    });
    wait_until(cx, &app.app, |a| {
        record();
        kinds
            .borrow()
            .ends_with(&[StatusKind::Connected, StatusKind::Error])
            && matches!(
                remote(a).map(|slot| &slot.state),
                Some(ConnState::Failed { .. })
            )
    });
    assert_eq!(
        remote_error(cx, &app).as_deref(),
        Some("Connected but failed to load session: 13 INTERNAL: boom")
    );
    app.app.read_with(cx, |a, _| {
        assert!(remote(a).unwrap().shown_session().is_none());
    });
    // And stops there.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    cx.run_until_parked();
    record();
    assert_eq!(
        *kinds.borrow(),
        [
            StatusKind::Reconnecting,
            StatusKind::Connected,
            StatusKind::Error
        ]
    );
}

fn tick(cx: &mut TestAppContext) {
    cx.executor()
        .advance_clock(treeterm_core::traffic::TRAFFIC_TICK);
    cx.run_until_parked();
}

/// The remote connection's download meters, and what its own one shows.
fn remote_traffic(
    cx: &mut TestAppContext,
    app: &App,
) -> (Entity<ConnectionTraffic>, Entity<TrafficMeter>, TrafficRate) {
    let traffic = app
        .app
        .read_with(cx, |a, _| remote(a).unwrap().traffic().clone());
    let meter = traffic.read_with(cx, |t, _| t.connection().clone());
    let rate = meter.read_with(cx, |m, _| m.rate());
    (traffic, meter, rate)
}

#[gpui_kit::test]
fn the_connections_download_meter_spans_its_reconnects(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    let app = app(cx, &h, None);
    app.connect(cx);
    let first = app.restored(cx);
    tick(cx);
    let (traffic, meter, before) = remote_traffic(cx, &app);
    assert!(before.total_bytes > 0, "the session was downloaded");

    let forced = app
        .app
        .read_with(cx, |a, _| a.manager().force_reconnect(&remote_id()));
    forced.unwrap();
    wait_until_app(cx, &app.app, |a, cx| {
        remote(a)
            .and_then(SessionSlot::session)
            .is_some_and(|s| *s != first && matches!(s.read(cx).state(), SessionState::Live))
    });
    tick(cx);

    let (traffic_after, meter_after, after) = remote_traffic(cx, &app);
    assert_eq!((traffic_after, meter_after), (traffic, meter.clone()));
    // Never "↓ …" again, and the next generation's bytes count on from the total.
    assert!(matches!(meter.read_with(cx, |m, _| m.reading()), Reading::Ready(_)));
    assert!(after.total_bytes > before.total_bytes);
}

#[gpui_kit::test]
fn a_failed_connection_replaced_by_a_new_connect_keeps_its_download_meter(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    let app = app(cx, &h, None);
    let (broken, _conn) = refusing_daemon(tonic::Status::internal("boom"));
    *app.host_daemon.lock().unwrap() = broken.socket_path().to_owned();
    app.connect(cx);
    wait_until(cx, &app.app, |a| {
        matches!(
            remote(a).map(|slot| &slot.state),
            Some(ConnState::Failed { .. })
        )
    });
    tick(cx);
    let (traffic, meter, before) = remote_traffic(cx, &app);
    // A refusal carries no response message, so it counts nothing (`grpcByteCounter.ts`).
    assert_eq!(before.total_bytes, 0);
    assert!(meter.read_with(cx, |m, _| m.is_sampled()));

    // Connect again: the failed connection is replaced (`connectionManager.ts`: a replaced
    // stale entry hands over its counter).
    *app.host_daemon.lock().unwrap() = h.daemon.socket_path().to_owned();
    app.connect(cx);
    app.restored(cx);
    tick(cx);

    let (traffic_after, meter_after, after) = remote_traffic(cx, &app);
    assert_eq!((traffic_after, meter_after), (traffic, meter));
    assert!(after.total_bytes > before.total_bytes);
}

const LOGS: [OutputKind; 3] = [
    OutputKind::Bootstrap,
    OutputKind::Tunnel,
    OutputKind::Daemon,
];

/// The session pane's `kind` log.
fn pane_log(cx: &mut TestAppContext, info: &Entity<SessionInfo>, kind: OutputKind) -> Vec<String> {
    info.read_with(cx, |info, _| info.log(kind).to_vec())
}

/// The remote connection's `kind` scrollback, as the manager holds it now.
fn scrollback(cx: &mut TestAppContext, app: &App, kind: OutputKind) -> Vec<String> {
    app.app.read_with(cx, |a, _| {
        a.manager()
            .watch_output(&remote_id(), kind)
            .unwrap()
            .initial
    })
}

fn info_lines(cx: &mut TestAppContext, info: &Entity<SessionInfo>) -> Vec<String> {
    info.read_with(cx, |info, cx| info.info_lines(cx))
}

/// The host's refusal, as the bootstrap log shows it.
fn refused(lines: &[String]) -> bool {
    lines
        .iter()
        .any(|line| line.ends_with("port 22: Connection refused"))
}

#[gpui_kit::test]
fn the_session_panes_logs_follow_the_connection_that_replaces_a_failed_one(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    let app = app(cx, &h, None);
    app.refuse.store(true, Ordering::SeqCst);
    app.connect(cx);
    wait_until(cx, &app.app, |a| {
        matches!(
            remote(a).map(|slot| &slot.state),
            Some(ConnState::Failed { .. })
        )
    });
    let info = cx.new(|cx| SessionInfo::new(&app.app, remote_id(), cx));
    assert!(refused(&pane_log(cx, &info, OutputKind::Bootstrap)));
    assert!(pane_log(cx, &info, OutputKind::Daemon).is_empty());
    let lines = info_lines(cx, &info);
    assert!(lines.contains(&"Status: error".to_owned()), "{lines:?}");
    assert!(
        lines.iter().any(|line| line.starts_with("Error: ")),
        "{lines:?}"
    );

    // The pane's Retry: a new connect replaces the failed connection, whose logs end.
    app.refuse.store(false, Ordering::SeqCst);
    app.connect(cx);
    app.restored(cx);
    wait_until(cx, &info, |info| {
        info.log(OutputKind::Daemon).last().map(String::as_str) == Some("Connected to daemon")
    });

    for kind in LOGS {
        assert_eq!(
            pane_log(cx, &info, kind),
            scrollback(cx, &app, kind),
            "{kind:?}"
        );
    }
    assert!(!refused(&pane_log(cx, &info, OutputKind::Bootstrap)));
}

#[gpui_kit::test]
fn a_reconnect_attempt_starts_the_session_panes_logs_over_and_the_info_tab_shows_no_error(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    let app = app(cx, &h, None);
    app.connect(cx);
    app.restored(cx);
    let info = cx.new(|cx| SessionInfo::new(&app.app, remote_id(), cx));
    let daemon = pane_log(cx, &info, OutputKind::Daemon);
    assert_eq!(
        daemon,
        ["Connecting to daemon via gRPC...", "Connected to daemon"]
    );

    // Lost, and the host refuses the next attempt.
    app.refuse.store(true, Ordering::SeqCst);
    let forced = app
        .app
        .read_with(cx, |a, _| a.manager().force_reconnect(&remote_id()));
    forced.unwrap();
    app.app.read_with(cx, |a, _| a.reconnect_now(&remote_id()));
    let app_store = app.app.clone();
    wait_until_app(cx, &info, |info, cx| {
        let attempt_failed = matches!(
            remote(app_store.read(cx)).map(|slot| &slot.state),
            Some(ConnState::Reconnecting { error, .. }) if error.starts_with("SSH bootstrap failed")
        );
        attempt_failed && refused(info.log(OutputKind::Bootstrap))
    });

    // Only the attempt's own lines: the connected tunnel's are gone.
    let bootstrap = pane_log(cx, &info, OutputKind::Bootstrap);
    let starts = bootstrap
        .iter()
        .filter(|line| line.starts_with("Bootstrapping remote daemon on "))
        .count();
    assert_eq!(starts, 1, "{bootstrap:?}");
    for kind in LOGS {
        assert_eq!(
            pane_log(cx, &info, kind),
            scrollback(cx, &app, kind),
            "{kind:?}"
        );
    }
    assert_eq!(pane_log(cx, &info, OutputKind::Daemon), daemon);
    // Electron shows the error once the connection stopped, not while it retries.
    let lines = info_lines(cx, &info);
    assert!(
        lines.contains(&"Status: reconnecting".to_owned()),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line.starts_with("Error: ")),
        "{lines:?}"
    );

    app.app
        .read_with(cx, |a, _| a.cancel_reconnect(&remote_id()));
    wait_until(cx, &app.app, |a| {
        matches!(
            remote(a).map(|slot| &slot.state),
            Some(ConnState::Failed { .. })
        )
    });
    let lines = info_lines(cx, &info);
    assert!(lines.contains(&"Status: error".to_owned()), "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("Error: SSH bootstrap failed")),
        "{lines:?}"
    );
}

/// The session pane's JSON tab, and the local connection's state it was read with.
fn pane_json(
    cx: &mut TestAppContext,
    app: &App,
    info: &Entity<SessionInfo>,
) -> (serde_json::Value, ConnState) {
    app.app.read_with(cx, |a, cx| {
        let state = a
            .sessions()
            .find(|slot| slot.is_local())
            .unwrap()
            .state
            .clone();
        (info.read(cx).debug_json(cx), state)
    })
}

/// The session pane's JSON tab (`sessionData`) at every status of the local connection: the
/// connection as Electron's `ConnectionInfo`, alone until its first session, then with the
/// session, also the one kept while the connection is down. Disconnected, nothing.
#[gpui_kit::test]
fn the_session_panes_json_shows_the_connection_and_its_session_at_any_status(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    publish(&h, &[("a", body("a"))]);
    let dir = tempfile::Builder::new()
        .prefix("tt")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = dir.path().join("p.sock");
    let app = app(cx, &h, Some(&socket));
    let id = local_id(cx, &app.app);
    let info = cx.new(|cx| SessionInfo::new(&app.app, id.clone(), cx));
    // The local connection's `ConnectionInfo` with these status fields.
    let local = |status: serde_json::Value| {
        let mut connection =
            serde_json::json!({ "id": id.to_string(), "target": { "type": "local" } });
        let serde_json::Value::Object(status) = status else {
            unreachable!("an object")
        };
        connection.as_object_mut().unwrap().extend(status);
        connection
    };

    // Nothing on the socket: the first connect fails before there is a session.
    wait_for_local(cx, &app, |state| matches!(state, ConnState::Failed { .. }));
    let (json, state) = pane_json(cx, &app, &info);
    let ConnState::Failed { error, .. } = state else {
        unreachable!("waited for it")
    };
    let failed =
        local(serde_json::json!({ "status": "error", "error": error, "errorKind": "generic" }));
    assert_eq!(json, serde_json::json!({ "connection": failed }));

    // Retried while still unreachable.
    app.app.read_with(cx, |a, _| a.reconnect(&id));
    wait_for_local(cx, &app, |state| {
        matches!(state, ConnState::Reconnecting { .. })
    });
    let (json, state) = pane_json(cx, &app, &info);
    let ConnState::Reconnecting { attempt, error } = state else {
        unreachable!("waited for it")
    };
    let retrying =
        local(serde_json::json!({ "status": "reconnecting", "error": error, "attempt": attempt }));
    assert_eq!(json, serde_json::json!({ "connection": retrying }));

    // Connected: the session, with its connection.
    let proxy = DaemonProxy::start(&socket, h.daemon.socket_path());
    wait_until_app(cx, &app.app, |a, cx| {
        a.local_session()
            .is_some_and(|s| matches!(s.read(cx).entry("a"), Some(WorkspaceEntry::Loaded(_))))
    });
    let (json, _) = pane_json(cx, &app, &info);
    assert_eq!(
        json["connection"],
        local(serde_json::json!({ "status": "connected" }))
    );
    assert!(
        json["sessionId"].as_str().is_some_and(|id| !id.is_empty()),
        "{json}"
    );
    assert_eq!(json["activeWorkspaceId"], "a");
    assert_eq!(json["workspaces"]["a"]["status"], "loaded");

    // Lost: the session kept from that generation, while reconnecting and once stopped.
    drop(proxy);
    wait_for_local(cx, &app, |state| {
        matches!(state, ConnState::Reconnecting { .. })
    });
    let (json, state) = pane_json(cx, &app, &info);
    let ConnState::Reconnecting { attempt, error } = state else {
        unreachable!("waited for it")
    };
    let lost =
        local(serde_json::json!({ "status": "reconnecting", "error": error, "attempt": attempt }));
    assert_eq!(json["connection"], lost);
    assert_eq!(json["activeWorkspaceId"], "a");
    assert_eq!(json["workspaces"]["a"]["status"], "loaded");

    app.app.read_with(cx, |a, _| a.cancel_reconnect(&id));
    wait_for_local(cx, &app, |state| matches!(state, ConnState::Failed { .. }));
    let (json, state) = pane_json(cx, &app, &info);
    let ConnState::Failed { error, .. } = state else {
        unreachable!("waited for it")
    };
    let stopped =
        local(serde_json::json!({ "status": "error", "error": error, "errorKind": "generic" }));
    assert_eq!(json["connection"], stopped);
    assert_eq!(json["workspaces"]["a"]["status"], "loaded");

    app.app.update(cx, |a, cx| a.disconnect(&id, cx));
    assert_eq!(
        info.read_with(cx, |info, cx| info.debug_json(cx)),
        serde_json::Value::Null
    );
}

/// The session pane's SSH > Monitor and SSH > Upload on the remote connection.
fn monitor_and_upload(
    cx: &mut TestAppContext,
    app: &App,
) -> (Entity<SystemMonitor>, Entity<SshUpload>) {
    let monitor = cx.new(|cx| SystemMonitor::new(app.app.clone(), remote_id(), cx));
    let remote = RemoteHost {
        app: app.app.downgrade(),
        connection: remote_id(),
    };
    let upload = cx.new(|cx| SshUpload::new(Some(remote), cx));
    (monitor, upload)
}

fn link(cx: &mut TestAppContext, upload: &Entity<SshUpload>) -> LinkStatus {
    match upload.read_with(cx, |u, cx| u.target(cx)) {
        UploadTarget::Remote { status, .. } => status,
        other => panic!("not a remote target: {other:?}"),
    }
}

fn failed_with(monitor: &SystemMonitor, prefix: &str) -> bool {
    matches!(monitor.metrics(), Metrics::Failed(why) if why.starts_with(prefix))
}

/// The next poll, and what it read.
fn poll_again(
    cx: &mut TestAppContext,
    monitor: &Entity<SystemMonitor>,
    done: impl Fn(&SystemMonitor) -> bool,
) {
    cx.executor().advance_clock(POLL_INTERVAL);
    wait_until(cx, monitor, done);
}

#[gpui_kit::test]
fn the_monitor_and_upload_keep_their_state_while_the_connection_is_down_and_work_again_after(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    let app = app(cx, &h, None);
    app.connect(cx);
    let first = app.restored(cx);
    let (monitor, upload) = monitor_and_upload(cx, &app);
    wait_until(cx, &monitor, |m| matches!(m.metrics(), Metrics::Loaded(_)));
    assert_eq!(link(cx, &upload), LinkStatus::Connected);
    assert!(upload.read_with(cx, |u, cx| u.fs(cx).is_some()));
    upload.update(cx, |u, cx| {
        u.set_local_path(PathBuf::from("/tmp/notes.txt"), cx);
        u.set_remote_dir("/srv/drop", cx);
    });

    // The host goes away and stays away.
    app.refuse.store(true, Ordering::SeqCst);
    app.tunnels.lock().unwrap().clear();
    wait_until(cx, &app.app, |a| {
        matches!(
            remote(a).map(|slot| &slot.state),
            Some(ConnState::Reconnecting { .. })
        )
    });
    // The monitor's next run fails with Electron's `getClient` error, keeping its history.
    poll_again(cx, &monitor, |m| {
        failed_with(m, "Connection remote-1 is reconnecting: ")
    });
    assert_eq!(
        monitor.read_with(cx, |m, cx| m.cpu_samples(cx).len()),
        1
    );
    // Retry shows "Collecting…", then why again.
    monitor.update(cx, |m, cx| m.refresh(cx));
    assert!(matches!(
        monitor.read_with(cx, |m, _| m.metrics().clone()),
        Metrics::Loading
    ));
    wait_until(cx, &monitor, |m| {
        failed_with(m, "Connection remote-1 is reconnecting: ")
    });
    // Upload says why it waits, keeps the file and directory, and has no host to browse.
    assert_eq!(
        link(cx, &upload),
        LinkStatus::Down(StatusKind::Reconnecting)
    );
    upload.read_with(cx, |u, cx| {
        assert_eq!(u.local_path(), Some(Path::new("/tmp/notes.txt")));
        assert_eq!(u.remote_dir(cx), "/srv/drop");
        assert!(!u.can_upload(cx));
        assert!(u.fs(cx).is_none());
    });

    // Back: both work on the new generation by themselves.
    app.refuse.store(false, Ordering::SeqCst);
    app.app.read_with(cx, |a, _| a.reconnect_now(&remote_id()));
    wait_until_app(cx, &app.app, |a, cx| {
        remote(a)
            .and_then(SessionSlot::session)
            .is_some_and(|s| *s != first && matches!(s.read(cx).state(), SessionState::Live))
    });
    poll_again(cx, &monitor, |m| matches!(m.metrics(), Metrics::Loaded(_)));
    assert_eq!(
        monitor.read_with(cx, |m, cx| m.cpu_samples(cx).len()),
        2
    );
    assert_eq!(link(cx, &upload), LinkStatus::Connected);
    upload.read_with(cx, |u, cx| {
        assert_eq!(u.local_path(), Some(Path::new("/tmp/notes.txt")));
        assert!(u.can_upload(cx));
        assert!(u.fs(cx).is_some());
    });
}

#[gpui_kit::test]
fn a_failed_connection_reads_error_in_the_monitor_and_upload(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    let app = app(cx, &h, None);
    app.refuse.store(true, Ordering::SeqCst);
    app.connect(cx);
    wait_until(cx, &app.app, |a| {
        matches!(
            remote(a).map(|slot| &slot.state),
            Some(ConnState::Failed { .. })
        )
    });

    let (monitor, upload) = monitor_and_upload(cx, &app);
    wait_until(cx, &monitor, |m| {
        failed_with(m, "Connection remote-1 is error: ")
    });
    assert_eq!(link(cx, &upload), LinkStatus::Down(StatusKind::Error));
    assert!(upload.read_with(cx, |u, cx| u.fs(cx).is_none()));

    // Removed: the connection is not found.
    app.app.update(cx, |a, cx| a.disconnect(&remote_id(), cx));
    poll_again(cx, &monitor, |m| {
        failed_with(m, "Connection not found: remote-1")
    });
}

/// Real time for the forward to settle (the ssh forward is Active after its stabilization),
/// the executor's clock for the page's polls of it.
fn wait_for_page(
    cx: &mut TestAppContext,
    page: &Entity<BrowserPageModel>,
    done: impl Fn(&BrowserPhase) -> bool,
) -> BrowserPhase {
    for _ in 0..500 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
        let phase = page.read_with(cx, |p, _| p.phase().clone());
        if done(&phase) {
            return phase;
        }
    }
    panic!("the page never got there");
}

#[gpui_kit::test]
fn a_remote_browser_page_opens_anew_on_its_forward_after_a_restart(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    isolated_home();
    let h = harness();
    let app = app(cx, &h, None);
    app.connect(cx);
    app.restored(cx);
    let remote = RemoteHost {
        app: app.app.downgrade(),
        connection: remote_id(),
    };
    let home = "http://localhost:3000/app/".to_owned();
    let page = cx.new(|cx| BrowserPageModel::new(home, "tab-1".to_owned(), None, Some(remote), cx));
    let mount = |cx: &mut TestAppContext| page.read_with(cx, |p, _| p.page_mount());
    assert_eq!(
        page.read_with(cx, |p, _| p.phase().clone()),
        BrowserPhase::Forwarding { port: 3000 }
    );
    assert_eq!(mount(cx), 0);

    // Ready on the forward's local end: the page opens for the first time.
    let ready = wait_for_page(cx, &page, |phase| matches!(phase, BrowserPhase::Ready { .. }));
    let forwards = app
        .app
        .read_with(cx, |a, _| a.manager().list_port_forwards(&remote_id()));
    let local = forwards[0].config.spec.local_port;
    assert_eq!(
        ready,
        BrowserPhase::Ready {
            home: format!("http://localhost:{local}/app/")
        }
    );
    assert_eq!(mount(cx), 1);

    // Restart goes through "Starting port forward…": the page closes, and opens anew once
    // the new forward is up (on the same local port, which it prefers).
    page.update(cx, |p, cx| p.restart_forward(cx));
    assert_eq!(
        page.read_with(cx, |p, _| p.phase().clone()),
        BrowserPhase::Forwarding { port: 3000 }
    );
    assert_eq!(
        wait_for_page(cx, &page, |phase| matches!(phase, BrowserPhase::Ready { .. })),
        ready
    );
    assert_eq!(mount(cx), 2);
    page.update(cx, |p, cx| p.close(cx));
}
