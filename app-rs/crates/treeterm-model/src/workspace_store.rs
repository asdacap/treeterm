//! One loaded workspace: its body and the runtime state of its tabs. Port of the tab and
//! metadata half of `src/renderer/store/createWorkspaceStore.ts`.
//!
//! Every mutation of the body emits [`WorkspaceEvent::Changed`]; the session store owns
//! persistence and writes the body to the daemon. Bodies from other writers come in
//! through [`WorkspaceStore::apply_external`].

use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::{AppContext as _, Context, Entity, EventEmitter, Subscription, Task};
use serde_json::Value;
use treeterm_core::activity::{ActivityStates, TransitionDetail};
use treeterm_core::analyzer::idle_timeout as analyzer_idle_timeout;
use treeterm_core::attention::{
    ActiveView as AttentionView, AttentionAction, AttentionMutation, AttentionSyncState, AttentionTracker,
    ObservedSession, ObservedWorkspace, SessionStatus, apply_attention_mutations, has_unread_workspace_attention,
};
use treeterm_core::daemon::DaemonConnection;
use treeterm_core::pty::CreatePty;
use treeterm_core::settings::Settings;
use treeterm_core::wire::workspace_file::{AppState, WorkspaceFile};
use treeterm_term::fit::WidthLimit;
use treeterm_term::palette::{DEFAULT_BACKGROUND, SANDBOX_BACKGROUND};

use crate::analyzer::{AnalyzerEvent, AnalyzerModel, LlmService};
use crate::apps::{AppKind, AppRegistry};
use crate::terminal::{TerminalEvent, TerminalModel, TerminalOptions};

/// A workspace as the app uses it: the file body joined with its ref.
#[derive(Debug, Clone, PartialEq)]
pub struct Workspace {
    pub id: String,
    pub path: String,
    pub file: WorkspaceFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceEvent {
    /// The body changed locally and must be persisted.
    Changed,
    /// Work finished while the workspace was not shown: it was marked unread.
    AttentionRecorded,
    /// The display name or description changed (the worktree registry keeps them).
    LabelsChanged,
}

impl EventEmitter<WorkspaceEvent> for WorkspaceStore {}

pub use treeterm_core::activity::ActivityState;

/// How an activity state is shown (`ActivityStateBadge.tsx`).
pub trait ActivityStyle {
    fn label(self) -> &'static str;
    /// Badge colour.
    fn color(self) -> u32;
    /// The tab / tree glyph (`ActivityIndicator`).
    fn icon(self) -> &'static str;
}

impl ActivityStyle for ActivityState {
    fn label(self) -> &'static str {
        match self {
            ActivityState::Idle => "idle",
            ActivityState::Working => "working",
            ActivityState::UserInputRequired => "input required",
            ActivityState::PermissionRequest => "permission request",
            ActivityState::SafePermissionRequested => "safe permission",
            ActivityState::Completed => "completed",
            ActivityState::ApplicationError => "application error",
            ActivityState::Error => "error",
        }
    }

    fn color(self) -> u32 {
        match self {
            ActivityState::Idle => 0x666666,
            ActivityState::Working => 0x2472c8,
            ActivityState::UserInputRequired => 0xe5e510,
            ActivityState::PermissionRequest => 0xcd6600,
            ActivityState::SafePermissionRequested => 0x0dbc79,
            ActivityState::Completed => 0x23d18b,
            ActivityState::ApplicationError => 0xd16969,
            ActivityState::Error => 0xf44747,
        }
    }

    fn icon(self) -> &'static str {
        match self {
            ActivityState::Idle => "○",
            ActivityState::Working => "◌",
            ActivityState::UserInputRequired => "▶",
            ActivityState::PermissionRequest | ActivityState::SafePermissionRequested | ActivityState::Error => "●",
            ActivityState::Completed => "✓",
            ActivityState::ApplicationError => "✗",
        }
    }
}

/// What a tab shows.
pub enum TabContent {
    Terminal(Entity<TerminalModel>),
    Files(Entity<crate::file_browser::FileBrowser>),
    Editor(Entity<crate::text_editor::TextEditor>),
    Review(Entity<crate::review::ReviewModel>),
    /// A web page; the view owns the native webview. `url` opens the page remembered in the
    /// tab's state; `home` is the instance's start page.
    Browser { url: String, home: String },
    /// The tab's PTY is being created.
    StartingTerminal,
    Failed(String),
    /// An application the native app does not have yet.
    NotPorted(String),
    /// A dev server and its page; `terminal` once its PTY is attached.
    WebApp {
        app: Entity<crate::web_app::WebAppModel>,
        terminal: Option<Entity<TerminalModel>>,
    },
    /// The branch's pull request, from the session's git controller of this workspace.
    GitHub,
    /// This workspace's settings (edited through the store).
    WorkspaceSettings,
    AnalyzerHistory(Entity<crate::tool_tabs::analyzer_history::AnalyzerHistory>),
    Comments(Entity<crate::tool_tabs::comments::CommentsModel>),
    TtyList(Entity<crate::tool_tabs::tty_list::TtyList>),
    Chat(Entity<crate::tool_tabs::chat::ChatModel>),
    PromptDebugger(Entity<crate::tool_tabs::prompt_debugger::PromptDebugger>),
    SshUpload(Entity<crate::tool_tabs::ssh_upload::SshUpload>),
}

/// Runtime state of one tab; never persisted.
enum TabRef {
    Terminal(TerminalTab),
    Files {
        model: Entity<crate::file_browser::FileBrowser>,
        _events: Subscription,
    },
    Editor {
        model: Entity<crate::text_editor::TextEditor>,
        _events: Subscription,
    },
    Review {
        model: Entity<crate::review::ReviewModel>,
        _events: Subscription,
    },
    Browser { url: String, home: String },
    Tool(crate::tool_tabs::ToolTab),
}

enum TerminalTab {
    Creating {
        handle: String,
        _task: Task<()>,
    },
    Attached {
        model: Entity<TerminalModel>,
        /// AI-harness tabs: the LLM analyzer reading this terminal.
        analyzer: Option<(Entity<AnalyzerModel>, Subscription)>,
        _events: Subscription,
    },
    Failed(String),
}

pub struct WorkspaceStore {
    workspace: Workspace,
    conn: DaemonConnection,
    registry: Rc<AppRegistry>,
    settings: Rc<Settings>,
    tabs: HashMap<String, TabRef>,
    /// The parent workspace's branch, kept current by the session (Review's base).
    parent_branch: Option<String>,
    /// Per-tab activity and its transitions; runtime only (`activityState` store).
    activity: ActivityStates,
    /// Bumped whenever a tab's activity changes; attention compares it.
    activity_version: u64,
    /// Unread-marker edits not yet seen in a watched body; re-applied over bodies from
    /// other writers until they are (`AttentionSyncState`).
    attention: AttentionSyncState,
    /// When to mark unread and acknowledge (`workspaceAttentionCoordinator`). Runs on every
    /// activity change as it happens: observers are coalesced, and a short burst goes
    /// Working → Idle within one update.
    attention_tracker: AttentionTracker,
    /// The window shows this workspace.
    viewed: bool,
    /// Web App tabs' servers and pages (their PTYs are ordinary terminal tabs).
    web_apps: HashMap<String, (Entity<crate::web_app::WebAppModel>, Subscription)>,
    /// The ssh host this workspace lives on (web apps forward their port from it).
    remote: Option<crate::web_app::RemoteHost>,
    /// Runs AI-harness analyzers; `None` leaves them off (tests, no LLM client).
    llm: Option<LlmService>,
    /// PTY bytes received by this workspace's terminals, and their rate.
    traffic: Entity<crate::traffic::TrafficMeter>,
    traffic_counter: treeterm_core::traffic::ByteCounter,
    _git_refresh: Option<Task<()>>,
}

