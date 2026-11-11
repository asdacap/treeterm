//! Window content: daemon start-up screen, then tree pane + workspace pane.

use std::collections::HashMap;

use gpui_kit::component::{h_flex, spinner::Spinner, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, Entity, EntityId, SharedString, Subscription, Window, div, px};
use treeterm_core::keybinding::{Action, PrefixAction};
use indexmap::IndexMap;
use treeterm_core::connection::ConnectionId;
use treeterm_model::app_store::{AppStore, ConnState};
use treeterm_model::keybinding_store::KeybindingStore;
use treeterm_model::session_store::{SessionStore, WorkspaceEntry};
use treeterm_model::workspace_store::{ActivityStyle as _, TabContent};

use crate::terminal_view::TerminalView;
use crate::theme::colors;
use crate::tree_pane::{TreePane, WorkspaceSelected};

pub struct AppView {
    keybindings: Entity<KeybindingStore>,
    settings: Entity<treeterm_model::settings_store::SettingsStore>,
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
    /// Web App tabs showing their shell instead of their page, by (workspace, tab).
    web_app_shell: std::collections::HashSet<(String, String)>,
    /// Focus the shown terminal on the next render (after a workspace or tab switch).
    focus_pending: bool,
    _selection: Subscription,
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&store, |view, store, cx| view.sync(&store, cx)).detach();
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
            focus: cx.focus_handle(),
            store: store.clone(),
            sessions: IndexMap::new(),
            active: None,
            info: None,
            info_pending: None,
            open_on_start: None,
            sidebar_collapsed: false,
        };
        view.sync(&store, cx);
        view
    }

    /// Follow the store: a view per connected session (rebuilt for each new connection
    /// generation), none for the others.
    fn sync(&mut self, store: &Entity<AppStore>, cx: &mut Context<Self>) {
        let wanted: Vec<(ConnectionId, Entity<SessionStore>, SharedString, bool)> = store
            .read(cx)
            .sessions()
            .filter_map(|slot| match &slot.state {
                ConnState::Connected(session) => Some((slot.id.clone(), session.clone(), slot.name().into(), !slot.is_local())),
                ConnState::Connecting(_) | ConnState::Reconnecting { .. } | ConnState::Failed { .. } => None,
            })
            .collect();
        self.sessions.retain(|id, connected| wanted.iter().any(|(w, session, ..)| w == id && *session == connected.session));
        for (id, session, name, remote) in wanted {
            if !self.sessions.contains_key(&id) {
                let connected = self.connect(id.clone(), session, name, remote, cx);
                self.sessions.insert(id, connected);
            }
        }
        // Keep sidebar order.
        let order: Vec<ConnectionId> = store.read(cx).sessions().map(|slot| slot.id.clone()).collect();
        self.sessions.sort_by(|a, _, b, _| {
            let rank = |id: &ConnectionId| order.iter().position(|o| o == id).unwrap_or(usize::MAX);
            rank(a).cmp(&rank(b))
        });
        if self.active.as_ref().is_none_or(|id| !self.sessions.contains_key(id)) {
            self.active = self.sessions.keys().next().cloned();
        }
        self.open_pending_workspace(cx);
        cx.notify();
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
        let local = self.store.read(cx).sessions().find(|slot| slot.is_local()).map(|slot| slot.id.clone());
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
        name: SharedString,
        remote: bool,
        cx: &mut Context<Self>,
    ) -> Connected {
        cx.observe(&session, |_, _, cx| cx.notify()).detach();
        let keybindings = self.keybindings.clone();
        let tree = cx.new(|cx| TreePane::new(session.clone(), keybindings, name, remote, cx));
        let info_id = id.clone();
        cx.subscribe(&tree, move |view, _, _: &crate::tree_pane::SessionInfoRequested, cx| {
            view.info_pending = Some(info_id.clone());
            cx.notify();
        })
        .detach();
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
            web_app_shell: std::collections::HashSet::new(),
            run_actions: HashMap::new(),
            focus_pending: false,
            _selection: selection,
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

    /// The collapsed sidebar: one initial per workspace of every connected session, with its
    /// activity glyph; click to show it.
    fn rail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (bg, border, hover, active, secondary) = (c.bg_secondary, c.border, c.bg_hover, c.bg_active, c.text_secondary);
        let mut rail = v_flex().gap_1().items_center();
        for (conn_id, connected) in &self.sessions {
            let session = connected.session.read(cx);
            let shown = match (&self.active, &connected.active) {
                (Some(active_id), ActiveView::Workspace(id)) if active_id == conn_id => Some(id.clone()),
                (Some(_), ActiveView::Workspace(_)) | (_, ActiveView::Nothing) | (None, ActiveView::Workspace(_)) => None,
            };
            for row in session.tree_rows(&Default::default(), cx) {
                let Some(store) = session.workspace_store(&row.id) else { continue };
                let workspace = store.read(cx);
                let name = workspace.file().display_name().to_owned();
                let initial: String = name.chars().next().map(|ch| ch.to_uppercase().collect()).unwrap_or_default();
                let activity = workspace.overall_activity();
                let is_shown = shown.as_deref() == Some(row.id.as_str());
                let (tree, id) = (connected.tree.clone(), row.id.clone());
                rail = rail.child(
                    div()
                        .id(SharedString::from(format!("rail-{}", row.id)))
                        .size(px(28.))
                        .rounded_md()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .cursor_pointer()
                        .when(is_shown, |d| d.bg(active))
                        .when(!is_shown, |d| d.hover(move |d| d.bg(hover)))
                        .child(if activity == treeterm_model::workspace_store::ActivityState::Idle {
                            div().child(initial).into_any_element()
                        } else {
                            div().text_color(gpui_kit::rgb(activity.color())).child(activity.icon()).into_any_element()
                        })
                        .on_click(move |_, _, cx| tree.update(cx, |tree, cx| tree.select(id.clone(), cx))),
                );
            }
        }
        v_flex()
            .h_full()
            .w(px(40.))
            .flex_shrink_0()
            .bg(bg)
            .border_r_1()
            .border_color(border)
            .pt(px(38.))
            .gap_2()
            .items_center()
            .child(
                div()
                    .id("expand-sidebar")
                    .px_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_color(secondary)
                    .hover(move |d| d.bg(hover))
                    .child("»")
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.sidebar_collapsed = false;
                        cx.notify();
                    })),
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
        let (bg, secondary, error, hover, border) = (c.bg_secondary, c.text_secondary, c.error, c.bg_hover, c.border);
        let slots: Vec<(ConnectionId, String, ConnState, bool)> = self
            .store
            .read(cx)
            .sessions()
            .map(|slot| (slot.id.clone(), slot.name(), slot.state.clone(), slot.is_local()))
            .collect();
        let mut list = v_flex().w_full();
        for (id, name, state, local) in slots {
            let banner = |text: String, color| div().px_3().py_1().text_xs().text_color(color).child(text);
            let button = |label: &'static str, key: String| {
                gpui_kit::component::button::Button::new(SharedString::from(key)).label(label).xsmall()
            };
            use gpui_kit::component::Sizable as _;
            use gpui_kit::component::button::ButtonVariants as _;
            let section = match state {
                ConnState::Connected(_) => match self.sessions.get(&id) {
                    Some(connected) => connected.tree.clone().into_any_element(),
                    None => banner(format!("{name}: starting…"), secondary).into_any_element(),
                },
                ConnState::Connecting(phase) => {
                    let what = match phase {
                        treeterm_core::connection::ConnectPhase::Bootstrap => "Bootstrapping…",
                        treeterm_core::connection::ConnectPhase::Tunnel => "Establishing tunnel…",
                        treeterm_core::connection::ConnectPhase::Daemon if local => "Starting daemon…",
                        treeterm_core::connection::ConnectPhase::Daemon => "Connecting to daemon…",
                    };
                    v_flex()
                        .child(session_name(&id, name.clone(), hover, cx))
                        .child(h_flex().px_3().gap_2().text_xs().text_color(secondary).child(Spinner::new()).child(what))
                        .into_any_element()
                }
                ConnState::Reconnecting { attempt, error: reason } => {
                    let (now, stop) = (id.clone(), id.clone());
                    v_flex()
                        .child(session_name(&id, name.clone(), hover, cx))
                        .child(banner(format!("Reconnecting (attempt {attempt})… {reason}"), secondary))
                        .child(
                            h_flex()
                                .px_3()
                                .gap_1()
                                .child(button("Retry now", format!("retry-{id}")).ghost().on_click(cx.listener(
                                    move |view, _, _, cx| view.store.read(cx).reconnect_now(&now),
                                )))
                                .child(button("Stop", format!("stop-{id}")).ghost().on_click(cx.listener(
                                    move |view, _, _, cx| view.store.read(cx).cancel_reconnect(&stop),
                                ))),
                        )
                        .into_any_element()
                }
                ConnState::Failed { error: reason, hash_mismatch } => {
                    let (again, gone) = (id.clone(), id.clone());
                    let mut actions = h_flex().px_3().gap_1().child(
                        button("Reconnect", format!("reconnect-{id}")).ghost().on_click(cx.listener(
                            move |view, _, _, cx| view.store.read(cx).reconnect(&again),
                        )),
                    );
                    if hash_mismatch {
                        let (refresh, accept) = (id.clone(), id.clone());
                        actions = actions
                            .child(button("Refresh daemon", format!("refresh-{id}")).ghost().on_click(cx.listener(
                                move |view, _, _, cx| view.reconnect_outdated(&refresh, treeterm_core::connection::OutdatedDaemon::Refresh, cx),
                            )))
                            .child(button("Connect anyway", format!("accept-{id}")).ghost().on_click(cx.listener(
                                move |view, _, _, cx| view.reconnect_outdated(&accept, treeterm_core::connection::OutdatedDaemon::Accept, cx),
                            )));
                    }
                    if !local {
                        actions = actions.child(button("Disconnect", format!("disconnect-{id}")).ghost().on_click(
                            cx.listener(move |view, _, _, cx| view.store.update(cx, |s, cx| s.disconnect(&gone, cx))),
                        ));
                    }
                    v_flex()
                        .child(session_name(&id, name.clone(), hover, cx))
                        .child(banner(reason, error))
                        .child(actions)
                        .into_any_element()
                }
            };
            list = list.child(div().pb_2().child(section));
        }
        v_flex()
            .h_full()
            .w(px(250.))
            .flex_shrink_0()
            .bg(bg)
            .border_r_1()
            .border_color(border)
            // macOS traffic lights sit over the top 38px, as in Electron's `.tree-pane-content`.
            .pt(px(38.))
            .child(
                h_flex()
                    .px_3()
                    .pb_2()
                    .justify_between()
                    .text_xs()
                    .text_color(secondary)
                    .child("SESSIONS")
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("collapse-sidebar")
                            .px_1()
                            .mr_2()
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(move |d| d.bg(hover))
                            .child("«")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.sidebar_collapsed = true;
                                cx.notify();
                            })),
                    )
                    .child({
                        let on = self.settings.read(cx).settings().notifications.sound_enabled;
                        div()
                            .id("sound-toggle")
                            .px_1()
                            .mr_2()
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(move |d| d.bg(hover))
                            .child(if on { "🔔" } else { "🔕" })
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.settings.update(cx, |store, cx| {
                                    store.update(|s| s.notifications.sound_enabled = !on, cx)
                                });
                            }))
                    })
                    .child(
                        div()
                            .id("connect-ssh")
                            .px_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(move |d| d.bg(hover))
                            .child("⇄ SSH")
                            .on_click(cx.listener(|view, _, window, cx| {
                                crate::connection_picker::open(view.store.clone(), window, cx);
                            })),
                    ),
            )
            .children(self.favourites(cx))
            .child(div().id("sessions").flex_1().overflow_y_scroll().child(list))
            .into_any_element()
    }

    /// Favourite workspaces of every connected session (`FavouritesPanel`); none, no panel.
    fn favourites(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let c = colors(cx);
        let (secondary, hover, star) = (c.text_secondary, c.bg_hover, gpui_kit::rgb(0xe5c07b));
        let mut rows = Vec::new();
        for connected in self.sessions.values() {
            let session = connected.session.read(cx);
            for id in session.workspace_ids() {
                let Some(store) = session.workspace_store(id) else { continue };
                let workspace = store.read(cx);
                if workspace.is_favourite() {
                    rows.push((id.clone(), workspace.file().display_name().to_owned(), connected.tree.clone()));
                }
            }
        }
        if rows.is_empty() {
            return None;
        }
        let mut panel = v_flex()
            .pb_2()
            .child(h_flex().px_3().pb_1().gap_1().text_xs().text_color(secondary).child(div().text_color(star).child("★")).child("FAVOURITES"));
        for (id, name, tree) in rows {
            panel = panel.child(
                div()
                    .id(SharedString::from(format!("fav-{id}")))
                    .px_3()
                    .py(px(2.))
                    .text_sm()
                    .truncate()
                    .cursor_pointer()
                    .hover(move |d| d.bg(hover))
                    .child(name)
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
            let app = self.store.clone();
            self.info = Some(cx.new(|cx| crate::session_info_view::SessionInfoView::new(app, id, window, cx)));
        }
        if let Some(info) = &self.info {
            let id = info.read(cx).connection(cx);
            if self.store.read(cx).session_slot(&id).is_none() {
                self.info = None;
            }
        }
    }

    /// Connect again to an outdated remote daemon, refreshing it or accepting it.
    fn reconnect_outdated(
        &mut self,
        id: &ConnectionId,
        outdated: treeterm_core::connection::OutdatedDaemon,
        cx: &mut Context<Self>,
    ) {
        let config = match self.store.read(cx).session_slot(id).map(|slot| slot.target.clone()) {
            Some(treeterm_core::connection::ConnectionTarget::Remote(config)) => (*config).clone(),
            Some(treeterm_core::connection::ConnectionTarget::Local) | None => return,
        };
        self.store.update(cx, |s, cx| s.connect_remote(config, outdated, cx));
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (bg, fg, secondary, error) = (c.bg_primary, c.text_primary, c.text_secondary, c.error);
        self.sync_info(window, cx);
        let sidebar = self.sidebar(cx);
        let info = self.info.clone();
        let collapsed = self.sidebar_collapsed && cfg!(target_os = "macos");
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
        // A merge's live output stays over the right side until dismissed.
        let merge = self
            .current()
            .and_then(|c| c.session.read(cx).merge_output().cloned().map(|output| (c.session.clone(), output)));
        let pane = div()
            .relative()
            .flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .child(pane)
            .when_some(merge, |d, (session, output)| d.child(merge_panel(session, output, cx)));
        let content = h_flex().size_full().child(sidebar).child(pane).into_any_element();
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
            .on_action(cx.listener(|view, _: &crate::menus::ActiveProcesses, window, cx| {
                if let Some(connected) = view.current() {
                    let settings = view.settings.read(cx).settings().terminal.clone();
                    crate::active_processes_view::open(&connected.session, settings, window, cx);
                }
            }))
            .on_action(cx.listener(|view, _: &crate::menus::Quit, window, cx| {
                if view.request_close(window, cx) {
                    cx.quit();
                }
            }))
            .child(content)
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
        crate::active_processes_view::confirm_close(unmerged, window, cx);
        false
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
            Action::Prefix(PrefixAction::OpenSettings) => {
                crate::settings_view::open(self.settings.clone(), window, cx);
            }
        }
    }
}

