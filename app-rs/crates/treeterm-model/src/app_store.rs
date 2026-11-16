//! Root store: every connection (the local daemon and ssh remotes) and, for each connected
//! one, its session store. Port of the session half of `src/renderer/store/app.ts` over
//! `treeterm_core::connection::ConnectionManager` (Electron's `connectionManager.ts`).
//!
//! The manager owns connecting, liveness and reconnecting. Each `Connected` status carries a
//! new connection generation; its session store is built on it. When a connection is lost
//! the store is kept, told it is disconnected: it is shown dimmed and defers its body
//! writes. When the next generation connects, the kept store writes those bodies on it
//! (`handleReconnected`'s flush) and is then disposed explicitly — a ghost store left alive
//! keeps writing the same files as its replacement — before a fresh one is built.

use std::sync::Arc;

use futures::StreamExt as _;
use gpui_kit::{AppContext as _, Context, Entity, Subscription, Task};
use indexmap::IndexMap;
use treeterm_core::connection::dialer::GrpcDialer;
use treeterm_core::connection::process::TokioSpawner;
use treeterm_core::connection::ssh::DaemonBinaries;
use treeterm_core::connection::{
    ConnectPhase, ConnectionErrorKind, ConnectionId, ConnectionInfo, ConnectionManager, ConnectionStatus,
    ConnectionTarget, OutdatedDaemon, SshEnvironment,
};
use treeterm_core::daemon::DaemonConnection;
use treeterm_core::local_daemon::LocalDaemonSpec;
use treeterm_core::rt::Rt;
use treeterm_core::settings::SshConnectionConfig;

use crate::session_store::{ConnectionStatus as SessionConnection, SessionEvent, SessionStore};
use crate::settings_store::SettingsStore;

/// What a connection is doing, with its session store once connected.
#[derive(Clone)]
pub enum ConnState {
    Connecting(ConnectPhase),
    Connected(Entity<SessionStore>),
    /// Lost; the manager retries with backoff.
    Reconnecting { attempt: u32, error: String },
    /// The first connect failed or reconnecting was stopped. `hash_mismatch`: the remote
    /// daemon is outdated (offer refresh / connect anyway).
    Failed { error: String, hash_mismatch: bool },
}

/// One connection in the sidebar.
pub struct SessionSlot {
    pub id: ConnectionId,
    pub target: ConnectionTarget,
    /// While a kept store hands over to the next generation, the state stays the one the
    /// connection was lost with.
    pub state: ConnState,
    previous: Previous,
    /// The saved port forwards were started (once per connection; the manager restarts
    /// them after reconnects).
    forwards_started: bool,
    _events: Option<Subscription>,
}

/// The last connection generation's session store, kept while the connection is down, as
/// Electron keeps the store and dims its tree.
enum Previous {
    Nothing,
    /// Disconnected: shown dimmed and inert; its body writes are deferred.
    Kept(Entity<SessionStore>),
    /// The next generation is up: the store writes its deferred bodies on it
    /// ([`SessionStore::hand_over`]), then it is disposed and replaced.
    HandingOver { session: Entity<SessionStore>, task: Task<()> },
}

impl SessionSlot {
    /// "Local", or the ssh label / `user@host`.
    pub fn name(&self) -> String {
        match &self.target {
            ConnectionTarget::Local => "Local".to_owned(),
            ConnectionTarget::Remote(config) => config.display_name(),
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self.target, ConnectionTarget::Local)
    }

    /// The session store of the connected generation.
    pub fn session(&self) -> Option<&Entity<SessionStore>> {
        match &self.state {
            ConnState::Connected(session) => Some(session),
            ConnState::Connecting(_) | ConnState::Reconnecting { .. } | ConnState::Failed { .. } => None,
        }
    }

    /// The session store the window shows: the connected one, else the last generation's
    /// while the connection is down (dimmed and inert).
    pub fn shown_session(&self) -> Option<&Entity<SessionStore>> {
        self.session().or(match &self.previous {
            Previous::Nothing => None,
            Previous::Kept(session) | Previous::HandingOver { session, .. } => Some(session),
        })
    }
}

/// Something the windows should react to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEvent {
    /// A workspace was marked unread: ding if sound is on.
    Attention { workspace: String },
}

impl gpui_kit::EventEmitter<AppEvent> for AppStore {}

