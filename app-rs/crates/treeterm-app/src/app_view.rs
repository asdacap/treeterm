//! Window content: daemon start-up screen, then tree pane + workspace pane.

use std::collections::HashMap;

use gpui_kit::component::{h_flex, spinner::Spinner, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, Entity, EntityId, SharedString, Subscription, Window, div, px};
use treeterm_core::keybinding::{Action, PrefixAction};
use treeterm_model::app_store::{AppState, AppStore};
use treeterm_model::keybinding_store::KeybindingStore;
use treeterm_model::session_store::{SessionStore, WorkspaceEntry};
use treeterm_model::workspace_store::TabContent;

use crate::terminal_view::TerminalView;
use crate::theme::colors;
use crate::tree_pane::{TreePane, WorkspaceSelected};

pub struct AppView {
    keybindings: Entity<KeybindingStore>,
    settings: Entity<treeterm_model::settings_store::SettingsStore>,
    /// Holds keyboard focus when nothing inside does, so window-level shortcuts dispatch.
    focus: gpui_kit::FocusHandle,
    /// Owned, not just observed: an observation does not keep the store alive, and dropping
    /// it cancels the daemon start-up and every watch it owns.
    _store: Entity<AppStore>,
    body: Body,
}

enum Body {
    StartingDaemon,
    Reconnecting { attempt: u32, last_error: SharedString },
    Failed(SharedString),
    Connected(Box<Connected>),
}

/// Per-window state for a connected session.
struct Connected {
    session: Entity<SessionStore>,
    tree: Entity<TreePane>,
    active: ActiveView,
    /// Views of terminal models, by model, so focus and IME state survive re-renders.
    terminal_views: HashMap<EntityId, Entity<TerminalView>>,
    /// Views of Files tabs, by model.
    file_views: HashMap<EntityId, Entity<crate::file_browser_view::FileBrowserView>>,
    /// Views of Editor tabs, by model.
    editor_views: HashMap<EntityId, Entity<crate::text_editor_view::TextEditorView>>,
    /// Views of Review tabs, by model.
    review_views: HashMap<EntityId, Entity<crate::review_view::ReviewView>>,
    /// Focus the shown terminal on the next render (after a workspace or tab switch).
    focus_pending: bool,
    _selection: Subscription,
}

enum ActiveView {
    Nothing,
    Workspace(String),
}

/// What the app store's state means for this window, read before mutating the view.
enum Next {
    StartingDaemon,
    Reconnecting { attempt: u32, last_error: SharedString },
    Failed(SharedString),
    Connected(Entity<SessionStore>),
}

