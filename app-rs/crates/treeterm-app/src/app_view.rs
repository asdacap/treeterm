//! Window content: daemon start-up screen, then tree pane + workspace pane.

use std::collections::HashMap;

use gpui_kit::component::{h_flex, spinner::Spinner, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, Entity, EntityId, SharedString, Subscription, WeakEntity, Window, div, px};
use treeterm_core::keybinding::{Action, PrefixAction};
use indexmap::IndexMap;
use treeterm_core::connection::ConnectionId;
use treeterm_model::app_store::{AppStore, ConnState};
use treeterm_model::keybinding_store::KeybindingStore;
use treeterm_model::session_names::Side;
use treeterm_model::session_store::{SessionEvent, SessionStore, WorkspaceEntry};
use treeterm_model::workspace_store::{ActivityStyle as _, PageHost, TabContent, WorkspaceStore};

use crate::terminal_view::TerminalView;
use crate::theme::colors;
use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use crate::tree_pane::{TreePane, WorkspaceSelected};

pub struct AppView {
    keybindings: Entity<KeybindingStore>,
    settings: Entity<treeterm_model::settings_store::SettingsStore>,
    /// Custom session names (renamed in the sidebar).
    names: Entity<treeterm_model::session_names::SessionNames>,
    /// Holds keyboard focus when nothing inside does, so window-level shortcuts dispatch.
    focus: gpui_kit::FocusHandle,
    /// Owned, not just observed: an observation does not keep the store alive, and dropping
    /// it cancels every connection and watch it owns.
    store: Entity<AppStore>,
    /// Each connected session's tree and views, in sidebar order.
    sessions: IndexMap<ConnectionId, Connected>,
    /// The session whose workspace the right side shows.
    active: Option<ConnectionId>,
    /// The session pane, shown instead of a workspace after clicking a session's name.
    info: Option<Entity<crate::session_info_view::SessionInfoView>>,
    /// A session pane to open on the next render (it needs the window).
    info_pending: Option<ConnectionId>,
    /// `treeterm <dir>`: open (or select) this workspace once the local session is live.
    open_on_start: Option<String>,
    /// The sidebar is a narrow rail of workspace initials (`CollapsedSessionPanel`).
    sidebar_collapsed: bool,
    /// Dragged on the divider, 150–400px (`App.tsx` `treeWidth`).
    sidebar_width: f32,
    /// Where a session being dragged in the sidebar would land.
    session_drop_hint: Option<(ConnectionId, Side)>,
    /// The Settings dialog while it is open: Settings again does not stack a second one
    /// (Electron's `isSettingsOpen`).
    settings_dialog: WeakEntity<crate::settings_view::SettingsView>,
    /// Window > Active Processes (Electron's `isActiveProcessesOpen`).
    processes_dialog: ProcessesDialog,
    /// The unmerged-workspaces confirmation while it is up: Quit or Close again does not
    /// stack a second one.
    close_confirm: std::rc::Weak<crate::active_processes_view::Unmerged>,
    menu_bar: MenuBar,
}

/// Window > Active Processes.
enum ProcessesDialog {
    Closed,
    /// Asked for while no session is current: it opens once one is, as Electron renders it
    /// once there is an active session store.
    Pending,
    /// Closing the dialog drops the view.
    Open(WeakEntity<crate::active_processes_view::ActiveProcessesView>),
}

/// Where the application menu shows.
enum MenuBar {
    /// macOS: the system menu bar.
    System,
    /// Elsewhere GPUI only stores the menus, so the window draws them, as Electron's Linux
    /// window shows its menu bar.
    InWindow(Entity<gpui_kit::component::menu::AppMenuBar>),
}

/// Dragging the sidebar divider.
#[derive(Clone)]
struct SidebarResize;

impl Render for SidebarResize {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::Empty
    }
}

/// A session being dragged in the sidebar (and its drag preview).
#[derive(Clone)]
struct DraggedSession {
    id: ConnectionId,
    name: SharedString,
}

impl Render for DraggedSession {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        div().px_2().py_1().rounded_sm().bg(c.bg_active).text_sm().child(self.name.clone())
    }
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
    /// Browser views by (workspace, tab): they own native webviews, hidden when not shown.
    browser_views: HashMap<(String, String), Entity<crate::browser_view::BrowserView>>,
    /// Each shown workspace's Run menu actions, by workspace id.
    run_actions: HashMap<String, Entity<treeterm_model::run_actions::RunActions>>,
    /// Each shown workspace's download badge, by workspace id.
    traffic_badges: HashMap<String, Entity<crate::traffic_badge::TrafficBadge>>,
    /// Web App and Browser tabs not on their Browser sub-tab, by (workspace, tab).
    page_subtabs: HashMap<(String, String), PageSubTab>,
    /// Each shown workspace's tab strip scroll, by workspace id.
    tab_strips: HashMap<String, crate::tab_strip::TabStripState>,
    /// Views of tool tabs (GitHub, Comments, Chat, …).
    tool_views: crate::tool_views::ToolViews,
    /// The header's in-place name or description editor.
    header_edit: Option<HeaderEdit>,
    /// The branch badge reads "Copied!" for 1.5s after a click (`BranchBadge`).
    branch_copied: Option<(String, gpui_kit::Task<()>)>,
    /// Focus the shown terminal on the next render (after a workspace or tab switch).
    focus_pending: bool,
    /// The terminal model the last render showed: a terminal coming into view fits again and
    /// takes focus.
    shown_terminal: Option<EntityId>,
    reselect: Reselect,
    _selection: Subscription,
    _session_events: Subscription,
}

/// What a session view selects once its session is live.
enum Reselect {
    Nothing,
    /// The view replaces the last connection generation's, which showed this workspace: show
    /// it again, or the first one if it is gone (`handleReconnected`'s `workspaceRefs[0]`).
    Workspace(String),
}

/// What the workspace header is editing in place (`workspace-edit-input` / `-textarea`).
enum HeaderEdit {
    Name { input: Entity<gpui_kit::component::input::InputState>, _save: Subscription },
    Description { input: Entity<gpui_kit::component::input::TextareaState>, _save: Subscription },
}

#[derive(Clone, Copy)]
enum HeaderField {
    Name,
    Description,
}

enum ActiveView {
    Nothing,
    Workspace(String),
}