/// `MergeAbandonButton` for a child worktree: "Review & Merge" (opens Review), or "Abandon"
/// when its branch has nothing to merge; the menu has the merge and abandon variants.
fn merge_menu(
    connected: &Connected,
    workspace: &Entity<treeterm_model::workspace_store::WorkspaceStore>,
    workspace_id: &str,
    cx: &mut Context<AppView>,
) -> Option<AnyElement> {
    use gpui_kit::component::Sizable as _;
    use gpui_kit::component::button::{Button, ButtonVariants as _};
    use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
    use treeterm_core::git_status::ParentDiff;
    use treeterm_model::session_store::{AfterMerge, RemoveMode};
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
    Some(
        Button::new(SharedString::from(format!("merge-{workspace_id}")))
            .xsmall()
            .ghost()
            .label(if clean { "Abandon ▾" } else { "Review & Merge ▾" })
            .dropdown_menu(move |menu, _window, _cx| {
                let review = ws.clone();
                let merge = |menu: gpui_kit::component::menu::PopupMenu, text: &'static str, squash: bool, after: AfterMerge| {
                    let (session, id, label) = (session.clone(), id.clone(), label.clone());
                    menu.item(PopupMenuItem::new(text).on_click(move |_, window, cx| {
                        crate::tree_pane::confirm_merge(session.clone(), id.clone(), label.clone(), squash, after, window, cx)
                    }))
                };
                let remove = |menu: gpui_kit::component::menu::PopupMenu, text: &'static str, mode: RemoveMode| {
                    let (session, id, label) = (session.clone(), id.clone(), label.clone());
                    menu.item(PopupMenuItem::new(text).on_click(move |_, window, cx| {
                        crate::tree_pane::confirm_remove(session.clone(), id.clone(), label.clone(), mode, window, cx)
                    }))
                };
                let menu = menu.item(PopupMenuItem::new("Open Review").on_click(move |_, _, cx| {
                    review.update(cx, |w, cx| {
                        let existing = w
                            .file()
                            .app_states
                            .iter()
                            .find(|(_, tab)| tab.application_id == "review")
                            .map(|(id, _)| id.clone());
                        match existing {
                            Some(tab) => w.set_active_tab(&tab, cx),
                            None => {
                                w.add_tab("review", cx);
                            }
                        }
                    });
                }));
                let menu = merge(menu.separator(), "Merge into parent and remove…", false, AfterMerge::Remove);
                let menu = merge(menu, "Squash-merge into parent and remove…", true, AfterMerge::Remove);
                let menu = merge(menu, "Merge into parent, keep workspace…", false, AfterMerge::Keep);
                let menu = remove(menu.separator(), "Abandon (delete worktree and branch)…", RemoveMode::DeleteWorktreeAndBranch);
                let menu = remove(menu, "Abandon, keep branch…", RemoveMode::KeepBranch);
                remove(menu, "Abandon, keep worktree and branch…", RemoveMode::KeepWorktreeAndBranch)
            })
            .into_any_element(),
    )
}