impl AppView {
    pub fn new(
        store: Entity<AppStore>,
        keybindings: Entity<KeybindingStore>,
        settings: Entity<treeterm_model::settings_store::SettingsStore>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&store, |view, store, cx| view.sync(&store, cx)).detach();
        cx.observe(&keybindings, |_, _, cx| cx.notify()).detach();
        let mut view = Self {
            keybindings,
            settings,
            focus: cx.focus_handle(),
            _store: store.clone(),
            body: Body::StartingDaemon,
        };
        view.sync(&store, cx);
        view
    }

    fn sync(&mut self, store: &Entity<AppStore>, cx: &mut Context<Self>) {
        let next = match store.read(cx).state() {
            AppState::StartingDaemon => Next::StartingDaemon,
            AppState::Reconnecting { attempt, last_error } => Next::Reconnecting {
                attempt: *attempt,
                last_error: last_error.clone().into(),
            },
            AppState::Failed(reason) => Next::Failed(reason.clone().into()),
            AppState::Connected(session) => Next::Connected(session.clone()),
        };
        self.body = match next {
            Next::StartingDaemon => Body::StartingDaemon,
            Next::Reconnecting { attempt, last_error } => Body::Reconnecting { attempt, last_error },
            Next::Failed(reason) => Body::Failed(reason),
            Next::Connected(session) => {
                if let Body::Connected(current) = &self.body
                    && current.session == session
                {
                    return;
                }
                Body::Connected(Box::new(self.connect(session, cx)))
            }
        };
        cx.notify();
    }

    fn connect(&mut self, session: Entity<SessionStore>, cx: &mut Context<Self>) -> Connected {
        cx.observe(&session, |_, _, cx| cx.notify()).detach();
        let keybindings = self.keybindings.clone();
        let tree = cx.new(|cx| TreePane::new(session.clone(), keybindings, cx));
        let selection = cx.subscribe(&tree, |view, _, WorkspaceSelected(id), cx| {
            if let Body::Connected(connected) = &mut view.body {
                connected.active = ActiveView::Workspace(id.clone());
                connected.focus_pending = true;
                cx.notify();
            }
        });
        Connected {
            session,
            tree,
            active: ActiveView::Nothing,
            terminal_views: HashMap::new(),
            file_views: HashMap::new(),
            editor_views: HashMap::new(),
            review_views: HashMap::new(),
            focus_pending: false,
            _selection: selection,
        }
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (bg, fg, secondary, error) = (c.bg_primary, c.text_primary, c.text_secondary, c.error);
        let content: AnyElement = match &mut self.body {
            Body::StartingDaemon => centered(
                h_flex()
                    .gap_2()
                    .text_color(secondary)
                    .child(Spinner::new())
                    .child("Starting TreeTerm daemon…"),
            ),
            Body::Reconnecting { attempt, last_error } => centered(
                v_flex()
                    .gap_2()
                    .max_w(px(600.))
                    .items_center()
                    .child(
                        h_flex()
                            .gap_2()
                            .text_color(secondary)
                            .child(Spinner::new())
                            .child(format!("Reconnecting to the TreeTerm daemon (attempt {attempt})…")),
                    )
                    .child(div().text_sm().text_color(secondary).child(last_error.clone())),
            ),
            Body::Failed(reason) => centered(
                v_flex()
                    .gap_2()
                    .max_w(px(600.))
                    .child(div().text_color(error).child("Cannot reach the TreeTerm daemon"))
                    .child(div().text_sm().text_color(secondary).child(reason.clone())),
            ),
            Body::Connected(connected) => {
                let tree = connected.tree.clone();
                let pane = workspace_pane(connected, window, cx);
                h_flex().size_full().child(tree).child(pane).into_any_element()
            }
        };
        if window.focused(cx).is_none() {
            window.focus(&self.focus, cx);
        }
        div()
            .size_full()
            .track_focus(&self.focus)
            .bg(bg)
            .text_color(fg)
            .on_action(|_: &crate::menus::Minimize, window, _| window.minimize_window())
            .on_action(cx.listener(|view, _: &crate::menus::OpenSettings, window, cx| {
                crate::settings_view::open(view.settings.clone(), window, cx);
            }))
            .child(content)
            .children(crate::keybinding_overlay::render(&self.keybindings, window, cx))
    }
}

impl AppView {
    /// The active workspace's store, if it is loaded.
    fn active_store(&self, cx: &App) -> Option<Entity<treeterm_model::workspace_store::WorkspaceStore>> {
        let Body::Connected(connected) = &self.body else {
            return None;
        };
        let ActiveView::Workspace(id) = &connected.active else {
            return None;
        };
        connected.session.read(cx).workspace_store(id).cloned()
    }

    fn focus_active_tab(&mut self, cx: &mut Context<Self>) {
        if let Body::Connected(connected) = &mut self.body {
            connected.focus_pending = true;
        }
        cx.notify();
    }