impl AppView {
    pub fn new(
        store: Entity<AppStore>,
        keybindings: Entity<KeybindingStore>,
        settings: Entity<treeterm_model::settings_store::SettingsStore>,
        names: Entity<treeterm_model::session_names::SessionNames>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&store, |view, store, cx| view.sync(&store, cx)).detach();
        cx.observe(&names, |view, _, cx| {
            let store = view.store.clone();
            view.sync(&store, cx);
        })
        .detach();
        // The attention ding (`playDing`): the system bell, when sound is on.
        cx.subscribe_in(&store, window, |view, _, event: &treeterm_model::app_store::AppEvent, window, cx| match event {
            treeterm_model::app_store::AppEvent::Attention { .. } => {
                if view.settings.read(cx).settings().notifications.sound_enabled {
                    window.play_system_bell();
                }
            }
        })
        .detach();
        cx.observe(&settings, |_, _, cx| cx.notify()).detach();
        cx.observe(&keybindings, |_, _, cx| cx.notify()).detach();
        let mut view = Self {
            keybindings,
            settings,
            names,
            focus: cx.focus_handle(),
            store: store.clone(),
            sessions: IndexMap::new(),
            active: None,
            info: None,
            info_pending: None,
            open_on_start: None,
            sidebar_collapsed: false,
            sidebar_width: 250.,
            session_drop_hint: None,
            settings_dialog: WeakEntity::new_invalid(),
            processes_dialog: ProcessesDialog::Closed,
            close_confirm: std::rc::Weak::new(),
            menu_bar: if cfg!(target_os = "macos") {
                MenuBar::System
            } else {
                MenuBar::InWindow(gpui_kit::component::menu::AppMenuBar::new(cx))
            },
        };
        view.sync(&store, cx);
        view
    }

    /// Follow the store: a view per session shown (rebuilt for each new connection
    /// generation; a lost connection's last session stays, dimmed), none for the others.
    fn sync(&mut self, store: &Entity<AppStore>, cx: &mut Context<Self>) {
        let wanted: Vec<(ConnectionId, Entity<SessionStore>, treeterm_core::connection::ConnectionTarget)> = store
            .read(cx)
            .sessions()
            .filter_map(|slot| Some((slot.id.clone(), slot.shown_session()?.clone(), slot.target.clone())))
            .collect();
        // A replaced view showing a workspace on the right hands it to its successor.
        let shown = if self.info.is_none() { self.active.clone() } else { None };
        let mut carried: HashMap<ConnectionId, String> = HashMap::new();
        self.sessions.retain(|id, connected| {
            let keep = wanted.iter().any(|(w, session, ..)| w == id && *session == connected.session);
            if !keep
                && shown.as_ref() == Some(id)
                && let ActiveView::Workspace(workspace) = &connected.active
            {
                carried.insert(id.clone(), workspace.clone());
            }
            keep
        });
        for (id, session, target) in wanted {
            if !self.sessions.contains_key(&id) {
                let mut connected = self.connect(id.clone(), session, target, cx);
                if let Some(workspace) = carried.remove(&id) {
                    connected.reselect = Reselect::Workspace(workspace);
                }
                self.sessions.insert(id, connected);
            }
        }
        // Keep sidebar order.
        let mut slots: Vec<_> = store.read(cx).sessions().collect();
        self.names.read(cx).sort(&mut slots, |slot| &slot.target);
        let order: Vec<ConnectionId> = slots.into_iter().map(|slot| slot.id.clone()).collect();
        self.sessions.sort_by(|a, _, b, _| {
            let rank = |id: &ConnectionId| order.iter().position(|o| o == id).unwrap_or(usize::MAX);
            rank(a).cmp(&rank(b))
        });
        if self.active.as_ref().is_none_or(|id| !self.sessions.contains_key(id)) {
            self.active = self.sessions.keys().next().cloned();
        }
        self.reselect_workspaces(cx);
        self.open_pending_workspace(cx);
        cx.notify();
    }

    /// Select each rebuilt view's carried-over workspace once its session is live.
    fn reselect_workspaces(&mut self, cx: &mut Context<Self>) {
        let mut picks = Vec::new();
        for connected in self.sessions.values_mut() {
            let Reselect::Workspace(previous) = &connected.reselect else {
                continue;
            };
            let session = connected.session.read(cx);
            if !matches!(session.state(), treeterm_model::session_store::SessionState::Live) {
                continue;
            }
            let pick = match session.entry(previous) {
                Some(_) => Some(previous.clone()),
                None => session.workspace_ids().next().cloned(),
            };
            connected.reselect = Reselect::Nothing;
            if let Some(workspace) = pick {
                picks.push((connected.tree.clone(), workspace));
            }
        }
        for (tree, workspace) in picks {
            tree.update(cx, |tree, cx| tree.select(workspace, cx));
        }
    }

    /// Open `path` in the local session as soon as it is live (the CLI's directory).
    pub fn open_on_start(&mut self, path: String, cx: &mut Context<Self>) {
        self.open_on_start = Some(path);
        self.open_pending_workspace(cx);
    }

    /// The initial workspace (`getInitialWorkspace`): an open workspace with the path is
    /// selected, otherwise the directory is opened.
    fn open_pending_workspace(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.open_on_start.clone() else {
            return;
        };
        // Connected: a session kept while the connection is down takes no new workspace.
        let local = self.store.read(cx).sessions().find(|slot| slot.is_local()).filter(|slot| slot.session().is_some()).map(|slot| slot.id.clone());
        let Some(connected) = local.and_then(|id| self.sessions.get(&id)) else {
            return;
        };
        let session = connected.session.read(cx);
        if !matches!(session.state(), treeterm_model::session_store::SessionState::Live) {
            return;
        }
        self.open_on_start = None;
        let existing = session.workspace_for_path(&path);
        let (session, tree) = (connected.session.clone(), connected.tree.clone());
        let id = match existing {
            Some(id) => id,
            None => session.update(cx, |s, cx| s.open_workspace(path, cx)),
        };
        tree.update(cx, |tree, cx| tree.select(id, cx));
    }

    fn connect(
        &mut self,
        id: ConnectionId,
        session: Entity<SessionStore>,
        target: treeterm_core::connection::ConnectionTarget,
        cx: &mut Context<Self>,
    ) -> Connected {
        cx.observe(&session, |_, _, cx| cx.notify()).detach();
        let (keybindings, names, app) = (self.keybindings.clone(), self.names.clone(), self.store.clone());
        let link = id.clone();
        let tree = cx.new(|cx| TreePane::new(session.clone(), app, link, keybindings, names, target, cx));
        let info_id = id.clone();
        let disconnect_id = id.clone();
        cx.subscribe(&tree, move |view, _, _: &crate::tree_pane::DisconnectRequested, cx| {
            view.store.update(cx, |s, cx| s.disconnect(&disconnect_id, cx));
        })
        .detach();
        cx.subscribe(&tree, move |view, _, _: &crate::tree_pane::SessionInfoRequested, cx| {
            view.info_pending = Some(info_id.clone());
            cx.notify();
        })
        .detach();
        // The shown workspace was dismissed or removed (`activeWorkspaceId` cleared): the
        // pane reads "No workspace selected" and no row is selected.
        let removed_id = id.clone();
        let session_events = cx.subscribe(&session, move |view, _, event: &SessionEvent, cx| match event {
            SessionEvent::WorkspaceRemoved(workspace) => {
                let Some(connected) = view.sessions.get_mut(&removed_id) else {
                    return;
                };
                if !matches!(&connected.active, ActiveView::Workspace(shown) if shown == workspace) {
                    return;
                }
                connected.active = ActiveView::Nothing;
                connected.header_edit = None;
                connected.tree.update(cx, |tree, cx| tree.clear_selection(cx));
                cx.notify();
            }
            SessionEvent::AttentionRecorded(_) | SessionEvent::Failed(_) => {}
        });
        let viewed = session.clone();
        let selection = cx.subscribe(&tree, move |view, _, WorkspaceSelected(workspace), cx| {
            view.info = None;
            view.activate_session(&id, cx);
            viewed.update(cx, |s, cx| s.set_viewed_workspace(Some(workspace.clone()), cx));
            if let Some(connected) = view.sessions.get_mut(&id) {
                connected.active = ActiveView::Workspace(workspace.clone());
                connected.focus_pending = true;
            }
            cx.notify();
        });
        Connected {
            session,
            tree,
            active: ActiveView::Nothing,
            terminal_views: HashMap::new(),
            file_views: HashMap::new(),
            editor_views: HashMap::new(),
            review_views: HashMap::new(),
            browser_views: HashMap::new(),
            page_subtabs: HashMap::new(),
            tab_strips: HashMap::new(),
            run_actions: HashMap::new(),
            traffic_badges: HashMap::new(),
            header_edit: None,
            branch_copied: None,
            tool_views: crate::tool_views::ToolViews::new(self.settings.clone()),
            focus_pending: false,
            shown_terminal: None,
            reselect: Reselect::Nothing,
            _selection: selection,
            _session_events: session_events,
        }
    }

    /// Show `id`'s session on the right; the other trees drop their selection.
    fn activate_session(&mut self, id: &ConnectionId, cx: &mut Context<Self>) {
        self.active = Some(id.clone());
        for (other, connected) in &self.sessions {
            if other != id {
                connected.tree.update(cx, |tree, cx| tree.clear_selection(cx));
                connected.session.update(cx, |s, cx| s.set_viewed_workspace(None, cx));
            }
        }
    }

    /// The session shown on the right.
    fn current(&self) -> Option<&Connected> {
        self.active.as_ref().and_then(|id| self.sessions.get(id))
    }

    fn current_mut(&mut self) -> Option<&mut Connected> {
        let id = self.active.clone()?;
        self.sessions.get_mut(&id)
    }

    /// The collapsed sidebar (`.tree-pane-collapsed`, 36px): sound toggle and expand, then one
    /// 32px icon per workspace of every session (`WorkspaceIcon`, favourite star overlay).
    fn rail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (bg, border, hover, active, secondary, primary) =
            (c.bg_secondary, c.border, c.bg_hover, c.bg_active, c.text_secondary, c.text_primary);
        let mut rail = v_flex().py(px(4.)).items_center();
        for (conn_id, connected) in &self.sessions {
            let session = connected.session.read(cx);
            let shown = match (&self.active, &connected.active) {
                (Some(active_id), ActiveView::Workspace(id)) if active_id == conn_id => Some(id.clone()),
                (Some(_), ActiveView::Workspace(_)) | (_, ActiveView::Nothing) | (None, ActiveView::Workspace(_)) => None,
            };
            for row in session.tree_rows(&Default::default(), cx) {
                let Some(entry) = session.entry(&row.id) else { continue };
                let is_shown = shown.as_deref() == Some(row.id.as_str());
                let color = if is_shown { primary } else { secondary };
                // A placeholder shows a spinner and a failed workspace an alert, under its name.
                let (name, icon, favourite) = match entry {
                    WorkspaceEntry::Loaded(store) | WorkspaceEntry::OperationError { store, .. } => {
                        let workspace = store.read(cx);
                        let activity = workspace.overall_activity();
                        let icon = if activity == treeterm_model::workspace_store::ActivityState::Idle {
                            let glyph = if workspace.file().is_worktree { IconName::GitBranch } else { IconName::Folder };
                            crate::ui::icon(glyph, 16., color).into_any_element()
                        } else {
                            crate::ui::activity_glyph(activity, 14., color)
                        };
                        (workspace.file().display_name().to_owned(), icon, workspace.is_favourite())
                    }
                    WorkspaceEntry::Loading { name, .. } => {
                        let spinner = Spinner::new()
                            .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
                            .with_size(px(16.))
                            .color(color);
                        (name.clone(), spinner.into_any_element(), false)
                    }
                    WorkspaceEntry::Error { name, .. } => {
                        (name.clone(), crate::ui::icon(IconName::CircleAlert, 16., c.error).into_any_element(), false)
                    }
                };
                let name = SharedString::from(name);
                let (tree, id) = (connected.tree.clone(), row.id.clone());
                rail = rail.child(
                    div()
                        .id(SharedString::from(format!("rail-{}", row.id)))
                        .relative()
                        .size(px(32.))
                        .flex_shrink_0()
                        .rounded(px(4.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .when(is_shown, |d| d.bg(active))
                        .when(!is_shown, |d| d.hover(move |d| d.bg(hover)))
                        .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(name.clone()).build(window, cx))
                        .child(icon)
                        .when(favourite, |d| {
                            d.child(
                                div()
                                    .absolute()
                                    .top(px(1.))
                                    .right(px(1.))
                                    .text_size(px(10.))
                                    .line_height(px(10.))
                                    .text_color(gpui_kit::rgb(0xf5c84a))
                                    .child("★"),
                            )
                        })
                        .on_click(move |_, _, cx| tree.update(cx, |tree, cx| tree.select(id.clone(), cx))),
                );
            }
        }
        let on = self.settings.read(cx).settings().notifications.sound_enabled;
        v_flex()
            .h_full()
            .w(px(36.))
            .flex_shrink_0()
            .bg(bg)
            .pt(px(38.))
            .child(
                v_flex()
                    .py(px(12.))
                    .items_center()
                    .border_b_1()
                    .border_color(border)
                    .child(
                        crate::ui::icon_button("sound-toggle", cx)
                            .child(crate::ui::icon(if on { IconName::Volume2 } else { IconName::VolumeX }, 14., primary))
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.settings.update(cx, |store, cx| {
                                    store.update(|s| s.notifications.sound_enabled = !on, cx)
                                });
                            })),
                    )
                    .child(
                        crate::ui::icon_button("expand-sidebar", cx)
                            .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Expand sidebar").build(window, cx))
                            .child(crate::ui::icon(IconName::PanelLeftOpen, 14., primary))
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.sidebar_collapsed = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(div().id("rail").flex_1().overflow_y_scroll().child(rail))
            .into_any_element()
    }

    /// The left column: every connection with its tree, status and actions.
    fn sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.sidebar_collapsed {
            return self.rail(cx);
        }
        let c = colors(cx);
        let (bg, secondary, border, fg) = (c.bg_secondary, c.text_secondary, c.border, c.text_primary);
        let names = self.names.read(cx);
        let mut slots: Vec<_> = self.store.read(cx).sessions().collect();
        names.sort(&mut slots, |slot| &slot.target);
        let slots: Vec<(ConnectionId, String, ConnState)> =
            slots.into_iter().map(|slot| (slot.id.clone(), names.name(&slot.target), slot.state.clone())).collect();
        let mut list = v_flex().w_full();
        for (id, name, state) in slots {
            let section = match (self.sessions.get(&id), state) {
                // Connected, or down with its last session kept (the tree shows the status).
                (Some(connected), _) => connected.tree.clone().into_any_element(),
                (None, ConnState::Connected(_)) => session_panel(&id, &name, SessionIcon::Connecting, None, None, cx),
                (None, ConnState::Connecting(phase)) => {
                    // `.tree-empty` with 12px text and 4px 8px padding.
                    let status =
                        div().px(px(8.)).py(px(4.)).text_size(px(12.)).text_color(secondary).child(connect_phase_label(phase));
                    session_panel(&id, &name, SessionIcon::Connecting, None, Some(status.into_any_element()), cx)
                }
                (None, ConnState::Reconnecting { attempt, error: reason }) => {
                    let (now, stop) = (id.clone(), id.clone());
                    let banner = connection_banner(
                        BannerKind::Reconnecting,
                        format!("Reconnecting (attempt {attempt})... {reason}"),
                        vec![
                            ("Retry now", Box::new(move |view: &mut AppView, cx: &mut Context<AppView>| view.store.read(cx).reconnect_now(&now))),
                            ("Stop", Box::new(move |view: &mut AppView, cx: &mut Context<AppView>| view.store.read(cx).cancel_reconnect(&stop))),
                        ],
                        cx,
                    );
                    session_panel(&id, &name, SessionIcon::Reconnecting, Some(banner), None, cx)
                }
                (None, ConnState::Failed { error: reason, hash_mismatch }) => {
                    let (again, gone) = (id.clone(), id.clone());
                    let mut buttons: Vec<(&'static str, BannerAction<AppView>)> = vec![
                        ("Reconnect", Box::new(move |view: &mut AppView, cx: &mut Context<AppView>| view.store.read(cx).reconnect(&again))),
                        ("Disconnect", Box::new(move |view: &mut AppView, cx: &mut Context<AppView>| view.store.update(cx, |s, cx| s.disconnect(&gone, cx)))),
                    ];
                    if hash_mismatch {
                        // Electron offers these in the session pane's Daemon Mismatch tab.
                        let (refresh, accept) = (id.clone(), id.clone());
                        buttons.push(("Refresh daemon", Box::new(move |view: &mut AppView, cx: &mut Context<AppView>| {
                            view.store.update(cx, |s, cx| s.reconnect_outdated(&refresh, treeterm_core::connection::OutdatedDaemon::Refresh, cx))
                        })));
                        buttons.push(("Connect anyway", Box::new(move |view: &mut AppView, cx: &mut Context<AppView>| {
                            view.store.update(cx, |s, cx| s.reconnect_outdated(&accept, treeterm_core::connection::OutdatedDaemon::Accept, cx))
                        })));
                    }
                    let banner = connection_banner(BannerKind::Error, reason, buttons, cx);
                    session_panel(&id, &name, SessionIcon::Failed, Some(banner), None, cx)
                }
            };
            list = list.child(self.session_drag_handle(id, name, section, cx));
        }
        let on = self.settings.read(cx).settings().notifications.sound_enabled;
        let tip = |text: &'static str| move |window: &mut Window, cx: &mut App| gpui_kit::component::tooltip::Tooltip::new(text).build(window, cx);
        v_flex()
            .h_full()
            .w(px(self.sidebar_width))
            .flex_shrink_0()
            .bg(bg)
            // macOS traffic lights sit over the top 38px, as in Electron's `.tree-pane-content`.
            .pt(px(38.))
            .child(
                // `.tree-header`: sound toggle, "SESSIONS", Connect to SSH, collapse.
                h_flex()
                    .px(px(16.))
                    .py(px(12.))
                    .justify_between()
                    .items_center()
                    .border_b_1()
                    .border_color(border)
                    .child(
                        crate::ui::icon_button("sound-toggle", cx)
                            .tooltip(tip(if on { "Mute workspace attention sound" } else { "Unmute workspace attention sound" }))
                            .child(crate::ui::icon(if on { IconName::Volume2 } else { IconName::VolumeX }, 14., fg))
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.settings.update(cx, |store, cx| {
                                    store.update(|s| s.notifications.sound_enabled = !on, cx)
                                });
                            })),
                    )
                    .child(crate::ui::caps_title("Sessions", cx))
                    .child(
                        h_flex()
                            .gap(px(4.))
                            .child(
                                crate::ui::icon_button("connect-ssh", cx)
                                    .tooltip(tip("Connect to SSH"))
                                    .child(crate::ui::icon(IconName::Monitor, 14., fg))
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        crate::connection_picker::open(view.store.clone(), window, cx);
                                    })),
                            )
                            .child(
                                crate::ui::icon_button("collapse-sidebar", cx)
                                    .tooltip(tip("Collapse sidebar"))
                                    .child(crate::ui::icon(IconName::PanelLeftClose, 14., fg))
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.sidebar_collapsed = true;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .children(self.favourites(cx))
            .child(div().id("sessions").flex_1().overflow_y_scroll().child(list))
            .into_any_element()
    }

    /// A session's sidebar section as a drag source and drop target (`.session-drag-handle`):
    /// dropped on the top half of another session it goes before it, else after it. Rows
    /// inside drag their workspace instead (the innermost drag source wins).
    fn session_drag_handle(
        &self,
        id: ConnectionId,
        name: String,
        section: AnyElement,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let accent = colors(cx).accent;
        let hint = self.session_drop_hint.as_ref().filter(|(over, _)| *over == id).map(|(_, side)| *side);
        let (move_id, drop_id) = (id.clone(), id.clone());
        div()
            .id(SharedString::from(format!("session-drag-{id}")))
            .relative()
            .on_drag(DraggedSession { id, name: name.into() }, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
            .on_drag_move(cx.listener(move |view, event: &gpui_kit::DragMoveEvent<DraggedSession>, _, cx| {
                let position = event.event.position;
                let next = (event.bounds.contains(&position) && event.drag(cx).id != move_id).then(|| {
                    let middle = event.bounds.top() + event.bounds.size.height / 2.;
                    (move_id.clone(), if position.y < middle { Side::Before } else { Side::After })
                });
                let changed = match (&view.session_drop_hint, &next) {
                    (Some((old, _)), None) => *old == move_id,
                    (old, Some(_)) => old.as_ref() != next.as_ref(),
                    (None, None) => false,
                };
                if changed {
                    view.session_drop_hint = next;
                    cx.notify();
                }
            }))
            .on_drop(cx.listener(move |view, dragged: &DraggedSession, _, cx| view.drop_session(&dragged.id, &drop_id, cx)))
            .child(section)
            // `inset 0 ±2px 0 0 var(--accent)`: a line over the section's edge, no layout shift.
            .when_some(hint, |d, side| {
                let line = div().absolute().left_0().right_0().h(px(2.)).bg(accent);
                d.child(match side {
                    Side::Before => line.top_0(),
                    Side::After => line.bottom_0(),
                })
            })
    }

    /// `dragged` was dropped on `onto`'s section: move it to the side the hint showed.
    fn drop_session(&mut self, dragged: &ConnectionId, onto: &ConnectionId, cx: &mut Context<Self>) {
        cx.notify();
        let side = match self.session_drop_hint.take() {
            Some((over, side)) if over == *onto => side,
            Some(_) | None => return, // dropped on itself
        };
        let store = self.store.read(cx);
        let target = |id: &ConnectionId| store.session_slot(id).map(|slot| slot.target.clone());
        // Either may have been disconnected while dragging.
        let (Some(dragged), Some(onto)) = (target(dragged), target(onto)) else {
            return;
        };
        let shown: Vec<_> = store.sessions().map(|slot| slot.target.clone()).collect();
        self.names.update(cx, |names, cx| names.reorder(&shown, &dragged, &onto, side, cx));
    }

    /// Favourite workspaces of every connected session (`FavouritesPanel`); none, no panel.
    fn favourites(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let c = colors(cx);
        let (secondary, primary, hover, active, accent, border) =
            (c.text_secondary, c.text_primary, c.bg_hover, c.bg_active, c.accent, c.border);
        let mut rows = Vec::new();
        for (conn_id, connected) in &self.sessions {
            let session = connected.session.read(cx);
            let shown = match (&self.active, &connected.active) {
                (Some(active_id), ActiveView::Workspace(id)) if active_id == conn_id => Some(id.clone()),
                (Some(_), ActiveView::Workspace(_)) | (_, ActiveView::Nothing) | (None, ActiveView::Workspace(_)) => None,
            };
            for id in session.workspace_ids() {
                let Some(store) = session.workspace_store(id) else { continue };
                let workspace = store.read(cx);
                if workspace.is_favourite() {
                    rows.push((
                        id.clone(),
                        workspace.file().display_name().to_owned(),
                        workspace.file().is_worktree,
                        workspace.has_unread_attention(),
                        workspace.overall_activity(),
                        shown.as_deref() == Some(id.as_str()),
                        connected.tree.clone(),
                    ));
                }
            }
        }
        if rows.is_empty() {
            return None;
        }
        // `.favourites-header`: filled star and "FAVOURITES", 11px semibold secondary.
        let mut panel = v_flex().pb(px(6.)).border_b(px(2.)).border_color(border).child(
            h_flex()
                .pt(px(6.))
                .px(px(12.))
                .pb(px(2.))
                .gap(px(4.))
                .items_center()
                .child(div().text_size(px(12.)).line_height(px(12.)).text_color(secondary).child("★"))
                .child(crate::ui::caps_title("Favourites", cx)),
        );
        for (id, name, is_worktree, unread, activity, is_shown, tree) in rows {
            let icon = if activity == treeterm_model::workspace_store::ActivityState::Idle {
                crate::ui::icon(if is_worktree { IconName::GitBranch } else { IconName::Folder }, 16., secondary).into_any_element()
            } else {
                crate::ui::activity_glyph(activity, 14., secondary)
            };
            panel = panel.child(
                h_flex()
                    .id(SharedString::from(format!("fav-{id}")))
                    .min_h(px(36.))
                    .py(px(6.))
                    .px(px(4.))
                    .items_center()
                    .cursor_pointer()
                    .when(is_shown, |d| d.bg(active))
                    .when(!is_shown, |d| d.hover(move |d| d.bg(hover)))
                    .when(unread, |d| d.border_l(px(2.)).border_color(accent))
                    .child(div().mr(px(8.)).flex_shrink_0().child(icon))
                    .child(div().flex_1().min_w_0().truncate().text_size(px(crate::ui::BODY)).text_color(primary).child(name))
                    .when(unread, |d| d.child(div().mx(px(4.)).text_size(px(10.)).text_color(accent).child("●")))
                    .on_click(move |_, _, cx| tree.update(cx, |tree, cx| tree.select(id.clone(), cx))),
            );
        }
        Some(panel.into_any_element())
    }

    /// Open a pending session pane, or drop one whose session is gone.
    fn sync_info(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.info_pending.take() {
            // A session pane shows no workspace: none is being viewed.
            for connected in self.sessions.values() {
                connected.session.update(cx, |s, cx| s.set_viewed_workspace(None, cx));
            }
            let (app, names) = (self.store.clone(), self.names.clone());
            self.info = Some(cx.new(|cx| crate::session_info_view::SessionInfoView::new(app, names, id, window, cx)));
        }
        if let Some(info) = &self.info {
            let id = info.read(cx).connection(cx);
            if self.store.read(cx).session_slot(&id).is_none() {
                self.info = None;
            }
        }
    }

}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (bg, fg, secondary, error, border, accent) =
            (c.bg_primary, c.text_primary, c.text_secondary, c.error, c.border, c.accent);
        self.sync_info(window, cx);
        let sidebar = self.sidebar(cx);
        let info = self.info.clone();
        let collapsed = self.sidebar_collapsed && cfg!(target_os = "macos");
        // Native webviews draw above GPUI: hide those of every workspace pane not on screen
        // (the shown one manages its own in `workspace_pane`).
        let on_screen = if info.is_none() { self.active.clone() } else { None };
        let hidden: Vec<Entity<crate::browser_view::BrowserView>> = self
            .sessions
            .iter()
            .filter(|(id, _)| on_screen.as_ref() != Some(*id))
            .flat_map(|(_, connected)| connected.browser_views.values().cloned().collect::<Vec<_>>())
            .collect();
        let hidden_files: Vec<Entity<crate::file_browser_view::FileBrowserView>> = self
            .sessions
            .iter()
            .filter(|(id, _)| on_screen.as_ref() != Some(*id))
            .flat_map(|(_, connected)| connected.file_views.values().cloned().collect::<Vec<_>>())
            .collect();
        for view in hidden {
            view.update(cx, |v, cx| v.set_shown(false, cx));
        }
        for view in hidden_files {
            view.update(cx, |v, cx| v.set_shown(false, cx));
        }
        for (_, connected) in self.sessions.iter().filter(|(id, _)| on_screen.as_ref() != Some(*id)) {
            hide_reviews(connected, cx);
        }
        // A session off screen shows no terminal: its terminal is shown again (fits again and
        // takes focus) when it returns.
        for (_, connected) in self.sessions.iter_mut().filter(|(id, _)| on_screen.as_ref() != Some(*id)) {
            connected.shown_terminal = None;
        }
        let pane = match (info, self.current_mut()) {
            (Some(info), _) => info.into_any_element(),
            (None, current) => match current {
            // macOS traffic lights sit over the header's left end once the sidebar is a rail.
            Some(connected) => workspace_pane(connected, if collapsed { px(72.) } else { px(16.) }, window, cx),
            None => {
                let starting = self.store.read(cx).sessions().next().map(|slot| slot.state.clone());
                match starting {
                    Some(ConnState::Failed { error: reason, .. }) => centered(
                        v_flex()
                            .gap_2()
                            .max_w(px(600.))
                            .child(div().text_color(error).child("Cannot reach the TreeTerm daemon"))
                            .child(div().text_sm().text_color(secondary).child(reason)),
                    ),
                    Some(ConnState::Reconnecting { attempt, error: reason }) => centered(
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
                            .child(div().text_sm().text_color(secondary).child(reason)),
                    ),
                    Some(ConnState::Connecting(_) | ConnState::Connected(_)) | None => centered(
                        h_flex()
                            .gap_2()
                            .text_color(secondary)
                            .child(Spinner::new())
                            .child("Starting TreeTerm daemon…"),
                    ),
                }
            }
            },
        };
        let pane = div().relative().flex().flex_1().h_full().min_w_0().child(pane);
        // `.divider`: 4px, accent while hovered or dragged.
        let divider = (!self.sidebar_collapsed).then(|| {
            div()
                .id("sidebar-divider")
                .w(px(4.))
                .h_full()
                .flex_shrink_0()
                .bg(border)
                .hover(move |d| d.bg(accent))
                .cursor_col_resize()
                .on_drag(SidebarResize, |drag, _, _, cx| cx.new(|_| drag.clone()))
        });
        let content = h_flex()
            .size_full()
            .child(sidebar)
            .children(divider)
            .child(pane)
            .on_drag_move(cx.listener(|view, event: &gpui_kit::DragMoveEvent<SidebarResize>, _, cx| {
                let width = f32::from(event.event.position.x).clamp(150., 400.);
                if (width - view.sidebar_width).abs() >= 1. {
                    view.sidebar_width = width;
                    cx.notify();
                }
            }))
            .into_any_element();
        if window.focused(cx).is_none() {
            window.focus(&self.focus, cx);
        }
        if matches!(self.processes_dialog, ProcessesDialog::Pending) && self.current().is_some() {
            cx.defer_in(window, |view, window, cx| view.open_active_processes(window, cx));
        }
        let menu_bar = match &self.menu_bar {
            MenuBar::System => None,
            MenuBar::InWindow(bar) => Some(crate::menus::in_window_bar(bar, cx)),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .bg(bg)
            .text_color(fg)
            // Chromium's `line-height: normal` for the system font: text boxes as tall as
            // Electron's (GPUI's default is about 1.6).
            .line_height(gpui_kit::relative(1.2))
            .children(menu_bar)
            .child(div().flex().flex_1().min_h_0().child(content))
            .children(crate::keybinding_overlay::render(&self.keybindings, window, cx))
    }
}