/// `#rrggbb` → colour; anything else is ignored (the default background is used).
fn parse_hex_color(text: &str) -> Option<treeterm_term::alacritty_terminal::vte::ansi::Rgb> {
    let hex = text.strip_prefix('#').filter(|h| h.len() == 6)?;
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(treeterm_term::alacritty_terminal::vte::ansi::Rgb {
        r: (value >> 16) as u8,
        g: (value >> 8) as u8,
        b: value as u8,
    })
}

pub(crate) fn new_tab_id() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let suffix: String = uuid::Uuid::new_v4().simple().to_string().chars().take(7).collect();
    format!("tab-{millis}-{suffix}")
}

impl WorkspaceStore {
    pub fn new(
        workspace: Workspace,
        conn: DaemonConnection,
        registry: Rc<AppRegistry>,
        settings: Rc<Settings>,
        llm: Option<LlmService>,
        remote: Option<crate::web_app::RemoteHost>,
        cx: &mut Context<Self>,
    ) -> Self {
        let traffic_counter = treeterm_core::traffic::ByteCounter::default();
        let counter = traffic_counter.clone();
        let traffic = cx.new(|cx| crate::traffic::TrafficMeter::new(counter, cx));
        let mut store = Self {
            workspace,
            conn,
            registry,
            settings,
            tabs: HashMap::new(),
            parent_branch: None,
            activity: ActivityStates::default(),
            activity_version: 0,
            attention: AttentionSyncState::default(),
            attention_tracker: AttentionTracker::default(),
            viewed: false,
            web_apps: HashMap::new(),
            remote,
            llm,
            traffic,
            traffic_counter,
            _git_refresh: None,
        };
        let tab_ids: Vec<String> = store.workspace.file.app_states.keys().cloned().collect();
        for tab_id in tab_ids {
            store.init_tab(&tab_id, cx);
        }
        store
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// The download rate of this workspace's terminals (`WorkspaceTrafficBadge`).
    pub fn traffic(&self) -> &Entity<crate::traffic::TrafficMeter> {
        &self.traffic
    }

    pub fn file(&self) -> &WorkspaceFile {
        &self.workspace.file
    }

    /// What the tab shows; `None` for a tab id the body does not have.
    pub fn tab_content(&self, tab_id: &str) -> Option<TabContent> {
        let tab = self.workspace.file.app_states.get(tab_id)?;
        if let Some((app, _)) = self.web_apps.get(tab_id) {
            let terminal = match self.tabs.get(tab_id) {
                Some(TabRef::Terminal(TerminalTab::Attached { model, .. })) => Some(model.clone()),
                Some(_) | None => None,
            };
            return Some(TabContent::WebApp { app: app.clone(), terminal });
        }
        Some(match self.tabs.get(tab_id) {
            Some(TabRef::Terminal(TerminalTab::Attached { model, .. })) => TabContent::Terminal(model.clone()),
            Some(TabRef::Terminal(TerminalTab::Creating { .. })) => TabContent::StartingTerminal,
            Some(TabRef::Terminal(TerminalTab::Failed(error))) => TabContent::Failed(error.clone()),
            Some(TabRef::Files { model, .. }) => TabContent::Files(model.clone()),
            Some(TabRef::Editor { model, .. }) => TabContent::Editor(model.clone()),
            Some(TabRef::Review { model, .. }) => TabContent::Review(model.clone()),
            Some(TabRef::Browser { url, home }) => TabContent::Browser { url: url.clone(), home: home.clone() },
            Some(TabRef::Tool(tool)) => tool.content(),
            None => TabContent::NotPorted(tab.application_id.clone()),
        })
    }

    /// The workspace's most urgent tab activity, for the tree (anything needing the user
    /// first, then work in progress).
    pub fn overall_activity(&self) -> ActivityState {
        self.activity.workspace_state(self.workspace.file.app_states.keys().map(String::as_str))
    }

    /// Every activity state and its transitions (debugging, attention).
    pub fn activity_states(&self) -> &ActivityStates {
        &self.activity
    }

    /// The analyzer of an AI-harness tab.
    pub fn analyzer(&self, tab_id: &str) -> Option<Entity<AnalyzerModel>> {
        match self.tabs.get(tab_id) {
            Some(TabRef::Terminal(TerminalTab::Attached { analyzer: Some((analyzer, _)), .. })) => Some(analyzer.clone()),
            Some(_) | None => None,
        }
    }

    /// Re-consult the LLM through the workspace's first AI-harness tab
    /// (`refreshTitleAndDescription` / `refreshBranchName`). Fails without one.
    pub fn refresh_labels(&mut self, branch: bool, cx: &mut Context<Self>) -> Result<(), String> {
        let analyzer = self
            .workspace
            .file
            .app_states
            .keys()
            .find_map(|tab| self.analyzer(tab))
            .ok_or_else(|| "No AI Harness tab in this workspace".to_owned())?;
        analyzer.update(cx, |a, cx| {
            if branch {
                a.refresh_branch_name(cx);
            } else {
                a.refresh_title_and_description(cx);
            }
        });
        Ok(())
    }

    pub fn is_favourite(&self) -> bool {
        self.workspace.file.metadata.get("isFavourite").map(String::as_str) == Some("true")
    }

    /// `promptHarness`: paste `text` into the workspace's first AI-harness tab (in tab
    /// order) whose terminal is attached, press Enter, and show that tab.
    pub fn prompt_harness(&mut self, text: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let target = self.workspace.file.app_states.iter().find_map(|(tab_id, tab)| {
            let app = self.registry.get(&tab.application_id)?;
            let AppKind::AiHarness { .. } = app.kind else {
                return None;
            };
            match self.tabs.get(tab_id) {
                Some(TabRef::Terminal(TerminalTab::Attached { model, .. })) => Some((tab_id.clone(), model.clone())),
                Some(
                    TabRef::Terminal(TerminalTab::Creating { .. } | TerminalTab::Failed(_))
                    | TabRef::Files { .. }
                    | TabRef::Editor { .. }
                    | TabRef::Review { .. }
                    | TabRef::Browser { .. }
                    | TabRef::Tool(_),
                )
                | None => None,
            }
        });
        let Some((tab_id, terminal)) = target else {
            return Err("No AI harness is running in this workspace".to_owned());
        };
        terminal.update(cx, |terminal, cx| {
            terminal.paste(text, cx);
            terminal.send_unfocused(b"\r");
        });
        self.set_active_tab(&tab_id, cx);
        Ok(())
    }

    /// The status bar's "Auto-approve safe" switch, persisted per tab.
    pub fn set_auto_approve(&mut self, tab_id: &str, enabled: bool, cx: &mut Context<Self>) {
        self.update_tab_state(
            tab_id,
            |state| {
                if let Value::Object(state) = state {
                    state.insert("autoApprove".into(), Value::Bool(enabled));
                }
            },
            cx,
        );
        if let Some(analyzer) = self.analyzer(tab_id) {
            analyzer.update(cx, |a, cx| a.set_auto_approve(enabled, cx));
        }
    }

    /// Set by the session whenever the parent's branch is known or changes. Review tabs
    /// opened before it was known start comparing once it is.
    pub fn set_parent_branch(&mut self, branch: Option<String>, cx: &mut Context<Self>) {
        if self.parent_branch == branch {
            return;
        }
        self.parent_branch = branch;
        let reviews: Vec<String> = self
            .tabs
            .iter()
            .filter(|(_, tab)| matches!(tab, TabRef::Review { .. }))
            .map(|(id, _)| id.clone())
            .collect();
        for tab_id in reviews {
            self.dispose_tab(&tab_id);
            self.init_tab(&tab_id, cx);
        }
        cx.notify();
    }

    /// The parent workspace's branch, when known (Review's base, the rebase prompt).
    pub fn parent_branch(&self) -> Option<&str> {
        self.parent_branch.as_deref()
    }

    /// `settings.terminal.maxCols`, the width the "col limit" switch caps at.
    pub fn max_cols(&self) -> u16 {
        self.settings.terminal.max_cols
    }

    pub fn activity(&self, tab_id: &str) -> ActivityState {
        self.activity.get(tab_id).unwrap_or_default()
    }

    fn set_activity(&mut self, tab_id: &str, state: ActivityState, detail: TransitionDetail, cx: &mut Context<Self>) {
        if self.activity.get(tab_id) != Some(state) {
            self.activity.set_tab_state(tab_id, state, detail, std::time::SystemTime::now());
            self.activity_version += 1;
            self.observe_attention(true, cx);
            cx.notify();
        }
    }

    /// The window starts or stops showing this workspace; shown, it is marked read.
    pub fn set_viewed(&mut self, viewed: bool, cx: &mut Context<Self>) {
        if self.viewed != viewed {
            self.viewed = viewed;
            self.observe_attention(false, cx);
        }
    }

    /// Run the attention tracker over the current state and apply what it decides.
    fn observe_attention(&mut self, activity_changed: bool, cx: &mut Context<Self>) {
        let observed = [ObservedWorkspace {
            id: &self.workspace.id,
            tab_ids: self.workspace.file.app_states.keys().map(String::as_str).collect(),
            metadata: &self.workspace.file.metadata,
        }];
        let view = if self.viewed {
            AttentionView::Workspace { session_id: "", workspace_id: &self.workspace.id }
        } else {
            AttentionView::Other
        };
        let session = ObservedSession { session_id: "", status: SessionStatus::Ready(&observed) };
        let actions = match self.attention_tracker.observe(session, &self.activity, view, activity_changed) {
            Ok(actions) => actions,
            Err(error) => return tracing::warn!(%error, workspace = %self.workspace.id, "attention"),
        };
        for action in actions {
            match action {
                AttentionAction::Record { .. } => {
                    self.record_attention(cx);
                    cx.emit(WorkspaceEvent::AttentionRecorded);
                }
                AttentionAction::Acknowledge { revision, .. } => self.acknowledge_attention(&revision, cx),
            }
        }
    }

    pub fn activity_version(&self) -> u64 {
        self.activity_version
    }

    /// The workspace carries an unread attention marker.
    pub fn has_unread_attention(&self) -> bool {
        match has_unread_workspace_attention(&self.workspace.file.metadata) {
            Ok(unread) => unread,
            Err(error) => {
                tracing::warn!(%error, workspace = %self.workspace.id, "unreadable attention metadata");
                false
            }
        }
    }

    /// Mark unread with a fresh revision (`recordWorkspaceAttention`).
    pub fn record_attention(&mut self, cx: &mut Context<Self>) {
        self.queue_attention(AttentionMutation::record(), cx);
    }

    /// Mark read if `revision` is still the current one (`acknowledgeWorkspaceAttention`).
    pub fn acknowledge_attention(&mut self, revision: &str, cx: &mut Context<Self>) {
        self.queue_attention(AttentionMutation::acknowledge(revision), cx);
    }

    fn queue_attention(&mut self, mutation: AttentionMutation, cx: &mut Context<Self>) {
        if !self.attention.queue(mutation) {
            return;
        }
        if self.reapply_attention() {
            self.sync_idle_timeouts(cx);
            self.changed(cx);
        }
    }

    /// Lay the pending attention edits over the current metadata; whether it changed.
    fn reapply_attention(&mut self) -> bool {
        let metadata = &self.workspace.file.metadata;
        match apply_attention_mutations(metadata, self.attention.mutations()) {
            Ok(std::borrow::Cow::Borrowed(_)) => false,
            Ok(std::borrow::Cow::Owned(updated)) if updated == *metadata => false,
            Ok(std::borrow::Cow::Owned(updated)) => {
                self.workspace.file.metadata = updated;
                true
            }
            Err(error) => {
                tracing::error!(%error, workspace = %self.workspace.id, "cannot apply attention");
                false
            }
        }
    }

    /// The idle debounce for this workspace's terminals: longer while it carries an unread
    /// attention marker.
    fn idle_timeout(&self) -> std::time::Duration {
        let unread = match has_unread_workspace_attention(&self.workspace.file.metadata) {
            Ok(unread) => unread,
            Err(error) => {
                tracing::warn!(%error, "unreadable attention metadata");
                false
            }
        };
        analyzer_idle_timeout(&self.settings.terminal_analyzer, unread)
    }

    /// Metadata or settings changed: terminals take the current idle debounce.
    fn sync_idle_timeouts(&mut self, cx: &mut Context<Self>) {
        let timeout = self.idle_timeout();
        let models: Vec<Entity<TerminalModel>> = self.terminals().cloned().collect();
        for model in models {
            model.update(cx, |m, cx| m.set_idle_timeout(Some(timeout), cx));
        }
    }

    /// Re-read the checked-out branch (the analyzer saw the program change things).
    pub fn refresh_git_branch(&mut self, cx: &mut Context<Self>) {
        let git = self.conn.git();
        let path = self.workspace.path.clone();
        self._git_refresh = Some(cx.spawn(async move |this, cx| {
            let info = git.get_info(&path).await;
            match this.update(cx, |store, cx| match info {
                treeterm_core::git::GitInfo::Repo { branch, .. } => store.set_git_branch(&branch, cx),
                treeterm_core::git::GitInfo::NotRepo => {}
            }) {
                Ok(()) | Err(_) => {} // Err: workspace gone
            }
        }));
    }

    /// A boolean in the tab's state that defaults to false (the `*Disabled` flags).
    pub fn tab_flag(&self, tab_id: &str, key: &str) -> bool {
        self.workspace
            .file
            .app_states
            .get(tab_id)
            .and_then(|tab| tab.state.get(key))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// The status bar's "col limit" switch: persisted per tab, applied to its terminal.
    pub fn set_width_limit_enabled(&mut self, tab_id: &str, enabled: bool, cx: &mut Context<Self>) {
        self.update_tab_state(
            tab_id,
            |state| {
                if let Value::Object(state) = state {
                    state.insert("widthLimitDisabled".into(), Value::Bool(!enabled));
                }
            },
            cx,
        );
        let limit = if enabled {
            WidthLimit::Capped(self.settings.terminal.max_cols)
        } else {
            WidthLimit::Off
        };
        if let Some(TabRef::Terminal(TerminalTab::Attached { model, .. })) = self.tabs.get(tab_id) {
            model.update(cx, |model, cx| model.set_width_limit(limit, cx));
        }
    }

    /// The status bar's "Idle detector" switch, persisted per tab.
    pub fn set_idle_detector_enabled(&mut self, tab_id: &str, enabled: bool, cx: &mut Context<Self>) {
        self.update_tab_state(
            tab_id,
            |state| {
                if let Value::Object(state) = state {
                    state.insert("idleDetectorDisabled".into(), Value::Bool(!enabled));
                }
            },
            cx,
        );
    }

    /// Every terminal this workspace holds, for automation and teardown.
    pub fn terminals(&self) -> impl Iterator<Item = &Entity<TerminalModel>> {
        self.tabs.values().filter_map(|tab| match tab {
            TabRef::Terminal(TerminalTab::Attached { model, .. }) => Some(model),
            TabRef::Terminal(TerminalTab::Creating { .. } | TerminalTab::Failed(_))
            | TabRef::Files { .. }
            | TabRef::Editor { .. }
            | TabRef::Review { .. }
            | TabRef::Browser { .. }
            | TabRef::Tool(_) => None,
        })
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(WorkspaceEvent::Changed);
        cx.notify();
    }

    // ---- tabs --------------------------------------------------------------------------

    /// Create the runtime side of a tab (`initTab` → `onWorkspaceLoad`).
    fn init_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if self.tabs.contains_key(tab_id) || self.web_apps.contains_key(tab_id) {
            return;
        }
        let Some(tab) = self.workspace.file.app_states.get(tab_id) else {
            return;
        };
        let Some(app) = self.registry.get(&tab.application_id) else {
            return; // not ported: the tab stays as the other app left it
        };
        let startup_command = app.startup_command(&self.workspace.path);
        match &app.kind {
            AppKind::WebApp { command, .. } => {
                let assigned = |key: &str| {
                    let port = tab.state.get(key)?;
                    (port.get("status").and_then(Value::as_str) == Some("assigned"))
                        .then(|| port.get("port").and_then(Value::as_u64))
                        .flatten()
                        .and_then(|p| u16::try_from(p).ok())
                };
                let (port, local_port) = (assigned("port"), assigned("localPort"));
                let pty = tab.state.get("ptyId").and_then(Value::as_str).map(str::to_owned);
                let handle = match tab.state.get("ptyHandle") {
                    Some(Value::String(handle)) => handle.clone(),
                    Some(_) | None => uuid::Uuid::new_v4().to_string(),
                };
                let (exec, owned, remote, command) =
                    (self.conn.exec_client(), tab_id.to_owned(), self.remote.clone(), command.clone());
                let model = cx.new(|cx| crate::web_app::WebAppModel::new(exec, owned, port, local_port, remote, cx));
                let owned_tab = tab_id.to_owned();
                let events = cx.subscribe(&model, move |store, _, event: &crate::web_app::WebAppEvent, cx| {
                    store.on_web_app_event(&owned_tab, *event, cx);
                });
                self.web_apps.insert(tab_id.to_owned(), (model, events));
                // The server runs in a terminal once its port is known.
                let terminal = match (pty, port) {
                    (Some(pty_id), _) => Some(self.attach_terminal(tab_id, pty_id, cx)),
                    (None, Some(port)) => {
                        Some(self.create_pty(tab_id, handle, Some(crate::web_app::resolve_command(&command, port)), cx))
                    }
                    (None, None) => None,
                };
                if let Some(terminal) = terminal {
                    self.tabs.insert(tab_id.to_owned(), TabRef::Terminal(terminal));
                }
            }
            AppKind::Files => {
                let fs = self.conn.fs_client();
                let exec: std::sync::Arc<dyn treeterm_core::exec::ExecApi> = std::sync::Arc::new(self.conn.exec_client());
                let root = self.workspace.path.clone();
                let state = tab.state.clone();
                let (workspace, favourites) = (cx.entity(), self.workspace.file.favourite_paths.clone());
                let model = cx.new(|cx| {
                    crate::file_browser::FileBrowser::new(fs, exec, root, &state, &workspace, favourites, cx)
                });
                let owned_tab = tab_id.to_owned();
                let events = cx.subscribe(&model, move |store, _, event: &crate::file_browser::FileBrowserEvent, cx| {
                    let crate::file_browser::FileBrowserEvent::StateChanged(next) = event;
                    store.merge_tab_state(&owned_tab, next.clone(), cx);
                });
                self.tabs.insert(tab_id.to_owned(), TabRef::Files { model, _events: events });
            }
            AppKind::Browser { url } => {
                let path = tab.state.get("path").and_then(Value::as_str).unwrap_or_default();
                let full = format!("{}{}", url.trim_end_matches('/'), if path.is_empty() || path.starts_with('/') { path.to_owned() } else { format!("/{path}") });
                self.tabs.insert(tab_id.to_owned(), TabRef::Browser { url: full, home: url.clone() });
            }
            AppKind::Review => {
                let base = match (tab.state.get("baseBranchOverride").and_then(Value::as_str), &self.parent_branch) {
                    (Some(branch), _) if !branch.is_empty() => crate::review::ReviewBase::Branch(branch.to_owned()),
                    (_, Some(branch)) => crate::review::ReviewBase::Branch(branch.clone()),
                    (_, None) => crate::review::ReviewBase::None,
                };
                let (exec, git, path, state) =
                    (self.conn.exec_client(), self.conn.git(), self.workspace.path.clone(), tab.state.clone());
                let workspace = cx.entity();
                let model = cx.new(|cx| crate::review::ReviewModel::new(exec, git, path, base, &state, &workspace, cx));
                let owned_tab = tab_id.to_owned();
                let events = cx.subscribe(&model, move |store, _, event: &crate::review::ReviewEvent, cx| {
                    match event {
                        crate::review::ReviewEvent::StateChanged(next) => {
                            store.merge_tab_state(&owned_tab, next.clone(), cx);
                        }
                        crate::review::ReviewEvent::Committed => {}
                    }
                });
                self.tabs.insert(tab_id.to_owned(), TabRef::Review { model, _events: events });
            }
            AppKind::Editor => {
                let fs = self.conn.fs_client();
                let root = self.workspace.path.clone();
                let state = tab.state.clone();
                let model = cx.new(|cx| crate::text_editor::TextEditor::new(fs, root, &state, cx));
                let owned_tab = tab_id.to_owned();
                let events = cx.subscribe(&model, move |store, _, event: &crate::text_editor::TextEditorEvent, cx| {
                    let crate::text_editor::TextEditorEvent::StateChanged(next) = event;
                    store.merge_tab_state(&owned_tab, next.clone(), cx);
                });
                self.tabs.insert(tab_id.to_owned(), TabRef::Editor { model, _events: events });
            }
            AppKind::Tool(tool) => {
                let (tool, state) = (*tool, tab.state.clone());
                let runtime = self.init_tool_tab(tab_id, tool, &state, cx);
                self.tabs.insert(tab_id.to_owned(), TabRef::Tool(runtime));
            }
            AppKind::Terminal { .. } | AppKind::AiHarness { .. } | AppKind::CustomRunner { .. } => {
                let terminal = match tab.state.get("ptyId") {
                    Some(Value::String(pty_id)) => self.attach_terminal(tab_id, pty_id.clone(), cx),
                    Some(_) | None => {
                        let handle = match tab.state.get("ptyHandle") {
                            Some(Value::String(handle)) => handle.clone(),
                            Some(_) | None => uuid::Uuid::new_v4().to_string(),
                        };
                        self.create_pty(tab_id, handle, startup_command, cx)
                    }
                };
                self.tabs.insert(tab_id.to_owned(), TabRef::Terminal(terminal));
            }
        }
    }

    /// The runtime side of a tool tab (`tool_tabs`).
    fn init_tool_tab(
        &mut self,
        tab_id: &str,
        tool: crate::apps::ToolApp,
        state: &Value,
        cx: &mut Context<Self>,
    ) -> crate::tool_tabs::ToolTab {
        use crate::apps::ToolApp;
        use crate::tool_tabs::{ToolTab, analyzer_history, chat, comments, prompt_debugger, ssh_upload, tty_list};
        let (workspace, weak) = (cx.entity(), cx.weak_entity());
        match tool {
            ToolApp::GitHub => ToolTab::GitHub,
            ToolApp::WorkspaceSettings => ToolTab::WorkspaceSettings,
            ToolApp::AnalyzerHistory => {
                let source = state.get("sourceTabId").and_then(Value::as_str).and_then(|tab| self.analyzer(tab));
                ToolTab::AnalyzerHistory(
                    cx.new(|cx| analyzer_history::AnalyzerHistory::new(state, &workspace, source, cx)),
                )
            }
            ToolApp::Comments => {
                let (fs, git, root) = (self.conn.fs_client(), self.conn.git(), self.workspace.path.clone());
                ToolTab::Comments(cx.new(|cx| comments::CommentsModel::new(fs, git, root, &workspace, cx)))
            }
            ToolApp::TtyList => {
                let (conn, path) = (self.conn.clone(), self.workspace.path.clone());
                ToolTab::TtyList(cx.new(|cx| tty_list::TtyList::new(conn, weak, path, cx)))
            }
            ToolApp::Chat => {
                let llm = self.llm.clone();
                let model = cx.new(|_| chat::ChatModel::new(llm, state, weak));
                let owned_tab = tab_id.to_owned();
                let events = cx.subscribe(&model, move |store, _, event: &chat::ChatEvent, cx| {
                    let chat::ChatEvent::StateChanged(next) = event;
                    store.merge_tab_state(&owned_tab, next.clone(), cx);
                });
                ToolTab::Chat { model, _events: events }
            }
            ToolApp::PromptDebugger => {
                let (llm, settings) = (self.llm.clone(), self.settings.clone());
                let model = cx.new(|_| prompt_debugger::PromptDebugger::new(llm, &settings, state, weak));
                let owned_tab = tab_id.to_owned();
                let events = cx.subscribe(&model, move |store, _, event: &prompt_debugger::PromptDebuggerEvent, cx| {
                    let prompt_debugger::PromptDebuggerEvent::SetActivity { state, detail } = event;
                    store.set_activity(&owned_tab, *state, detail.clone(), cx);
                });
                ToolTab::PromptDebugger { model, _events: events }
            }
            ToolApp::SshUpload => {
                let remote = self.remote.clone();
                ToolTab::SshUpload(cx.new(|cx| ssh_upload::SshUpload::new(remote, cx)))
            }
        }
    }

    fn terminal_options(&self, tab_id: &str, pty_id: String) -> TerminalOptions {
        let state = &self.workspace.file.app_states[tab_id].state;
        let settings = &self.settings;
        let flag = |key: &str| state.get(key).and_then(Value::as_bool).unwrap_or(false);
        let sandboxed = state
            .get("sandbox")
            .and_then(|s| s.get("enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let app_background = self
            .registry
            .get(&self.workspace.file.app_states[tab_id].application_id)
            .and_then(|app| app.background())
            .and_then(parse_hex_color);
        TerminalOptions {
            pty_id,
            background: match (app_background, sandboxed) {
                (Some(color), _) => color,
                (None, true) => SANDBOX_BACKGROUND,
                (None, false) => DEFAULT_BACKGROUND,
            },
            settings: settings.terminal.clone(),
            keep_on_exit: flag("keepOnExit"),
            width_limit: if flag("widthLimitDisabled") {
                WidthLimit::Off
            } else {
                WidthLimit::Capped(settings.terminal.max_cols)
            },
            // Always detecting: the per-tab switch gates only the Working edge, read when it
            // fires, so turning it off still lets a pending Idle land.
            idle_timeout: Some(self.idle_timeout()),
            traffic: self.traffic_counter.clone(),
        }
    }

    fn attach_terminal(&mut self, tab_id: &str, pty_id: String, cx: &mut Context<Self>) -> TerminalTab {
        let options = self.terminal_options(tab_id, pty_id);
        let conn = self.conn.clone();
        let model = cx.new(|cx| TerminalModel::new(conn, options, cx));
        let analyzer = self.start_analyzer(tab_id, &model, cx);
        let analyzed = analyzer.is_some();
        let owned_tab = tab_id.to_owned();
        let events = cx.subscribe(&model, move |store, _, event: &TerminalEvent, cx| match event {
            TerminalEvent::CloseRequested { .. } => store.remove_tab(&owned_tab, cx),
            // An analyzed tab's activity comes from its analyzer.
            TerminalEvent::Activity(snapshot) if !analyzed => {
                if !store.tab_flag(&owned_tab, "idleDetectorDisabled") {
                    let detail = TransitionDetail::ViewportChanged { snapshot: snapshot.clone() };
                    store.set_activity(&owned_tab, ActivityState::Working, detail, cx);
                }
            }
            TerminalEvent::Idle(edge) if !analyzed => {
                let detail = TransitionDetail::ViewportIdle {
                    snapshot: edge.snapshot.clone(),
                    idle_timeout: edge.idle_timeout,
                };
                store.set_activity(&owned_tab, ActivityState::Idle, detail, cx);
            }
            TerminalEvent::Activity(_)
            | TerminalEvent::Idle(_)
            | TerminalEvent::Clipboard(_)
            | TerminalEvent::Bell
            | TerminalEvent::Input(_)
            | TerminalEvent::Exited(_) => {}
        });
        TerminalTab::Attached {
            model,
            analyzer,
            _events: events,
        }
    }

    /// An AI-harness tab gets an analyzer (`initAnalyzer`), when an LLM client exists.
    fn start_analyzer(
        &mut self,
        tab_id: &str,
        terminal: &Entity<TerminalModel>,
        cx: &mut Context<Self>,
    ) -> Option<(Entity<AnalyzerModel>, Subscription)> {
        let tab = self.workspace.file.app_states.get(tab_id)?;
        let app = self.registry.get(&tab.application_id)?;
        let AppKind::AiHarness { .. } = app.kind else {
            return None;
        };
        let llm = self.llm.clone()?;
        let auto_approve = tab.state.get("autoApprove").and_then(Value::as_bool).unwrap_or(false);
        let cwd = self.workspace.path.clone();
        let repo_path = self.workspace.file.git_root_path.clone().unwrap_or_else(|| cwd.clone());
        let (settings, git, terminal, workspace, owned_tab) =
            (self.settings.clone(), self.conn.git(), terminal.clone(), cx.weak_entity(), tab_id.to_owned());
        let analyzer = cx.new(|cx| {
            AnalyzerModel::new(&settings, cwd, repo_path, auto_approve, llm, git, terminal, workspace, owned_tab, cx)
        });
        let owned_tab = tab_id.to_owned();
        let events = cx.subscribe(&analyzer, move |store, _, event: &AnalyzerEvent, cx| match event {
            AnalyzerEvent::SetActivity { state, detail } => store.set_activity(&owned_tab, *state, detail.clone(), cx),
            AnalyzerEvent::UpdateMetadata { key, value } => store.update_metadata(key, value, cx),
            AnalyzerEvent::BranchRenamed(branch) => store.set_git_branch(branch, cx),
            AnalyzerEvent::RefreshGit => store.refresh_git_branch(cx),
        });
        Some((analyzer, events))
    }

    /// `ensureTty`: create the tab's PTY, keyed by its persisted handle so the daemon
    /// returns the same PTY however often this runs (reconciliation can dispose and
    /// re-init a tab mid-creation). On success the id lands in the tab's state (and so on
    /// disk) and the terminal attaches.
    fn create_pty(
        &mut self,
        tab_id: &str,
        handle: String,
        startup_command: Option<String>,
        cx: &mut Context<Self>,
    ) -> TerminalTab {
        let create = self.conn.create_pty(CreatePty {
            cwd: self.workspace.path.clone(),
            handle: Some(handle.clone()),
            startup_command,
            ..CreatePty::default()
        });
        let tab_id = tab_id.to_owned();
        let owned_handle = handle.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = match create.await {
                Ok(Ok(pty_id)) => Ok(pty_id),
                Ok(Err(error)) => Err(error.to_string()),
                Err(lost) => Err(lost.to_string()),
            };
            match this.update(cx, |store, cx| store.on_pty_created(&tab_id, &owned_handle, result, cx)) {
                Ok(()) | Err(_) => {} // Err: the workspace went away; the daemon kept the PTY
            }
        });
        TerminalTab::Creating { handle, _task: task }
    }

    fn on_pty_created(&mut self, tab_id: &str, handle: &str, result: Result<String, String>, cx: &mut Context<Self>) {
        let pty_id = match result {
            Ok(pty_id) => pty_id,
            Err(error) => {
                tracing::error!(%tab_id, %error, "failed to create PTY");
                if self.tabs.contains_key(tab_id) {
                    self.tabs.insert(
                        tab_id.to_owned(),
                        TabRef::Terminal(TerminalTab::Failed(format!("Failed to start terminal: {error}"))),
                    );
                    cx.notify();
                }
                return;
            }
        };
        // Closed while the PTY was starting: nothing references it any more, so kill it.
        let Some(tab) = self.workspace.file.app_states.get_mut(tab_id) else {
            let kill = self.conn.kill_pty(&pty_id);
            cx.spawn(async move |_, _| match kill.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::error!(%pty_id, %error, "failed to kill orphaned PTY"),
                Err(lost) => tracing::error!(%pty_id, %lost, "failed to kill orphaned PTY"),
            })
            .detach();
            return;
        };
        if let Value::Object(state) = &mut tab.state {
            state.insert("ptyId".into(), Value::String(pty_id.clone()));
            state.insert("ptyHandle".into(), Value::String(handle.to_owned()));
        }
        let terminal = self.attach_terminal(tab_id, pty_id, cx);
        self.tabs.insert(tab_id.to_owned(), TabRef::Terminal(terminal));
        self.changed(cx);
    }