    /// Perform a shortcut decided by the keybinding machine (`WorkspacePane`'s handlers).
    pub fn perform(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            Action::SwitchToTab { index } => {
                let Some(store) = self.active_store(cx) else { return };
                let tab = store.read(cx).file().app_states.get_index(index).map(|(id, _)| id.clone());
                if let Some(tab) = tab {
                    store.update(cx, |w, cx| w.set_active_tab(&tab, cx));
                    self.focus_active_tab(cx);
                }
            }
            Action::SetActiveWorkspace { workspace_id } => {
                if let Body::Connected(connected) = &self.body {
                    connected.tree.update(cx, |tree, cx| tree.select(workspace_id, cx));
                }
            }
            Action::Prefix(PrefixAction::NewTab) => {
                let Some(store) = self.active_store(cx) else { return };
                let Body::Connected(connected) = &self.body else { return };
                let app = connected.session.read(cx).registry().default_app().map(|app| app.id.clone());
                if let Some(app) = app {
                    store.update(cx, |w, cx| w.add_tab(&app, cx));
                    self.focus_active_tab(cx);
                }
            }
            Action::Prefix(PrefixAction::CloseTab) => {
                let Some(store) = self.active_store(cx) else { return };
                // Electron never closes the last tab from the keyboard.
                let file = store.read(cx).file();
                if file.app_states.len() > 1
                    && let Some(active) = file.active_tab_id.clone()
                {
                    store.update(cx, |w, cx| w.remove_tab(&active, cx));
                    self.focus_active_tab(cx);
                }
            }
            Action::Prefix(direction @ (PrefixAction::NextTab | PrefixAction::PrevTab)) => {
                let Some(store) = self.active_store(cx) else { return };
                let file = store.read(cx).file();
                let count = file.app_states.len();
                if count == 0 {
                    return;
                }
                let current = file
                    .active_tab_id
                    .as_ref()
                    .and_then(|id| file.app_states.get_index_of(id));
                let next = match (direction, current) {
                    (PrefixAction::NextTab, Some(i)) => (i + 1) % count,
                    (PrefixAction::NextTab, None) => 0,
                    (_, Some(0) | None) => count - 1,
                    (_, Some(i)) => i - 1,
                };
                let tab = file.app_states.get_index(next).map(|(id, _)| id.clone());
                if let Some(tab) = tab {
                    store.update(cx, |w, cx| w.set_active_tab(&tab, cx));
                    self.focus_active_tab(cx);
                }
            }
            Action::Prefix(PrefixAction::WorkspaceFocus) => {
                let Body::Connected(connected) = &self.body else { return };
                let session = connected.session.read(cx);
                let ids: Vec<String> = session
                    .tree_rows(&Default::default(), cx)
                    .into_iter()
                    .map(|row| row.id)
                    .collect();
                let current = match &connected.active {
                    ActiveView::Workspace(id) => ids.iter().position(|w| w == id).unwrap_or(0),
                    ActiveView::Nothing => 0,
                };
                if let Err(error) = self
                    .keybindings
                    .update(cx, |kb, cx| kb.enter_workspace_focus(ids, current, cx))
                {
                    tracing::error!(%error, "workspace focus");
                }
            }
            Action::Prefix(PrefixAction::OpenSettings) => {
                crate::settings_view::open(self.settings.clone(), window, cx);
            }
        }
    }
}

fn centered(child: impl IntoElement) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(child)
        .into_any_element()
}