impl AppView {
    /// Whether the window may close now; when child workspaces are unmerged, asks first
    /// (and quits if the user confirms).
    pub fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let unmerged: Vec<(String, Option<String>)> = self
            .sessions
            .values()
            .flat_map(|connected| connected.session.read(cx).unmerged_sub_workspaces(cx))
            .collect();
        if unmerged.is_empty() {
            return true;
        }
        if self.close_confirm.strong_count() == 0 {
            self.close_confirm = crate::active_processes_view::confirm_close(unmerged, window, cx);
        }
        false
    }

    /// Quit (Cmd+Q): Electron's `app.quit()` closes the window, which asks first.
    pub fn quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.request_close(window, cx) {
            cx.quit();
        }
    }

    /// Close Window (Cmd+W, Ctrl+W): as the window's close button. Closing the last window
    /// quits (`main`), as Electron's `window-all-closed`.
    pub fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.request_close(window, cx) {
            window.remove_window();
        }
    }

    /// Settings (Cmd+, / Ctrl+,), unless it is already open.
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_dialog.upgrade().is_none() {
            self.settings_dialog = crate::settings_view::open(self.settings.clone(), window, cx).downgrade();
        }
    }

    /// Window > Active Processes for the current session, unless it is already open; with
    /// no session yet, once there is one.
    pub fn open_active_processes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let ProcessesDialog::Open(view) = &self.processes_dialog
            && view.upgrade().is_some()
        {
            return;
        }
        self.processes_dialog = match self.current() {
            Some(connected) => {
                let settings = self.settings.read(cx).settings().terminal.clone();
                let view = crate::active_processes_view::open(&connected.session, settings, window, cx);
                ProcessesDialog::Open(view.downgrade())
            }
            None => ProcessesDialog::Pending,
        };
        cx.notify();
    }

    /// View > Reset Input: Electron blurs and focuses the window so its focus handlers run
    /// again (a terminal claims its PTY's input back). Here focus leaves for one frame and
    /// returns, which runs the views' blur and focus handlers.
    pub fn reset_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = window.focused(cx);
        window.blur(cx);
        // Focus handlers run when a drawn frame's focus differs from the last one's, and
        // next-frame callbacks run before that frame is drawn: return after the next frame.
        cx.on_next_frame(window, move |_, window, cx| {
            cx.on_next_frame(window, move |view, window, cx| match focused {
                Some(handle) => window.focus(&handle, cx),
                None => view.focus_active_tab(cx),
            });
        });
    }

    /// Edit the active workspace's name or description in place; Enter or blur saves.
    fn edit_header(&mut self, field: HeaderField, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
        let Some(store) = self.active_store(cx) else { return };
        let edit = match field {
            HeaderField::Name => {
                let text = store.read(cx).file().display_name().to_owned();
                let input = cx.new(|cx| InputState::new(window, cx).default_value(text));
                input.update(cx, |input, cx| input.focus(window, cx));
                let save = cx.subscribe(&input, move |view, input, event: &InputEvent, cx| match event {
                    InputEvent::PressEnter { .. } | InputEvent::Blur => {
                        let text = input.read(cx).value().to_string();
                        if let Some(connected) = view.current_mut() {
                            connected.header_edit = None;
                        }
                        store.update(cx, |w, cx| w.set_display_name(&text, cx));
                        cx.notify();
                    }
                    InputEvent::Change | InputEvent::Focus => {}
                });
                HeaderEdit::Name { input, _save: save }
            }
            HeaderField::Description => {
                let text = store.read(cx).file().metadata.get("description").cloned().unwrap_or_default();
                let input = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .rows(1)
                        .submit_on_enter(true)
                        .placeholder("Add a description...")
                        .default_value(text)
                });
                input.update(cx, |input, cx| input.focus(window, cx));
                let save = cx.subscribe(&input, move |view, input, event: &InputEvent, cx| match event {
                    InputEvent::PressEnter { .. } | InputEvent::Blur => {
                        let text = input.read(cx).value().to_string();
                        if let Some(connected) = view.current_mut() {
                            connected.header_edit = None;
                        }
                        store.update(cx, |w, cx| w.set_description(&text, cx));
                        cx.notify();
                    }
                    InputEvent::Change | InputEvent::Focus => {}
                });
                HeaderEdit::Description { input, _save: save }
            }
        };
        if let Some(connected) = self.current_mut() {
            connected.header_edit = Some(edit);
        }
        cx.notify();
    }

    /// The branch badge: copy the name and read "Copied!" for 1.5s.
    fn copy_branch(&mut self, branch: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(branch.clone()));
        let executor = cx.background_executor().clone();
        let reset = cx.spawn(async move |view, cx| {
            executor.timer(std::time::Duration::from_millis(1500)).await;
            match view.update(cx, |view, cx| {
                if let Some(connected) = view.current_mut() {
                    connected.branch_copied = None;
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {}
            }
        });
        if let Some(connected) = self.current_mut() {
            connected.branch_copied = Some((branch, reset));
        }
        cx.notify();
    }

    /// The active workspace's store, if it is loaded.
    fn active_store(&self, cx: &App) -> Option<Entity<treeterm_model::workspace_store::WorkspaceStore>> {
        let connected = self.current()?;
        let ActiveView::Workspace(id) = &connected.active else {
            return None;
        };
        connected.session.read(cx).workspace_store(id).cloned()
    }

    fn focus_active_tab(&mut self, cx: &mut Context<Self>) {
        if let Some(connected) = self.current_mut() {
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
                if let Some(connected) = self.current() {
                    connected.tree.update(cx, |tree, cx| tree.select(workspace_id, cx));
                }
            }
            Action::Prefix(PrefixAction::NewTab) => {
                let Some(store) = self.active_store(cx) else { return };
                let Some(connected) = self.current() else { return };
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
                let Some(connected) = self.current() else { return };
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
            Action::Prefix(PrefixAction::OpenSettings) => self.open_settings(window, cx),
        }
    }
}