    /// Release a tab's runtime state without touching the body (`disposeTabResources`).
    /// Detaches its terminal; never kills the PTY.
    /// A web app picked its port (start its server) or its local forward port.
    fn on_web_app_event(&mut self, tab_id: &str, event: crate::web_app::WebAppEvent, cx: &mut Context<Self>) {
        use crate::web_app::WebAppEvent;
        let (key, port) = match event {
            WebAppEvent::PortAssigned(port) => ("port", port),
            WebAppEvent::LocalPortAssigned(port) => ("localPort", port),
        };
        self.update_tab_state(
            tab_id,
            |state| {
                if let Value::Object(state) = state {
                    state.insert(key.into(), serde_json::json!({ "status": "assigned", "port": port }));
                }
            },
            cx,
        );
        let WebAppEvent::PortAssigned(port) = event else {
            return;
        };
        if self.tabs.contains_key(tab_id) {
            return;
        }
        let Some(tab) = self.workspace.file.app_states.get(tab_id) else {
            return;
        };
        let command = match self.registry.get(&tab.application_id).map(|app| &app.kind) {
            Some(AppKind::WebApp { command, .. }) => command.clone(),
            Some(_) | None => return,
        };
        let handle = match tab.state.get("ptyHandle") {
            Some(Value::String(handle)) => handle.clone(),
            Some(_) | None => uuid::Uuid::new_v4().to_string(),
        };
        let terminal = self.create_pty(tab_id, handle, Some(crate::web_app::resolve_command(&command, port)), cx);
        self.tabs.insert(tab_id.to_owned(), TabRef::Terminal(terminal));
        cx.notify();
    }