/// "▶ Run" (`RunActionDropdown`): the workspace's detected actions by source, then the
/// custom runners.
fn run_menu(
    connected: &mut Connected,
    workspace: &Entity<treeterm_model::workspace_store::WorkspaceStore>,
    workspace_id: &str,
    cx: &mut Context<AppView>,
) -> AnyElement {
    use gpui_kit::component::Sizable as _;
    use gpui_kit::component::button::{Button, ButtonVariants as _};
    use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
    use treeterm_model::run_actions::{Detection, RunActions};
    let conn = connected.session.read(cx).connection().clone();
    let model = connected
        .run_actions
        .entry(workspace_id.to_owned())
        .or_insert_with(|| cx.new(|cx| RunActions::new(&conn, workspace, cx)))
        .clone();
    let runners: Vec<(String, String)> = connected
        .session
        .read(cx)
        .registry()
        .new_tab_menu()
        .filter(|app| app.id.starts_with("customrunner-"))
        .map(|app| (app.id.clone(), app.name.clone()))
        .collect();
    let error = model.read(cx).error().map(str::to_owned);
    let (menu_model, menu_workspace) = (model.clone(), workspace.clone());
    h_flex()
        .gap_1()
        .child(
            Button::new(SharedString::from(format!("run-{workspace_id}")))
                .xsmall()
                .ghost()
                .label("▶ Run")
                .dropdown_menu(move |menu, _window, cx| {
                    let mut menu = menu;
                    let detection = menu_model.read(cx).detection().clone();
                    match detection {
                        Detection::Detecting => return menu.item(PopupMenuItem::label("Loading…")),
                        Detection::Done(actions) => {
                            if actions.is_empty() && runners.is_empty() {
                                return menu.item(PopupMenuItem::label("No actions found"));
                            }
                            let mut source = None;
                            for action in actions {
                                if source != Some(action.source) {
                                    source = Some(action.source);
                                    menu = menu.item(PopupMenuItem::label(action.source.as_str()));
                                }
                                let (model, label) = (menu_model.clone(), action.name.clone());
                                menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                                    model.update(cx, |m, cx| m.run(&action, cx));
                                }));
                            }
                        }
                    }
                    if !runners.is_empty() {
                        menu = menu.separator().item(PopupMenuItem::label("Custom Runners"));
                        for (app_id, name) in &runners {
                            let (workspace, app_id) = (menu_workspace.clone(), app_id.clone());
                            menu = menu.item(PopupMenuItem::new(name.clone()).on_click(move |_, _, cx| {
                                workspace.update(cx, |w, cx| {
                                    w.add_tab(&app_id, cx);
                                });
                            }));
                        }
                    }
                    let refresh = menu_model.clone();
                    menu.separator().item(PopupMenuItem::new("Refresh").on_click(move |_, _, cx| {
                        refresh.update(cx, |m, cx| m.detect(cx));
                    }))
                }),
        )
        .when_some(error, |d, error| d.child(div().text_xs().text_color(colors(cx).error).child(error)))
        .into_any_element()
}