/// The active workspace: header, tab strip and the active tab. Terminal tabs are live;
/// other application kinds arrive in later milestones.
fn workspace_pane(connected: &mut Connected, window: &mut Window, cx: &mut Context<AppView>) -> AnyElement {
    let c = colors(cx);
    let (secondary, error, border, active_bg, hover, primary) =
        (c.text_secondary, c.error, c.border, c.bg_primary, c.bg_hover, c.text_primary);
    let tab_bar_bg = c.bg_secondary;
    prune_terminal_views(connected, cx);
    let ActiveView::Workspace(id) = &connected.active else {
        return centered(div().text_color(secondary).child("Select a workspace"));
    };
    let id = id.clone();
    let workspace = match connected.session.read(cx).entry(&id) {
        // The selection can outlive its workspace when another window removes it.
        None => return centered(div().text_color(secondary).child("Workspace was removed")),
        Some(WorkspaceEntry::Loading { message, output, .. }) => {
            // Git's output while a worktree is being created.
            let tail: Vec<String> = output.iter().rev().take(30).rev().cloned().collect();
            return v_flex()
                .flex_1()
                .h_full()
                .p_4()
                .pt(px(38.))
                .gap_2()
                .child(h_flex().gap_2().text_color(secondary).child(Spinner::new()).child(message.clone()))
                .child(
                    v_flex()
                        .font_family("Menlo")
                        .text_size(px(12.))
                        .text_color(secondary)
                        .children(tail.into_iter().map(|line| div().child(line))),
                )
                .into_any_element();
        }
        Some(WorkspaceEntry::Error { error: reason, .. }) => {
            return centered(div().text_color(error).child(reason.clone()));
        }
        Some(WorkspaceEntry::Loaded(store) | WorkspaceEntry::OperationError { store, .. }) => store.clone(),
    };
    let operation_error = match connected.session.read(cx).entry(&id) {
        Some(WorkspaceEntry::OperationError { error, .. }) => Some(error.clone()),
        Some(_) | None => None,
    };
    let (path, file) = {
        let ws = workspace.read(cx).workspace();
        (ws.path.clone(), ws.file.clone())
    };
    let active = file.active_tab_id.clone().filter(|tab| file.app_states.contains_key(tab));

    let mut tabs = h_flex().h(px(32.)).bg(tab_bar_bg).border_b_1().border_color(border);
    let registry = connected.session.read(cx).registry().clone();
    let order = treeterm_core::layout::order_tabs(
        file.app_states.keys(),
        file.metadata.get("layoutModel").map(String::as_str),
    );
    for (tab_id, tab) in order.iter().filter_map(|id| file.app_states.get_key_value(id)) {
        let is_active = active.as_deref() == Some(tab_id.as_str());
        // Tabs of applications the native app lacks (or that cannot close) keep no close button.
        let closable = registry.get(&tab.application_id).is_some_and(|app| app.can_close);
        let select = (workspace.clone(), tab_id.clone());
        let close = (workspace.clone(), tab_id.clone());
        tabs = tabs.child(
            h_flex()
                .id(SharedString::from(format!("tab-{tab_id}")))
                .h_full()
                .pl_3()
                .pr_1()
                .gap_1()
                .items_center()
                .text_sm()
                .border_r_1()
                .border_color(border)
                .when(is_active, |d| d.bg(active_bg).text_color(primary))
                .when(!is_active, |d| d.text_color(secondary).hover(move |s| s.bg(hover)))
                .cursor_pointer()
                .child(tab.title.clone())
                .when(!closable, |d| d.pr_2())
                .when(closable, |d| d.child(
                    div()
                        .id(SharedString::from(format!("tab-close-{tab_id}")))
                        .px_1()
                        .rounded_sm()
                        .text_color(secondary)
                        .hover(move |s| s.bg(hover).text_color(primary))
                        .child("×")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.stop_propagation();
                            let (workspace, tab) = &close;
                            workspace.update(cx, |w, cx| w.remove_tab(tab, cx));
                        })),
                ))
                .on_click(cx.listener(move |view, _, _, cx| {
                    let (workspace, tab) = &select;
                    workspace.update(cx, |w, cx| w.set_active_tab(tab, cx));
                    if let Body::Connected(connected) = &mut view.body {
                        connected.focus_pending = true;
                    }
                    cx.notify();
                })),
        );
    }
    // "+" opens the new-tab menu: every application offered for new tabs.
    let menu_apps: Vec<(String, String, String)> = registry
        .new_tab_menu()
        .map(|app| (app.id.clone(), app.name.clone(), app.icon.clone()))
        .collect();
    if !menu_apps.is_empty() {
        use gpui_kit::component::button::{Button, ButtonVariants as _};
        use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
        let target = workspace.clone();
        let view_entity = cx.entity();
        tabs = tabs.child(
            Button::new("new-tab")
                .ghost()
                .label("+")
                .dropdown_menu(move |mut menu, _window, _cx| {
                    for (id, name, icon) in &menu_apps {
                        let (target, view, id) = (target.clone(), view_entity.clone(), id.clone());
                        menu = menu.item(PopupMenuItem::new(format!("{icon}  {name}")).on_click(
                            move |_, _, cx| {
                                target.update(cx, |w, cx| w.add_tab(&id, cx));
                                view.update(cx, |view, cx| view.focus_active_tab(cx));
                            },
                        ));
                    }
                    menu
                }),
        );
    }

    let header = h_flex()
        .h(px(38.))
        .pl(px(16.))
        .gap_3()
        .items_center()
        .border_b_1()
        .border_color(border)
        .child(div().child(file.display_name().to_owned()))
        .child(div().text_sm().text_color(secondary).child(path))
        .when_some(file.git_branch.clone(), |d, branch| {
            d.child(div().text_sm().text_color(secondary).child(format!("⎇ {branch}")))
        })
        .when_some(operation_error, |d, error| {
            d.child(div().text_sm().text_color(colors(cx).error).child(format!("⚠ {error}")))
        });

    let content: AnyElement = match &active {
        None => centered(div().text_color(secondary).child("No tabs")),
        Some(tab_id) => match workspace.read(cx).tab_content(tab_id) {
            Some(TabContent::Terminal(model)) => {
                let view = connected
                    .terminal_views
                    .entry(model.entity_id())
                    .or_insert_with(|| cx.new(|cx| TerminalView::new(model.clone(), window, cx)))
                    .clone();
                if std::mem::take(&mut connected.focus_pending) {
                    let focus = view.read(cx).focus_handle().clone();
                    window.focus(&focus, cx);
                }
                v_flex()
                    .size_full()
                    .child(div().flex_1().min_h_0().child(view))
                    .child(terminal_status_bar(&workspace, tab_id, cx))
                    .into_any_element()
            }
            Some(TabContent::Files(model)) => {
                let view = connected
                    .file_views
                    .entry(model.entity_id())
                    .or_insert_with(|| {
                        cx.new(|cx| crate::file_browser_view::FileBrowserView::new(model.clone(), workspace.clone(), cx))
                    })
                    .clone();
                div().size_full().child(view).into_any_element()
            }
            Some(TabContent::Editor(model)) => {
                let view = connected
                    .editor_views
                    .entry(model.entity_id())
                    .or_insert_with(|| cx.new(|cx| crate::text_editor_view::TextEditorView::new(model.clone(), cx)))
                    .clone();
                div().size_full().child(view).into_any_element()
            }
            Some(TabContent::Review(model)) => {
                let view = connected
                    .review_views
                    .entry(model.entity_id())
                    .or_insert_with(|| cx.new(|cx| crate::review_view::ReviewView::new(model.clone(), cx)))
                    .clone();
                div().size_full().child(view).into_any_element()
            }
            Some(TabContent::StartingTerminal) => {
                centered(div().text_color(secondary).child("Starting terminal…"))
            }
            Some(TabContent::Failed(message)) => centered(div().text_color(error).child(message)),
            Some(TabContent::NotPorted(app)) => centered(
                div()
                    .text_color(secondary)
                    .child(format!("“{app}” tabs are not available in the native app yet")),
            ),
            None => centered(div().text_color(secondary).child("No tabs")),
        },
    };

    v_flex()
        .flex_1()
        .h_full()
        .min_w_0()
        .child(header)
        .child(tabs)
        .child(div().flex_1().min_h_0().child(content))
        .into_any_element()
}