/// `MergeAbandonButton` for a child worktree: "Review & Merge" (opens Review), or "Abandon"
/// when its branch has nothing to merge, split from a chevron with the other choices.
fn merge_menu(
    connected: &Connected,
    workspace: &Entity<treeterm_model::workspace_store::WorkspaceStore>,
    workspace_id: &str,
    cx: &mut Context<AppView>,
) -> Option<AnyElement> {
    use gpui_kit::component::menu::DropdownMenu as _;
    use treeterm_core::git_status::ParentDiff;
    use treeterm_model::session_store::RemoveMode;
    let file = workspace.read(cx).file().clone();
    if !file.is_worktree || file.parent_id.as_deref().is_none_or(str::is_empty) {
        return None;
    }
    let clean = connected
        .session
        .read(cx)
        .git_controller(workspace_id)
        .is_some_and(|git| git.read(cx).status().parent_diff == ParentDiff::Clean);
    let (session, ws, id, label): (_, _, String, SharedString) =
        (connected.session.clone(), workspace.clone(), workspace_id.to_owned(), file.display_name().to_owned().into());
    let accent = colors(cx).accent;
    // `.workspace-action-btn-merge`: accent fill, white text.
    let merge_style = move |button: gpui_kit::Stateful<gpui_kit::Div>| {
        button.bg(accent).border_color(accent).text_color(gpui_kit::white())
    };
    let remove = {
        let (session, id, label) = (session.clone(), id.clone(), label.clone());
        move |mode: RemoveMode, window: &mut Window, cx: &mut App| {
            crate::tree_pane::confirm_remove(session.clone(), id.clone(), label.clone(), mode, window, cx)
        }
    };
    let (main_ws, main_remove) = (ws.clone(), remove.clone());
    let main = merge_style(crate::ui::action_button(SharedString::from(format!("merge-{workspace_id}")), cx))
        .rounded_r(px(0.))
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(if clean {
                "Abandon: No changes to merge — delete worktree and branch"
            } else {
                "Review & Merge: Review changes and merge this workspace"
            })
            .build(window, cx)
        })
        .child(if clean { "Abandon" } else { "Review & Merge" })
        .on_click(move |_, window, cx| {
            if clean {
                main_remove(RemoveMode::DeleteWorktreeAndBranch, window, cx);
            } else {
                open_review(&main_ws, cx);
            }
        });
    let chevron = crate::ui::Trigger::new(
        merge_style(crate::ui::action_button(SharedString::from(format!("merge-more-{workspace_id}")), cx))
            .ml(px(1.))
            .px(px(4.))
            .rounded_l(px(0.))
            .child(crate::ui::icon(IconName::ChevronDown, 14., gpui_kit::white())),
    )
    .dropdown_menu_with_anchor(gpui_kit::Anchor::TopRight, move |menu, _window, _cx| {
        let menu = if clean {
            let review = ws.clone();
            menu.item(crate::ui::menu_item_with_hint("Review & Merge", "Review changes and merge").on_click(move |_, _, cx| open_review(&review, cx)))
        } else {
            let abandon = remove.clone();
            menu.item(crate::ui::menu_item_with_hint("Abandon", "Delete worktree and branch").on_click(move |_, window, cx| {
                abandon(RemoveMode::DeleteWorktreeAndBranch, window, cx)
            }))
        };
        let (keep_branch, keep_both) = (remove.clone(), remove.clone());
        menu.item(crate::ui::menu_item_with_hint("Abandon (Keep Branch)", "Delete worktree, keep branch").on_click(move |_, window, cx| {
            keep_branch(RemoveMode::KeepBranch, window, cx)
        }))
        .item(crate::ui::menu_item_with_hint("Abandon (Keep Both)", "Keep worktree and branch").on_click(move |_, window, cx| {
            keep_both(RemoveMode::KeepWorktreeAndBranch, window, cx)
        }))
    });
    Some(h_flex().child(main).child(chevron).into_any_element())
}

/// Show the workspace's Review tab, opening one if it has none.
fn open_review(workspace: &Entity<treeterm_model::workspace_store::WorkspaceStore>, cx: &mut App) {
    workspace.update(cx, |w, cx| {
        if w.open_or_focus_tab("review", cx).is_none() {
            tracing::error!("the review application is not registered");
        }
    });
}

/// "▶ Run" (`RunActionDropdown`): the workspace's detected actions by source, then the
/// custom runners.
/// The workspace's terminal download rate (`WorkspaceTrafficBadge`).
fn traffic_badge(
    connected: &mut Connected,
    workspace: &Entity<treeterm_model::workspace_store::WorkspaceStore>,
    workspace_id: &str,
    cx: &mut Context<AppView>,
) -> Entity<crate::traffic_badge::TrafficBadge> {
    let meter = workspace.read(cx).traffic().clone();
    connected
        .traffic_badges
        .entry(workspace_id.to_owned())
        .or_insert_with(|| cx.new(|cx| crate::traffic_badge::TrafficBadge::new(meter, "workspace-traffic", cx)))
        .clone()
}

fn run_menu(
    connected: &mut Connected,
    workspace: &Entity<treeterm_model::workspace_store::WorkspaceStore>,
    workspace_id: &str,
    cx: &mut Context<AppView>,
) -> AnyElement {
    use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
    use treeterm_model::run_actions::{Detection, RunActions};
    let conn = connected.session.read(cx).connection().clone();
    let model = connected
        .run_actions
        .entry(workspace_id.to_owned())
        .or_insert_with(|| cx.new(|cx| RunActions::new(&conn, workspace, cx)))
        .clone();
    let runners: Vec<(String, String, String)> = connected
        .session
        .read(cx)
        .registry()
        .new_tab_menu()
        .filter(|app| app.id.starts_with("customrunner-"))
        .map(|app| (app.id.clone(), app.name.clone(), app.icon.clone()))
        .collect();
    let error = model.read(cx).error().map(str::to_owned);
    let (menu_model, menu_workspace) = (model.clone(), workspace.clone());
    let fg = colors(cx).text_primary;
    h_flex()
        .gap_1()
        .child(
            crate::ui::Trigger::new(
                crate::ui::action_button(SharedString::from(format!("run-{workspace_id}")), cx)
                    .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Run an action").build(window, cx))
                    .child("▶ Run")
                    .child(crate::ui::icon(IconName::ChevronDown, 12., fg)),
            )
                .dropdown_menu_with_anchor(gpui_kit::Anchor::TopRight, move |menu, _window, cx| {
                    // `.run-action-menu`: uppercase 11px group headers, 13px items, a rule
                    // between groups; detection runs once, as in Electron.
                    let secondary = colors(cx).text_secondary;
                    let note = move |text: &'static str| {
                        PopupMenuItem::element(move |_, _| div().px(px(0.)).py(px(2.)).text_size(px(12.)).text_color(secondary).child(text))
                    };
                    let header = move |text: String| {
                        PopupMenuItem::element(move |_, _| div().text_size(px(11.)).text_color(secondary).child(text.to_uppercase()))
                    };
                    let item = |text: SharedString| {
                        PopupMenuItem::element(move |_, _| div().py(px(2.)).text_size(px(13.)).whitespace_nowrap().child(text.clone()))
                    };
                    let mut menu = menu;
                    let detection = menu_model.read(cx).detection().clone();
                    let mut groups = 0;
                    match detection {
                        Detection::Detecting => return menu.item(note("Loading...")),
                        Detection::Done(actions) => {
                            if actions.is_empty() && runners.is_empty() {
                                return menu.item(note("No actions found"));
                            }
                            let mut source = None;
                            for action in actions {
                                if source != Some(action.source) {
                                    source = Some(action.source);
                                    if groups > 0 {
                                        menu = menu.separator();
                                    }
                                    groups += 1;
                                    menu = menu.item(header(action.source.as_str().to_owned()));
                                }
                                let (model, label) = (menu_model.clone(), SharedString::from(action.name.clone()));
                                menu = menu.item(item(label).on_click(move |_, _, cx| {
                                    model.update(cx, |m, cx| m.run(&action, cx));
                                }));
                            }
                        }
                    }
                    if !runners.is_empty() {
                        if groups > 0 {
                            menu = menu.separator();
                        }
                        menu = menu.item(header("Custom Runners".to_owned()));
                        for (app_id, name, icon) in &runners {
                            let (workspace, app_id) = (menu_workspace.clone(), app_id.clone());
                            let label = SharedString::from(format!("{icon}\u{2009}\u{2009}{name}"));
                            menu = menu.item(item(label).on_click(move |_, _, cx| {
                                workspace.update(cx, |w, cx| {
                                    w.add_tab(&app_id, cx);
                                });
                            }));
                        }
                    }
                    menu
                }),
        )
        .when_some(error, |d, error| d.child(div().text_xs().text_color(colors(cx).error).child(error)))
        .into_any_element()
}