/// The header's git status button (`GitStatusButton`), behind count and PR link.
fn git_status_items(
    controller: &Entity<treeterm_model::git_controller::GitController>,
    cx: &mut Context<AppView>,
) -> Vec<AnyElement> {
    use treeterm_core::git_status::{ParentConflicts, PullRequest, WorkingTree};
    let c = colors(cx);
    let hover = c.bg_hover;
    let (status, refreshing, failures) = {
        let git = controller.read(cx);
        (git.status().clone(), git.is_refreshing(), git.failures().len())
    };
    let (label, color): (&str, gpui_kit::Hsla) = if refreshing {
        ("◌ Checking…", c.text_secondary)
    } else if status.parent_conflicts == ParentConflicts::Conflicting {
        ("⚠ Conflicts", gpui_kit::rgb(0xff9800).into())
    } else if status.working_tree == WorkingTree::Dirty {
        ("● Uncommitted", gpui_kit::rgb(0xe2c08d).into())
    } else {
        ("✓ Clean", gpui_kit::rgb(0x4caf50).into())
    };
    let refresh = controller.clone();
    let mut items = vec![
        div()
            .id("git-status")
            .px_2()
            .rounded_sm()
            .text_sm()
            .text_color(color)
            .cursor_pointer()
            .hover(move |d| d.bg(hover))
            .child(if failures > 0 { format!("{label} ({failures} check{} failed)", if failures > 1 { "s" } else { "" }) } else { label.to_owned() })
            .on_click(move |_, _, cx| refresh.update(cx, |git, cx| git.refresh(cx)))
            .into_any_element(),
    ];
    // `GitPullButton`: behind upstream → pull.
    let pull_state = controller.read(cx).pull_state().clone();
    if status.behind_count > 0 || pull_state != treeterm_model::git_controller::PullState::Idle {
        use treeterm_model::git_controller::PullState;
        let pull = controller.clone();
        let (label, color) = match &pull_state {
            PullState::Pulling => ("◌ Pulling…".to_owned(), c.text_secondary),
            PullState::Failed(error) => (format!("⚠ {error}"), c.error),
            PullState::Idle => (format!("↓ Pull ({} behind)", status.behind_count), c.text_secondary),
        };
        items.push(
            div()
                .id("git-pull")
                .px_2()
                .rounded_sm()
                .text_sm()
                .text_color(color)
                .cursor_pointer()
                .hover(move |d| d.bg(hover))
                .child(label)
                .on_click(move |_, _, cx| pull.update(cx, |git, cx| git.pull(cx)))
                .into_any_element(),
        );
    }
    if let PullRequest::Found(pr) = &status.pull_request {
        let url = pr.url.clone();
        let unresolved = pr.unresolved_count();
        items.push(
            div()
                .id("pr-link")
                .px_2()
                .rounded_sm()
                .text_sm()
                .text_color(c.accent)
                .cursor_pointer()
                .hover(move |d| d.bg(hover))
                .child(if unresolved > 0 { format!("PR #{} · {unresolved} unresolved", pr.number) } else { format!("PR #{}", pr.number) })
                .on_click(move |_, _, cx| cx.open_url(&url))
                .into_any_element(),
        );
    }
    items
}