/// The bar under a terminal: activity badge, idle-detector and width-limit switches.
/// Port of `TerminalStatusBar.tsx`; both switches persist in the tab's state.
fn terminal_status_bar(
    workspace: &Entity<treeterm_model::workspace_store::WorkspaceStore>,
    tab_id: &str,
    cx: &mut Context<AppView>,
) -> AnyElement {
    use gpui_kit::component::Sizable as _;
    use gpui_kit::component::switch::Switch;
    let store = workspace.read(cx);
    let activity = store.activity(tab_id);
    let idle_enabled = !store.tab_flag(tab_id, "idleDetectorDisabled");
    let width_enabled = !store.tab_flag(tab_id, "widthLimitDisabled");
    let max_cols = store.max_cols();
    let (idle_ws, idle_tab) = (workspace.clone(), tab_id.to_owned());
    let (width_ws, width_tab) = (workspace.clone(), tab_id.to_owned());
    h_flex()
        .h(px(28.))
        .flex_shrink_0()
        .px_2()
        .gap_3()
        .items_center()
        .bg(gpui_kit::rgb(0x1e1e1e))
        .border_t_1()
        .border_color(gpui_kit::rgb(0x333333))
        .text_size(px(11.))
        .child(
            div()
                .px_2()
                .py(px(2.))
                .rounded(px(12.))
                .bg(gpui_kit::rgb(activity.color()))
                .opacity(0.8)
                .text_color(gpui_kit::white())
                .child(activity.label()),
        )
        .child(
            Switch::new(SharedString::from(format!("idle-switch-{tab_id}")))
                .checked(idle_enabled)
                .label("Idle detector")
                .xsmall()
                .on_click(move |checked, _, cx| {
                    idle_ws.update(cx, |w, cx| w.set_idle_detector_enabled(&idle_tab, *checked, cx));
                }),
        )
        .child(
            Switch::new(SharedString::from(format!("width-switch-{tab_id}")))
                .checked(width_enabled)
                .label(format!("{max_cols} col limit"))
                .xsmall()
                .on_click(move |checked, _, cx| {
                    width_ws.update(cx, |w, cx| w.set_width_limit_enabled(&width_tab, *checked, cx));
                }),
        )
        .into_any_element()
}