pub struct AppStore {
    /// The LLM client every session's analyzers share (`None` if it could not be built).
    llm: Option<crate::analyzer::LlmService>,
    settings: Entity<SettingsStore>,
    /// Identifies this process to the daemon (`sender_id`, `listener_id`), so it can skip
    /// echoes of its own session updates. One per process: windows share stores.
    client_id: String,
    manager: ConnectionManager<GrpcDialer>,
    sessions: IndexMap<ConnectionId, SessionSlot>,
    _status: Task<()>,
}

impl AppStore {
    pub fn new(rt: Rt, spec: LocalDaemonSpec, settings: Entity<SettingsStore>, cx: &mut Context<Self>) -> Self {
        let llm = match crate::analyzer::LlmService::http(rt.clone()) {
            Ok(llm) => Some(llm),
            Err(error) => {
                tracing::error!(%error, "no LLM client: AI-harness analysis is off");
                None
            }
        };
        // Remote hosts get the musl daemons shipped next to the local one.
        let daemon_dir = spec
            .binary
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_default();
        let ssh = SshEnvironment {
            daemon_binaries: DaemonBinaries::new(daemon_dir),
            forward_socket_dir: treeterm_core::connection::socket_path::forward_socket_dir(
                treeterm_core::paths::current_uid(),
            ),
        };
        let mut manager =
            ConnectionManager::new(rt.clone(), GrpcDialer::new(rt, spec), Arc::new(TokioSpawner), ssh);
        // Subscribe before connecting so no status is missed.
        let mut updates = manager.status_changes();
        let status = cx.spawn(async move |this, cx| {
            while let Some(info) = updates.next().await {
                if this.update(cx, |store, cx| store.on_status(info, cx)).is_err() {
                    return;
                }
            }
        });
        let local = manager.connect_local();
        let mut store = Self {
            llm,
            settings,
            client_id: uuid::Uuid::new_v4().to_string(),
            manager,
            sessions: IndexMap::new(),
            _status: status,
        };
        store.insert_slot(local.id().clone(), ConnectionTarget::Local);
        store
    }

    fn insert_slot(&mut self, id: ConnectionId, target: ConnectionTarget) {
        self.sessions.entry(id.clone()).or_insert(SessionSlot {
            id,
            target,
            state: ConnState::Connecting(ConnectPhase::Daemon),
            previous: Previous::Nothing,
            forwards_started: false,
            _events: None,
        });
    }

    /// Connections in sidebar order (local first, then remotes as connected).
    pub fn sessions(&self) -> impl Iterator<Item = &SessionSlot> {
        self.sessions.values()
    }

    pub fn session_slot(&self, id: &ConnectionId) -> Option<&SessionSlot> {
        self.sessions.get(id)
    }

    /// The local connection's session store, when connected.
    pub fn local_session(&self) -> Option<Entity<SessionStore>> {
        self.sessions.values().find(|slot| slot.is_local()).and_then(|slot| slot.session().cloned())
    }

    pub fn manager(&self) -> &ConnectionManager<GrpcDialer> {
        &self.manager
    }

    pub fn settings(&self) -> &Entity<SettingsStore> {
        &self.settings
    }

    /// Connect to an ssh host (`connectRemote`). An existing healthy connection is kept.
    pub fn connect_remote(&mut self, config: SshConnectionConfig, outdated: OutdatedDaemon, cx: &mut Context<Self>) {
        let target = ConnectionTarget::Remote(Arc::new(config.clone()));
        let settled = self.manager.connect_remote(config, outdated);
        let id = settled.id().clone();
        // A replaced (failed) connection starts over as connecting.
        if let Some(slot) = self.sessions.get_mut(&id)
            && !matches!(slot.state, ConnState::Connected(_))
        {
            slot.state = ConnState::Connecting(ConnectPhase::Bootstrap);
        }
        self.insert_slot(id, target);
        cx.notify();
    }

    /// Close a connection; its sidebar entry goes away (local reconnects stay possible
    /// through the manager only on the next start).
    pub fn disconnect(&mut self, id: &ConnectionId, cx: &mut Context<Self>) {
        self.manager.disconnect(id);
        if let Some(slot) = self.sessions.shift_remove(id) {
            Self::retire(slot, cx);
        }
        cx.notify();
    }