    fn dispose_tab(&mut self, tab_id: &str) {
        self.tabs.remove(tab_id);
        self.web_apps.remove(tab_id);
        self.activity.remove_tab_state(tab_id);
        self.activity_version += 1;
    }

    /// Re-baseline attention after tabs came or went (state removal is not completion).
    fn tabs_changed(&mut self, cx: &mut Context<Self>) {
        self.observe_attention(false, cx);
    }

    /// Add a tab running `application_id` and make it active. Returns the tab id, or `None`
    /// for an application this app does not have.
    pub fn add_tab(&mut self, application_id: &str, cx: &mut Context<Self>) -> Option<String> {
        let app = self.registry.get(application_id)?.clone();
        let tab_id = new_tab_id();
        let existing = self
            .workspace
            .file
            .app_states
            .values()
            .filter(|tab| tab.application_id == application_id)
            .count();
        self.workspace.file.app_states.insert(
            tab_id.clone(),
            AppState {
                application_id: application_id.to_owned(),
                title: format!("{} {}", app.name, existing + 1),
                state: app.initial_state(),
            },
        );
        self.workspace.file.active_tab_id = Some(tab_id.clone());
        self.tabs_changed(cx);
        self.changed(cx);
        self.init_tab(&tab_id, cx);
        Some(tab_id)
    }