/// Drop views whose terminal is gone (tab closed, workspace removed): a view holds its
/// model, and a model kept alive here would stay attached to its PTY.
fn prune_terminal_views(connected: &mut Connected, cx: &App) {
    let session = connected.session.read(cx);
    let live: std::collections::HashSet<EntityId> = session
        .workspace_ids()
        .filter_map(|id| session.workspace_store(id))
        .flat_map(|store| store.read(cx).terminals().map(|t| t.entity_id()).collect::<Vec<_>>())
        .collect();
    connected.terminal_views.retain(|model_id, _| live.contains(model_id));
}

#[cfg(feature = "automation")]
impl AppView {
    /// A JSON snapshot of what this window shows, for automation.
    pub fn automation_state(&self, window: &Window, cx: &gpui_kit::App) -> serde_json::Value {
        use serde_json::json;
        match &self.body {
            Body::StartingDaemon => json!({ "body": "starting" }),
            Body::Reconnecting { attempt, last_error } => {
                json!({ "body": "reconnecting", "attempt": attempt, "error": last_error.to_string() })
            }
            Body::Failed(reason) => json!({ "body": "failed", "reason": reason.to_string() }),
            Body::Connected(connected) => {
                let session = connected.session.read(cx);
                let workspaces: Vec<_> = session
                    .workspace_ids()
                    .map(|id| {
                        let entry = match session.entry(id) {
                            Some(WorkspaceEntry::Loading { message, .. }) => json!({ "loading": message }),
                            Some(WorkspaceEntry::Error { error, .. }) => json!({ "error": error }),
                            Some(WorkspaceEntry::Loaded(store) | WorkspaceEntry::OperationError { store, .. }) => {
                                let ws = store.read(cx).workspace();
                                json!({
                                    "name": ws.file.display_name(),
                                    "path": ws.path,
                                    "activeTabId": ws.file.active_tab_id,
                                    "tabs": ws.file.app_states.iter().map(|(id, t)| json!({
                                        "id": id, "app": t.application_id, "title": t.title, "state": t.state
                                    })).collect::<Vec<_>>(),
                                })
                            }
                            None => serde_json::Value::Null,
                        };
                        json!({ "id": id, "entry": entry })
                    })
                    .collect();
                let active = match &connected.active {
                    ActiveView::Nothing => serde_json::Value::Null,
                    ActiveView::Workspace(id) => json!(id),
                };
                let terminals: Vec<_> = connected
                    .terminal_views
                    .values()
                    .map(|view| {
                        let view_focused = view.read(cx).focus_handle().is_focused(window);
                        let model = view.read(cx).model().read(cx);
                        let size = model.grid_size();
                        json!({
                            "pty": model.pty_id(),
                            "cols": size.cols,
                            "rows": size.rows,
                            "overlay": model.overlay().message(),
                            "focused": model.is_focused(),
                            "viewFocused": view_focused,
                            "screen": model.screen_text(),
                        })
                    })
                    .collect();
                json!({
                    "body": "connected",
                    "version": session.version(),
                    "workspaces": workspaces,
                    "active": active,
                    "terminals": terminals,
                })
            }
        }
    }

