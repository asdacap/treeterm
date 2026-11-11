//! One daemon session: its workspace membership and every workspace's body, kept in sync
//! with the daemon and with every other writer (other windows, the Electron app).
//!
//! Port of `src/renderer/store/createSessionStore.ts`. Two independent syncs:
//! - **Membership** (the ref list) goes through `UpdateSession` with an optimistic version
//!   check, serialized through one op queue on the session channel. Rejections adopt the
//!   daemon's list and retry; workspaces added here but not yet published survive that.
//! - **Bodies** (`<workspace_data_dir>/<id>.json`) are written with compare-and-swap and
//!   observed with `WatchFile`. Each body chains the sha of the one it replaces
//!   (`parentHash`), so hashes never repeat and a watch echo of our own write is recognised
//!   by sha alone. A write that loses a race is rewritten on top of the winner.
//!
//! Constants and names follow the TypeScript so the two can be read side by side.

use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::oneshot;
use gpui_kit::{App, AppContext as _, Context, Entity, EventEmitter, Subscription, Task};
use indexmap::IndexMap;
use serde::Serialize;
use treeterm_core::daemon::{ApiError, DaemonConnection};
use treeterm_core::settings::Settings;
use treeterm_core::tree::{self, TreeNode, TreeRow};
use treeterm_core::wire::stable_json::{self, sha256_hex};
use treeterm_core::wire::workspace_file::{WorkspaceFile, parse_workspace_file, stored_json};
use treeterm_core::workspace_io::{BodyWrite, Expected};
use treeterm_proto::treeterm::file_watch_event::State as FileState;
use treeterm_proto::treeterm::{FileWatchEvent, Session, SessionLock, SessionWatchEvent, WorkspaceRef};

use crate::apps::AppRegistry;
use crate::workspace_store::{Workspace, WorkspaceEvent, WorkspaceStore};

/// Consecutive rejected ref syncs before unpublished workspaces are surfaced as errors.
const MAX_REF_SYNC_ATTEMPTS: u32 = 5;
/// Self-write hashes kept for echo suppression (a backstop bound).
const RECENT_HASHES_MAX: usize = 32;
/// Consecutive CAS conflicts before the divergence is surfaced as a workspace error.
const MAX_CONFLICT_STREAK: u32 = 5;
/// Reopens of a failed watch stream before the workspace is surfaced as broken.
const MAX_WATCH_RETRIES: u32 = 3;
const WATCH_RETRY_BASE: Duration = Duration::from_secs(1);

pub enum SessionState {
    /// Waiting for the first SessionWatch event.
    Waiting,
    Live,
    /// The session watch failed or ended; the connection layer must rebuild this store.
    Failed(String),
}

/// The connection this store syncs over. Driven by the connection layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
}

pub enum WorkspaceEntry {
    /// A placeholder until the body arrives (or while a new workspace is being set up).
    Loading { name: String, message: String, output: Vec<String> },
    Error { name: String, error: String },
    Loaded(Entity<WorkspaceStore>),
    /// Loaded, but an operation on it failed; the error is shown until dismissed.
    OperationError { store: Entity<WorkspaceStore>, error: String },
}

