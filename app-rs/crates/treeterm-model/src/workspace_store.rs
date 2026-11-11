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
use treeterm_core::daemon::DaemonConnection;
use treeterm_core::pty::CreatePty;
use treeterm_core::settings::Settings;
use treeterm_core::wire::workspace_file::{AppState, WorkspaceFile};
use treeterm_term::fit::WidthLimit;
use treeterm_term::palette::{DEFAULT_BACKGROUND, SANDBOX_BACKGROUND};

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
}

impl EventEmitter<WorkspaceEvent> for WorkspaceStore {}

/// What a tab's program is doing. Port of `ActivityState` (`src/shared/types.ts`). The
/// viewport idle detector produces `Working`/`Idle`; the LLM analyzer the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivityState {
    #[default]
    Idle,
    Working,
    UserInputRequired,
    PermissionRequest,
    SafePermissionRequested,
    Completed,
    ApplicationError,
    Error,
}

impl ActivityState {
    pub fn label(self) -> &'static str {
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

    /// Badge colour, as `ActivityStateBadge.tsx`.
    pub fn color(self) -> u32 {
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
}

/// What a tab shows.
pub enum TabContent {
    Terminal(Entity<TerminalModel>),
    /// The tab's PTY is being created.
    StartingTerminal,
    Failed(String),
    /// An application the native app does not have yet.
    NotPorted(String),
}

/// Runtime state of one tab; never persisted.
enum TabRef {
    Terminal(TerminalTab),
}

enum TerminalTab {
    Creating {
        handle: String,
        _task: Task<()>,
    },
    Attached {
        model: Entity<TerminalModel>,
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
    /// Per-tab activity; runtime only (`activityState` store).
    activity: HashMap<String, ActivityState>,
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
        cx: &mut Context<Self>,
    ) -> Self {
        let mut store = Self {
            workspace,
            conn,
            registry,
            settings,
            tabs: HashMap::new(),
            activity: HashMap::new(),
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

    pub fn file(&self) -> &WorkspaceFile {
        &self.workspace.file
    }

    /// What the tab shows; `None` for a tab id the body does not have.
    pub fn tab_content(&self, tab_id: &str) -> Option<TabContent> {
        let tab = self.workspace.file.app_states.get(tab_id)?;
        Some(match self.tabs.get(tab_id) {
            Some(TabRef::Terminal(TerminalTab::Attached { model, .. })) => TabContent::Terminal(model.clone()),
            Some(TabRef::Terminal(TerminalTab::Creating { .. })) => TabContent::StartingTerminal,
            Some(TabRef::Terminal(TerminalTab::Failed(error))) => TabContent::Failed(error.clone()),
            None => TabContent::NotPorted(tab.application_id.clone()),
        })
    }

    /// `settings.terminal.maxCols`, the width the "col limit" switch caps at.
    pub fn max_cols(&self) -> u16 {
        self.settings.terminal.max_cols
    }

    pub fn activity(&self, tab_id: &str) -> ActivityState {
        self.activity.get(tab_id).copied().unwrap_or_default()
    }

    fn set_activity(&mut self, tab_id: &str, state: ActivityState, cx: &mut Context<Self>) {
        if self.activity.insert(tab_id.to_owned(), state) != Some(state) {
            cx.notify();
        }
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
            TabRef::Terminal(TerminalTab::Creating { .. } | TerminalTab::Failed(_)) => None,
        })
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(WorkspaceEvent::Changed);
        cx.notify();
    }

    // ---- tabs --------------------------------------------------------------------------

    /// Create the runtime side of a tab (`initTab` → `onWorkspaceLoad`).
    fn init_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if self.tabs.contains_key(tab_id) {
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
            idle_timeout: Some(settings.terminal_analyzer.idle_debounce),
        }
    }

    fn attach_terminal(&mut self, tab_id: &str, pty_id: String, cx: &mut Context<Self>) -> TerminalTab {
        let options = self.terminal_options(tab_id, pty_id);
        let conn = self.conn.clone();
        let model = cx.new(|cx| TerminalModel::new(conn, options, cx));
        let owned_tab = tab_id.to_owned();
        let events = cx.subscribe(&model, move |store, _, event: &TerminalEvent, cx| match event {
            TerminalEvent::CloseRequested { .. } => store.remove_tab(&owned_tab, cx),
            TerminalEvent::Activity(_) => {
                if !store.tab_flag(&owned_tab, "idleDetectorDisabled") {
                    store.set_activity(&owned_tab, ActivityState::Working, cx);
                }
            }
            TerminalEvent::Idle(_) => store.set_activity(&owned_tab, ActivityState::Idle, cx),
            TerminalEvent::Clipboard(_) | TerminalEvent::Bell => {}
        });
        TerminalTab::Attached {
            model,
            _events: events,
        }
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
    fn dispose_tab(&mut self, tab_id: &str) {
        self.tabs.remove(tab_id);
        self.activity.remove(tab_id);
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
        self.changed(cx);
        self.init_tab(&tab_id, cx);
        Some(tab_id)
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
        match self.tabs.get(tab_id) {
            Some(TabRef::Terminal(TerminalTab::Attached { model, .. })) => {
                model.update(cx, |model, cx| model.kill(cx));
            }
            Some(TabRef::Terminal(TerminalTab::Creating { handle, .. })) => {
                // Its creation finds the tab gone and kills the PTY (`on_pty_created`).
                tracing::debug!(%handle, "tab closed while its PTY was starting");
            }
            Some(TabRef::Terminal(TerminalTab::Failed(_))) | None => {}
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
        self.changed(cx);
    }

    pub fn delete_metadata(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.workspace.file.metadata.remove(key).is_some() {
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
        self.workspace.file = file;
        let tab_ids: Vec<String> = self.workspace.file.app_states.keys().cloned().collect();
        for tab_id in tab_ids {
            self.init_tab(&tab_id, cx);
        }
        cx.notify();
    }

    /// New settings: the registry decides which tabs are native and how new ones start;
    /// live terminals take the emulator and font settings.
    pub fn apply_settings(&mut self, registry: Rc<AppRegistry>, settings: Rc<Settings>, cx: &mut Context<Self>) {
        self.registry = registry;
        self.settings = settings;
        for tab in self.tabs.values() {
            if let TabRef::Terminal(TerminalTab::Attached { model, .. }) = tab {
                let terminal = self.settings.terminal.clone();
                model.update(cx, |model, cx| model.apply_settings(terminal, cx));
            }
        }
        // Tabs of applications that just appeared (a new AI harness instance) start now.
        let tab_ids: Vec<String> = self.workspace.file.app_states.keys().cloned().collect();
        for tab_id in tab_ids {
            self.init_tab(&tab_id, cx);
        }
        cx.notify();
    }

    /// The workspace is being removed: kill every tab's daemon resources (`ref.close()`).
    pub fn close_all_tabs(&mut self, cx: &mut Context<Self>) {
        for tab in self.tabs.values() {
            match tab {
                TabRef::Terminal(TerminalTab::Attached { model, .. }) => {
                    model.update(cx, |model, cx| model.kill(cx));
                }
                // A PTY still starting is killed by its creation finding the tab gone.
                TabRef::Terminal(TerminalTab::Creating { .. } | TerminalTab::Failed(_)) => {}
            }
        }
    }

    /// The workspace leaves the session: detach every terminal. PTYs keep running.
    pub fn dispose(&mut self) {
        self.tabs.clear();
    }
}