    pub fn automation_select_workspace(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Body::Connected(connected) = &self.body {
            let id = id.to_owned();
            connected.tree.update(cx, |tree, cx| tree.select(id, cx));
        }
    }

    pub fn automation_select_tab(&mut self, workspace: &str, tab: &str, cx: &mut Context<Self>) {
        if let Body::Connected(connected) = &mut self.body {
            if let Some(store) = connected.session.read(cx).workspace_store(workspace).cloned() {
                store.update(cx, |w, cx| w.set_active_tab(tab, cx));
            }
            connected.focus_pending = true;
            cx.notify();
        }
    }

    pub fn automation_new_tab(&mut self, workspace: &str, app: &str, cx: &mut Context<Self>) -> Option<String> {
        let Body::Connected(connected) = &mut self.body else {
            return None;
        };
        let store = connected.session.read(cx).workspace_store(workspace).cloned()?;
        let app = if app.is_empty() {
            connected.session.read(cx).registry().default_app()?.id.clone()
        } else {
            app.to_owned()
        };
        connected.focus_pending = true;
        store.update(cx, |w, cx| w.add_tab(&app, cx))
    }

    pub fn automation_session(&self) -> Option<Entity<SessionStore>> {
        match &self.body {
            Body::Connected(connected) => Some(connected.session.clone()),
            Body::StartingDaemon | Body::Reconnecting { .. } | Body::Failed(_) => None,
        }
    }

    pub fn automation_open_workspace(&mut self, path: String, cx: &mut Context<Self>) -> Option<String> {
        let Body::Connected(connected) = &self.body else {
            return None;
        };
        let id = connected.session.update(cx, |s, cx| s.open_workspace(path, cx));
        connected.tree.update(cx, |tree, cx| tree.select(id.clone(), cx));
        Some(id)
    }

    /// Open `path` in the active workspace's active Files tab.
    pub fn automation_open_file(&mut self, path: &str, cx: &mut Context<Self>) -> bool {
        let Some(store) = self.active_store(cx) else {
            return false;
        };
        let Some(active) = store.read(cx).file().active_tab_id.clone() else {
            return false;
        };
        let Some(TabContent::Files(model)) = store.read(cx).tab_content(&active) else {
            return false;
        };
        model.update(cx, |m, cx| m.open(path, false, cx));
        true
    }

    pub fn automation_close_tab(&mut self, workspace: &str, tab: &str, cx: &mut Context<Self>) {
        if let Body::Connected(connected) = &self.body
            && let Some(store) = connected.session.read(cx).workspace_store(workspace).cloned()
        {
            store.update(cx, |w, cx| w.remove_tab(tab, cx));
        }
    }
}