/// The header's git buttons: status (`GitStatusButton`), pull (`GitPullButton`) and, for a
/// child worktree on a branch, the GitHub pair (`GitHubButton`).
fn git_status_items(
    controller: &Entity<treeterm_model::git_controller::GitController>,
    workspace: &Entity<treeterm_model::workspace_store::WorkspaceStore>,
    github: bool,
    cx: &mut Context<AppView>,
) -> Vec<AnyElement> {
    use gpui_kit::component::Sizable as _;
    use treeterm_core::git_status::{ParentConflicts, PullRequest, WorkingTree};
    use treeterm_model::git_controller::PullState;
    let c = colors(cx);
    let (status, refreshing, failures) = {
        let git = controller.read(cx);
        (git.status().clone(), git.is_refreshing(), git.failures().to_vec())
    };
    let spinner = |size: f32, color: gpui_kit::Hsla| {
        Spinner::new()
            .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
            .with_size(px(size))
            .color(color)
            .into_any_element()
    };
    // (icon, label, tooltip, colour, border) as `.workspace-action-btn-git-*` colour them.
    let (icon, label, title, color, border): (AnyElement, &str, String, gpui_kit::Hsla, gpui_kit::Hsla) = if refreshing {
        (spinner(14., c.text_primary), "Checking...", "Checking git status...".into(), c.text_primary, c.border)
    } else if status.parent_conflicts == ParentConflicts::Conflicting {
        let red = gpui_kit::rgb(0xcf3536);
        (crate::ui::icon(IconName::TriangleAlert, 14., red).into_any_element(), "Conflicts", "Merge conflicts with parent".into(), red.into(), gpui_kit::rgba(0xcf353666).into())
    } else if status.working_tree == WorkingTree::Dirty {
        let amber = gpui_kit::rgb(0xd29922);
        (crate::ui::icon(IconName::CircleDot, 14., amber).into_any_element(), "Uncommitted", "Uncommitted changes".into(), amber.into(), gpui_kit::rgba(0xd2992266).into())
    } else {
        let green = gpui_kit::rgb(0x2ea043);
        (crate::ui::icon(IconName::Check, 14., green).into_any_element(), "Clean", "Working tree clean".into(), green.into(), gpui_kit::rgba(0x2ea04366).into())
    };
    let title = if failures.is_empty() { title } else { format!("{title}\n{}", failures.join("\n")) };
    let refresh = controller.clone();
    let mut items = vec![
        crate::ui::action_button("git-status", cx)
            .text_color(color)
            .border_color(border)
            .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(title.clone()).build(window, cx))
            .child(icon)
            .child(label)
            .on_click(move |_, _, cx| refresh.update(cx, |git, cx| git.refresh(cx)))
            .into_any_element(),
    ];
    // `.workspace-action-btn-pull`: accent fill while behind upstream.
    let pull_state = controller.read(cx).pull_state().clone();
    if status.behind_count > 0 {
        let pull = controller.clone();
        let behind = status.behind_count;
        let title = match &pull_state {
            PullState::Failed(error) => error.clone(),
            PullState::Idle | PullState::Pulling => format!("Pull {behind} commit{} from remote", if behind > 1 { "s" } else { "" }),
        };
        let accent = c.accent;
        items.push(
            crate::ui::action_button("git-pull", cx)
                .bg(accent)
                .border_color(accent)
                .text_color(gpui_kit::white())
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(title.clone()).build(window, cx))
                .child(match pull_state {
                    PullState::Pulling => spinner(14., gpui_kit::white()),
                    PullState::Idle | PullState::Failed(_) => crate::ui::icon(IconName::ArrowDownToLine, 14., gpui_kit::white()).into_any_element(),
                })
                .child(
                    h_flex()
                        .gap(px(3.))
                        .child("Pull")
                        .child(div().text_size(px(10.)).font_weight(gpui_kit::FontWeight::SEMIBOLD).child(behind.to_string())),
                )
                .on_click(move |_, _, cx| pull.update(cx, |git, cx| git.pull(cx)))
                .into_any_element(),
        );
    }
    if github {
        let (pr, unresolved) = match &status.pull_request {
            PullRequest::Found(pr) => (true, pr.unresolved_count()),
            PullRequest::None => (false, 0),
        };
        // `.workspace-action-btn-github` (green) with a PR, `-github-comments` (amber) with
        // unresolved threads.
        let tint: Option<(u32, u32)> = if unresolved > 0 {
            Some((0xd29922, 0xd2992226))
        } else if pr {
            Some((0x238636, 0x23863626))
        } else {
            None
        };
        let style = move |button: gpui_kit::Stateful<gpui_kit::Div>| match tint {
            Some((fg, bg)) => button.text_color(gpui_kit::rgb(fg)).border_color(gpui_kit::rgb(fg)).bg(gpui_kit::rgba(bg)),
            None => button,
        };
        let label = if unresolved > 0 {
            format!("Address {unresolved} comment{}", if unresolved == 1 { "" } else { "s" })
        } else if pr {
            "Open PR".to_owned()
        } else {
            "Create PR".to_owned()
        };
        let main_title = if unresolved > 0 { label.clone() } else if pr { "Open GitHub PR".into() } else { "Create GitHub PR".into() };
        let browser_title = if pr { "Open GitHub PR in browser" } else { "Open create PR page in browser" };
        let fg = match tint {
            Some((fg, _)) => gpui_kit::rgb(fg).into(),
            None => c.text_primary,
        };
        let open = {
            let controller = controller.clone();
            move |window: &mut Window, cx: &mut App| {
                let task = controller.update(cx, |git, cx| git.open_github(cx));
                let handle = window.window_handle();
                cx.spawn(async move |cx| match task.await {
                    Ok(url) => cx.update(|cx| cx.open_url(&url)),
                    Err(error) => match handle.update(cx, |_, window, cx| {
                        use gpui_kit::component::WindowExt as _;
                        window.push_notification(error, cx);
                    }) {
                        Ok(()) | Err(_) => {}
                    },
                })
                .detach();
            }
        };
        let (main_open, comments_ws) = (open.clone(), workspace.clone());
        items.push(
            h_flex()
                .child(
                    style(crate::ui::action_button("github-main", cx))
                        .rounded_r(px(0.))
                        .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(main_title.clone()).build(window, cx))
                        .child(crate::ui::icon(IconName::Github, 14., fg))
                        .child(label)
                        .on_click(move |_, window, cx| {
                            if unresolved > 0 {
                                comments_ws.update(cx, |w, cx| {
                                    if w.open_or_focus_tab("github", cx).is_none() {
                                        tracing::error!("the github application is not registered");
                                    }
                                });
                            } else {
                                main_open(window, cx);
                            }
                        }),
                )
                .child(
                    style(crate::ui::action_button("github-browser", cx))
                        .ml(px(1.))
                        .px(px(6.))
                        .rounded_l(px(0.))
                        .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(browser_title).build(window, cx))
                        .child(crate::ui::icon(IconName::ExternalLink, 14., fg))
                        .on_click(move |_, window, cx| open(window, cx)),
                )
                .into_any_element(),
        );
    }
    items
}

/// The status icon in a session header (`renderStatusIcon`) of a session that is not
/// connected.
pub(crate) enum SessionIcon {
    Connecting,
    Reconnecting,
    Failed,
}

/// 14px: a spinner while connecting (orange while reconnecting), a red alert once failed.
pub(crate) fn session_icon(icon: SessionIcon, cx: &App) -> AnyElement {
    match icon {
        SessionIcon::Connecting | SessionIcon::Reconnecting => Spinner::new()
            .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
            .with_size(px(14.))
            .color(match icon {
                SessionIcon::Reconnecting => gpui_kit::rgb(0xff9800).into(),
                SessionIcon::Connecting | SessionIcon::Failed => colors(cx).text_secondary,
            })
            .into_any_element(),
        SessionIcon::Failed => crate::ui::icon(IconName::CircleAlert, 14., gpui_kit::rgb(0xf44336)).into_any_element(),
    }
}

/// What a connection that is still connecting shows in place of its tree.
pub(crate) fn connect_phase_label(phase: treeterm_core::connection::ConnectPhase) -> &'static str {
    match phase {
        treeterm_core::connection::ConnectPhase::Bootstrap => "Bootstrapping...",
        treeterm_core::connection::ConnectPhase::Tunnel => "Establishing tunnel...",
        treeterm_core::connection::ConnectPhase::Daemon => "Connecting to daemon...",
    }
}

pub(crate) enum BannerKind {
    Reconnecting,
    Error,
}

pub(crate) type BannerAction<V> = Box<dyn Fn(&mut V, &mut Context<V>)>;

/// `.connection-banner`: an 11px coloured strip with small outlined buttons.
pub(crate) fn connection_banner<V: 'static>(
    kind: BannerKind,
    text: String,
    buttons: Vec<(&'static str, BannerAction<V>)>,
    cx: &mut Context<V>,
) -> AnyElement {
    let (color, background, icon): (u32, u32, AnyElement) = match kind {
        BannerKind::Reconnecting => (
            0xff9800,
            0xff980014,
            Spinner::new()
                .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
                .with_size(px(12.))
                .color(gpui_kit::rgb(0xff9800).into())
                .into_any_element(),
        ),
        BannerKind::Error => (0xf44336, 0xf4433614, crate::ui::icon(IconName::CircleAlert, 12., gpui_kit::rgb(0xf44336)).into_any_element()),
    };
    let border = colors(cx).border;
    let mut banner = h_flex()
        .gap(px(6.))
        .px(px(8.))
        .py(px(4.))
        .items_center()
        .text_size(px(11.))
        .text_color(gpui_kit::rgb(color))
        .bg(gpui_kit::rgba(background))
        .border_b_1()
        .border_color(border)
        .child(icon)
        .child(div().flex_1().min_w_0().truncate().child(text));
    for (index, (label, action)) in buttons.into_iter().enumerate() {
        banner = banner.child(
            div()
                .id(("banner-button", index))
                .flex_shrink_0()
                .px(px(6.))
                .py(px(1.))
                .rounded(px(3.))
                .border_1()
                .border_color(gpui_kit::rgb(color))
                .text_size(px(10.))
                .whitespace_nowrap()
                .cursor_pointer()
                .hover(|d| d.bg(gpui_kit::rgba(0xffffff1a)))
                .child(label)
                .on_click(cx.listener(move |view, _, _, cx| action(view, cx))),
        );
    }
    banner.into_any_element()
}

/// A session that is not connected: the same header as a live one (`.session-panel-header`)
/// with its status icon and no add button, then its banner or status line.
fn session_panel(
    id: &ConnectionId,
    name: &str,
    icon: SessionIcon,
    banner: Option<AnyElement>,
    status: Option<AnyElement>,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let c = colors(cx);
    let (secondary, primary, accent, hover, border) = (c.text_secondary, c.text_primary, c.accent, c.bg_hover, c.border);
    let icon = session_icon(icon, cx);
    let open = id.clone();
    v_flex()
        .w_full()
        .border_b_1()
        .border_color(border)
        .child(
            h_flex()
                .id(SharedString::from(format!("session-name-{id}")))
                .h(px(40.))
                .px(px(16.))
                .gap(px(4.))
                .items_center()
                .border_l(px(3.))
                .border_color(accent)
                .cursor_pointer()
                .hover(move |d| d.bg(hover))
                .child(div().size(px(16.)).opacity(0.6).child(crate::ui::icon(IconName::ChevronDown, 16., primary)))
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(6.))
                        .items_center()
                        .text_size(px(11.))
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(secondary)
                        .child(icon)
                        .child(div().truncate().child(name.to_uppercase()))
                        .child(div().font_weight(gpui_kit::FontWeight::NORMAL).child("↓ …")),
                )
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.info_pending = Some(open.clone());
                    cx.notify();
                })),
        )
        .child(v_flex().pb(px(4.)).children(banner).children(status))
        .into_any_element()
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
/// `.workspace-header`: the name (editable) with its refresh button and actions, then the
/// description row; 8px 16px padding on the secondary background.
fn workspace_header(
    connected: &mut Connected,
    workspace: &Entity<treeterm_model::workspace_store::WorkspaceStore>,
    id: &str,
    header_inset: gpui_kit::Pixels,
    operation_error: Option<String>,
    cx: &mut Context<AppView>,
) -> AnyElement {
    use gpui_kit::component::Sizable as _;
    let c = colors(cx);
    let (secondary, primary, border, bg, accent, error) =
        (c.text_secondary, c.text_primary, c.border, c.bg_secondary, c.accent, c.error);
    let file = workspace.read(cx).file().clone();
    let root = file.parent_id.as_deref().is_none_or(str::is_empty);
    let description = file.metadata.get("description").cloned().filter(|d| !d.is_empty());
    let tip = |text: &'static str| move |window: &mut Window, cx: &mut App| gpui_kit::component::tooltip::Tooltip::new(text).build(window, cx);
    // `.workspace-edit-btn`: a bare ✎ (13px, 11px beside the description).
    let edit_button = |id: &'static str, size: f32, title: &'static str, field: HeaderField, cx: &mut Context<AppView>| {
        div()
            .id(id)
            .px(px(4.))
            .py(px(2.))
            .flex_shrink_0()
            .cursor_pointer()
            .text_size(px(size))
            .text_color(secondary)
            .hover(move |d| d.text_color(primary))
            .tooltip(tip(title))
            .child("✎")
            .on_click(cx.listener(move |view, _, window, cx| view.edit_header(field, window, cx)))
    };
    let escape = cx.listener(|view, event: &gpui_kit::KeyDownEvent, _, cx| {
        if event.keystroke.key == "escape" {
            if let Some(connected) = view.current_mut() {
                connected.header_edit = None;
            }
            cx.notify();
        }
    });
    let (name_edit, description_edit) = match &connected.header_edit {
        Some(HeaderEdit::Name { input, .. }) => (Some(input.clone()), None),
        Some(HeaderEdit::Description { input, .. }) => (None, Some(input.clone())),
        None => (None, None),
    };

    let mut top = h_flex().h(px(24.)).gap(px(4.)).items_center();
    top = match name_edit {
        Some(input) => top.child(
            div()
                .w(px(240.))
                .on_key_down(escape)
                .child(gpui_kit::component::input::Input::new(&input).xsmall()),
        ),
        None => {
            let refresh = workspace.clone();
            top.child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(crate::ui::BODY))
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .text_color(primary)
                    .child(file.display_name().to_owned()),
            )
            .child(edit_button("edit-name", 13., "Edit name", HeaderField::Name, cx))
            .child(
                div()
                    .id("refresh-title")
                    .px(px(4.))
                    .py(px(2.))
                    .flex_shrink_0()
                    .cursor_pointer()
                    .tooltip(tip("Refresh title & description"))
                    .child(crate::ui::icon(IconName::RefreshCw, 12., secondary))
                    .on_click(move |_, window, cx| {
                        if let Err(error) = refresh.update(cx, |w, cx| w.refresh_labels(false, cx)) {
                            use gpui_kit::component::WindowExt as _;
                            window.push_notification(error, cx);
                        }
                    }),
            )
        }
    };

    // `.workspace-actions`, right-aligned, 8px apart.
    let mut actions = h_flex().ml_auto().gap(px(8.)).items_center().flex_shrink_0();
    actions = actions.child(traffic_badge(connected, workspace, id, cx));
    if let Some(branch) = file.git_branch.clone() {
        let copied = connected.branch_copied.as_ref().is_some_and(|(b, _)| *b == branch);
        let (fg, badge_bg): (gpui_kit::Hsla, gpui_kit::Hsla) = if copied {
            (gpui_kit::rgb(0x2ea043).into(), gpui_kit::rgba(0x00b45033).into())
        } else {
            (accent, gpui_kit::rgba(0x0078d426).into())
        };
        let (copy, path, rename_ws) = (branch.clone(), workspace.read(cx).workspace().path.clone(), workspace.clone());
        let can_rename = file.is_worktree && !root;
        use gpui_kit::component::menu::ContextMenuExt as _;
        actions = actions.child(
            crate::ui::pill(if copied { "Copied!".to_owned() } else { branch }, fg, badge_bg)
                .id("branch-badge")
                .cursor_pointer()
                .tooltip(tip("Copy branch name"))
                .on_click(cx.listener(move |view, _, _, cx| view.copy_branch(copy.clone(), cx)))
                .context_menu(move |menu, _, _| {
                    let path = path.clone();
                    let menu = menu.item(crate::ui::menu_item("Copy worktree path").on_click(move |_, _, cx| {
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(path.clone()));
                    }));
                    if can_rename {
                        let ws = rename_ws.clone();
                        menu.item(crate::ui::menu_item("Auto-rename branch").on_click(move |_, window, cx| {
                            if let Err(error) = ws.update(cx, |w, cx| w.refresh_labels(true, cx)) {
                                use gpui_kit::component::WindowExt as _;
                                window.push_notification(error, cx);
                            }
                        }))
                    } else {
                        menu
                    }
                }),
        );
    }
    if file.is_git_repo
        && let Some(controller) = connected.session.read(cx).git_controller(id).cloned()
    {
        let github = file.is_worktree && !root && file.git_branch.is_some();
        actions = actions.children(git_status_items(&controller, workspace, github, cx));
    }
    actions = actions.child(run_menu(connected, workspace, id, cx));
    actions = actions.children(merge_menu(connected, workspace, id, cx));
    if root && file.is_git_repo {
        let review = workspace.clone();
        actions = actions.child(
            crate::ui::action_button("open-review", cx)
                .tooltip(tip("Review changes and commit history"))
                .child("Review")
                .on_click(move |_, _, cx| open_review(&review, cx)),
        );
    }
    if root {
        // Electron names the workspace (not its display name) in this question.
        let (session, ws_id, label) = (connected.session.clone(), id.to_owned(), SharedString::from(file.name.clone()));
        actions = actions.child(
            crate::ui::action_button("close-workspace", cx)
                .px(px(6.))
                .text_color(secondary)
                .tooltip(tip("Close workspace"))
                .child(crate::ui::icon(IconName::X, 14., secondary))
                .on_click(move |_, window, cx| {
                    // `handleCloseWorkspace`: Electron's own wording for the header's ×.
                    let (session, ws_id) = (session.clone(), ws_id.clone());
                    crate::ui::confirm(format!("Close workspace \"{label}\"?"), window, cx, move |_, cx| {
                        session.update(cx, |s, cx| {
                            s.remove_workspace(&ws_id, treeterm_model::session_store::RemoveMode::KeepWorktreeAndBranch, cx)
                        });
                    });
                }),
        );
    }
    top = top.child(actions);

    // `.workspace-header-description-row`: 11px secondary text.
    let mut description_row = h_flex().min_h(px(18.)).items_center().text_size(px(crate::ui::SMALL)).text_color(secondary);
    description_row = match (description_edit, description) {
        (Some(input), _) => description_row.child(
            div().w_full().on_key_down(escape_description(cx)).child(gpui_kit::component::input::Textarea::new(&input).xsmall()),
        ),
        (None, Some(text)) => description_row
            .child(div().min_w_0().child(text))
            .child(edit_button("edit-description", 11., "Edit description", HeaderField::Description, cx)),
        (None, None) => description_row
            .child(div().italic().opacity(0.5).child("no description"))
            .child(edit_button("add-description", 11., "Add description", HeaderField::Description, cx)),
    };

    v_flex()
        .flex_shrink_0()
        .gap(px(4.))
        .pl(header_inset)
        .pr(px(16.))
        .py(px(8.))
        .bg(bg)
        .border_b_1()
        .border_color(border)
        .child(top)
        .child(description_row)
        .when_some(operation_error, |d, message| d.child(div().text_size(px(crate::ui::SMALL)).text_color(error).child(format!("⚠ {message}"))))
        .into_any_element()
}