    /// Merge `next`'s keys over the tab's state (keys the other app wrote survive).
    fn merge_tab_state(&mut self, tab_id: &str, next: Value, cx: &mut Context<Self>) {
        self.update_tab_state(
            tab_id,
            |state| {
                if let (Value::Object(state), Value::Object(next)) = (state, next) {
                    for (key, value) in next {
                        state.insert(key, value);
                    }
                }
            },
            cx,
        );
    }

    /// Add a tab with `overrides` merged over the application's initial state
    /// (`addTab(applicationId, initialState)`), e.g. an editor for a given file.
    pub fn add_tab_with(&mut self, application_id: &str, overrides: Value, cx: &mut Context<Self>) -> Option<String> {
        let app = self.registry.get(application_id)?.clone();
        let tab_id = new_tab_id();
        let existing = self
            .workspace
            .file
            .app_states
            .values()
            .filter(|tab| tab.application_id == application_id)
            .count();
        let mut state = app.initial_state();
        if let (Value::Object(state), Value::Object(overrides)) = (&mut state, overrides) {
            for (key, value) in overrides {
                state.insert(key, value);
            }
        }
        self.workspace.file.app_states.insert(
            tab_id.clone(),
            AppState {
                application_id: application_id.to_owned(),
                title: format!("{} {}", app.name, existing + 1),
                state,
            },
        );
        self.workspace.file.active_tab_id = Some(tab_id.clone());
        self.tabs_changed(cx);
        self.changed(cx);
        self.init_tab(&tab_id, cx);
        Some(tab_id)
    }

