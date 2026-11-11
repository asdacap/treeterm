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
use crate::settings_store::{SettingsEvent, SettingsStore};
use treeterm_core::tree::{self, TreeNode, TreeRow};
use treeterm_core::wire::stable_json::{self, sha256_hex};
use treeterm_core::wire::workspace_file::{WorkspaceFile, WorkspaceStatus, parse_workspace_file, stored_json};
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

/// What happens to a removed workspace's git worktree and branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveMode {
    /// `removeWorkspace`: remove the worktree and delete its branch (unless detached).
    DeleteWorktreeAndBranch,
    /// `removeWorkspaceKeepBranch`.
    KeepBranch,
    /// `removeWorkspaceKeepBoth`: only leave the session.
    KeepWorktreeAndBranch,
}

/// Where a new child worktree's branch comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChildSource {
    /// A new branch named `name`, from the parent's current branch.
    NewBranch { name: String },
    /// A local branch that has no worktree yet.
    ExistingBranch { branch: String },
    /// A remote-tracking branch such as `origin/feature`.
    RemoteBranch { remote_branch: String },
    /// A pull request's head branch (fetched first); its title becomes the display name.
    PullRequest { head_ref: String, title: String },
}

impl ChildSource {
    /// The worktree (and workspace) name: the last path segment of the branch.
    pub fn worktree_name(&self) -> String {
        let last = |branch: &str| branch.rsplit('/').find(|s| !s.is_empty()).unwrap_or(branch).to_owned();
        match self {
            ChildSource::NewBranch { name } => name.clone(),
            ChildSource::ExistingBranch { branch } => last(branch),
            ChildSource::RemoteBranch { remote_branch } => last(remote_branch),
            ChildSource::PullRequest { head_ref, .. } => last(head_ref),
        }
    }

    fn message(&self) -> &'static str {
        match self {
            ChildSource::NewBranch { .. } => "Creating worktree...",
            ChildSource::ExistingBranch { .. } => "Creating worktree from branch...",
            ChildSource::RemoteBranch { .. } => "Creating worktree from remote...",
            ChildSource::PullRequest { .. } => "Creating worktree from pull request...",
        }
    }

    /// Metadata the child starts with (`createChildWithLoading`).
    fn metadata(&self, description: Option<String>) -> std::collections::BTreeMap<String, String> {
        let mut metadata = std::collections::BTreeMap::new();
        match self {
            ChildSource::NewBranch { .. } => {}
            ChildSource::ExistingBranch { .. } | ChildSource::RemoteBranch { .. } => {
                metadata.insert("branchIsUserDefined".to_owned(), "true".to_owned());
            }
            ChildSource::PullRequest { title, .. } => {
                metadata.insert("branchIsUserDefined".to_owned(), "true".to_owned());
                // The PR body is not used: it can be very long.
                if !title.is_empty() {
                    metadata.insert("displayName".to_owned(), title.clone());
                }
            }
        }
        if let Some(description) = description.filter(|d| !d.is_empty()) {
            metadata.insert("description".to_owned(), description);
        }
        metadata
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChildOptions {
    pub detached: bool,
    pub description: Option<String>,
}

/// What happens to a child after its branch is merged into the parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterMerge {
    /// Remove it with its worktree and branch.
    Remove,
    Keep,
}

/// The git side of removing one workspace.
enum GitCleanup {
    Nothing,
    RemoveWorktree {
        root: String,
        path: String,
        branch: treeterm_core::git::BranchDisposal,
    },
}