    /// Connect again to an outdated remote daemon, refreshing it or accepting it.
    pub fn reconnect_outdated(&mut self, id: &ConnectionId, outdated: OutdatedDaemon, cx: &mut Context<Self>) {
        let config = match self.sessions.get(id).map(|slot| &slot.target) {
            Some(ConnectionTarget::Remote(config)) => (**config).clone(),
            Some(ConnectionTarget::Local) | None => return,
        };
        self.connect_remote(config, outdated, cx);
    }

    /// Retry a failed connection.
    pub fn reconnect(&self, id: &ConnectionId) {
        self.manager.reconnect(id);
    }

    /// Skip the backoff wait.
    pub fn reconnect_now(&self, id: &ConnectionId) {
        self.manager.reconnect_now(id);
    }

    /// Stop retrying (the connection fails with "Reconnection cancelled").
    pub fn cancel_reconnect(&self, id: &ConnectionId) {
        self.manager.cancel_reconnect(id);
    }

    /// The connection is closed for good: dispose its stores. A kept store's unsaved bodies
    /// go with it, as with Electron's `disconnectSession`.
    fn retire(slot: SessionSlot, cx: &mut Context<Self>) {
        if let ConnState::Connected(session) = slot.state {
            session.update(cx, |session, cx| session.dispose(cx));
        }
        match slot.previous {
            Previous::Nothing => {}
            Previous::Kept(session) | Previous::HandingOver { session, .. } => {
                session.update(cx, |session, cx| session.dispose(cx));
            }
        }
    }

    /// The connected generation, if any, is gone: keep its store, told it is disconnected
    /// (`handleConnectionStatusChange`). A hand-over in progress stops (its task is
    /// dropped); the store defers its writes again until the next generation. The caller
    /// sets the state the new status carries.
    fn keep_previous(slot: &mut SessionSlot, cx: &mut Context<Self>) {
        let kept = match std::mem::replace(&mut slot.previous, Previous::Nothing) {
            Previous::Nothing => None,
            Previous::Kept(session) | Previous::HandingOver { session, .. } => Some(session),
        };
        let kept = match &slot.state {
            ConnState::Connected(session) => {
                slot._events = None;
                Some(session.clone())
            }
            ConnState::Connecting(_) | ConnState::Reconnecting { .. } | ConnState::Failed { .. } => kept,
        };
        if let Some(session) = kept {
            session.update(cx, |s, cx| s.set_connection_status(SessionConnection::Disconnected, cx));
            slot.previous = Previous::Kept(session);
        }
    }

    fn on_status(&mut self, info: ConnectionInfo<DaemonConnection>, cx: &mut Context<Self>) {
        let ConnectionInfo { id, target, status } = info;
        if matches!(status, ConnectionStatus::Disconnected) {
            if let Some(slot) = self.sessions.shift_remove(&id) {
                Self::retire(slot, cx);
            }
            cx.notify();
            return;
        }
        self.insert_slot(id.clone(), target);
        let Some(slot) = self.sessions.get_mut(&id) else {
            return;
        };
        // Every status change after Connected means that generation is gone.
        Self::keep_previous(slot, cx);
        match status {
            ConnectionStatus::Connecting { phase } => slot.state = ConnState::Connecting(phase),
            ConnectionStatus::Connected { daemon } => {
                tracing::info!(%id, "connected");
                match std::mem::replace(&mut slot.previous, Previous::Nothing) {
                    Previous::Nothing => self.connect_session(&id, daemon, cx),
                    Previous::Kept(session) | Previous::HandingOver { session, .. } => {
                        self.hand_over(&id, session, daemon, cx);
                    }
                }
            }
            ConnectionStatus::Reconnecting { attempt, error } => {
                tracing::warn!(%id, attempt, %error, "reconnecting");
                slot.state = ConnState::Reconnecting { attempt, error: error.to_string() };
            }
            ConnectionStatus::Error { error } => {
                tracing::error!(%id, %error, "connection failed");
                slot.state = ConnState::Failed {
                    hash_mismatch: error.kind() == ConnectionErrorKind::DaemonHashMismatch,
                    error: error.to_string(),
                };
            }
            ConnectionStatus::Disconnected => {} // handled above
        }
        cx.notify();
    }