/// `mergeOutput`: a merge's title, git's output so far and how it ended.
fn merge_panel(
    session: Entity<SessionStore>,
    output: treeterm_model::session_store::MergeOutput,
    cx: &mut Context<AppView>,
) -> AnyElement {
    use gpui_kit::component::Sizable as _;
    use gpui_kit::component::button::{Button, ButtonVariants as _};
    use treeterm_model::session_store::MergeState;
    let c = colors(cx);
    let (status, color) = match &output.state {
        MergeState::Running => ("Running…".to_owned(), c.text_secondary),
        MergeState::Succeeded => ("Done".to_owned(), gpui_kit::rgb(0x4caf50).into()),
        MergeState::Failed(error) => (format!("Failed: {error}"), c.error),
    };
    let finished = output.state != MergeState::Running;
    let mut lines = v_flex().id("merge-output").flex_1().min_h_0().overflow_y_scroll().px_3().py_1().font_family("Menlo").text_size(px(12.));
    for line in output.lines.iter().rev().take(200).rev() {
        lines = lines.child(div().child(line.clone()));
    }
    v_flex()
        .absolute()
        .bottom_0()
        .left_0()
        .right_0()
        .h(px(220.))
        .bg(c.bg_secondary)
        .border_t_1()
        .border_color(c.border)
        .child(
            h_flex()
                .px_3()
                .py_1()
                .gap_3()
                .items_center()
                .border_b_1()
                .border_color(c.border)
                .child(div().text_sm().child(output.title.clone()))
                .child(div().flex_1().truncate().text_sm().text_color(color).child(status))
                .when(finished, |d| {
                    d.child(Button::new("merge-output-close").xsmall().ghost().label("Close").on_click(move |_, _, cx| {
                        session.update(cx, |s, cx| s.dismiss_merge_output(cx));
                    }))
                }),
        )
        .child(lines)
        .into_any_element()
}