fn escape_description(cx: &mut Context<AppView>) -> impl Fn(&gpui_kit::KeyDownEvent, &mut Window, &mut App) + 'static {
    cx.listener(|view, event: &gpui_kit::KeyDownEvent, _, cx| {
        if event.keystroke.key == "escape" {
            if let Some(connected) = view.current_mut() {
                connected.header_edit = None;
            }
            cx.notify();
        }
    })
}

/// The workspace's tab strip: its tabs in layout order, then "+" and the new-tab menu.
fn workspace_tab_strip(
    connected: &mut Connected,
    workspace: &Entity<WorkspaceStore>,
    workspace_id: &str,
    active: Option<String>,
    cx: &mut Context<AppView>,
) -> AnyElement {
    use std::rc::Rc;
    use crate::tab_strip::{NewTabApp, OnId, StripTab, TabStrip};
    let registry = connected.session.read(cx).registry().clone();
    let store = workspace.read(cx);
    let file = &store.workspace().file;
    let tabs: Vec<StripTab> = store
        .layout()
        .tab_order()
        .iter()
        .filter_map(|id| file.app_states.get_key_value(id))
        .map(|(tab_id, tab)| {
            let app = registry.get(&tab.application_id);
            StripTab {
                id: tab_id.clone(),
                title: tab.title.clone(),
                icon: app.map(|app| app.icon.clone()).unwrap_or_default(),
                closable: app.is_some_and(|app| app.can_close),
                activity: store.activity(tab_id),
            }
        })
        .collect();
    let apps: Vec<NewTabApp> = registry
        .new_tab_menu_for(store.is_remote())
        .map(|app| NewTabApp { id: app.id.clone(), name: app.name.clone(), icon: app.icon.clone() })
        .collect();
    let view = cx.entity();
    let on_select: OnId = {
        let (workspace, view) = (workspace.clone(), view.clone());
        Rc::new(move |tab: &str, _: &mut Window, cx: &mut App| {
            workspace.update(cx, |w, cx| w.set_active_tab(tab, cx));
            view.update(cx, |view, cx| view.focus_active_tab(cx));
        })
    };
    let on_close: OnId = {
        let workspace = workspace.clone();
        Rc::new(move |tab: &str, _: &mut Window, cx: &mut App| workspace.update(cx, |w, cx| w.remove_tab(tab, cx)))
    };
    let on_new_tab: OnId = {
        let workspace = workspace.clone();
        Rc::new(move |app: &str, _: &mut Window, cx: &mut App| {
            workspace.update(cx, |w, cx| w.add_tab(app, cx));
            view.update(cx, |view, cx| view.focus_active_tab(cx));
        })
    };
    let strip = TabStrip { tabs, active, apps, on_select, on_close, on_new_tab };
    let state = connected.tab_strips.entry(workspace_id.to_owned()).or_default();
    strip.render(workspace_id, state, cx.entity_id(), cx)
}

/// No workspace's tabs are on screen (none picked, or the one picked was removed, is loading
/// or failed): the session's Review views are hidden, so they refresh when shown again.
fn hide_reviews(connected: &Connected, cx: &mut Context<AppView>) {
    let views: Vec<Entity<crate::review_view::ReviewView>> = connected.review_views.values().cloned().collect();
    for view in views {
        view.update(cx, |v, cx| {
            v.set_shown(false, cx);
            v.set_visible(false, cx);
        });
    }
}