impl GitCleanup {
    fn for_workspace(file: &WorkspaceFile, path: &str, mode: RemoveMode) -> Self {
        use treeterm_core::git::BranchDisposal;
        let (true, Some(root)) = (file.is_worktree, file.git_root_path.clone()) else {
            return GitCleanup::Nothing;
        };
        let branch = match mode {
            RemoveMode::KeepWorktreeAndBranch => return GitCleanup::Nothing,
            RemoveMode::KeepBranch => BranchDisposal::Keep,
            RemoveMode::DeleteWorktreeAndBranch if file.is_detached == Some(true) => BranchDisposal::Keep,
            RemoveMode::DeleteWorktreeAndBranch => BranchDisposal::Delete,
        };
        GitCleanup::RemoveWorktree {
            root,
            path: path.to_owned(),
            branch,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    WorkspaceRemoved(String),
    /// The session watch failed or ended: this store's connection is gone.
    Failed(String),
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

/// One op on the session-version queue: everything that bumps the daemon's session
/// version (UpdateSession, LockSession, UnlockSession) runs in enqueue order, so no two
/// race on the daemon (`enqueueOp` / `enqueueSync`).
enum QueuedOp {
    /// Un-started syncs absorb later sync requests.
    Sync(QueuedSync),
    Lock(oneshot::Sender<Result<(), String>>),
    Unlock(oneshot::Sender<()>),
}

struct QueuedSync {
    reasons: Vec<&'static str>,
    done: Vec<oneshot::Sender<()>>,
}

/// An op taken off the queue, with its RPC in flight.
enum RunningOp {
    Sync(QueuedSync, Option<(RefSync, treeterm_core::rt::Fut<Result<Session, ApiError>>)>),
    Lock(
        oneshot::Sender<Result<(), String>>,
        treeterm_core::rt::Fut<Result<treeterm_core::daemon::SessionLockResult, ApiError>>,
    ),
    Unlock(oneshot::Sender<()>, treeterm_core::rt::Fut<Result<Session, ApiError>>),
}

/// How long a lock taken for a multi-step operation lives if it is never released.
const LOCK_TTL: Duration = Duration::from_secs(60);

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

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// `ws-<millis>-<7 random chars>`, as Electron mints them.
fn new_workspace_id() -> String {
    let suffix: String = uuid::Uuid::new_v4().simple().to_string().chars().take(7).collect();
    format!("ws-{}-{suffix}", now_millis())
}

fn name_from_path(path: &str) -> String {
    path.rsplit('/').find(|part| !part.is_empty()).unwrap_or(path).to_owned()
}

pub struct SessionStore {
    conn: DaemonConnection,
    client_id: String,
    settings: Entity<SettingsStore>,
    registry: Rc<AppRegistry>,
    _settings_events: Subscription,
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
    ops: VecDeque<QueuedOp>,
    op_runner: Option<Task<()>>,
    disposed: bool,
    _session_watch: Task<()>,
}

impl SessionStore {
    pub fn new(
        conn: DaemonConnection,
        client_id: String,
        settings: Entity<SettingsStore>,
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
        let registry = Rc::new(AppRegistry::from_settings(settings.read(cx).settings()));
        let settings_events = cx.subscribe(&settings, |session, _, event: &SettingsEvent, cx| match event {
            SettingsEvent::Changed => session.on_settings_changed(cx),
        });
        Self {
            conn,
            client_id,
            settings,
            registry,
            _settings_events: settings_events,
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
        if self.disposed {
            return;
        }
        tracing::error!(reason, "session store failed");
        self.state = SessionState::Failed(reason.to_owned());
        self.connection = ConnectionStatus::Disconnected;
        cx.emit(SessionEvent::Failed(reason.to_owned()));
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

    /// Settings changed (here or in another app): rebuild the application registry and
    /// hand both to every workspace, which applies them to its live terminals.
    fn on_settings_changed(&mut self, cx: &mut Context<Self>) {
        let settings = self.settings.read(cx).settings().clone();
        self.registry = Rc::new(AppRegistry::from_settings(&settings));
        let stores: Vec<Entity<WorkspaceStore>> = self.entries.values().filter_map(|e| e.store().cloned()).collect();
        for store in stores {
            let (registry, settings) = (self.registry.clone(), settings.clone());
            store.update(cx, |w, cx| w.apply_settings(registry, settings, cx));
        }
        cx.notify();
    }

    fn new_workspace_store(&mut self, workspace: Workspace, cx: &mut Context<Self>) -> Entity<WorkspaceStore> {
        let id = workspace.id.clone();
        let conn = self.conn.clone();
        let registry = self.registry.clone();
        let settings = self.settings.read(cx).settings().clone();
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
            Some(QueuedOp::Sync(pending)) => {
                pending.reasons.push(reason);
                pending.done.extend(done);
            }
            Some(QueuedOp::Lock(_) | QueuedOp::Unlock(_)) | None => self.ops.push_back(QueuedOp::Sync(QueuedSync {
                reasons: vec![reason],
                done: done.into_iter().collect(),
            })),
        }
        self.ensure_op_runner(cx);
    }

    /// Take the session lock for a multi-step operation (`acquireLock`). Resolves with an
    /// error message when another window holds it.
    pub fn acquire_lock(&mut self, cx: &mut Context<Self>) -> oneshot::Receiver<Result<(), String>> {
        let (tx, rx) = oneshot::channel();
        self.ops.push_back(QueuedOp::Lock(tx));
        self.ensure_op_runner(cx);
        rx
    }

    pub fn release_lock(&mut self, cx: &mut Context<Self>) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        self.ops.push_back(QueuedOp::Unlock(tx));
        self.ensure_op_runner(cx);
        rx
    }

    fn ensure_op_runner(&mut self, cx: &mut Context<Self>) {
        if self.op_runner.is_some() {
            return;
        }
        self.op_runner = Some(cx.spawn(async move |this, cx| {
            loop {
                let Ok(Some(running)) = this.update(cx, |store, cx| store.start_next_op(cx)) else {
                    return;
                };
                let finished = match running {
                    RunningOp::Sync(op, call) => {
                        let ok = match call {
                            Some((sync, call)) => {
                                let result = call.await;
                                this.update(cx, |store, cx| store.finish_sync(sync, result, cx)).is_ok()
                            }
                            None => true,
                        };
                        for done in op.done {
                            done.send(()).ok(); // the requester may not wait for it
                        }
                        ok
                    }
                    RunningOp::Lock(done, call) => {
                        let result = call.await;
                        let outcome = this.update(cx, |store, cx| store.finish_lock(result, cx));
                        let ok = outcome.is_ok();
                        done.send(outcome.unwrap_or_else(|_| Err("session store dropped".to_owned()))).ok();
                        ok
                    }
                    RunningOp::Unlock(done, call) => {
                        let result = call.await;
                        let ok = this.update(cx, |store, cx| store.finish_unlock(result, cx)).is_ok();
                        done.send(()).ok();
                        ok
                    }
                };
                if !finished {
                    return;
                }
            }
        }));
    }

    fn start_next_op(&mut self, cx: &mut Context<Self>) -> Option<RunningOp> {
        let Some(op) = self.ops.pop_front() else {
            self.op_runner = None;
            return None;
        };
        Some(match op {
            QueuedOp::Sync(sync) => {
                let call = self.start_sync(&sync, cx);
                RunningOp::Sync(sync, call)
            }
            QueuedOp::Lock(done) => RunningOp::Lock(done, self.conn.lock_session(LOCK_TTL)),
            QueuedOp::Unlock(done) => RunningOp::Unlock(done, self.conn.unlock_session()),
        })
    }

    fn finish_lock(
        &mut self,
        result: Result<Result<treeterm_core::daemon::SessionLockResult, ApiError>, treeterm_core::rt::TaskLost>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        use treeterm_core::daemon::SessionLockResult;
        let outcome = match result {
            Ok(Ok(SessionLockResult::Acquired(session))) => {
                self.version = session.version;
                self.lock = session.lock;
                Ok(())
            }
            Ok(Ok(SessionLockResult::HeldElsewhere(session))) => {
                self.lock = session.lock;
                Err("Session is locked by another window".to_owned())
            }
            Ok(Err(error)) => Err(error.to_string()),
            Err(lost) => Err(lost.to_string()),
        };
        cx.notify();
        outcome
    }

    fn finish_unlock(&mut self, result: Result<Result<Session, ApiError>, treeterm_core::rt::TaskLost>, cx: &mut Context<Self>) {
        match result {
            // The version comes from the daemon's answer, never an optimistic increment.
            Ok(Ok(session)) => {
                self.version = session.version;
                self.lock = session.lock;
            }
            Ok(Err(error)) => tracing::error!(%error, "failed to unlock session"),
            Err(lost) => tracing::error!(%lost, "failed to unlock session"),
        }
        cx.notify();
    }

    fn start_sync(
        &mut self,
        op: &QueuedSync,
        cx: &mut Context<Self>,
    ) -> Option<(RefSync, treeterm_core::rt::Fut<Result<Session, ApiError>>)> {
        if self.connection != ConnectionStatus::Connected || self.disposed {
            return None;
        }
        let refs = self.current_refs(cx);
        let json = refs_json(&refs);
        if json == self.last_synced_refs {
            return None;
        }
        tracing::info!(reasons = ?op.reasons, "syncing workspace refs to daemon");
        // Optimistic: our send becomes the known base so its echo is skipped; restored
        // if the call fails outright.
        let previous = std::mem::replace(&mut self.last_synced_refs, json.clone());
        let expected = self.version;
        let call = self.conn.update_session(refs, Some(expected), &self.client_id);
        Some((
            RefSync {
                previous,
                sent: json,
                expected,
            },
            call,
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

    /// Open a directory as a new root workspace (`addWorkspace`): a placeholder at once,
    /// then git info, the default tab, its body and its ref. Returns the new id.
    pub fn open_workspace(&mut self, path: String, cx: &mut Context<Self>) -> String {
        let id = new_workspace_id();
        let name = name_from_path(&path);
        self.entries.insert(
            id.clone(),
            WorkspaceEntry::Loading {
                name: name.clone(),
                message: "Loading workspace...".to_owned(),
                output: Vec::new(),
            },
        );
        cx.notify();
        let git = self.conn.git();
        let owned = id.clone();
        cx.spawn(async move |this, cx| {
            let info = git.get_info(&path).await;
            match this.update(cx, |session, cx| session.finish_open_workspace(owned, name, path, info, cx)) {
                Ok(()) | Err(_) => {} // Err: store dropped
            }
        })
        .detach();
        id
    }

    fn finish_open_workspace(
        &mut self,
        id: String,
        name: String,
        path: String,
        info: treeterm_core::git::GitInfo,
        cx: &mut Context<Self>,
    ) {
        use treeterm_core::git::GitInfo;
        use treeterm_core::wire::workspace_file::{AppState, WorkspaceStatus, WorktreeSettings};
        // Dismissed while git info was loading.
        if !matches!(self.entries.get(&id), Some(WorkspaceEntry::Loading { .. })) {
            return;
        }
        let global_default = self.settings.read(cx).settings().global_default_application_id.clone();
        let mut app_states = IndexMap::new();
        let mut active_tab_id = None;
        if let Some(app) = self.registry.default_for_workspace([], &global_default) {
            let tab_id = crate::workspace_store::new_tab_id();
            app_states.insert(
                tab_id.clone(),
                AppState {
                    application_id: app.id.clone(),
                    title: app.name.clone(),
                    state: app.initial_state(),
                },
            );
            active_tab_id = Some(tab_id);
        }
        let (is_git_repo, git_branch, git_root_path) = match info {
            GitInfo::NotRepo => (false, None, None),
            GitInfo::Repo { branch, root_path } => (true, Some(branch), Some(root_path)),
        };
        let now = now_millis();
        let file = WorkspaceFile {
            name,
            parent_id: None,
            status: WorkspaceStatus::Active,
            is_git_repo,
            git_branch,
            git_root_path,
            is_worktree: false,
            is_detached: Some(false),
            app_states,
            active_tab_id,
            metadata: [("sortOrder".to_owned(), self.next_sort_order(None, cx))].into(),
            settings: WorktreeSettings::default(),
            favourite_paths: Vec::new(),
            created_at: now,
            last_activity: now,
            extra: Default::default(),
        };
        // Opening does not wait for the publish; the ref sync retries on its own.
        drop(self.add_workspace(Workspace { id, path, file }, cx));
    }

    /// One past the largest `sortOrder` among a parent's children (or the roots).
    fn next_sort_order(&self, parent: Option<&str>, cx: &App) -> String {
        let max = self
            .entries
            .values()
            .filter_map(|entry| entry.store())
            .map(|store| store.read(cx).file())
            .filter(|file| file.parent_id.as_deref() == parent)
            .map(tree::sort_order)
            .max();
        match max {
            Some(max) => (max + 1).to_string(),
            None => "0".to_owned(),
        }
    }

    /// Drop a placeholder or failed workspace from this window (`dismissWorkspace`).
    pub fn dismiss_workspace(&mut self, id: &str, cx: &mut Context<Self>) {
        if matches!(
            self.entries.get(id),
            Some(WorkspaceEntry::Loading { .. } | WorkspaceEntry::Error { .. })
        ) {
            self.entries.shift_remove(id);
            cx.emit(SessionEvent::WorkspaceRemoved(id.to_owned()));
            cx.notify();
        }
    }

    /// Add a workspace built here (`createWorkspaceFile` + ref sync): write its first body
    /// (must-not-exist CAS), open its watch, then publish it.
    /// Returns a receiver that resolves once the membership including it is published.
    pub fn add_workspace(&mut self, workspace: Workspace, cx: &mut Context<Self>) -> oneshot::Receiver<()> {
        let (published_tx, published_rx) = oneshot::channel();
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
            published_tx.send(()).ok(); // the caller may not wait
        })
        .detach();
        drop(store);
        cx.notify();
        published_rx
    }

    /// Create a git worktree under `parent_id` and add it as a child workspace
    /// (`addChildWorkspace` → `createChildWithLoading`), branching from the parent's
    /// current branch. Returns the child's id at once; it shows as Loading with the git output
    /// until done, or as an Error.
    pub fn add_child_workspace(
        &mut self,
        parent_id: &str,
        name: String,
        detached: bool,
        description: Option<String>,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        self.create_child(
            parent_id,
            ChildSource::NewBranch { name },
            ChildOptions { detached, description },
            cx,
        )
    }

    /// Create a child workspace from `source` (`addChildWorkspace`,
    /// `createWorktreeFromBranch`, `createWorktreeFromRemote`, `createWorktreeFromPr`).
    pub fn create_child(
        &mut self,
        parent_id: &str,
        source: ChildSource,
        options: ChildOptions,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let parent = self
            .workspace_store(parent_id)
            .ok_or("Parent workspace not found")?
            .clone();
        let (parent_path, root_path) = {
            let ws = parent.read(cx).workspace();
            match (ws.file.is_git_repo, ws.file.git_root_path.clone()) {
                (true, Some(root)) => (ws.path.clone(), root),
                (true, None) | (false, _) => return Err("Parent workspace is not a git repository".to_owned()),
            }
        };
        let name = source.worktree_name();
        let id = new_workspace_id();
        self.entries.insert(
            id.clone(),
            WorkspaceEntry::Loading {
                name: name.clone(),
                message: source.message().to_owned(),
                output: Vec::new(),
            },
        );
        cx.notify();
        let lock = self.acquire_lock(cx);
        let git = self.conn.git();
        let child_id = id.clone();
        cx.spawn(async move |this, cx| {
            let locked = lock.await.unwrap_or_else(|_| Err("session store dropped".to_owned()));
            if let Err(error) = locked {
                match this.update(cx, |s, cx| s.fail_child(&child_id, &name, error, cx)) {
                    Ok(()) | Err(_) => {}
                }
                return;
            }
            // Before the git operation (`preOperation`).
            match &source {
                ChildSource::NewBranch { .. } => {
                    // The parent may have switched branches since it was last read.
                    if let treeterm_core::git::GitInfo::Repo { branch, .. } = git.get_info(&parent_path).await {
                        match this.update(cx, |_, cx| parent.update(cx, |p, cx| p.set_git_branch(&branch, cx))) {
                            Ok(()) | Err(_) => {}
                        }
                    }
                }
                // The PR head must exist as a remote-tracking ref locally.
                ChildSource::PullRequest { .. } => {
                    if let Err(error) = git.fetch(&root_path).await {
                        tracing::warn!(%error, "fetch before PR worktree failed; trying anyway");
                    }
                }
                ChildSource::ExistingBranch { .. } | ChildSource::RemoteBranch { .. } => {}
            }
            // Git's output goes to the Loading row as it arrives.
            let (lines_tx, mut lines_rx) = futures::channel::mpsc::unbounded::<String>();
            let report = move |line: &str| {
                lines_tx.unbounded_send(line.to_owned()).ok(); // the row may be gone
            };
            let drain = {
                let child_id = child_id.clone();
                this.update(cx, |_, cx| {
                    cx.spawn(async move |this, cx| {
                        while let Some(line) = lines_rx.next().await {
                            if this.update(cx, |s, cx| s.append_output(&child_id, line, cx)).is_err() {
                                return;
                            }
                        }
                    })
                })
            };
            let progress = treeterm_core::git::Progress::Report(&report);
            let created = match &source {
                ChildSource::NewBranch { name } => {
                    let base = match this.update(cx, |_, cx| parent.read(cx).file().git_branch.clone()) {
                        Ok(Some(branch)) => treeterm_core::git::WorktreeBase::Ref(branch),
                        Ok(None) | Err(_) => treeterm_core::git::WorktreeBase::Head,
                    };
                    git.create_worktree(&root_path, name, base, progress).await
                }
                ChildSource::ExistingBranch { branch } => {
                    git.create_worktree_from_branch(&root_path, branch, &source.worktree_name(), progress).await
                }
                ChildSource::RemoteBranch { remote_branch } => {
                    git.create_worktree_from_remote(&root_path, remote_branch, &source.worktree_name(), progress)
                        .await
                }
                ChildSource::PullRequest { head_ref, .. } => {
                    let remote_branch = format!("origin/{head_ref}");
                    git.create_worktree_from_remote(&root_path, &remote_branch, &source.worktree_name(), progress)
                        .await
                }
            };
            drop(report);
            drop(drain);
            let published = this.update(cx, |s, cx| match created {
                Ok(info) => {
                    let metadata = source.metadata(options.description.clone());
                    Some(s.finish_child(&child_id, &parent, name.clone(), info, options.detached, metadata, cx))
                }
                Err(error) => {
                    s.fail_child(&child_id, &name, error.to_string(), cx);
                    None
                }
            });
            if let Ok(Some(Some(published))) = published
                && published.await.is_err()
            {
                tracing::debug!("child publish dropped with the store");
            }
            release(&this, cx).await;
        })
        .detach();
        Ok(id)
    }

    /// Add an existing worktree of the parent's repository as a child workspace
    /// (`adoptExistingWorktree`). No git operation; refused if it is already open.
    pub fn adopt_existing_worktree(
        &mut self,
        parent_id: &str,
        worktree: treeterm_core::git::WorktreeInfo,
        description: Option<String>,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let parent = self
            .workspace_store(parent_id)
            .ok_or("Parent workspace not found")?
            .clone();
        let already_open = self
            .entries
            .values()
            .filter_map(WorkspaceEntry::store)
            .any(|store| store.read(cx).workspace().path == worktree.path);
        if already_open {
            return Err("This worktree is already open".to_owned());
        }
        let name = name_from_path(&worktree.path);
        let id = new_workspace_id();
        self.entries.insert(
            id.clone(),
            WorkspaceEntry::Loading {
                name: name.clone(),
                message: "Opening worktree...".to_owned(),
                output: Vec::new(),
            },
        );
        cx.notify();
        let lock = self.acquire_lock(cx);
        let child_id = id.clone();
        cx.spawn(async move |this, cx| {
            let locked = lock.await.unwrap_or_else(|_| Err("session store dropped".to_owned()));
            if let Err(error) = locked {
                match this.update(cx, |s, cx| s.fail_child(&child_id, &name, error, cx)) {
                    Ok(()) | Err(_) => {}
                }
                return;
            }
            let mut metadata = std::collections::BTreeMap::new();
            metadata.insert("branchIsUserDefined".to_owned(), "true".to_owned());
            if let Some(description) = description.filter(|d| !d.is_empty()) {
                metadata.insert("description".to_owned(), description);
            }
            let published = this.update(cx, |s, cx| s.finish_child(&child_id, &parent, name, worktree, false, metadata, cx));
            if let Ok(Some(published)) = published
                && published.await.is_err()
            {
                tracing::debug!("child publish dropped with the store");
            }
            release(&this, cx).await;
        })
        .detach();
        Ok(id)
    }

    fn append_output(&mut self, id: &str, line: String, cx: &mut Context<Self>) {
        if let Some(WorkspaceEntry::Loading { output, .. }) = self.entries.get_mut(id) {
            output.push(line);
            cx.notify();
        }
    }

    fn fail_child(&mut self, id: &str, name: &str, error: String, cx: &mut Context<Self>) {
        if self.entries.contains_key(id) {
            self.entries.insert(id.to_owned(), WorkspaceEntry::Error { name: name.to_owned(), error });
            cx.notify();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_child(
        &mut self,
        id: &str,
        parent: &Entity<WorkspaceStore>,
        name: String,
        info: treeterm_core::git::WorktreeInfo,
        detached: bool,
        extra_metadata: std::collections::BTreeMap<String, String>,
        cx: &mut Context<Self>,
    ) -> Option<oneshot::Receiver<()>> {
        use treeterm_core::wire::workspace_file::{AppState, WorkspaceStatus};
        // Dismissed while git ran.
        if !matches!(self.entries.get(id), Some(WorkspaceEntry::Loading { .. })) {
            return None;
        }
        let (parent_id, git_root_path, settings) = {
            let ws = parent.read(cx).workspace();
            (ws.id.clone(), ws.file.git_root_path.clone(), ws.file.settings.clone())
        };
        let inherited = self.inherited_default_apps(&parent_id, cx);
        let global_default = self.settings.read(cx).settings().global_default_application_id.clone();
        let mut app_states = IndexMap::new();
        let mut active_tab_id = None;
        let candidates = std::iter::once(settings.default_application_id.as_str())
            .filter(|id| !id.is_empty())
            .chain(inherited.iter().map(String::as_str));
        if let Some(app) = self.registry.default_for_workspace(candidates, &global_default) {
            let tab_id = crate::workspace_store::new_tab_id();
            app_states.insert(
                tab_id.clone(),
                AppState {
                    application_id: app.id.clone(),
                    title: app.name.clone(),
                    state: app.initial_state(),
                },
            );
            active_tab_id = Some(tab_id);
        }
        let mut metadata = extra_metadata;
        metadata.insert("sortOrder".to_owned(), self.next_sort_order(Some(&parent_id), cx));
        let now = now_millis();
        let file = WorkspaceFile {
            name,
            parent_id: Some(parent_id),
            status: WorkspaceStatus::Active,
            is_git_repo: true,
            git_branch: Some(info.branch),
            git_root_path,
            is_worktree: true,
            is_detached: Some(detached),
            app_states,
            active_tab_id,
            metadata,
            settings: Default::default(),
            favourite_paths: Vec::new(),
            created_at: now,
            last_activity: now,
            extra: Default::default(),
        };
        Some(self.add_workspace(
            Workspace {
                id: id.to_owned(),
                path: info.path,
                file,
            },
            cx,
        ))
    }

    /// Default application ids set on a workspace and its ancestors, nearest first
    /// (`getInheritedDefaultApplicationIds`).
    fn inherited_default_apps(&self, id: &str, cx: &App) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut current = Some(id.to_owned());
        while let Some(next) = current.take() {
            if !seen.insert(next.clone()) {
                break;
            }
            let Some(store) = self.workspace_store(&next) else { break };
            let file = store.read(cx).file();
            if !file.settings.default_application_id.is_empty() {
                out.push(file.settings.default_application_id.clone());
            }
            current = file.parent_id.clone();
        }
        out
    }

    /// Remove a workspace and its descendants (`removeWorkspace*`): under the session lock,
    /// children first, kill each one's tabs, clean up git as `mode` says, drop it; then
    /// publish the smaller membership and delete the body files. A failure stops the
    /// removal and shows on the workspace.
    pub fn remove_workspace(&mut self, id: &str, mode: RemoveMode, cx: &mut Context<Self>) {
        let Some(store) = self.workspace_store(id).cloned() else {
            return;
        };
        let id = id.to_owned();
        let lock = self.acquire_lock(cx);
        let git = self.conn.git();
        cx.spawn(async move |this, cx| {
            let locked = lock.await.unwrap_or_else(|_| Err("session store dropped".to_owned()));
            match locked {
                Ok(()) => {
                    if let Err(error) = remove_locked(&this, &id, mode, &git, cx).await {
                        match this.update(cx, |session, cx| session.set_operation_error(&id, &store, error, cx)) {
                            Ok(()) | Err(_) => {}
                        }
                    }
                    release(&this, cx).await;
                }
                Err(error) => match this.update(cx, |session, cx| session.set_operation_error(&id, &store, error, cx)) {
                    Ok(()) | Err(_) => {} // Err: store dropped
                },
            }
        })
        .detach();
    }

    /// Merge a child worktree's branch into its parent (`mergeAndRemoveWorkspace` /
    /// `mergeAndKeepWorkspace`), under the session lock. Refused while the parent has
    /// uncommitted changes. Before a merge that removes the child, its uncommitted work is
    /// committed as WIP; after it the child is marked merged and removed with its worktree
    /// and branch. Failures show on the child.
    pub fn merge_workspace(&mut self, id: &str, squash: bool, after: AfterMerge, cx: &mut Context<Self>) {
        let Some(store) = self.workspace_store(id).cloned() else {
            return;
        };
        let id = id.to_owned();
        let lock = self.acquire_lock(cx);
        let git = self.conn.git();
        cx.spawn(async move |this, cx| {
            let locked = lock.await.unwrap_or_else(|_| Err("session store dropped".to_owned()));
            let result = match locked {
                Ok(()) => {
                    let merged = merge_locked(&this, &id, squash, after, &git, cx).await;
                    let result = match (merged, after) {
                        (Ok(()), AfterMerge::Remove) => {
                            match this.update(cx, |_, cx| store.update(cx, |w, cx| w.set_status(WorkspaceStatus::Merged, cx))) {
                                Ok(()) | Err(_) => {}
                            }
                            remove_locked(&this, &id, RemoveMode::DeleteWorktreeAndBranch, &git, cx)
                                .await
                                .map_err(|error| format!("Merge succeeded but cleanup failed: {error}"))
                        }
                        (Ok(()), AfterMerge::Keep) => {
                            match this.update(cx, |session, cx| session.clear_workspace_error(&id, cx)) {
                                Ok(()) | Err(_) => {}
                            }
                            Ok(())
                        }
                        (Err(error), _) => Err(error),
                    };
                    release(&this, cx).await;
                    result
                }
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                match this.update(cx, |session, cx| session.set_operation_error(&id, &store, error, cx)) {
                    Ok(()) | Err(_) => {}
                }
            }
        })
        .detach();
    }

    /// `removeWorkspaceKeepBoth`.
    pub fn remove_workspace_keep_both(&mut self, id: &str, cx: &mut Context<Self>) {
        self.remove_workspace(id, RemoveMode::KeepWorktreeAndBranch, cx);
    }

    /// `id` and every workspace below it, children first.
    fn subtree(&self, id: &str, cx: &App) -> Vec<String> {
        let mut out = Vec::new();
        let children: Vec<String> = self
            .entries
            .iter()
            .filter_map(|(child, entry)| {
                let parent = entry.store()?.read(cx).file().parent_id.clone()?;
                (parent == id).then(|| child.clone())
            })
            .collect();
        for child in children {
            out.extend(self.subtree(&child, cx));
        }
        out.push(id.to_owned());
        out
    }

    fn set_operation_error(&mut self, id: &str, store: &Entity<WorkspaceStore>, error: String, cx: &mut Context<Self>) {
        if let Some(entry) = self.entries.get_mut(id) {
            *entry = WorkspaceEntry::OperationError {
                store: store.clone(),
                error,
            };
            cx.notify();
        }
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

type WeakSession = gpui_kit::WeakEntity<SessionStore>;

/// Release the session lock taken for an operation.
async fn release(this: &WeakSession, cx: &mut gpui_kit::AsyncApp) {
    let Ok(unlock) = this.update(cx, |session, cx| session.release_lock(cx)) else {
        return;
    };
    if unlock.await.is_err() {
        tracing::debug!("unlock dropped with the store");
    }
}

/// `removeWorkspaceInternal`, with the session lock already held: children first, kill
/// each workspace's tabs, clean up git, drop it; then publish the smaller membership and
/// delete the body files. Stops at the first git failure.
async fn remove_locked(
    this: &WeakSession,
    id: &str,
    mode: RemoveMode,
    git: &treeterm_core::git::GitApi,
    cx: &mut gpui_kit::AsyncApp,
) -> Result<(), String> {
    let gone = || "session store dropped".to_owned();
    let subtree = this.update(cx, |session, cx| session.subtree(id, cx)).map_err(|_| gone())?;
    let mut failure = None;
    for removed_id in &subtree {
        let target = this
            .update(cx, |session, cx| {
                let ws = session.workspace_store(removed_id)?.clone();
                ws.update(cx, |w, cx| w.close_all_tabs(cx));
                let w = ws.read(cx).workspace();
                Some(GitCleanup::for_workspace(&w.file, &w.path, mode))
            })
            .map_err(|_| gone())?;
        let cleaned = match target {
            Some(GitCleanup::Nothing) | None => Ok(()),
            Some(GitCleanup::RemoveWorktree { root, path, branch }) => {
                git.remove_worktree(&root, &path, branch, treeterm_core::git::Progress::Silent).await
            }
        };
        if let Err(error) = cleaned {
            failure = Some(error.to_string());
            break;
        }
        this.update(cx, |session, cx| session.on_workspace_removed(removed_id, cx))
            .map_err(|_| gone())?;
    }
    let published = this
        .update(cx, |session, cx| session.sync_refs("removeWorkspace", cx))
        .map_err(|_| gone())?;
    if published.await.is_err() {
        return Err(gone());
    }
    // Membership is the source of truth; deleting the files is cleanup after.
    let deletes = this
        .update(cx, |session, _| {
            subtree
                .iter()
                .filter(|removed_id| !session.entries.contains_key(*removed_id))
                .map(|removed_id| {
                    (removed_id.clone(), session.conn.delete_body(&session.data_dir, &format!("{removed_id}.json")))
                })
                .collect::<Vec<_>>()
        })
        .map_err(|_| gone())?;
    for (removed_id, delete) in deletes {
        match delete.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::error!(workspace = %removed_id, %error, "failed to delete workspace file"),
            Err(lost) => tracing::error!(workspace = %removed_id, %lost, "failed to delete workspace file"),
        }
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// `mergeWorkspaceCore`, with the session lock already held.
async fn merge_locked(
    this: &WeakSession,
    id: &str,
    squash: bool,
    after: AfterMerge,
    git: &treeterm_core::git::GitApi,
    cx: &mut gpui_kit::AsyncApp,
) -> Result<(), String> {
    use treeterm_core::git::{MergeMode, Progress};
    let (child_path, child_name, child_branch, parent_path) = this
        .update(cx, |session, cx| -> Result<_, String> {
            let child = session.workspace_store(id).ok_or("Workspace not found")?.read(cx).workspace();
            let (true, Some(parent_id)) = (child.file.is_worktree, child.file.parent_id.clone()) else {
                return Err("Not a worktree workspace".to_owned());
            };
            let parent = session
                .workspace_store(&parent_id)
                .map(|p| p.read(cx).workspace())
                .filter(|p| p.file.git_root_path.is_some() && p.file.git_branch.is_some())
                .ok_or("Parent workspace not found or not a git repo")?;
            let branch = child.file.git_branch.clone().ok_or("Workspace has no branch to merge")?;
            Ok((child.path.clone(), child.file.name.clone(), branch, parent.path.clone()))
        })
        .map_err(|_| "session store dropped".to_owned())??;
    if git.has_uncommitted_changes(&parent_path).await.map_err(|e| e.to_string())? {
        return Err("Parent workspace has uncommitted changes. Commit or stash them before merging.".to_owned());
    }
    if after == AfterMerge::Remove && git.has_uncommitted_changes(&child_path).await.map_err(|e| e.to_string())? {
        // Deviation from Electron: stage untracked files too. `commit -am` alone skips
        // them, so untracked-only work made the merge fail, and new files next to edits were
        // left out of the commit and then deleted with the worktree.
        git.stage_all(&child_path)
            .await
            .map_err(|e| format!("Failed to stage changes: {e}"))?;
        git.commit_all(&child_path, &format!("WIP: Auto-commit before merge from {child_name}"), Progress::Silent)
            .await
            .map_err(|e| format!("Failed to commit changes: {e}"))?;
    }
    let mode = if squash { MergeMode::Squash } else { MergeMode::Merge };
    git.merge(&parent_path, &child_branch, mode, Progress::Silent)
        .await
        .map_err(|e| format!("Merge failed: {e}"))
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