/// A session's name in the sidebar; clicking it opens the session pane.
fn session_name(id: &ConnectionId, name: String, hover: gpui_kit::Hsla, cx: &mut Context<AppView>) -> impl IntoElement + use<> {
    let id = id.clone();
    div()
        .id(SharedString::from(format!("session-name-{id}")))
        .px_2()
        .py_1()
        .text_sm()
        .cursor_pointer()
        .hover(move |d| d.bg(hover))
        .child(name)
        .on_click(cx.listener(move |view, _, _, cx| {
            view.info_pending = Some(id.clone());
            cx.notify();
        }))
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
fn workspace_pane(
    connected: &mut Connected,
    header_inset: gpui_kit::Pixels,
    window: &mut Window,
    cx: &mut Context<AppView>,
) -> AnyElement {
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
                .when_some(
                    Some(workspace.read(cx).activity(tab_id))
                        .filter(|state| *state != treeterm_model::workspace_store::ActivityState::Idle),
                    |d, state| {
                        d.child(div().text_xs().flex_shrink_0().text_color(gpui_kit::rgb(state.color())).child(state.icon()))
                    },
                )
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
                    if let Some(connected) = view.current_mut() {
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
        .pl(header_inset)
        .gap_3()
        .items_center()
        .border_b_1()
        .border_color(border)
        .child(div().child(file.display_name().to_owned()))
        .child(div().text_sm().text_color(secondary).child(path))
        .when_some(file.git_branch.clone(), |d, branch| {
            d.child(div().text_sm().text_color(secondary).child(format!("⎇ {branch}")))
        })
        .when_some(connected.session.read(cx).git_controller(&id).cloned(), |d, controller| {
            d.children(git_status_items(&controller, cx))
        })
        .child(run_menu(connected, &workspace, &id, cx))
        .children(merge_menu(connected, &workspace, &id, cx))
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
                    .child(terminal_status_bar(&workspace, connected.session.read(cx).git_controller(&id).cloned(), tab_id, cx))
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
            Some(TabContent::Browser { url }) => {
                let view = connected
                    .browser_views
                    .entry((id.clone(), tab_id.clone()))
                    .or_insert_with(|| cx.new(|_| crate::browser_view::BrowserView::new(url)))
                    .clone();
                div().size_full().child(view).into_any_element()
            }
            Some(TabContent::WebApp { app, terminal }) => {
                web_app_tab(connected, &workspace, &id, tab_id, &app, terminal, window, cx)
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
    // A Web App tab shows its page only on its Browser sub-tab.
    let shown_key = active
        .clone()
        .map(|tab| (id.clone(), tab))
        .filter(|key| !connected.web_app_shell.contains(key));
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
fn web_app_tab(
    connected: &mut Connected,
    workspace: &Entity<treeterm_model::workspace_store::WorkspaceStore>,
    workspace_id: &str,
    tab_id: &str,
    app: &Entity<treeterm_model::web_app::WebAppModel>,
    terminal: Option<Entity<treeterm_model::terminal::TerminalModel>>,
    window: &mut Window,
    cx: &mut Context<AppView>,
) -> AnyElement {
    use gpui_kit::component::Sizable as _;
    use gpui_kit::component::button::{Button, ButtonVariants as _};
    use treeterm_model::web_app::WebAppPhase;
    let c = colors(cx);
    let (secondary, error, border) = (c.text_secondary, c.error, c.border);
    let key = (workspace_id.to_owned(), tab_id.to_owned());
    let shell = connected.web_app_shell.contains(&key);
    let mut bar = h_flex().px_2().py_1().gap_1().border_b_1().border_color(border);
    for (label, wants_shell) in [("Browser", false), ("Shell", true)] {
        let button = Button::new(SharedString::from(format!("webapp-{label}-{tab_id}"))).xsmall().label(label);
        let button = if shell == wants_shell { button.primary() } else { button.ghost() };
        let key = key.clone();
        bar = bar.child(button.on_click(cx.listener(move |view, _, _, cx| {
            if let Some(connected) = view.current_mut() {
                if wants_shell {
                    connected.web_app_shell.insert(key.clone());
                    connected.focus_pending = true;
                } else {
                    connected.web_app_shell.remove(&key);
                }
            }
            cx.notify();
        })));
    }
    let body = if shell {
        match terminal {
            Some(model) => {
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
                    .child(terminal_status_bar(workspace, None, tab_id, cx))
                    .into_any_element()
            }
            None => centered(div().text_color(secondary).child("The server starts once a port is found…")),
        }
    } else {
        match app.read(cx).phase().clone() {
            WebAppPhase::Ready { url } => {
                let stale = connected.browser_views.get(&key).is_some_and(|view| view.read(cx).url() != url);
                if stale {
                    connected.browser_views.remove(&key);
                }
                let view = connected
                    .browser_views
                    .entry(key)
                    .or_insert_with(|| cx.new(|_| crate::browser_view::BrowserView::new(url)))
                    .clone();
                div().size_full().child(view).into_any_element()
            }
            WebAppPhase::AllocatingPort => centered(div().text_color(secondary).child("Finding a free port…")),
            WebAppPhase::WaitingForServer { port } => {
                centered(div().text_color(secondary).child(format!("Waiting for the server on port {port}…")))
            }
            WebAppPhase::Forwarding { port } => {
                centered(div().text_color(secondary).child(format!("Forwarding port {port} from the remote host…")))
            }
            WebAppPhase::Error(message) => {
                let retry = app.clone();
                centered(
                    v_flex()
                        .gap_2()
                        .items_center()
                        .child(div().text_color(error).child(message))
                        .child(Button::new("webapp-retry").small().label("Retry").on_click(move |_, _, cx| {
                            retry.update(cx, |app, cx| app.retry(cx));
                        })),
                )
            }
        }
    };
    v_flex().size_full().child(bar).child(div().flex_1().min_h_0().child(body)).into_any_element()
}

/// The bar under a terminal: activity badge, idle-detector and width-limit switches.
/// Port of `TerminalStatusBar.tsx`; both switches persist in the tab's state.
fn terminal_status_bar(
    workspace: &Entity<treeterm_model::workspace_store::WorkspaceStore>,
    git: Option<Entity<treeterm_model::git_controller::GitController>>,
    tab_id: &str,
    cx: &mut Context<AppView>,
) -> AnyElement {
    use gpui_kit::component::Sizable as _;
    use gpui_kit::component::switch::Switch;
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
    // AI-harness prompts (`PromptCommitButton`, `PromptRebaseButton`).
    let prompts: Vec<(&'static str, String)> = match (&analysis, &git) {
        (Some(_), Some(git)) => {
            use treeterm_core::git_status::{ParentConflicts, WorkingTree};
            let status = git.read(cx).status().clone();
            let file = store.file();
            let mut prompts = Vec::new();
            let conflicting = status.parent_conflicts == ParentConflicts::Conflicting;
            if status.working_tree == WorkingTree::Dirty && !conflicting {
                prompts.push(("Prompt Commit", "commit".to_owned()));
            }
            if let (true, Some(branch), Some(parent)) = (conflicting, file.git_branch.as_deref(), store.parent_branch()) {
                prompts.push(("Prompt Rebase", format!("rebase local branch {branch} onto {parent}")));
            }
            prompts
        }
        (Some(_), None) | (None, _) => Vec::new(),
    };
    let (badge_state, analyzing, reason, auto_approve) = match analysis {
        Some((state, analyzing, reason, auto_approve)) => (state, analyzing, Some(reason), Some(auto_approve)),
        None => (activity, false, None, None),
    };
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
                .bg(gpui_kit::rgb(badge_state.color()))
                .opacity(0.8)
                .text_color(gpui_kit::white())
                .child(if analyzing { format!("{} …", badge_state.label()) } else { badge_state.label().to_owned() }),
        )
        .when_some(reason.filter(|r| !r.is_empty()), |bar, reason| {
            bar.child(div().max_w(px(360.)).truncate().text_color(gpui_kit::rgb(0x858585)).child(reason))
        })
        .children(prompts.into_iter().map(|(label, text)| {
            use gpui_kit::component::button::{Button, ButtonVariants as _};
            let target = workspace.clone();
            Button::new(SharedString::from(format!("{label}-{tab_id}")))
                .xsmall()
                .ghost()
                .label(label)
                .on_click(move |_, window, cx| {
                    if let Err(error) = target.update(cx, |w, cx| w.prompt_harness(&text, cx)) {
                        use gpui_kit::component::WindowExt as _;
                        window.push_notification(error, cx);
                    }
                })
        }))
        .when_some(auto_approve, |bar, checked| {
            bar.child(
                Switch::new(SharedString::from(format!("approve-switch-{tab_id}")))
                    .checked(checked)
                    .label("Auto-approve safe")
                    .xsmall()
                    .on_click(move |checked, _, cx| {
                        approve_ws.update(cx, |w, cx| w.set_auto_approve(&approve_tab, *checked, cx));
                    }),
            )
        })
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
                json!({ "id": slot.id.as_str(), "name": slot.name(), "state": state })
            })
            .collect();
        match self.current() {
            None => json!({ "body": "starting", "sessions": sessions }),
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
        if let Some(connected) = self.current()
            && let Some(store) = connected.session.read(cx).workspace_store(workspace).cloned()
        {
            store.update(cx, |w, cx| w.remove_tab(tab, cx));
        }
    }
}