    /// Build the session store of a new connection generation.
    fn connect_session(&mut self, id: &ConnectionId, daemon: DaemonConnection, cx: &mut Context<Self>) {
        let Some(slot) = self.sessions.get(id) else {
            return;
        };
        let (client_id, settings, llm) = (self.client_id.clone(), self.settings.clone(), self.llm.clone());
        let remote = (!slot.is_local()).then(|| crate::web_app::RemoteHost { app: cx.weak_entity(), connection: id.clone() });
        let session = cx.new(|cx| {
            let mut session = SessionStore::new(daemon, client_id, settings, llm, cx);
            session.use_worktree_registry();
            if let Some(remote) = remote {
                session.set_remote_host(remote);
            }
            session
        });
        cx.observe(&session, |_, _, cx| cx.notify()).detach();
        let owned = id.clone();
        let events = cx.subscribe(&session, move |store, session, event: &SessionEvent, cx| match event {
            SessionEvent::Failed(reason) => store.on_session_failed(&owned, session, reason, cx),
            SessionEvent::WorkspaceRemoved(_) => {}
            SessionEvent::AttentionRecorded(workspace) => {
                cx.emit(AppEvent::Attention { workspace: workspace.clone() });
            }
        });
        let Some(slot) = self.sessions.get_mut(id) else {
            return;
        };
        slot._events = Some(events);
        slot.state = ConnState::Connected(session);
        // `autoStartPortForwards`: a remote's saved forwards start on its first connect.
        if !slot.forwards_started && !slot.is_local() {
            slot.forwards_started = true;
            if let Err(error) = self.manager.start_saved_port_forwards(id) {
                tracing::error!(%error, "cannot start saved port forwards");
            }
        }
    }

    /// The next generation is up while the last one's store is kept (`handleReconnected`):
    /// the kept store writes its deferred bodies on the new connection, then a fresh store
    /// replaces it. Until then the slot keeps the state the connection was lost with.
    fn hand_over(&mut self, id: &ConnectionId, session: Entity<SessionStore>, daemon: DaemonConnection, cx: &mut Context<Self>) {
        let flush = session.update(cx, |s, cx| s.hand_over(daemon.clone(), cx));
        let owned = id.clone();
        let task = cx.spawn(async move |this, cx| {
            flush.await;
            match this.update(cx, |store, cx| store.finish_hand_over(&owned, daemon, cx)) {
                Ok(()) | Err(_) => {} // Err: the app is shutting down
            }
        });
        if let Some(slot) = self.sessions.get_mut(id) {
            slot.previous = Previous::HandingOver { session, task };
        }
    }

    fn finish_hand_over(&mut self, id: &ConnectionId, daemon: DaemonConnection, cx: &mut Context<Self>) {
        let Some(slot) = self.sessions.get_mut(id) else {
            return;
        };
        let Previous::HandingOver { session, task } = std::mem::replace(&mut slot.previous, Previous::Nothing) else {
            unreachable!("a hand-over that is replaced is cancelled with its task");
        };
        // This very task is finishing: detach it rather than cancel it mid-poll.
        task.detach();
        session.update(cx, |session, cx| session.dispose(cx));
        self.connect_session(id, daemon, cx);
        cx.notify();
    }

    /// The session store's own watch died before the manager noticed: force a new
    /// generation (the store is kept when the status changes, and hands over to it).
    fn on_session_failed(&mut self, id: &ConnectionId, session: Entity<SessionStore>, reason: &str, cx: &mut Context<Self>) {
        let current = self.sessions.get(id).and_then(SessionSlot::session);
        if current != Some(&session) {
            return; // already replaced
        }
        tracing::warn!(%id, %reason, "session watch lost; reconnecting");
        if let Err(error) = self.manager.force_reconnect(id) {
            tracing::error!(%error, "cannot reconnect");
        }
        cx.notify();
    }

    /// Write a remote connection's saved port forwards back to its saved connection in the
    /// settings (`syncSavedPortForwards`).
    pub fn save_port_forwards(&mut self, id: &ConnectionId, cx: &mut Context<Self>) {
        let specs = self.manager.saved_port_forward_specs(id);
        let key = id.as_str().to_owned();
        self.settings.update(cx, |settings, cx| {
            settings.update(
                move |s| {
                    if let Some(saved) = s.ssh.saved_connections.iter_mut().find(|c| c.id == key) {
                        saved.port_forwards = specs;
                    }
                },
                cx,
            )
        });
    }

    /// "Exit and Kill Daemon": stop the local daemon (and with it every local PTY).
    pub fn shutdown_daemon(
        &self,
        cx: &gpui_kit::App,
    ) -> Option<treeterm_core::rt::Fut<Result<(), treeterm_core::daemon::ApiError>>> {
        self.local_session().map(|session| session.read(cx).connection().shutdown())
    }
}