fn workspace_pane(
    connected: &mut Connected,
    header_inset: gpui_kit::Pixels,
    window: &mut Window,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let c = colors(cx);
    let (secondary, error) = (c.text_secondary, c.error);
    prune_terminal_views(connected, cx);
    connected.tool_views.prune(connected.session.read(cx), cx);
    // Set again below if this render shows a terminal.
    let previous_terminal = connected.shown_terminal.take();
    let ActiveView::Workspace(id) = &connected.active else {
        hide_reviews(connected, cx);
        // `.workspace-empty`.
        return centered(
            v_flex()
                .items_center()
                .child(div().mb(px(8.)).text_size(px(18.)).font_weight(gpui_kit::FontWeight::MEDIUM).text_color(colors(cx).text_primary).child("No workspace selected"))
                .child(div().text_size(px(13.)).text_color(secondary).child("Select a workspace from the sidebar or add a new one to get started.")),
        );
    };
    let id = id.clone();
    let tabs_shown =
        matches!(connected.session.read(cx).entry(&id), Some(WorkspaceEntry::Loaded(_) | WorkspaceEntry::OperationError { .. }));
    if !tabs_shown {
        hide_reviews(connected, cx);
    }
    let workspace = match connected.session.read(cx).entry(&id) {
        // The selection can outlive its workspace when another window removes it.
        None => return centered(div().text_color(secondary).child("Workspace was removed")),
        Some(WorkspaceEntry::Loading { message, output, .. }) => {
            // `.workspace-loading`: git's output while a worktree is being created.
            let tail: Vec<String> = output.iter().rev().take(200).rev().cloned().collect();
            let c = colors(cx);
            return v_flex()
                .flex_1()
                .h_full()
                .p(px(16.))
                .pt(px(38.))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .mb(px(12.))
                        .items_center()
                        .text_size(px(13.))
                        .text_color(secondary)
                        .child(
                            Spinner::new()
                                .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
                                .with_size(px(16.))
                                .color(secondary),
                        )
                        .child(message.clone()),
                )
                .child(
                    v_flex()
                        .id("workspace-loading-output")
                        .flex_1()
                        .overflow_y_scroll()
                        .p(px(12.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(c.border)
                        .bg(c.bg_secondary)
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
    let active = workspace.read(cx).shown_tab();

    let tabs = workspace_tab_strip(connected, &workspace, &id, active.clone(), cx);

    let header = workspace_header(connected, &workspace, &id, header_inset, operation_error, cx);

    let content: AnyElement = match &active {
        // An empty tabset: blank under a strip of only "+".
        None => div().size_full().into_any_element(),
        Some(tab_id) => match workspace.read(cx).tab_content(tab_id) {
            Some(TabContent::Terminal(model)) => {
                let view = connected
                    .terminal_views
                    .entry(model.entity_id())
                    .or_insert_with(|| cx.new(|cx| TerminalView::new(model.clone(), window, cx)))
                    .clone();
                show_terminal(connected, previous_terminal, model.entity_id(), &view, window, cx);
                let prompts = workspace
                    .read(cx)
                    .analyzer(tab_id)
                    .is_some()
                    .then(|| harness_prompt_buttons(&connected.session, &id, cx))
                    .flatten();
                v_flex()
                    .size_full()
                    .child(div().relative().flex_1().min_h_0().child(view).children(prompts))
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
                    .or_insert_with(|| cx.new(|cx| crate::review_view::ReviewView::new(model.clone(), workspace.clone(), window, cx)))
                    .clone();
                if std::mem::take(&mut connected.focus_pending) {
                    let focus = view.read(cx).focus_handle().clone();
                    window.focus(&focus, cx);
                }
                div().size_full().child(view).into_any_element()
            }
            Some(TabContent::Browser { page, forward }) => browser_tab(connected, &workspace, &id, tab_id, &page, forward, cx),
            Some(TabContent::WebApp { app, terminal, forward }) => {
                web_app_tab(connected, previous_terminal, &workspace, &id, tab_id, &app, terminal, forward, window, cx)
            }
            Some(
                tool @ (TabContent::GitHub
                | TabContent::WorkspaceSettings
                | TabContent::AnalyzerHistory(_)
                | TabContent::Comments(_)
                | TabContent::TtyList(_)
                | TabContent::Chat(_)
                | TabContent::PromptDebugger(_)
                | TabContent::SshUpload(_)),
            ) => {
                let session = connected.session.clone();
                let view = connected.tool_views.render(&session, &workspace, &id, tab_id, tool, window, cx);
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

    // Native webviews draw above GPUI: show only the active tab's, and none under a dialog.
    let dialog_open = {
        use gpui_kit::component::WindowExt as _;
        window.has_active_dialog(cx)
    };
    // A Web App or Browser tab shows its page only on its Browser sub-tab.
    let shown_key = active
        .clone()
        .map(|tab| (id.clone(), tab))
        .filter(|key| connected.page_subtabs.get(key).is_none_or(|sub| *sub == PageSubTab::Browser));
    let views: Vec<((String, String), Entity<crate::browser_view::BrowserView>)> =
        connected.browser_views.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (key, view) in views {
        let shown = !dialog_open && shown_key.as_ref() == Some(&key);
        view.update(cx, |v, cx| v.set_shown(shown, cx));
    }
    let shown_files = match &active {
        Some(tab) => match workspace.read(cx).tab_content(tab) {
            Some(TabContent::Files(model)) => Some(model.entity_id()),
            Some(_) | None => None,
        },
        None => None,
    };
    let file_views: Vec<(EntityId, Entity<crate::file_browser_view::FileBrowserView>)> =
        connected.file_views.iter().map(|(k, v)| (*k, v.clone())).collect();
    for (model_id, view) in file_views {
        let shown = !dialog_open && shown_files == Some(model_id);
        view.update(cx, |v, cx| v.set_shown(shown, cx));
    }
    // Review shows a favourite's file (a PDF or model in a webview) in place of the diff, and
    // refreshes when it is shown again (whatever switched to it: the tab strip, a shortcut,
    // the header's Review button, another workspace and back, a synced tab change).
    let shown_review = match &active {
        Some(tab) => match workspace.read(cx).tab_content(tab) {
            Some(TabContent::Review(model)) => Some(model.entity_id()),
            Some(_) | None => None,
        },
        None => None,
    };
    let review_views: Vec<(EntityId, Entity<crate::review_view::ReviewView>)> =
        connected.review_views.iter().map(|(k, v)| (*k, v.clone())).collect();
    for (model_id, view) in review_views {
        let visible = shown_review == Some(model_id);
        view.update(cx, |v, cx| {
            v.set_shown(!dialog_open && visible, cx);
            v.set_visible(visible, cx);
        });
    }

    v_flex()
        .flex_1()
        .h_full()
        .min_w_0()
        .child(header)
        .child(tabs)
        .child(div().flex_1().min_h_0().child(content))
        .into_any_element()
}

/// A Web App tab (`WebAppPane`): Browser and Shell sub-tabs over its page and its server's
/// terminal.
#[allow(clippy::too_many_arguments)]
/// A Web App's or Browser tab's sub-tab (`WebAppSubTab` / `BrowserSubTab`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageSubTab {
    Browser,
    Shell,
    PortForward,
}

impl PageSubTab {
    fn label(self) -> &'static str {
        match self {
            PageSubTab::Browser => "Browser",
            PageSubTab::Shell => "Shell",
            PageSubTab::PortForward => "Port Forward",
        }
    }
}

/// `.ssh-pane-subtabs`: 11px tabs with a 2px accent underline on the active one.
fn page_subtab_bar(key: &(String, String), tabs: &[PageSubTab], active: PageSubTab, cx: &mut Context<AppView>) -> AnyElement {
    let c = colors(cx);
    let (secondary, border, primary, accent) = (c.text_secondary, c.border, c.text_primary, c.accent);
    let mut bar = h_flex().pl(px(4.)).border_b_1().border_color(border);
    for &tab in tabs {
        let key = key.clone();
        let is_active = tab == active;
        bar = bar.child(
            div()
                .id(SharedString::from(format!("page-{}-{}", tab.label(), key.1)))
                .px(px(12.))
                .py(px(6.))
                .text_size(px(11.))
                .border_b(px(2.))
                .border_color(if is_active { accent } else { gpui_kit::transparent_black() })
                .text_color(if is_active { primary } else { secondary })
                .cursor_pointer()
                .hover(move |d| d.text_color(primary))
                .child(tab.label())
                .on_click(cx.listener(move |view, _, _, cx| show_page_subtab(view, &key, tab, cx))),
        );
    }
    bar.into_any_element()
}

fn show_page_subtab(view: &mut AppView, key: &(String, String), tab: PageSubTab, cx: &mut Context<AppView>) {
    if let Some(connected) = view.current_mut() {
        connected.page_subtabs.insert(key.clone(), tab);
        if tab == PageSubTab::Shell {
            connected.focus_pending = true;
        }
    }
    cx.notify();
}

/// A link under a page's waiting message or error ("View shell", "View port forward").
fn page_link(key: &(String, String), tab: PageSubTab, label: &'static str, cx: &mut Context<AppView>) -> gpui_kit::Stateful<gpui_kit::Div> {
    let key = key.clone();
    div()
        .id(SharedString::from(format!("page-link-{label}-{}", key.1)))
        .text_size(px(12.))
        .text_color(colors(cx).accent)
        .cursor_pointer()
        .child(label)
        .on_click(cx.listener(move |view, _, _, cx| show_page_subtab(view, &key, tab, cx)))
}

/// `.webapp-waiting`: a 20px spinner over a 13px message, and the links.
fn page_waiting(message: String, links: Vec<gpui_kit::Stateful<gpui_kit::Div>>, cx: &App) -> AnyElement {
    let c = colors(cx);
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap(px(12.))
        .text_size(px(13.))
        .text_color(c.text_secondary)
        .child(
            Spinner::new()
                .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
                .with_size(px(20.))
                .color(c.accent),
        )
        .child(message)
        .children(links)
        .into_any_element()
}

/// A page that failed: the message, Retry and the links.
fn page_error(message: String, retry: impl Fn(&mut App) + 'static, links: Vec<gpui_kit::Stateful<gpui_kit::Div>>, cx: &App) -> AnyElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap(px(12.))
        .text_size(px(13.))
        .child(div().px(px(8.)).text_color(gpui_kit::rgb(0xf44336)).child(message))
        .child(
            div()
                .id("webapp-retry")
                .text_size(px(12.))
                .text_color(colors(cx).accent)
                .cursor_pointer()
                .child("Retry")
                .on_click(move |_, _, cx| retry(cx)),
        )
        .children(links)
        .into_any_element()
}

/// The page's view once it is ready: made when the page's URL changes (a new forward).
fn page_view(connected: &mut Connected, key: (String, String), url: String, memory: Option<(WeakEntity<WorkspaceStore>, String)>, home: String, cx: &mut Context<AppView>) -> AnyElement {
    let stale = connected.browser_views.get(&key).is_some_and(|view| view.read(cx).url() != home);
    if stale {
        connected.browser_views.remove(&key);
    }
    let view = connected
        .browser_views
        .entry(key)
        .or_insert_with(|| {
            cx.new(|_| {
                let view = crate::browser_view::BrowserView::new(url);
                match memory {
                    Some((workspace, tab)) => view.remember_pages_in(workspace, tab, home),
                    None => view,
                }
            })
        })
        .clone();
    div().size_full().child(view).into_any_element()
}

/// `PortForwardView`: the tab's forward (`localhost:L → host:R (status)`, Restart once it
/// failed or stopped, its error) over its ssh output.
fn port_forward_pane(monitor: &Entity<treeterm_model::forward_monitor::ForwardMonitor>, restart: impl Fn(&mut App) + 'static, cx: &App) -> AnyElement {
    use treeterm_core::connection::PortForwardStatus;
    use treeterm_model::forward_monitor::ForwardState;
    let c = colors(cx);
    let (secondary, border) = (c.text_secondary, c.border);
    match monitor.read(cx).state().clone() {
        ForwardState::Missing => v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .text_size(px(13.))
            .text_color(secondary)
            .child("No port forward yet. It starts once the server is ready.")
            .into_any_element(),
        ForwardState::Live { info, log } => {
            let spec = &info.config.spec;
            let can_restart = matches!(info.status, PortForwardStatus::Error(_) | PortForwardStatus::Stopped);
            let error = match &info.status {
                PortForwardStatus::Error(error) => Some(error.to_string()),
                PortForwardStatus::Connecting | PortForwardStatus::Active | PortForwardStatus::Stopped => None,
            };
            let toolbar = h_flex()
                .flex_shrink_0()
                .gap(px(8.))
                .px(px(8.))
                .py(px(4.))
                .border_b_1()
                .border_color(border)
                .bg(c.bg_secondary)
                .text_size(px(12.))
                .child(
                    div().flex_1().min_w_0().truncate().font_family(crate::ui::MONO).child(format!(
                        "localhost:{} → {}:{} ({})",
                        spec.local_port,
                        spec.remote_host,
                        spec.remote_port,
                        info.status.as_str()
                    )),
                )
                .when(can_restart, |d| {
                    d.child(
                        div()
                            .id("forward-restart")
                            .px(px(8.))
                            .py(px(2.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(border)
                            .cursor_pointer()
                            .hover(|d| d.bg(colors(cx).bg_hover))
                            .child("Restart")
                            .on_click(move |_, _, cx| restart(cx)),
                    )
                });
            // `.ssh-pane-output`.
            let output = div()
                .id("forward-output")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(8.))
                .font_family(crate::ui::MONO)
                .text_size(px(12.))
                .text_color(secondary)
                .children(log.into_iter().map(|line| div().child(line)));
            v_flex()
                .size_full()
                .child(toolbar)
                .when_some(error, |d, error| {
                    d.child(div().px(px(8.)).py(px(4.)).text_size(px(12.)).text_color(gpui_kit::rgb(0xf44336)).child(error))
                })
                .child(output)
                .into_any_element()
        }
    }
}

/// A Browser tab (`BrowserPane`): its page, and on an ssh session a Port Forward sub-tab.
fn browser_tab(
    connected: &mut Connected,
    workspace: &Entity<WorkspaceStore>,
    workspace_id: &str,
    tab_id: &str,
    page: &Entity<treeterm_model::browser_page::BrowserPageModel>,
    forward: PageHost,
    cx: &mut Context<AppView>,
) -> AnyElement {
    use treeterm_model::browser_page::BrowserPhase;
    let key = (workspace_id.to_owned(), tab_id.to_owned());
    let sub = connected.page_subtabs.get(&key).copied().unwrap_or(PageSubTab::Browser);
    let links = |cx: &mut Context<AppView>| match &forward {
        PageHost::Local => Vec::new(),
        PageHost::Remote(_) => vec![page_link(&key, PageSubTab::PortForward, "View port forward", cx)],
    };
    let browser = match page.read(cx).phase().clone() {
        // Page memory: the tab reopens on the page it last showed (`path`).
        BrowserPhase::Ready { url, home } => {
            page_view(connected, key.clone(), url, Some((workspace.downgrade(), tab_id.to_owned())), home, cx)
        }
        BrowserPhase::Forwarding { port } => page_waiting(format!("Starting port forward for port {port}…"), links(cx), cx),
        BrowserPhase::Error(message) => {
            let retry = page.clone();
            page_error(message, move |cx| retry.update(cx, |page, cx| page.retry(cx)), links(cx), cx)
        }
    };
    let monitor = match forward {
        // Local sessions open the page directly: nothing else to show.
        PageHost::Local => return browser,
        PageHost::Remote(monitor) => monitor,
    };
    let bar = page_subtab_bar(&key, &[PageSubTab::Browser, PageSubTab::PortForward], sub, cx);
    let body = match sub {
        PageSubTab::PortForward => {
            let restart = page.clone();
            port_forward_pane(&monitor, move |cx| restart.update(cx, |page, cx| page.restart_forward(cx)), cx)
        }
        PageSubTab::Browser | PageSubTab::Shell => browser,
    };
    v_flex().size_full().child(bar).child(div().flex_1().min_h_0().child(body)).into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn web_app_tab(
    connected: &mut Connected,
    previous_terminal: Option<EntityId>,
    workspace: &Entity<treeterm_model::workspace_store::WorkspaceStore>,
    workspace_id: &str,
    tab_id: &str,
    app: &Entity<treeterm_model::web_app::WebAppModel>,
    terminal: Option<Entity<treeterm_model::terminal::TerminalModel>>,
    forward: PageHost,
    window: &mut Window,
    cx: &mut Context<AppView>,
) -> AnyElement {
    use treeterm_model::web_app::WebAppPhase;
    let c = colors(cx);
    let secondary = c.text_secondary;
    let key = (workspace_id.to_owned(), tab_id.to_owned());
    let sub = connected.page_subtabs.get(&key).copied().unwrap_or(PageSubTab::Browser);
    let tabs: &[PageSubTab] = match &forward {
        PageHost::Local => &[PageSubTab::Browser, PageSubTab::Shell],
        PageHost::Remote(_) => &[PageSubTab::Browser, PageSubTab::Shell, PageSubTab::PortForward],
    };
    let bar = page_subtab_bar(&key, tabs, sub, cx);
    let view_shell = |cx: &mut Context<AppView>| vec![page_link(&key, PageSubTab::Shell, "View shell", cx)];
    let waiting = |message: String, links: Vec<gpui_kit::Stateful<gpui_kit::Div>>, cx: &mut Context<AppView>| page_waiting(message, links, cx);
    let body = if let (PageSubTab::PortForward, PageHost::Remote(monitor)) = (sub, &forward) {
        let restart = app.clone();
        port_forward_pane(monitor, move |cx| restart.update(cx, |app, cx| app.restart_forward(cx)), cx)
    } else if sub == PageSubTab::Shell {
        match terminal {
            Some(model) => {
                let view = connected
                    .terminal_views
                    .entry(model.entity_id())
                    .or_insert_with(|| cx.new(|cx| TerminalView::new(model.clone(), window, cx)))
                    .clone();
                show_terminal(connected, previous_terminal, model.entity_id(), &view, window, cx);
                v_flex()
                    .size_full()
                    .child(div().flex_1().min_h_0().child(view))
                    .child(terminal_status_bar(workspace, tab_id, cx))
                    .into_any_element()
            }
            None => centered(div().text_color(secondary).child("The server starts once a port is found…")),
        }
    } else {
        match app.read(cx).phase().clone() {
            WebAppPhase::Ready { url } => page_view(connected, key.clone(), url.clone(), None, url, cx),
            WebAppPhase::AllocatingPort => waiting("Finding a free port…".to_owned(), view_shell(cx), cx),
            WebAppPhase::WaitingForServer { port } => waiting(format!("Waiting for server on port {port}…"), view_shell(cx), cx),
            WebAppPhase::Forwarding { port } => waiting(format!("Starting port forward for port {port}…"), view_shell(cx), cx),
            WebAppPhase::Error(message) => {
                let retry = app.clone();
                page_error(message, move |cx| retry.update(cx, |app, cx| app.retry(cx)), view_shell(cx), cx)
            }
        }
    };
    v_flex().size_full().child(bar).child(div().flex_1().min_h_0().child(body)).into_any_element()
}

/// The bar under a terminal: activity badge, idle-detector and width-limit switches.
/// Port of `TerminalStatusBar.tsx`; both switches persist in the tab's state.
/// The AI harness's floating prompt buttons (`.review-comments-button`), stacked above the
/// terminal's two circle buttons (16px margin + 2 × (32px + 8px gap)).
fn harness_prompt_buttons(session: &Entity<SessionStore>, workspace_id: &str, cx: &mut Context<AppView>) -> Option<AnyElement> {
    let prompts = treeterm_model::harness_prompts::harness_prompts(session.read(cx), workspace_id, cx);
    if prompts.is_empty() {
        return None;
    }
    let mut column = v_flex().absolute().right(px(16.)).bottom(px(96.)).gap(px(8.)).items_end();
    for (index, prompt) in prompts.into_iter().enumerate() {
        let (session, id, title) = (session.clone(), workspace_id.to_owned(), prompt.title());
        column = column.child(
            div()
                .id(("harness-prompt", index))
                .px(px(16.))
                .py(px(8.))
                .rounded(px(6.))
                .border_1()
                .border_color(gpui_kit::rgb(0xcc8800))
                .bg(gpui_kit::rgb(0xf0a000))
                .text_color(gpui_kit::rgb(0x1e1e1e))
                .text_size(px(13.))
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .whitespace_nowrap()
                .opacity(0.9)
                .cursor_pointer()
                .hover(|d| d.bg(gpui_kit::rgb(0xffb820)).border_color(gpui_kit::rgb(0xf0a000)).opacity(1.))
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(title).build(window, cx))
                .child(prompt.label())
                .on_click(move |_, window, cx| {
                    let task = treeterm_model::harness_prompts::send(&prompt, &session, &id, cx);
                    let handle = window.window_handle();
                    cx.spawn(async move |cx| {
                        if let Err(error) = task.await {
                            match handle.update(cx, |_, window, cx| {
                                use gpui_kit::component::WindowExt as _;
                                window.push_notification(error, cx);
                            }) {
                                Ok(()) | Err(_) => {} // Err: the window closed
                            }
                        }
                    })
                    .detach();
                }),
        );
    }
    Some(column.into_any_element())
}

fn terminal_status_bar(
    workspace: &Entity<treeterm_model::workspace_store::WorkspaceStore>,
    tab_id: &str,
    cx: &mut Context<AppView>,
) -> AnyElement {
    use gpui_kit::component::Sizable as _;
    let store = workspace.read(cx);
    let activity = store.activity(tab_id);
    // AI-harness tabs: the analyzer's state, whether it is classifying, and why.
    let analysis = store.analyzer(tab_id).map(|analyzer| {
        let a = analyzer.read(cx).analyzer();
        (a.ai_state(), a.analyzing(), a.reason().to_owned(), a.auto_approve())
    });
    let idle_enabled = !store.tab_flag(tab_id, "idleDetectorDisabled");
    let width_enabled = !store.tab_flag(tab_id, "widthLimitDisabled");
    let max_cols = store.max_cols();
    let (idle_ws, idle_tab) = (workspace.clone(), tab_id.to_owned());
    let (width_ws, width_tab) = (workspace.clone(), tab_id.to_owned());
    let (approve_ws, approve_tab) = (workspace.clone(), tab_id.to_owned());
    // Right-click on the badge: the debug menu (`ActivityStateBadge`).
    let (debug_ws, debug_tab) = (workspace.clone(), tab_id.to_owned());
    let debug_menu = move |menu: gpui_kit::component::menu::PopupMenu, _: &mut Window, _: &mut gpui_kit::Context<gpui_kit::component::menu::PopupMenu>| {
        let (prompt_ws, prompt_tab) = (debug_ws.clone(), debug_tab.clone());
        let (history_ws, history_tab) = (debug_ws.clone(), debug_tab.clone());
        menu.item(crate::ui::menu_item("Debug System Prompt").on_click(move |_, _, cx| {
            prompt_ws.update(cx, |w, cx| w.open_prompt_debugger_for_tab(&prompt_tab, cx));
        }))
        .item(crate::ui::menu_item("History").on_click(move |_, _, cx| {
            history_ws.update(cx, |w, cx| w.open_activity_history(&history_tab, cx));
        }))
    };
    let (badge_state, analyzing, reason, auto_approve) = match analysis {
        Some((state, analyzing, reason, auto_approve)) => (state, analyzing, Some(reason), Some(auto_approve)),
        None => (activity, false, None, None),
    };
    // `.terminal-status-bar`: 28px, 8px padding, 12px apart.
    h_flex()
        .h(px(28.))
        .flex_shrink_0()
        .px(px(8.))
        .gap(px(12.))
        .items_center()
        .bg(gpui_kit::rgb(0x1e1e1e))
        .border_t_1()
        .border_color(gpui_kit::rgb(0x333333))
        .text_size(px(11.))
        .child({
            use gpui_kit::component::menu::ContextMenuExt as _;
            // `.activity-state-badge`; the analyzer's reason is its tooltip.
            let reason = reason.filter(|r| !r.is_empty());
            div()
                .id(SharedString::from(format!("activity-badge-{tab_id}")))
                .flex()
                .items_center()
                .px(px(8.))
                .py(px(2.))
                .rounded(px(12.))
                .bg(gpui_kit::rgb(badge_state.color()))
                .opacity(0.8)
                .text_color(gpui_kit::white())
                .when(analyzing, |d| {
                    d.child(
                        div().mr(px(4.)).child(
                            Spinner::new()
                                .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
                                .with_size(px(9.))
                                .color(gpui_kit::white()),
                        ),
                    )
                })
                .child(badge_state.label())
                .when_some(reason, |d, reason| {
                    d.tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(reason.clone()).build(window, cx))
                })
                .context_menu(debug_menu)
        })
        .when_some(auto_approve, |bar, checked| {
            bar.child(crate::ui::toggle(SharedString::from(format!("approve-switch-{tab_id}")), checked, "Auto-approve safe", move |checked, _, cx| {
                approve_ws.update(cx, |w, cx| w.set_auto_approve(&approve_tab, checked, cx));
            }))
        })
        .child(crate::ui::toggle(SharedString::from(format!("idle-switch-{tab_id}")), idle_enabled, "Idle detector", move |checked, _, cx| {
            idle_ws.update(cx, |w, cx| w.set_idle_detector_enabled(&idle_tab, checked, cx));
        }))
        .child(crate::ui::toggle_owned(SharedString::from(format!("width-switch-{tab_id}")), width_enabled, format!("{max_cols} col limit"), move |checked, _, cx| {
            width_ws.update(cx, |w, cx| w.set_width_limit_enabled(&width_tab, checked, cx));
        }))
        .into_any_element()
}

/// Show the terminal of `model` in the workspace pane. Electron mounts only the shown tab's
/// terminal, so one not shown in the last render comes into view as a mount does (it fits
/// again and takes keyboard focus): after its tab is added or picked here or in another
/// window, a workspace switch, or the shown tab closing or its program exiting. A tab or
/// workspace switch that asked for focus also focuses it when it stays shown.
fn show_terminal(
    connected: &mut Connected,
    previous: Option<EntityId>,
    model: EntityId,
    view: &Entity<TerminalView>,
    window: &mut Window,
    cx: &mut Context<AppView>,
) {
    connected.shown_terminal = Some(model);
    if previous != Some(model) {
        view.update(cx, |view, cx| view.on_shown(window, cx));
    }
    if std::mem::take(&mut connected.focus_pending) {
        let focus = view.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
    }
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
        let sessions: Vec<_> = self
            .store
            .read(cx)
            .sessions()
            .map(|slot| {
                let state = match &slot.state {
                    ConnState::Connecting(_) => "connecting".to_owned(),
                    ConnState::Connected(_) => "connected".to_owned(),
                    ConnState::Reconnecting { attempt, error } => format!("reconnecting ({attempt}): {error}"),
                    ConnState::Failed { error, .. } => format!("failed: {error}"),
                };
                json!({ "id": slot.id.as_str(), "name": self.names.read(cx).name(&slot.target), "state": state })
            })
            .collect();
        // The dialogs GPUI Kit shows, and which of this view's own are up.
        let dialogs = {
            use gpui_kit::component::WindowExt as _;
            let active_processes = match &self.processes_dialog {
                ProcessesDialog::Closed => "closed",
                ProcessesDialog::Pending => "pending",
                ProcessesDialog::Open(view) if view.upgrade().is_some() => "open",
                ProcessesDialog::Open(_) => "closed",
            };
            json!({
                "open": window.active_dialog_count(cx),
                "settings": self.settings_dialog.upgrade().is_some(),
                "activeProcesses": active_processes,
                "closeConfirm": self.close_confirm.strong_count() > 0,
            })
        };
        match self.current() {
            None => json!({ "body": "starting", "sessions": sessions, "dialogs": dialogs }),
            Some(connected) => {
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
                    "sessions": sessions,
                    "version": session.version(),
                    "workspaces": workspaces,
                    "active": active,
                    "terminals": terminals,
                    "dialogs": dialogs,
                })
            }
        }
    }

    pub fn automation_select_workspace(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(connected) = self.current() {
            let id = id.to_owned();
            connected.tree.update(cx, |tree, cx| tree.select(id, cx));
        }
    }

    pub fn automation_select_tab(&mut self, workspace: &str, tab: &str, cx: &mut Context<Self>) {
        if let Some(connected) = self.current_mut() {
            if let Some(store) = connected.session.read(cx).workspace_store(workspace).cloned() {
                store.update(cx, |w, cx| w.set_active_tab(tab, cx));
            }
            connected.focus_pending = true;
            cx.notify();
        }
    }

    pub fn automation_new_tab(&mut self, workspace: &str, app: &str, cx: &mut Context<Self>) -> Option<String> {
        let connected = self.current_mut()?;
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
        self.current().map(|connected| connected.session.clone())
    }

    /// "+" on the current session: the Open Workspace dialog.
    pub fn automation_open_workspace_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(connected) = self.current() else {
            return false;
        };
        connected.tree.update(cx, |tree, cx| tree.open_workspace(window, cx));
        true
    }

    pub fn automation_open_workspace(&mut self, path: String, cx: &mut Context<Self>) -> Option<String> {
        let connected = self.current()?;
        let id = connected.session.update(cx, |s, cx| s.open_workspace(path, cx));
        connected.tree.update(cx, |tree, cx| tree.select(id.clone(), cx));
        Some(id)
    }

    /// Open `path` in the active workspace's active Files tab.
    pub fn automation_open_file(&mut self, path: &str, cx: &mut Context<Self>) -> bool {
        let Some(store) = self.active_store(cx) else {
            return false;
        };
        let Some(active) = store.read(cx).shown_tab() else {
            return false;
        };
        let Some(TabContent::Files(model)) = store.read(cx).tab_content(&active) else {
            return false;
        };
        model.update(cx, |m, cx| m.open(path, false, cx));
        true
    }

    pub fn automation_close_tab(&mut self, workspace: &str, tab: &str, cx: &mut Context<Self>) {
        if let Some(connected) = self.current()
            && let Some(store) = connected.session.read(cx).workspace_store(workspace).cloned()
        {
            store.update(cx, |w, cx| w.remove_tab(tab, cx));
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::{TestAppContext, VisualTestContext, WindowOptions};
    use treeterm_model::settings_store::SettingsStore;

    use super::*;

    struct Empty;

    impl Render for Empty {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    fn window(cx: &mut TestAppContext) -> &mut VisualTestContext {
        let window = cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::install(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Empty)).expect("open a window").0
        });
        VisualTestContext::from_window(window, cx).into_mut()
    }

    /// `AppView` opens Settings, and the close confirmation, only while its handle to the
    /// last one is dead: the handle must live exactly as long as the dialog.
    #[gpui_kit::test]
    fn the_settings_handle_lives_while_its_dialog_is_open(cx: &mut TestAppContext) {
        let cx = window(cx);
        let store = cx.update(|_, cx| cx.new(|_| SettingsStore::in_memory(Default::default())));
        let settings = cx.update(|window, cx| crate::settings_view::open(store, window, cx).downgrade());
        cx.run_until_parked();
        assert!(settings.upgrade().is_some(), "open");

        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
        assert!(settings.upgrade().is_none(), "closed with Escape");
    }

    #[gpui_kit::test]
    fn the_close_confirmation_handle_lives_while_its_dialog_is_open(cx: &mut TestAppContext) {
        let cx = window(cx);
        let unmerged = vec![("child".to_owned(), Some("child".to_owned()))];
        let confirm = cx.update(|window, cx| crate::active_processes_view::confirm_close(unmerged, window, cx));
        cx.run_until_parked();
        assert_eq!(confirm.strong_count(), 1, "open");

        // What its Cancel button does.
        cx.update(|window, cx| window.close_dialog(cx));
        cx.run_until_parked();
        assert_eq!(confirm.strong_count(), 0, "closed");
    }
}