    /// Open `path` in an Editor tab, titled after the file.
    pub fn open_in_editor(&mut self, path: &str, cx: &mut Context<Self>) -> Option<String> {
        let tab = self.add_tab_with("editor", serde_json::json!({ "status": "loading", "filePath": path }), cx)?;
        let title = path.rsplit('/').next().unwrap_or(path).to_owned();
        self.update_tab_title(&tab, &title, cx);
        Some(tab)
    }

    /// `openOrFocusTab`: show the first tab running `application_id`, else add one.
    pub fn open_or_focus_tab(&mut self, application_id: &str, cx: &mut Context<Self>) -> Option<String> {
        let existing = self
            .workspace
            .file
            .app_states
            .iter()
            .find(|(_, tab)| tab.application_id == application_id)
            .map(|(id, _)| id.clone());
        match existing {
            Some(tab_id) => {
                self.set_active_tab(&tab_id, cx);
                Some(tab_id)
            }
            None => self.add_tab(application_id, cx),
        }
    }

    /// The System Prompt Debugger, seeded with `buffer` (`addTab('system-prompt-debugger', …)`).
    pub fn open_prompt_debugger(&mut self, buffer: &str, cx: &mut Context<Self>) -> Option<String> {
        self.add_tab_with("system-prompt-debugger", serde_json::json!({ "bufferText": buffer }), cx)
    }