impl WorkspaceEntry {
    pub fn store(&self) -> Option<&Entity<WorkspaceStore>> {
        match self {
            WorkspaceEntry::Loaded(store) | WorkspaceEntry::OperationError { store, .. } => Some(store),
            WorkspaceEntry::Loading { .. } | WorkspaceEntry::Error { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    WorkspaceRemoved(String),
}

impl EventEmitter<SessionEvent> for SessionStore {}

/// Per-workspace body-sync bookkeeping (`WorkspaceSyncState`).
#[derive(Default)]
struct WorkspaceSync {
    /// sha of the body we believe the daemon holds ("" = absent/unknown). The CAS guard
    /// for, and the `parentHash` of, the next write.
    last_seen_sha: String,
    /// Last body we wrote, to skip redundant writes.
    last_written_json: String,
    /// shas of bodies we wrote or applied recently, newest last.
    recent_hashes: VecDeque<String>,
    watch: WatchState,
    writer: Writer,
    /// Waiting for the writer to go idle (first write of a new workspace, reconnect flush).
    idle_waiters: Vec<oneshot::Sender<()>>,
    /// A write was skipped while disconnected; flushed when the connection returns.
    dirty: bool,
    /// A write conflicted before its guard advanced; the next watch event retries it.
    retry_on_next_event: bool,
    conflict_streak: u32,
    /// An external body was applied since the current write captured its guard.
    external_applied: bool,
    watch_retries: u32,
}

#[derive(Default)]
enum WatchState {
    #[default]
    Closed,
    Open { _stream: Task<()> },
    /// Waiting to reopen after the stream failed.
    Retrying { _timer: Task<()> },
}

#[derive(Default)]
enum Writer {
    #[default]
    Idle,
    /// A write task is running; `rerun` asks it to write again once done (coalescing).
    Running { rerun: bool, task: Option<Task<()>> },
}

/// One op on the session-version queue. Un-started syncs absorb later sync requests.
struct QueuedSync {
    reasons: Vec<&'static str>,
    done: Vec<oneshot::Sender<()>>,
}

/// A ref sync in flight: what was sent, and the base to restore if the call fails.
struct RefSync {
    previous: String,
    sent: String,
    expected: u64,
}

#[derive(Serialize)]
struct RefJson<'a> {
    id: &'a str,
    path: &'a str,
}

fn refs_json(refs: &[WorkspaceRef]) -> String {
    let list: Vec<RefJson<'_>> = refs
        .iter()
        .map(|r| RefJson {
            id: &r.id,
            path: &r.path,
        })
        .collect();
    stable_json::to_string(&list).expect("refs serialize")
}

fn name_from_path(path: &str) -> String {
    path.rsplit('/').find(|part| !part.is_empty()).unwrap_or(path).to_owned()
}

pub struct SessionStore {
    conn: DaemonConnection,
    client_id: String,
    settings: Rc<Settings>,
    registry: Rc<AppRegistry>,
    state: SessionState,
    connection: ConnectionStatus,
    version: u64,
    lock: Option<SessionLock>,
    data_dir: String,
    /// In insertion order, like the TypeScript `Map`.
    entries: IndexMap<String, WorkspaceEntry>,
    store_events: HashMap<String, Subscription>,
    syncs: HashMap<String, WorkspaceSync>,
    /// The ref list last sent to or received from the daemon, for echo/no-op detection.
    last_synced_refs: String,
    /// Created here, not yet acknowledged by the daemon: not "removed elsewhere".
    unpublished: HashSet<String>,
    /// id → path of every ref the daemon lists; placeholders must still be re-published.
    known_refs: IndexMap<String, String>,
    ref_sync_attempts: u32,
    ref_sync_retry_on_next_event: bool,
    ops: VecDeque<QueuedSync>,
    op_runner: Option<Task<()>>,
    disposed: bool,
    _session_watch: Task<()>,
}

impl SessionStore {
    pub fn new(
        conn: DaemonConnection,
        client_id: String,
        settings: Rc<Settings>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut events = conn.watch_session(&client_id);
        let session_watch = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this
                    .update(cx, |store, cx| store.on_session_event(event, cx))
                    .is_err()
                {
                    return; // store dropped
                }
            }
            match this.update(cx, |store, cx| {
                store.fail("session watch ended: connection to the daemon lost", cx)
            }) {
                Ok(()) | Err(_) => {} // Err: store dropped, nothing to report to
            }
        });
        let registry = Rc::new(AppRegistry::from_settings(&settings));
        Self {
            conn,
            client_id,
            settings,
            registry,
            state: SessionState::Waiting,
            connection: ConnectionStatus::Connected,
            version: 0,
            lock: None,
            data_dir: String::new(),
            entries: IndexMap::new(),
            store_events: HashMap::new(),
            syncs: HashMap::new(),
            last_synced_refs: String::new(),
            unpublished: HashSet::new(),
            known_refs: IndexMap::new(),
            ref_sync_attempts: 0,
            ref_sync_retry_on_next_event: false,
            ops: VecDeque::new(),
            op_runner: None,
            disposed: false,
            _session_watch: session_watch,
        }
    }

    // ---- reads -------------------------------------------------------------------------

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    pub fn connection(&self) -> &DaemonConnection {
        &self.conn
    }

    pub fn registry(&self) -> &Rc<AppRegistry> {
        &self.registry
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// Workspace ids in session order.
    pub fn workspace_ids(&self) -> impl Iterator<Item = &String> {
        self.entries.keys()
    }

    pub fn entry(&self, id: &str) -> Option<&WorkspaceEntry> {
        self.entries.get(id)
    }

    pub fn workspace_store(&self, id: &str) -> Option<&Entity<WorkspaceStore>> {
        self.entries.get(id).and_then(WorkspaceEntry::store)
    }

    /// Visible tree rows in display order; empty until the session is live.
    pub fn tree_rows(&self, collapsed: &HashSet<String>, cx: &App) -> Vec<TreeRow> {
        let SessionState::Live = self.state else {
            return Vec::new();
        };
        let files: Vec<(&String, Option<&WorkspaceFile>)> = self
            .entries
            .iter()
            .map(|(id, entry)| (id, entry.store().map(|store| store.read(cx).file())))
            .collect();
        let nodes: Vec<TreeNode<'_>> = files
            .iter()
            .map(|(id, file)| match file {
                Some(file) => TreeNode::Loaded { id, file },
                None => TreeNode::Unloaded { id },
            })
            .collect();
        tree::flatten(&nodes, collapsed)
    }

    // ---- session watch ------------------------------------------------------------------

    fn fail(&mut self, reason: &str, cx: &mut Context<Self>) {
        tracing::error!(reason, "session store failed");
        self.state = SessionState::Failed(reason.to_owned());
        cx.notify();
    }

    fn on_session_event(&mut self, event: Result<SessionWatchEvent, ApiError>, cx: &mut Context<Self>) {
        match event.map(|e| e.session) {
            Ok(Some(session)) => self.handle_external_update(session, cx),
            Ok(None) => self.fail("SessionWatch event without a session", cx),
            Err(error) => self.fail(&format!("session watch failed: {error}"), cx),
        }
    }

    /// `handleExternalUpdate`.
    fn handle_external_update(&mut self, session: Session, cx: &mut Context<Self>) {
        if session.version < self.version {
            return; // overtaken by a newer snapshot
        }
        self.version = session.version;
        self.lock = session.lock;
        self.data_dir = session.workspace_data_dir;
        self.state = SessionState::Live;

        // A ref sync rejected under another window's lock waits for the unlock. An
        // expired lock is only cleared lazily by the daemon, so treat it as gone too.
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(i64::MAX);
        let unlocked = self.lock.as_ref().is_none_or(|lock| lock.expires_at <= now_ms);
        if self.ref_sync_retry_on_next_event && unlocked {
            self.ref_sync_retry_on_next_event = false;
            self.enqueue_sync("retry after lock released", None, cx);
        }

        let incoming = refs_json(&session.workspace_refs);
        if incoming != self.last_synced_refs {
            self.last_synced_refs = incoming;
            self.reconcile_refs(&session.workspace_refs, cx);
        }
        cx.notify();
    }

    /// Open watches and placeholders for new refs; remove loaded workspaces that are gone.
    fn reconcile_refs(&mut self, refs: &[WorkspaceRef], cx: &mut Context<Self>) {
        let incoming: HashSet<&str> = refs.iter().map(|r| r.id.as_str()).collect();
        // Placeholders that only existed because the daemon listed them go with their ref.
        // (The TypeScript leaves them behind as stale Loading/Error rows.)
        let dropped_placeholders: Vec<String> = self
            .known_refs
            .keys()
            .filter(|id| !incoming.contains(id.as_str()))
            .filter(|id| self.entries.get(*id).is_some_and(|entry| entry.store().is_none()))
            .cloned()
            .collect();
        for id in dropped_placeholders {
            self.syncs.remove(&id);
            self.entries.shift_remove(&id);
            cx.emit(SessionEvent::WorkspaceRemoved(id));
        }
        self.known_refs.retain(|id, _| incoming.contains(id.as_str()));
        for r in refs {
            self.known_refs.insert(r.id.clone(), r.path.clone());
            self.unpublished.remove(&r.id); // the daemon knows it now
            self.ensure_watch(&r.id, cx);
            if !self.entries.contains_key(&r.id) {
                self.entries.insert(
                    r.id.clone(),
                    WorkspaceEntry::Loading {
                        name: name_from_path(&r.path),
                        message: "Loading workspace...".to_owned(),
                        output: Vec::new(),
                    },
                );
            }
        }
        let gone: Vec<String> = self
            .entries
            .iter()
            .filter(|(id, entry)| {
                !incoming.contains(id.as_str()) && !self.unpublished.contains(*id) && entry.store().is_some()
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in gone {
            self.on_workspace_removed(&id, cx);
        }
    }

    fn on_workspace_removed(&mut self, id: &str, cx: &mut Context<Self>) {
        self.unpublished.remove(id);
        self.known_refs.shift_remove(id);
        self.syncs.remove(id); // drops the watch and any write in flight
        self.store_events.remove(id);
        if let Some(entry) = self.entries.shift_remove(id)
            && let Some(store) = entry.store()
        {
            store.update(cx, |store, _| store.dispose());
        }
        cx.emit(SessionEvent::WorkspaceRemoved(id.to_owned()));
        cx.notify();
    }

    // ---- body watch ---------------------------------------------------------------------

    fn sync_state(&mut self, id: &str) -> &mut WorkspaceSync {
        self.syncs.entry(id.to_owned()).or_default()
    }

    /// Open the content watch for a workspace's body (idempotent). The first event is
    /// the current state.
    fn ensure_watch(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.disposed || self.data_dir.is_empty() || self.connection != ConnectionStatus::Connected {
            return;
        }
        if !matches!(self.sync_state(id).watch, WatchState::Closed) {
            return;
        }
        let mut events = self.conn.watch_file(&self.data_dir, &format!("{id}.json"));
        let owned = id.to_owned();
        let task = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this.update(cx, |store, cx| store.on_file_event(&owned, event, cx)).is_err() {
                    return;
                }
            }
            match this.update(cx, |store, cx| {
                store.on_watch_failed(&owned, "watch stream ended".to_owned(), cx);
            }) {
                Ok(()) | Err(_) => {} // Err: store dropped
            }
        });
        self.sync_state(id).watch = WatchState::Open { _stream: task };
    }

    fn on_file_event(&mut self, id: &str, event: Result<FileWatchEvent, ApiError>, cx: &mut Context<Self>) {
        if self.disposed || !self.syncs.contains_key(id) {
            return;
        }
        match event.map(|e| e.state) {
            Ok(Some(FileState::Present(file))) => {
                self.sync_state(id).watch_retries = 0;
                let sha = file.sha256;
                let body = match parse_workspace_file(&file.content) {
                    Ok(body) => body,
                    Err(error) => {
                        return self.set_workspace_file_error(id, format!("Invalid workspace file: {error}"), cx);
                    }
                };
                let sync = self.sync_state(id);
                let own_echo = sync.recent_hashes.contains(&sha);
                sync.last_seen_sha = sha.clone();
                if own_echo {
                    // A conflicted write lost to one of our own earlier writes: local
                    // state still holds the losing delta, so rewrite it on the winner.
                    if std::mem::take(&mut sync.retry_on_next_event) {
                        self.enqueue_content_sync(id, cx);
                    }
                    return;
                }
                // A genuine external edit: adopting it resolves any divergence.
                sync.retry_on_next_event = false;
                sync.external_applied = true;
                sync.conflict_streak = 0;
                remember_hash(sync, sha);
                self.apply_workspace_file(id, body, cx);
            }
            Ok(Some(FileState::Absent(_))) => {
                self.sync_state(id).watch_retries = 0;
                // Membership changes go through the ref list; a missing body is a fault.
                self.set_workspace_file_error(id, "Workspace file is missing".to_owned(), cx);
            }
            Ok(None) => self.on_watch_failed(id, "daemon sent an empty file event".to_owned(), cx),
            Err(error) => self.on_watch_failed(id, error.to_string(), cx),
        }
    }

    /// `reopenWatchAfterError`: reopen with backoff, then give up loudly.
    fn on_watch_failed(&mut self, id: &str, message: String, cx: &mut Context<Self>) {
        if self.disposed || self.connection != ConnectionStatus::Connected || !self.syncs.contains_key(id) {
            return;
        }
        let sync = self.sync_state(id);
        sync.watch = WatchState::Closed;
        sync.watch_retries += 1;
        if sync.watch_retries > MAX_WATCH_RETRIES {
            return self.set_workspace_file_error(id, format!("Workspace file watch failed: {message}"), cx);
        }
        let delay = WATCH_RETRY_BASE * 2u32.pow(sync.watch_retries - 1);
        let owned = id.to_owned();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |this, cx| {
            executor.timer(delay).await;
            match this.update(cx, |store, cx| {
                if let Some(sync) = store.syncs.get_mut(&owned) {
                    sync.watch = WatchState::Closed;
                }
                store.ensure_watch(&owned, cx);
            }) {
                Ok(()) | Err(_) => {} // Err: store dropped
            }
        });
        self.sync_state(id).watch = WatchState::Retrying { _timer: task };
    }

    fn set_workspace_file_error(&mut self, id: &str, error: String, cx: &mut Context<Self>) {
        // A dead connection fails every workspace at once; the connection banner owns it.
        if self.connection != ConnectionStatus::Connected {
            return;
        }
        tracing::error!(workspace = id, %error, "workspace sync error");
        let Some(entry) = self.entries.get_mut(id) else {
            return;
        };
        *entry = match std::mem::replace(entry, WorkspaceEntry::Loading {
            name: String::new(),
            message: String::new(),
            output: Vec::new(),
        }) {
            WorkspaceEntry::Loaded(store) | WorkspaceEntry::OperationError { store, .. } => {
                WorkspaceEntry::OperationError { store, error }
            }
            WorkspaceEntry::Loading { name, .. } | WorkspaceEntry::Error { name, .. } => {
                WorkspaceEntry::Error { name, error }
            }
        };
        cx.notify();
    }

    pub fn clear_workspace_error(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(entry) = self.entries.get_mut(id)
            && let WorkspaceEntry::OperationError { store, .. } = entry
        {
            *entry = WorkspaceEntry::Loaded(store.clone());
            cx.notify();
        }
    }

    /// Apply a body from another writer, building the workspace on first sight.
    fn apply_workspace_file(&mut self, id: &str, file: WorkspaceFile, cx: &mut Context<Self>) {
        if let Some(store) = self.entries.get(id).and_then(WorkspaceEntry::store) {
            store.update(cx, |store, cx| store.apply_external(file, cx));
            return;
        }
        let Some(path) = self.known_refs.get(id).cloned() else {
            return; // not a member any more
        };
        let mut file = file;
        if file.active_tab_id.as_deref().is_none_or(str::is_empty) {
            file.active_tab_id = file.app_states.keys().next().cloned();
        }
        let workspace = Workspace {
            id: id.to_owned(),
            path,
            file,
        };
        let store = self.new_workspace_store(workspace, cx);
        self.entries.insert(id.to_owned(), WorkspaceEntry::Loaded(store));
        cx.notify();
    }

    fn new_workspace_store(&mut self, workspace: Workspace, cx: &mut Context<Self>) -> Entity<WorkspaceStore> {
        let id = workspace.id.clone();
        let conn = self.conn.clone();
        let registry = self.registry.clone();
        let settings = self.settings.clone();
        let store = cx.new(|cx| WorkspaceStore::new(workspace, conn, registry, settings, cx));
        let owned = id.clone();
        let subscription = cx.subscribe(&store, move |session, _, event: &WorkspaceEvent, cx| match event {
            WorkspaceEvent::Changed => session.enqueue_content_sync(&owned, cx),
        });
        self.store_events.insert(id, subscription);
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        store
    }

    // ---- body writes --------------------------------------------------------------------

    /// Queue a write of the workspace's current body. Coalesced: a write requested while
    /// one runs makes that one write again with the latest state when it finishes.
    pub fn enqueue_content_sync(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.disposed {
            return;
        }
        let sync = self.sync_state(id);
        match &mut sync.writer {
            Writer::Running { rerun, .. } => *rerun = true,
            Writer::Idle => {
                sync.writer = Writer::Running {
                    rerun: false,
                    task: None,
                };
                let owned = id.to_owned();
                let task = cx.spawn(async move |this, cx| {
                    loop {
                        let Ok(prepared) = this.update(cx, |store, cx| store.prepare_write(&owned, cx)) else {
                            return;
                        };
                        if let Some((json, sha, guard, write)) = prepared {
                            let result = write.await;
                            if this
                                .update(cx, |store, cx| store.finish_write(&owned, json, sha, guard, result, cx))
                                .is_err()
                            {
                                return;
                            }
                        }
                        let Ok(again) = this.update(cx, |store, _| store.take_rerun(&owned)) else {
                            return;
                        };
                        if !again {
                            return;
                        }
                    }
                });
                if let Some(WorkspaceSync {
                    writer: Writer::Running { task: slot, .. },
                    ..
                }) = self.syncs.get_mut(id)
                {
                    *slot = Some(task);
                }
            }
        }
    }

    /// After a write: run again if asked, else go idle.
    fn take_rerun(&mut self, id: &str) -> bool {
        let Some(sync) = self.syncs.get_mut(id) else {
            return false;
        };
        match &mut sync.writer {
            Writer::Running { rerun, .. } if *rerun => {
                *rerun = false;
                true
            }
            Writer::Running { task, .. } => {
                // Detach the finishing task from the slot without cancelling it mid-poll.
                if let Some(task) = task.take() {
                    task.detach();
                }
                sync.writer = Writer::Idle;
                for waiter in sync.idle_waiters.drain(..) {
                    waiter.send(()).ok(); // the waiter may have given up
                }
                false
            }
            Writer::Idle => false,
        }
    }

    /// The synchronous half of `writeWorkspaceContent`: serialize, record the hash before
    /// writing (the watch echo can beat the write's reply), and start the RPC.
    #[allow(clippy::type_complexity)]
    fn prepare_write(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Option<(String, String, String, treeterm_core::rt::Fut<Result<BodyWrite, ApiError>>)> {
        if self.disposed {
            return None;
        }
        let connected = self.connection == ConnectionStatus::Connected;
        let body = self.entries.get(id)?.store()?.read(cx).file().clone();
        let sync = self.syncs.get_mut(id)?;
        if !connected {
            // Not droppable: local state may hold changes the disk copy lacks.
            sync.dirty = true;
            return None;
        }
        if self.data_dir.is_empty() {
            return None;
        }
        sync.dirty = false;
        let guard = sync.last_seen_sha.clone();
        sync.external_applied = false;
        let json = match stored_json(&body, &guard) {
            Ok(json) => json,
            Err(error) => {
                let message = format!("Failed to serialize workspace: {error}");
                self.set_workspace_file_error(id, message, cx);
                return None;
            }
        };
        if json == sync.last_written_json {
            return None;
        }
        let sha = sha256_hex(&json);
        remember_hash(sync, sha.clone());
        let expected = if guard.is_empty() {
            Expected::Absent
        } else {
            Expected::Sha(guard.clone())
        };
        let write = self
            .conn
            .write_body(&self.data_dir, &format!("{id}.json"), json.clone(), expected);
        Some((json, sha, guard, write))
    }

    fn finish_write(
        &mut self,
        id: &str,
        json: String,
        sha: String,
        guard: String,
        result: Result<Result<BodyWrite, ApiError>, treeterm_core::rt::TaskLost>,
        cx: &mut Context<Self>,
    ) {
        let Some(sync) = self.syncs.get_mut(id) else {
            return;
        };
        let outcome = match result {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(error)) => BodyWrite::Failed(error.to_string()),
            Err(lost) => BodyWrite::Failed(lost.to_string()),
        };
        match outcome {
            BodyWrite::Written => {
                sync.last_written_json = json;
                sync.last_seen_sha = sha;
                sync.conflict_streak = 0;
            }
            BodyWrite::Conflict => {
                // The winner's event already replaced local state: nothing of ours is
                // left to rewrite, and a retry would only republish the winner.
                if sync.external_applied {
                    return;
                }
                // Local state may hold changes the winner lacks: rewrite on top of it
                // instead of silently diverging (the cause of ptyId:null on disk).
                sync.conflict_streak += 1;
                if sync.conflict_streak >= MAX_CONFLICT_STREAK {
                    let streak = sync.conflict_streak;
                    return self.set_workspace_file_error(
                        id,
                        format!("Workspace file keeps conflicting with another writer ({streak} attempts)"),
                        cx,
                    );
                }
                if sync.last_seen_sha != guard {
                    // The winner's watch event already arrived: retry on top of it now.
                    self.enqueue_content_sync(id, cx);
                } else {
                    sync.retry_on_next_event = true;
                }
            }
            BodyWrite::Failed(error) => {
                // Always re-written by the next reconnect flush; while the connection
                // reads healthy it is also a genuine failure.
                sync.dirty = true;
                if self.connection == ConnectionStatus::Connected {
                    self.set_workspace_file_error(id, format!("Failed to save workspace: {error}"), cx);
                }
            }
        }
    }

    // ---- membership -----------------------------------------------------------------------

    /// The ref list to publish: every loaded workspace, plus daemon-listed refs that are
    /// still placeholders here.
    fn current_refs(&self, cx: &App) -> Vec<WorkspaceRef> {
        self.entries
            .iter()
            .filter_map(|(id, entry)| match entry.store() {
                Some(store) => {
                    let ws = store.read(cx).workspace();
                    Some(WorkspaceRef {
                        id: ws.id.clone(),
                        path: ws.path.clone(),
                    })
                }
                None => self.known_refs.get(id).map(|path| WorkspaceRef {
                    id: id.clone(),
                    path: path.clone(),
                }),
            })
            .collect()
    }

    /// Publish the membership; resolves once the sync carrying this request has run.
    pub fn sync_refs(&mut self, reason: &'static str, cx: &mut Context<Self>) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        self.enqueue_sync(reason, Some(tx), cx);
        rx
    }

    /// Queue a membership sync. Requests made before the queued sync starts share it.
    fn enqueue_sync(&mut self, reason: &'static str, done: Option<oneshot::Sender<()>>, cx: &mut Context<Self>) {
        match self.ops.back_mut() {
            Some(pending) => {
                pending.reasons.push(reason);
                pending.done.extend(done);
            }
            None => self.ops.push_back(QueuedSync {
                reasons: vec![reason],
                done: done.into_iter().collect(),
            }),
        }
        if self.op_runner.is_none() {
            self.op_runner = Some(cx.spawn(async move |this, cx| {
                loop {
                    let Ok(next) = this.update(cx, |store, cx| store.start_next_sync(cx)) else {
                        return;
                    };
                    let Some((op, call)) = next else {
                        return;
                    };
                    if let Some((sync, call)) = call {
                        let result = call.await;
                        if this
                            .update(cx, |store, cx| store.finish_sync(sync, result, cx))
                            .is_err()
                        {
                            return;
                        }
                    }
                    for done in op.done {
                        done.send(()).ok(); // the requester may not wait for it
                    }
                }
            }));
        }
    }

    #[allow(clippy::type_complexity)]
    fn start_next_sync(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<(
        QueuedSync,
        Option<(RefSync, treeterm_core::rt::Fut<Result<Session, ApiError>>)>,
    )> {
        let Some(op) = self.ops.pop_front() else {
            self.op_runner = None;
            return None;
        };
        if self.connection != ConnectionStatus::Connected || self.disposed {
            return Some((op, None));
        }
        let refs = self.current_refs(cx);
        let json = refs_json(&refs);
        if json == self.last_synced_refs {
            return Some((op, None));
        }
        tracing::info!(reasons = ?op.reasons, "syncing workspace refs to daemon");
        // Optimistic: our send becomes the known base so its echo is skipped; restored
        // if the call fails outright.
        let previous = std::mem::replace(&mut self.last_synced_refs, json.clone());
        let expected = self.version;
        let call = self.conn.update_session(refs, Some(expected), &self.client_id);
        Some((
            op,
            Some((
                RefSync {
                    previous,
                    sent: json,
                    expected,
                },
                call,
            )),
        ))
    }

    fn finish_sync(
        &mut self,
        RefSync { previous, sent, expected }: RefSync,
        result: Result<Result<Session, ApiError>, treeterm_core::rt::TaskLost>,
        cx: &mut Context<Self>,
    ) {
        let session = match result {
            Ok(Ok(session)) => session,
            Ok(Err(error)) => {
                tracing::error!(%error, "failed to sync refs to daemon");
                self.last_synced_refs = previous;
                return;
            }
            Err(lost) => {
                tracing::error!(%lost, "failed to sync refs to daemon");
                self.last_synced_refs = previous;
                return;
            }
        };
        // An accepted update echoes exactly what we sent, at the next version. Anything
        // else is the daemon's current state after a rejection.
        let returned = refs_json(&session.workspace_refs);
        let version_ok = session.version == expected + 1;
        let accepted = version_ok && returned == sent;
        self.version = session.version;
        self.lock = session.lock;
        self.data_dir = session.workspace_data_dir.clone();
        if accepted {
            for r in &session.workspace_refs {
                self.unpublished.remove(&r.id);
            }
            self.ref_sync_attempts = 0;
            self.ref_sync_retry_on_next_event = false;
            return;
        }
        let locked_by_other = session.lock.is_some();
        let reason = if locked_by_other {
            "session locked by another window".to_owned()
        } else if version_ok {
            "refs changed under us (content mismatch)".to_owned()
        } else {
            format!("version mismatch, expected {} got {}", expected + 1, session.version)
        };
        tracing::warn!(%reason, "ref update rejected; reconciling");
        self.last_synced_refs = returned;
        self.reconcile_refs(&session.workspace_refs, cx);
        self.schedule_ref_sync_retry(&reason, locked_by_other, cx);
    }

    fn schedule_ref_sync_retry(&mut self, reason: &str, locked_by_other: bool, cx: &mut Context<Self>) {
        if self.unpublished.is_empty() && refs_json(&self.current_refs(cx)) == self.last_synced_refs {
            return;
        }
        self.ref_sync_attempts += 1;
        if self.ref_sync_attempts >= MAX_REF_SYNC_ATTEMPTS {
            let attempts = self.ref_sync_attempts;
            let ids: Vec<String> = self.unpublished.iter().cloned().collect();
            for id in ids {
                self.set_workspace_file_error(
                    &id,
                    format!("Could not publish workspace to the session after {attempts} attempts: {reason}"),
                    cx,
                );
            }
            return;
        }
        if locked_by_other {
            self.ref_sync_retry_on_next_event = true;
            return;
        }
        self.enqueue_sync("retry after rejection", None, cx);
    }

    // ---- workspace lifecycle ----------------------------------------------------------------

    /// Add a workspace built here (`createWorkspaceFile` + ref sync): write its first body
    /// (must-not-exist CAS), open its watch, then publish it.
    pub fn add_workspace(&mut self, workspace: Workspace, cx: &mut Context<Self>) -> Entity<WorkspaceStore> {
        let id = workspace.id.clone();
        self.unpublished.insert(id.clone());
        self.ref_sync_attempts = 0;
        let store = self.new_workspace_store(workspace, cx);
        self.entries.insert(id.clone(), WorkspaceEntry::Loaded(store.clone()));
        self.sync_state(&id);
        self.enqueue_content_sync(&id, cx);
        // The watch opens once the first write lands, so its first event is our echo.
        let written = self.when_written(&id);
        let owned = id.clone();
        cx.spawn(async move |this, cx| {
            if written.await.is_err() {
                return; // the workspace went away before its first write finished
            }
            let Ok(published) = this.update(cx, |store, cx| {
                store.ensure_watch(&owned, cx);
                store.sync_refs("addWorkspace", cx)
            }) else {
                return;
            };
            if published.await.is_err() {
                tracing::debug!("ref sync dropped with the store");
            }
        })
        .detach();
        cx.notify();
        store
    }

    /// Resolves once no write of the workspace's body is running or queued.
    fn when_written(&mut self, id: &str) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        let sync = self.sync_state(id);
        match sync.writer {
            Writer::Idle => {
                tx.send(()).ok(); // rx is held right here
            }
            Writer::Running { .. } => sync.idle_waiters.push(tx),
        }
        rx
    }

    /// Write every body whose write was deferred while disconnected, and wait for those
    /// writes. Run before a reconnect replaces this store, so the flush cannot race the
    /// new store's first read.
    pub fn flush_deferred_writes(&mut self, cx: &mut Context<Self>) -> Vec<oneshot::Receiver<()>> {
        let dirty: Vec<String> = self
            .syncs
            .iter()
            .filter(|(_, sync)| sync.dirty)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &dirty {
            self.enqueue_content_sync(id, cx);
        }
        let ids: Vec<String> = self.syncs.keys().cloned().collect();
        ids.iter().map(|id| self.when_written(id)).collect()
    }

    /// The connection came back or went away (driven by the connection layer).
    pub fn set_connection_status(&mut self, status: ConnectionStatus, cx: &mut Context<Self>) {
        let previous = self.connection;
        self.connection = status;
        if status == ConnectionStatus::Disconnected && previous == ConnectionStatus::Connected {
            // Errors raised by the dying connection describe the connection, not the
            // workspaces; its banner owns them now.
            for entry in self.entries.values_mut() {
                if let WorkspaceEntry::OperationError { store, .. } = entry {
                    *entry = WorkspaceEntry::Loaded(store.clone());
                }
            }
        }
        if status == ConnectionStatus::Connected && previous == ConnectionStatus::Disconnected {
            let dirty: Vec<String> = self
                .syncs
                .iter()
                .filter(|(_, sync)| sync.dirty)
                .map(|(id, _)| id.clone())
                .collect();
            for id in dirty {
                self.enqueue_content_sync(&id, cx);
            }
        }
        cx.notify();
    }

    /// Stop every watch and write and detach every terminal. Explicit, because entities
    /// are reference counted: a store kept alive by a stray handle must not keep writing
    /// the same files as its replacement (the "ghost session writer").
    pub fn dispose(&mut self, cx: &mut Context<Self>) {
        self.disposed = true;
        self.syncs.clear();
        self.ops.clear();
        self.op_runner = None;
        self.store_events.clear();
        for entry in self.entries.values() {
            if let Some(store) = entry.store() {
                store.update(cx, |store, _| store.dispose());
            }
        }
    }
}

fn remember_hash(sync: &mut WorkspaceSync, sha: String) {
    if sync.recent_hashes.contains(&sha) {
        return;
    }
    sync.recent_hashes.push_back(sha);
    if sync.recent_hashes.len() > RECENT_HASHES_MAX {
        sync.recent_hashes.pop_front();
    }
}