    /// The activity badge's "Debug System Prompt": the debugger on the tab's current
    /// viewport (empty when the tab has no terminal).
    pub fn open_prompt_debugger_for_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) -> Option<String> {
        let viewport = match self.tabs.get(tab_id) {
            Some(TabRef::Terminal(TerminalTab::Attached { model, .. })) => model.read(cx).viewport(),
            Some(_) | None => String::new(),
        };
        self.open_prompt_debugger(&viewport, cx)
    }

    /// The activity badge's "History": the tab's transitions and analyzer calls.
    pub fn open_activity_history(&mut self, source_tab: &str, cx: &mut Context<Self>) -> Option<String> {
        self.add_tab_with("analyzer-history", serde_json::json!({ "sourceTabId": source_tab }), cx)
    }

    /// The application registry this workspace's tabs follow.
    pub fn registry(&self) -> &Rc<AppRegistry> {
        &self.registry
    }

    /// The app settings, as last applied.
    pub fn settings(&self) -> &Rc<Settings> {
        &self.settings
    }

    /// The workspace lives on an ssh host.
    pub fn is_remote(&self) -> bool {
        self.remote.is_some()
    }

    /// The Settings tab's name: a blank one is ignored.
    pub fn set_display_name(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.trim();
        if !name.is_empty() {
            self.update_metadata("displayName", name, cx);
        }
    }

    /// The Settings tab's description (blank clears it).
    pub fn set_description(&mut self, description: &str, cx: &mut Context<Self>) {
        self.update_metadata("description", description.trim(), cx);
    }

    /// "Default Application for Children"; empty inherits (`updateSettings`).
    pub fn set_default_application(&mut self, application_id: &str, cx: &mut Context<Self>) {
        if self.workspace.file.settings.default_application_id == application_id {
            return;
        }
        self.workspace.file.settings.default_application_id = application_id.to_owned();
        self.changed(cx);
    }

    /// The workspace as JSON ("Raw Workspace JSON"): the body with its id and path.
    pub fn workspace_json(&self) -> Result<String, serde_json::Error> {
        let mut value = serde_json::to_value(&self.workspace.file)?;
        if let Value::Object(fields) = &mut value {
            fields.insert("id".into(), Value::String(self.workspace.id.clone()));
            fields.insert("path".into(), Value::String(self.workspace.path.clone()));
        }
        serde_json::to_string_pretty(&value)
    }

    /// Close a tab the user asked to close: kill its PTY, then drop it from the body.
    /// Applications that cannot close (or are not ported) are left alone.
    pub fn remove_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        let Some(tab) = self.workspace.file.app_states.get(tab_id) else {
            return;
        };
        let Some(app) = self.registry.get(&tab.application_id) else {
            return;
        };
        if !app.can_close {
            return;
        }
        if let Some((app, _)) = self.web_apps.get(tab_id) {
            app.update(cx, |app, cx| app.close(cx));
        }
        match self.tabs.get(tab_id) {
            Some(TabRef::Terminal(TerminalTab::Attached { model, .. })) => {
                model.update(cx, |model, cx| model.kill(cx));
            }
            Some(TabRef::Terminal(TerminalTab::Creating { handle, .. })) => {
                // Its creation finds the tab gone and kills the PTY (`on_pty_created`).
                tracing::debug!(%handle, "tab closed while its PTY was starting");
            }
            Some(
                TabRef::Terminal(TerminalTab::Failed(_))
                | TabRef::Files { .. }
                | TabRef::Editor { .. }
                | TabRef::Review { .. }
                | TabRef::Browser { .. }
                | TabRef::Tool(_),
            )
            | None => {}
        }
        self.dispose_tab(tab_id);

        let file = &mut self.workspace.file;
        let removed_index = file.app_states.get_index_of(tab_id);
        file.app_states.shift_remove(tab_id);
        if file.active_tab_id.as_deref() == Some(tab_id) {
            let next = removed_index
                .map(|index| index.min(file.app_states.len().saturating_sub(1)))
                .and_then(|index| file.app_states.get_index(index))
                .map(|(id, _)| id.clone());
            file.active_tab_id = next;
        }
        self.tabs_changed(cx);
        self.changed(cx);
    }

    pub fn set_active_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if self.workspace.file.active_tab_id.as_deref() == Some(tab_id)
            || !self.workspace.file.app_states.contains_key(tab_id)
        {
            return;
        }
        self.workspace.file.active_tab_id = Some(tab_id.to_owned());
        self.changed(cx);
    }

    pub fn update_tab_title(&mut self, tab_id: &str, title: &str, cx: &mut Context<Self>) {
        let Some(tab) = self.workspace.file.app_states.get_mut(tab_id) else {
            return;
        };
        tab.title = title.to_owned();
        self.changed(cx);
    }

    /// Replace a tab's state. Skipped for a tab that is already gone, so a late update
    /// cannot resurrect it.
    pub fn update_tab_state(&mut self, tab_id: &str, update: impl FnOnce(&mut Value), cx: &mut Context<Self>) {
        let Some(tab) = self.workspace.file.app_states.get_mut(tab_id) else {
            return;
        };
        update(&mut tab.state);
        self.changed(cx);
    }

    // ---- metadata ----------------------------------------------------------------------

    pub fn update_metadata(&mut self, key: &str, value: &str, cx: &mut Context<Self>) {
        if self.workspace.file.metadata.get(key).map(String::as_str) == Some(value) {
            return;
        }
        self.workspace.file.metadata.insert(key.to_owned(), value.to_owned());
        self.sync_idle_timeouts(cx);
        self.changed(cx);
        if key == "displayName" || key == "description" {
            cx.emit(WorkspaceEvent::LabelsChanged);
        }
    }

    pub fn delete_metadata(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.workspace.file.metadata.remove(key).is_some() {
            self.sync_idle_timeouts(cx);
            self.changed(cx);
        }
    }

    /// Moved in the tree (`moveWorkspace`): a new parent and/or sort order.
    pub fn set_placement(&mut self, parent_id: Option<String>, sort_order: i64, cx: &mut Context<Self>) {
        let order = sort_order.to_string();
        let file = &mut self.workspace.file;
        if file.parent_id == parent_id && file.metadata.get("sortOrder") == Some(&order) {
            return;
        }
        file.parent_id = parent_id;
        file.metadata.insert("sortOrder".to_owned(), order);
        self.changed(cx);
    }

    /// `updateStatus`: merged or abandoned.
    pub fn set_status(&mut self, status: treeterm_core::wire::workspace_file::WorkspaceStatus, cx: &mut Context<Self>) {
        if self.workspace.file.status != status {
            self.workspace.file.status = status;
            self.changed(cx);
        }
    }

    /// Git reports another branch than the body records (`updateGitInfo`).
    pub fn set_git_branch(&mut self, branch: &str, cx: &mut Context<Self>) {
        if self.workspace.file.git_branch.as_deref() == Some(branch) {
            return;
        }
        self.workspace.file.git_branch = Some(branch.to_owned());
        self.changed(cx);
    }

    pub fn toggle_favourite(&mut self, cx: &mut Context<Self>) {
        if self.workspace.file.metadata.get("isFavourite").map(String::as_str) == Some("true") {
            self.delete_metadata("isFavourite", cx);
        } else {
            self.update_metadata("isFavourite", "true", cx);
        }
    }

    /// The workspace's own favourite files and directories (workspace-relative).
    pub fn favourite_paths(&self) -> &[String] {
        &self.workspace.file.favourite_paths
    }

    /// `addFavouritePath`: one already covered by a favourite directory is not added.
    pub fn add_favourite_path(&mut self, raw_path: &str, cx: &mut Context<Self>) {
        let path = treeterm_core::files::normalize_favourite_path(raw_path);
        if path.is_empty() || crate::file_browser::is_favourite_path(&path, &self.workspace.file.favourite_paths) {
            return;
        }
        self.workspace.file.favourite_paths.push(path);
        self.changed(cx);
    }

    /// `removeFavouritePath`: only a path the workspace itself lists.
    pub fn remove_favourite_path(&mut self, raw_path: &str, cx: &mut Context<Self>) {
        let path = treeterm_core::files::normalize_favourite_path(raw_path);
        let before = self.workspace.file.favourite_paths.len();
        self.workspace.file.favourite_paths.retain(|favourite| *favourite != path);
        if self.workspace.file.favourite_paths.len() != before {
            self.changed(cx);
        }
    }

    // ---- sync --------------------------------------------------------------------------

    /// A body written elsewhere replaces ours (`applyWorkspaceFile`): tabs it dropped are
    /// detached, new tabs initialised, and a terminal whose tab now names another PTY
    /// re-attaches. Emits no change: the body is already on disk.
    pub fn apply_external(&mut self, mut file: WorkspaceFile, cx: &mut Context<Self>) {
        let removed: Vec<String> = self
            .workspace
            .file
            .app_states
            .keys()
            .filter(|id| !file.app_states.contains_key(*id))
            .cloned()
            .collect();
        for tab_id in &removed {
            self.dispose_tab(tab_id);
        }
        if file.active_tab_id.as_deref().is_none_or(str::is_empty) {
            file.active_tab_id = file.app_states.keys().next().cloned();
        }
        // A tab whose PTY changed under it (refreshed or recreated by another window).
        let moved: Vec<String> = file
            .app_states
            .iter()
            .filter_map(|(id, tab)| {
                let Some(TabRef::Terminal(TerminalTab::Attached { model, .. })) = self.tabs.get(id) else {
                    return None;
                };
                let incoming = tab.state.get("ptyId").and_then(Value::as_str)?;
                (incoming != model.read(cx).pty_id()).then(|| id.clone())
            })
            .collect();
        for tab_id in &moved {
            self.dispose_tab(tab_id);
        }
        // Attention edits the incoming body already carries are done; the rest go on top
        // of it and are written again.
        if let Err(error) = self.attention.reconcile_write(&file.metadata, "") {
            tracing::warn!(%error, "unreadable attention metadata in an incoming body");
        }
        self.workspace.file = file;
        let tab_ids: Vec<String> = self.workspace.file.app_states.keys().cloned().collect();
        for tab_id in tab_ids {
            self.init_tab(&tab_id, cx);
        }
        let rewrite = self.attention.has_pending() && self.reapply_attention();
        self.observe_attention(false, cx);
        self.sync_idle_timeouts(cx);
        if rewrite {
            self.changed(cx);
        }
        cx.notify();
    }

    /// New settings: the registry decides which tabs are native and how new ones start;
    /// live terminals take the emulator and font settings.
    pub fn apply_settings(&mut self, registry: Rc<AppRegistry>, settings: Rc<Settings>, cx: &mut Context<Self>) {
        self.registry = registry;
        self.settings = settings;
        for tab in self.tabs.values() {
            if let TabRef::Terminal(TerminalTab::Attached { model, analyzer, .. }) = tab {
                let terminal = self.settings.terminal.clone();
                model.update(cx, |model, cx| model.apply_settings(terminal, cx));
                if let Some((analyzer, _)) = analyzer {
                    let settings = self.settings.clone();
                    analyzer.update(cx, |a, cx| a.set_settings(&settings, cx));
                }
            }
        }
        self.sync_idle_timeouts(cx);
        // Tabs of applications that just appeared (a new AI harness instance) start now.
        let tab_ids: Vec<String> = self.workspace.file.app_states.keys().cloned().collect();
        for tab_id in tab_ids {
            self.init_tab(&tab_id, cx);
        }
        cx.notify();
    }

    /// The workspace is being removed: kill every tab's daemon resources (`ref.close()`).
    pub fn close_all_tabs(&mut self, cx: &mut Context<Self>) {
        let apps: Vec<Entity<crate::web_app::WebAppModel>> = self.web_apps.values().map(|(app, _)| app.clone()).collect();
        for app in apps {
            app.update(cx, |app, cx| app.close(cx));
        }
        for tab in self.tabs.values() {
            match tab {
                TabRef::Terminal(TerminalTab::Attached { model, .. }) => {
                    model.update(cx, |model, cx| model.kill(cx));
                }
                // A PTY still starting is killed by its creation finding the tab gone.
                TabRef::Terminal(TerminalTab::Creating { .. } | TerminalTab::Failed(_))
                | TabRef::Files { .. }
                | TabRef::Editor { .. }
                | TabRef::Review { .. }
                | TabRef::Browser { .. }
                | TabRef::Tool(_) => {}
            }
        }
    }

    /// The workspace leaves the session: detach every terminal. PTYs keep running.
    pub fn dispose(&mut self) {
        self.tabs.clear();
    }
}
