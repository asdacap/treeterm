//! Window content: daemon start-up screen, then tree pane + workspace pane.

use std::collections::{HashMap, HashSet};

use gpui_kit::component::{h_flex, spinner::Spinner, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, Entity, EntityId, SharedString, Subscription, WeakEntity, Window, div, px};
use treeterm_core::keybinding::{Action, PrefixAction};
use treeterm_core::layout::{DropTarget, LayoutTabSet, NodePath};
use indexmap::IndexMap;
use treeterm_core::connection::ConnectionId;
use treeterm_core::wire::workspace_file::WorktreeSettings;
use treeterm_model::app_store::{AppStore, IdlePane};
use treeterm_model::keybinding_store::KeybindingStore;
use treeterm_model::navigation::AfterDisconnect;
use treeterm_model::session_names::Side;
use treeterm_model::session_store::{SessionEvent, SessionStore, WorkspaceEntry};
use treeterm_model::traffic::TrafficMeter;
use treeterm_model::workspace_store::{ActivityStyle as _, PageHost, TabContent, WorkspaceStore};

use crate::active_processes_view::CloseAnyway;
use crate::terminal_view::{Focus, TerminalView};
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
    /// Each connection's sidebar section, whatever its state.
    trees: HashMap<ConnectionId, SessionTree>,
    /// Each shown session's views, in sidebar order.
    sessions: IndexMap<ConnectionId, Connected>,
    /// The session whose workspace the right side shows (Electron's `activeView`). None
    /// until one is picked (a session restored with workspaces, a row clicked, the CLI's
    /// directory), and once the shown one is disconnected with none left: the right side then
    /// shows the [`IdlePane`].
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
    /// The divider is being dragged: it stays accent-coloured wherever the pointer is
    /// (`isResizing`, `.divider.active`).
    resizing: bool,
    /// A session being dragged in the sidebar or the rail (`dragState`).
    session_drag: SessionDrag,
    /// The Settings dialog: Settings again does not stack a second one (Electron's
    /// `isSettingsOpen`), and it reopens on the section shown last.
    settings_dialog: crate::settings_view::SettingsDialog,
    /// "SSH Connections": kept while closed, as Electron's stays mounted, so it reopens as
    /// it was left.
    connection_picker: Entity<crate::connection_picker::ConnectionPicker>,
    /// Window > Active Processes (Electron's `isActiveProcessesOpen`).
    processes_dialog: ProcessesDialog,
    /// The unmerged-workspaces confirmation while it is up: Quit or Close again does not
    /// stack a second one.
    close_confirm: std::rc::Weak<crate::active_processes_view::CloseConfirm>,
    menu_bar: MenuBar,
    /// Where the sound toggle was laid out last (window coordinates): its save status shows
    /// under it. `None` until it is first drawn.
    sound_toggle_bounds: Option<gpui_kit::Bounds<gpui_kit::Pixels>>,
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

/// Dragging a session's section to reorder the sessions (`dragState`).
enum SessionDrag {
    Idle,
    /// `dragged`'s section dims (`.dragging`); `over` is the section and side it would land
    /// on (`drag-before` / `drag-after`), none while over no other section.
    Dragging { dragged: ConnectionId, over: Option<(ConnectionId, Side)> },
}

impl SessionDrag {
    /// Whether `section` is the one dragged (`.dragging`).
    fn dims(&self, section: &ConnectionId) -> bool {
        matches!(self, SessionDrag::Dragging { dragged, .. } if dragged == section)
    }

    /// The side of `section` the dragged session would land on (`drag-before`, `drag-after`).
    fn hint(&self, section: &ConnectionId) -> Option<Side> {
        match self {
            SessionDrag::Dragging { over: Some((over, side)), .. } if over == section => Some(*side),
            SessionDrag::Dragging { .. } | SessionDrag::Idle => None,
        }
    }

    /// The pointer moved to `pointer` while `section` is laid out at `bounds`
    /// (`handleSessionDragOver`; every section hears every move): over another session's top
    /// half it would land before it, over its bottom half after it. Whether the hint changed.
    fn moved(
        &mut self,
        section: &ConnectionId,
        bounds: gpui_kit::Bounds<gpui_kit::Pixels>,
        pointer: gpui_kit::Point<gpui_kit::Pixels>,
    ) -> bool {
        let SessionDrag::Dragging { dragged, over } = self else {
            return false; // dragged from another window
        };
        let next = (bounds.contains(&pointer) && dragged != section).then(|| {
            let middle = bounds.top() + bounds.size.height / 2.;
            (section.clone(), if pointer.y < middle { Side::Before } else { Side::After })
        });
        let changed = match (&*over, &next) {
            (Some((old, _)), None) => old == section,
            (old, Some(_)) => old.as_ref() != next.as_ref(),
            (None, None) => false,
        };
        if changed {
            *over = next;
        }
        changed
    }

    /// Dropped on `onto`'s section (`handleSessionDrop`): the drag ends, and the dragged
    /// session moves to the side of it the hint showed; none when dropped on itself.
    fn drop_on(&mut self, onto: &ConnectionId) -> Option<Side> {
        match std::mem::replace(self, SessionDrag::Idle) {
            SessionDrag::Dragging { over: Some((over, side)), .. } if over == *onto => Some(side),
            SessionDrag::Dragging { .. } | SessionDrag::Idle => None,
        }
    }
}

/// A connection's sidebar section: made once, it outlives the connection's generations.
struct SessionTree {
    pane: Entity<TreePane>,
    _events: [Subscription; 3],
}

/// A favourite's row, copied out of its session so rendering can borrow the context mutably.
struct FavouriteRow {
    id: String,
    conn: ConnectionId,
    /// The workspace, for the row's menu.
    store: Entity<WorkspaceStore>,
    /// Its session's tree: the menu's Settings shows the workspace through it.
    tree: Entity<TreePane>,
    name: String,
    /// Shown on hover (`title={data.path}`).
    path: SharedString,
    is_worktree: bool,
    /// Background work finished while the workspace was not shown.
    unread: bool,
    /// An unread-marker edit is not yet saved.
    pending: bool,
    /// The attention flash's state and opacity now.
    flash: Option<(treeterm_model::workspace_store::ActivityState, f32)>,
    activity: treeterm_model::workspace_store::ActivityState,
    /// The workspace the window shows.
    is_shown: bool,
    /// `PrIndicators`: the PR's number and what it needs.
    pr: Vec<AnyElement>,
}

/// Per-window state for a session shown (a connection generation's store).
struct Connected {
    session: Entity<SessionStore>,
    /// Views of terminal models, by model, so focus and IME state survive re-renders.
    terminal_views: HashMap<EntityId, Entity<TerminalView>>,
    /// Views of Files tabs, by model.
    file_views: HashMap<EntityId, Entity<crate::file_browser_view::FileBrowserView>>,
    /// Views of Editor tabs, by model.
    editor_views: HashMap<EntityId, Entity<crate::text_editor_view::TextEditorView>>,
    /// Views of Review tabs, by model.
    review_views: HashMap<EntityId, Entity<crate::review_view::ReviewView>>,
    /// Browser views by (workspace, tab), with the opening of the page each shows: they own
    /// native webviews, hidden when not shown.
    browser_views: HashMap<(String, String), (PageMount, Entity<crate::browser_view::BrowserView>)>,
    /// Each shown workspace's Run menu actions, by workspace id.
    run_actions: HashMap<String, Entity<treeterm_model::run_actions::RunActions>>,
    /// Each shown workspace's download badge, by workspace id.
    traffic_badges: HashMap<String, Entity<crate::traffic_badge::TrafficBadge>>,
    /// Web App and Browser tabs not on their Browser sub-tab, by (workspace, tab).
    page_subtabs: HashMap<(String, String), PageSubTab>,
    /// Each shown tabset's strip scroll, by workspace id and tabset.
    tab_strips: HashMap<(String, NodePath), crate::tab_strip::TabStripState>,
    /// Each shown workspace's layout (splitters), by workspace id.
    tab_layouts: HashMap<String, crate::tab_layout_view::TabLayoutState>,
    /// Views of tool tabs (GitHub, Comments, Chat, …).
    tool_views: crate::tool_views::ToolViews,
    /// The header's in-place name or description editor.
    header_edit: Option<HeaderEdit>,
    /// The branch badge reads "Copied!" for 1.5s after a click (`BranchBadge`).
    branch_copied: Option<(String, gpui_kit::Task<()>)>,
    /// Focus the active tab's terminal (or Review) on the next render (after a workspace or
    /// tab switch).
    focus_pending: bool,
    /// The terminal models the last render showed: a terminal coming into view fits again
    /// (and, as the active tab, takes focus).
    shown_terminals: HashSet<EntityId>,
    /// The shown loading workspace's git output (`AutoScrollPre`).
    loading_output: LoadingOutput,
    _session_events: Subscription,
}

/// A loading workspace's output box, kept scrolled to its newest line (`AutoScrollPre`).
#[derive(Default)]
struct LoadingOutput {
    scroll: gpui_kit::ScrollHandle,
    /// The workspace and the number of lines the box last scrolled to the bottom of.
    seen: Option<(String, usize)>,
}

impl LoadingOutput {
    /// `workspace`'s box shows `lines` lines: when it newly shows or a line arrived, scroll to
    /// the bottom on this frame. A box scrolled up stays put until the next line.
    fn follow(&mut self, workspace: &str, lines: usize) {
        if self.seen.as_ref().is_some_and(|(shown, count)| shown == workspace && *count == lines) {
            return;
        }
        self.scroll.scroll_to_bottom();
        self.seen = Some((workspace.to_owned(), lines));
    }
}

/// `.workspace-loading`: workspace `workspace` is being created. The message by a spinner,
/// over all of git's output once there is some.
fn loading_pane(workspace: &str, message: &str, output: &[String], loading: &mut LoadingOutput, cx: &App) -> AnyElement {
    let secondary = colors(cx).text_secondary;
    loading.follow(workspace, output.len());
    v_flex()
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
                .child(message.to_owned()),
        )
        .when(!output.is_empty(), |d| d.child(loading_output(output.to_vec(), &loading.scroll, cx)))
        .into_any_element()
}

/// `.workspace-loading-output`: the lines in a scrolling monospace box.
fn loading_output(lines: Vec<String>, scroll: &gpui_kit::ScrollHandle, cx: &App) -> AnyElement {
    let c = colors(cx);
    v_flex()
        .id("workspace-loading-output")
        .flex_1()
        .overflow_y_scroll()
        .track_scroll(scroll)
        .p(px(12.))
        .rounded(px(4.))
        .border_1()
        .border_color(c.border)
        .bg(c.bg_secondary)
        .font_family("Menlo")
        .text_size(px(12.))
        .text_color(c.text_secondary)
        .children(lines.into_iter().map(|line| div().child(line)))
        .into_any_element()
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
        cx.subscribe_in(&store, window, |view, _, event: &treeterm_model::app_store::AppEvent, window, cx| match event {
            // The attention ding (`playDing`): the system bell, when sound is on.
            treeterm_model::app_store::AppEvent::Attention { .. } => {
                if view.settings.read(cx).settings().notifications.sound_enabled {
                    window.play_system_bell();
                }
            }
            // A connect started from the picker: its session pane, while it connects.
            treeterm_model::app_store::AppEvent::ShowSessionPane(id) => {
                view.info_pending = Some(id.clone());
                cx.notify();
            }
            // A session restored (`setActiveView`): its active workspace, focused.
            treeterm_model::app_store::AppEvent::ShowSession(id) => {
                if view.sessions.contains_key(id) {
                    view.show_session(id, cx);
                    if let Some(connected) = view.sessions.get_mut(id) {
                        connected.focus_pending = true;
                    }
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
            trees: HashMap::new(),
            sessions: IndexMap::new(),
            active: None,
            info: None,
            info_pending: None,
            open_on_start: None,
            sidebar_collapsed: false,
            sidebar_width: 250.,
            resizing: false,
            session_drag: SessionDrag::Idle,
            settings_dialog: crate::settings_view::SettingsDialog::default(),
            connection_picker: cx.new(|cx| crate::connection_picker::ConnectionPicker::new(store.clone(), window, cx)),
            processes_dialog: ProcessesDialog::Closed,
            close_confirm: std::rc::Weak::new(),
            menu_bar: if cfg!(target_os = "macos") {
                MenuBar::System
            } else {
                MenuBar::InWindow(gpui_kit::component::menu::AppMenuBar::new(cx))
            },
            sound_toggle_bounds: None,
        };
        view.sync(&store, cx);
        view
    }

    /// Follow the store: a sidebar section per connection, whatever its state, and a view per
    /// session shown (rebuilt for each new connection generation; a lost connection's last
    /// session stays, dimmed).
    fn sync(&mut self, store: &Entity<AppStore>, cx: &mut Context<Self>) {
        let wanted: Vec<_> = store
            .read(cx)
            .sessions()
            .map(|slot| {
                let traffic = slot.traffic().read(cx).connection().clone();
                (slot.id.clone(), slot.shown_session().cloned(), slot.target.clone(), traffic)
            })
            .collect();
        // The session the window shows: its pane, else its workspace view.
        let shown_before = match &self.info {
            Some(info) => Some(info.read(cx).connection(cx)),
            None => self.info_pending.clone().or_else(|| self.active.clone()),
        };
        self.trees.retain(|id, _| wanted.iter().any(|(w, ..)| w == id));
        self.sessions
            .retain(|id, connected| wanted.iter().any(|(w, session, ..)| w == id && session.as_ref() == Some(&connected.session)));
        for (id, session, target, traffic) in wanted {
            let pane = match self.trees.get(&id) {
                Some(tree) => tree.pane.clone(),
                None => {
                    let tree = self.new_tree(id.clone(), target, traffic, cx);
                    let pane = tree.pane.clone();
                    self.trees.insert(id.clone(), tree);
                    pane
                }
            };
            pane.update(cx, |pane, cx| pane.set_session(session.clone(), cx));
            if let Some(session) = session
                && !self.sessions.contains_key(&id)
            {
                let connected = self.connect(id.clone(), session, cx);
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
        // The shown session was disconnected (`disconnectSession`): the next one in the
        // sidebar is shown.
        if let Some(gone) = shown_before
            && store.read(cx).session_slot(&gone).is_none()
        {
            let remaining: Vec<(ConnectionId, bool)> = order
                .iter()
                .map(|id| {
                    let has_workspaces =
                        self.sessions.get(id).is_some_and(|c| c.session.read(cx).workspace_ids().next().is_some());
                    (id.clone(), has_workspaces)
                })
                .collect();
            self.info = None;
            self.info_pending = None;
            self.active = match treeterm_model::navigation::after_disconnect(&remaining) {
                AfterDisconnect::Workspace(next) => Some(next),
                AfterDisconnect::SessionPane(next) => {
                    self.info_pending = Some(next.clone());
                    Some(next)
                }
                AfterDisconnect::Nothing => None,
            };
        }
        self.apply_shown(cx);
        self.open_pending_workspace(cx);
        cx.notify();
    }

    /// Tell each session whether the window shows its workspace view (`activeView`): the
    /// session shown, unless its session pane (or another's) is up.
    fn apply_shown(&mut self, cx: &mut Context<Self>) {
        let pane = self.info.is_some() || self.info_pending.is_some();
        for (id, connected) in &self.sessions {
            let shown = !pane && self.active.as_ref() == Some(id);
            connected.session.update(cx, |s, cx| s.set_shown(shown, cx));
        }
    }

    /// Show `id`'s workspace view on the right (closing a session pane).
    fn show_session(&mut self, id: &ConnectionId, cx: &mut Context<Self>) {
        self.info = None;
        self.info_pending = None;
        self.active = Some(id.clone());
        self.apply_shown(cx);
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
        // Connected: a session kept while the connection is down takes no new workspace.
        let local = self.store.read(cx).sessions().find(|slot| slot.is_local()).filter(|slot| slot.session().is_some()).map(|slot| slot.id.clone());
        let Some((id, connected)) = local.and_then(|id| Some((id.clone(), self.sessions.get(&id)?))) else {
            return;
        };
        let session = connected.session.read(cx);
        if !matches!(session.state(), treeterm_model::session_store::SessionState::Live) {
            return;
        }
        self.open_on_start = None;
        let existing = session.workspace_for_path(&path);
        let session = connected.session.clone();
        let workspace = match existing {
            Some(workspace) => workspace,
            None => session.update(cx, |s, cx| s.open_workspace(path, WorktreeSettings::default(), cx)),
        };
        self.select_workspace(&id, workspace, cx);
    }

    /// Show `workspace` of connection `id`'s session, focused (a row, rail icon or favourite
    /// picked).
    fn select_workspace(&mut self, id: &ConnectionId, workspace: String, cx: &mut Context<Self>) {
        let Some(connected) = self.sessions.get_mut(id) else {
            return; // the session went with its connection
        };
        connected.session.update(cx, |s, cx| s.set_active_workspace(workspace, cx));
        connected.focus_pending = true;
        self.show_session(id, cx);
    }

    /// Connection `id`'s sidebar section, with its download meter `traffic`; [`Self::sync`]
    /// gives it the session to show.
    fn new_tree(
        &mut self,
        id: ConnectionId,
        target: treeterm_core::connection::ConnectionTarget,
        traffic: Entity<TrafficMeter>,
        cx: &mut Context<Self>,
    ) -> SessionTree {
        let (keybindings, names, app) = (self.keybindings.clone(), self.names.clone(), self.store.clone());
        let pane = cx.new(|cx| TreePane::new(app, id.clone(), traffic, keybindings, names, target, cx));
        let (disconnect_id, info_id) = (id.clone(), id.clone());
        let events = [
            cx.subscribe(&pane, move |view, _, _: &crate::tree_pane::DisconnectRequested, cx| {
                view.store.update(cx, |s, cx| s.disconnect(&disconnect_id, cx));
            }),
            cx.subscribe(&pane, move |view, _, _: &crate::tree_pane::SessionInfoRequested, cx| {
                view.info_pending = Some(info_id.clone());
                cx.notify();
            }),
            cx.subscribe(&pane, move |view, _, WorkspaceSelected(workspace), cx| {
                view.select_workspace(&id, workspace.clone(), cx);
            }),
        ];
        SessionTree { pane, _events: events }
    }

    fn connect(&mut self, id: ConnectionId, session: Entity<SessionStore>, cx: &mut Context<Self>) -> Connected {
        cx.observe(&session, |_, _, cx| cx.notify()).detach();
        let events_id = id.clone();
        let session_events = cx.subscribe(&session, move |view, session, event: &SessionEvent, cx| match event {
            SessionEvent::WorkspaceRemoved(workspace) => {
                let Some(connected) = view.sessions.get_mut(&events_id) else {
                    return;
                };
                // Its badge holds its meter, gone from the connection's (`removeWorkspace`).
                connected.traffic_badges.remove(workspace);
                // The workspace shown was dismissed or removed (its session has none
                // active): an edit of its header goes with it.
                if session.read(cx).active_workspace().is_none() {
                    connected.header_edit = None;
                    cx.notify();
                }
            }
            // Where the window lands once restored is the app store's (`AppEvent::ShowSession`).
            SessionEvent::Restored
            | SessionEvent::AttentionRecorded(_)
            | SessionEvent::Failed(_)
            | SessionEvent::LoadFailed(_) => {}
        });
        Connected {
            session,
            terminal_views: HashMap::new(),
            file_views: HashMap::new(),
            editor_views: HashMap::new(),
            review_views: HashMap::new(),
            browser_views: HashMap::new(),
            page_subtabs: HashMap::new(),
            tab_strips: HashMap::new(),
            tab_layouts: HashMap::new(),
            run_actions: HashMap::new(),
            traffic_badges: HashMap::new(),
            header_edit: None,
            branch_copied: None,
            tool_views: crate::tool_views::ToolViews::new(self.settings.clone()),
            focus_pending: false,
            shown_terminals: HashSet::new(),
            loading_output: LoadingOutput::default(),
            _session_events: session_events,
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

    /// The collapsed sidebar (`.tree-pane-collapsed`, 36px): sound toggle and expand, then each
    /// session's block (`CollapsedSessionPanel`), dragged to reorder the sessions as in the
    /// expanded sidebar: one 32px icon per workspace (`WorkspaceIcon`, favourite star overlay).
    fn rail(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (bg, border, hover, active, secondary, primary) =
            (c.bg_secondary, c.border, c.bg_hover, c.bg_active, c.text_secondary, c.text_primary);
        let now = cx.background_executor().now();
        let mut flashing = false;
        let (store, names) = (self.store.read(cx), self.names.read(cx));
        let mut rail = v_flex().py(px(4.)).items_center();
        for (conn, connected) in &self.sessions {
            let Some(slot) = store.session_slot(conn) else { continue };
            let session = connected.session.read(cx);
            let shown = session.viewed_workspace().map(str::to_owned);
            let mut block = v_flex();
            for row in session.tree_rows(&Default::default(), cx) {
                let Some(entry) = session.entry(&row.id) else { continue };
                let is_shown = shown.as_deref() == Some(row.id.as_str());
                let color = if is_shown { primary } else { secondary };
                // A placeholder shows a spinner and a failed workspace an alert, under its name.
                let (name, icon, favourite, flash) = match entry {
                    WorkspaceEntry::Loaded(store) | WorkspaceEntry::OperationError { store, .. } => {
                        let workspace = store.read(cx);
                        let activity = workspace.overall_activity();
                        let icon = if activity == treeterm_model::workspace_store::ActivityState::Idle {
                            let glyph = if workspace.file().is_worktree { IconName::GitBranch } else { IconName::Folder };
                            crate::ui::icon(glyph, 16., color).into_any_element()
                        } else {
                            crate::ui::activity_glyph(SharedString::from(format!("rail-activity-{}", row.id)), activity, 14., color)
                                .tooltip(crate::ui::activity_hint(activity))
                                .into_any_element()
                        };
                        let flash = workspace.attention_flash(now);
                        (workspace.file().display_name().to_owned(), icon, workspace.is_favourite(), flash)
                    }
                    WorkspaceEntry::Loading { name, .. } => {
                        let spinner = Spinner::new()
                            .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
                            .with_size(px(16.))
                            .color(color);
                        (name.clone(), spinner.into_any_element(), false, None)
                    }
                    WorkspaceEntry::Error { name, .. } => {
                        (name.clone(), crate::ui::icon(IconName::CircleAlert, 16., c.error).into_any_element(), false, None)
                    }
                };
                flashing |= flash.is_some();
                let name = SharedString::from(name);
                let (conn, id) = (conn.clone(), row.id.clone());
                block = block.child(
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
                        // `WorkspaceStoreAttentionFlash`: over the icon, under the star.
                        .children(flash.map(|flash| crate::ui::attention_flash(flash, 4.)))
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
                        .on_click(cx.listener(move |view, _, _, cx| view.select_workspace(&conn, id.clone(), cx))),
                );
            }
            rail = rail.child(self.session_drag_handle(conn.clone(), names.name(&slot.target), block.into_any_element(), cx));
        }
        // A flash fades frame by frame until it is over.
        if flashing {
            window.request_animation_frame();
        }
        let sound_toggle = self.sound_toggle(primary, cx);
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
                    .child(sound_toggle)
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

    /// `SoundToggle` (`.sound-toggle`), the same in the expanded and the collapsed sidebar:
    /// shows whether the attention sound is on, says so in its tooltip and toggles it. While
    /// the toggle saves, it spins (the store ignores clicks). Its status box is drawn by
    /// [`Self::sound_status`], under where this was laid out.
    fn sound_toggle(&self, color: gpui_kit::Hsla, cx: &mut Context<Self>) -> AnyElement {
        use treeterm_model::settings_store::SoundSave;
        let store = self.settings.read(cx);
        let on = store.settings().notifications.sound_enabled;
        let label = if on { "Mute workspace notification sounds" } else { "Enable workspace notification sounds" };
        let volume = || crate::ui::icon(if on { IconName::Volume2 } else { IconName::VolumeX }, 14., color).into_any_element();
        let (icon, status_shown) = match store.sound_save() {
            SoundSave::Idle => (volume(), false),
            SoundSave::Saving => (
                Spinner::new()
                    .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
                    .with_size(px(14.))
                    .color(color)
                    .into_any_element(),
                true,
            ),
            SoundSave::Failed(_) => (volume(), true),
        };
        // Where the toggle was laid out; a shown status box moves with it on the next frame.
        let view = cx.entity().downgrade();
        let measure = gpui_kit::canvas(
            move |bounds, _, cx| {
                if let Err(error) = view.update(cx, |view, cx| {
                    if view.sound_toggle_bounds != Some(bounds) {
                        view.sound_toggle_bounds = Some(bounds);
                        if status_shown {
                            cx.notify();
                        }
                    }
                }) {
                    tracing::debug!(%error, "window closed");
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        div()
            .relative()
            .flex()
            .child(
                crate::ui::icon_button("sound-toggle", cx)
                    .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(label).build(window, cx))
                    .child(icon)
                    .on_click(cx.listener(|view, _, _, cx| view.settings.update(cx, |store, cx| store.toggle_sound(cx)))),
            )
            .child(measure)
            .into_any_element()
    }

    /// The sound toggle's `[role="status"]` ("Saving sound preference…") or `[role="alert"]`
    /// ("Could not save sound preference: …", until the next click) box: under the toggle,
    /// 220px wide, 8px padding, an accent border, the page's 16px text. Its z-index (100) puts
    /// it over the sidebar and the pane but under dialogs (1000): drawn last in the window
    /// content, not deferred, which would draw it over the dialogs too.
    fn sound_status(&self, cx: &App) -> Option<AnyElement> {
        use treeterm_model::settings_store::SoundSave;
        let text: SharedString = match self.settings.read(cx).sound_save() {
            SoundSave::Idle => return None,
            SoundSave::Saving => "Saving sound preference…".into(),
            SoundSave::Failed(error) => format!("Could not save sound preference: {error}").into(),
        };
        let toggle = self.sound_toggle_bounds?;
        let c = colors(cx);
        Some(
            gpui_kit::anchored()
                .position(toggle.bottom_left())
                .position_mode(gpui_kit::AnchoredPositionMode::Window)
                .child(
                    div()
                        .occlude()
                        .w(px(220.))
                        .p(px(8.))
                        .bg(c.bg_secondary)
                        .border_1()
                        .border_color(c.accent)
                        .text_size(px(16.))
                        .line_height(px(19.))
                        .text_color(c.text_primary)
                        .child(text),
                )
                .into_any_element(),
        )
    }

    /// The left column: every connection with its tree, status and actions.
    fn sidebar(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        if self.sidebar_collapsed {
            return self.rail(window, cx);
        }
        let c = colors(cx);
        let (bg, border, fg, secondary) = (c.bg_secondary, c.border, c.text_primary, c.text_secondary);
        let names = self.names.read(cx);
        let mut slots: Vec<_> = self.store.read(cx).sessions().collect();
        names.sort(&mut slots, |slot| &slot.target);
        // Every connection's section, whatever its state (`SessionPanel`).
        let sections: Vec<(ConnectionId, String, Entity<TreePane>)> = slots
            .into_iter()
            .filter_map(|slot| Some((slot.id.clone(), names.name(&slot.target), self.trees.get(&slot.id)?.pane.clone())))
            .collect();
        let mut list = v_flex().w_full();
        // `.tree-empty`: every session was disconnected.
        if sections.is_empty() {
            list = list.child(
                div().p(px(16.)).text_size(px(12.)).text_color(secondary).text_center().child("No sessions. Click + to browse."),
            );
        }
        for (id, name, tree) in sections {
            list = list.child(self.session_drag_handle(id, name, tree.into_any_element(), cx));
        }
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
                    .child(self.sound_toggle(fg, cx))
                    .child(crate::ui::caps_title("Sessions", cx))
                    .child(
                        h_flex()
                            .gap(px(4.))
                            .child(
                                crate::ui::icon_button("connect-ssh", cx)
                                    .tooltip(tip("Connect to SSH"))
                                    .child(crate::ui::icon(IconName::Monitor, 14., fg))
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        crate::connection_picker::open(view.connection_picker.clone(), window, cx);
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
            .children(self.favourites(window, cx))
            .child(div().id("sessions").flex_1().overflow_y_scroll().child(list))
            .into_any_element()
    }

    /// A session's section in the sidebar, or its block in the rail, as a drag source and
    /// drop target (`.session-drag-handle`): dropped on the top half of another session it
    /// goes before it, else after it. Rows inside drag their workspace instead (the innermost
    /// drag source wins). While dragged it dims to 40% (`.dragging`); GPUI has no ghost image
    /// of it, so a chip with its name follows the pointer.
    fn session_drag_handle(
        &self,
        id: ConnectionId,
        name: String,
        section: AnyElement,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let accent = colors(cx).accent;
        let (dragging, hint) = (self.session_drag.dims(&id), self.session_drag.hint(&id));
        let (move_id, drop_id) = (id.clone(), id.clone());
        let view = cx.entity().downgrade();
        div()
            .id(SharedString::from(format!("session-drag-{id}")))
            .relative()
            .when(dragging, |d| d.opacity(0.4))
            .on_drag(DraggedSession { id, name: name.into() }, move |dragged, _, _, cx| {
                // `handleSessionDragStart`.
                if let Err(error) = view.update(cx, |view, cx| {
                    view.session_drag = SessionDrag::Dragging { dragged: dragged.id.clone(), over: None };
                    cx.notify();
                }) {
                    tracing::debug!(%error, "window closed");
                }
                cx.new(|_| dragged.clone())
            })
            .on_drag_move(cx.listener(move |view, event: &gpui_kit::DragMoveEvent<DraggedSession>, _, cx| {
                if view.session_drag.moved(&move_id, event.bounds, event.event.position) {
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
        let Some(side) = self.session_drag.drop_on(onto) else {
            return; // dropped on itself
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
    fn favourites(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        use gpui_kit::component::menu::ContextMenuExt as _;
        let c = colors(cx);
        let (secondary, primary, hover, active, accent, border) =
            (c.text_secondary, c.text_primary, c.bg_hover, c.bg_active, c.accent, c.border);
        let now = cx.background_executor().now();
        let mut flashing = false;
        let mut rows = Vec::new();
        for (conn, connected) in &self.sessions {
            // Every connection has its sidebar tree (made before its session is shown).
            let Some(tree) = self.trees.get(conn) else { continue };
            let session = connected.session.read(cx);
            let shown = session.viewed_workspace().map(str::to_owned);
            for id in session.workspace_ids() {
                let Some(store) = session.workspace_store(id) else { continue };
                let workspace = store.read(cx);
                if workspace.is_favourite() {
                    let flash = workspace.attention_flash(now);
                    flashing |= flash.is_some();
                    rows.push(FavouriteRow {
                        id: id.clone(),
                        conn: conn.clone(),
                        store: store.clone(),
                        tree: tree.pane.clone(),
                        name: workspace.file().display_name().to_owned(),
                        path: workspace.workspace().path.clone().into(),
                        is_worktree: workspace.file().is_worktree,
                        unread: workspace.has_unread_attention(),
                        pending: workspace.attention_pending(),
                        flash,
                        activity: workspace.overall_activity(),
                        is_shown: shown.as_deref() == Some(id.as_str()),
                        // `PrIndicators`, as in the tree.
                        pr: session
                            .git_controller(id)
                            .map_or_else(Vec::new, |git| crate::tree_pane::pr_indicators(git.read(cx).status())),
                    });
                }
            }
        }
        if rows.is_empty() {
            return None;
        }
        // A flash fades frame by frame until it is over.
        if flashing {
            window.request_animation_frame();
        }
        // `.favourites-panel`: a 2px rule, then 4px before the sessions. `.favourites-header`:
        // filled star and "FAVOURITES", 11px semibold secondary.
        let mut panel = v_flex().pb(px(6.)).mb(px(4.)).border_b(px(2.)).border_color(border).child(
            h_flex()
                .pt(px(6.))
                .px(px(12.))
                .pb(px(2.))
                .gap(px(4.))
                .items_center()
                .child(div().text_size(px(12.)).line_height(px(12.)).text_color(secondary).child("★"))
                .child(crate::ui::caps_title("Favourites", cx)),
        );
        for row in rows {
            let FavouriteRow { id, conn, store, tree, name, path, is_worktree, unread, pending, flash, activity, is_shown, pr } =
                row;
            let icon = if activity == treeterm_model::workspace_store::ActivityState::Idle {
                crate::ui::icon(if is_worktree { IconName::GitBranch } else { IconName::Folder }, 16., secondary).into_any_element()
            } else {
                crate::ui::activity_glyph(SharedString::from(format!("fav-activity-{id}")), activity, 14., secondary)
                    .tooltip(crate::ui::activity_hint(activity))
                    .into_any_element()
            };
            let (unread_id, menu_id) = (SharedString::from(format!("fav-unread-{id}")), id.clone());
            panel = panel.child(
                h_flex()
                    .id(SharedString::from(format!("fav-{id}")))
                    .relative()
                    .min_h(px(36.))
                    .py(px(6.))
                    .px(px(4.))
                    .items_center()
                    .cursor_pointer()
                    .when(is_shown, |d| d.bg(active))
                    .when(!is_shown, |d| d.hover(move |d| d.bg(hover)))
                    .when(unread, |d| d.border_l(px(2.)).border_color(accent))
                    .tooltip(crate::ui::title_tooltip(path))
                    .child(div().mr(px(8.)).flex_shrink_0().child(icon))
                    .children(pr)
                    .child(div().flex_1().min_w_0().truncate().text_size(px(crate::ui::BODY)).text_color(primary).child(name))
                    .when(unread, |d| d.child(crate::ui::unread_marker(unread_id, accent).tooltip(crate::ui::unread_hint)))
                    .when(pending, |d| d.child(crate::ui::attention_saving(primary)))
                    // `WorkspaceAttentionFlash`, over the row's content.
                    .children(flash.map(|flash| crate::ui::attention_flash(flash, 0.)))
                    .on_click(cx.listener(move |view, _, _, cx| view.select_workspace(&conn, id.clone(), cx)))
                    // `fav-context-<id>`: Settings, as in the tree row's menu, and Unmark as Favourite.
                    .context_menu(move |menu, _, _| {
                        let (settings_store, tree, id) = (store.clone(), tree.clone(), menu_id.clone());
                        let favourite_store = store.clone();
                        menu.item(crate::ui::menu_item("Settings").on_click(move |_, _, cx| {
                            crate::tree_pane::open_workspace_settings(&settings_store, &tree, id.clone(), cx)
                        }))
                        .item(
                            crate::ui::menu_item("Unmark as Favourite")
                                .on_click(move |_, _, cx| favourite_store.update(cx, |w, cx| w.toggle_favourite(cx))),
                        )
                    }),
            );
        }
        Some(panel.into_any_element())
    }

    /// Open a pending session pane, or drop one whose session is gone.
    fn sync_info(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.info_pending.take() {
            let (app, names) = (self.store.clone(), self.names.clone());
            self.info = Some(cx.new(|cx| crate::session_info_view::SessionInfoView::new(app, names, id, window, cx)));
        }
        if let Some(info) = &self.info {
            let id = info.read(cx).connection(cx);
            if self.store.read(cx).session_slot(&id).is_none() {
                self.info = None;
            }
        }
        // A session pane shows no workspace: none is being viewed.
        self.apply_shown(cx);
    }

}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (bg, fg, secondary, error, border, accent) =
            (c.bg_primary, c.text_primary, c.text_secondary, c.error, c.border, c.accent);
        self.sync_info(window, cx);
        // `handleSessionDragEnd`: a session dropped, or let go of where nothing takes it, is no
        // longer dragged. `handleMouseUp`: the divider let go of is no longer resizing.
        if !cx.has_active_drag() {
            self.session_drag = SessionDrag::Idle;
            self.resizing = false;
        }
        let sidebar = self.sidebar(window, cx);
        let info = self.info.clone();
        // Electron mounts the keybinding overlay in the workspace pane, which is hidden while
        // a session pane is shown and absent with no session.
        let overlay = if info.is_none() && self.current().is_some() {
            crate::keybinding_overlay::render(&self.keybindings, window, cx)
        } else {
            None
        };
        let collapsed = self.sidebar_collapsed && cfg!(target_os = "macos");
        // Native webviews draw above GPUI: hide those of every workspace pane not on screen
        // (the shown one manages its own in `workspace_pane`).
        let on_screen = if info.is_none() { self.active.clone() } else { None };
        for (_, connected) in self.sessions.iter().filter(|(id, _)| on_screen.as_ref() != Some(*id)) {
            hide_tab_views(connected, cx);
        }
        // A session off screen shows no terminal: its terminals are shown again (fit again,
        // the active tab's takes focus) when it returns.
        for (_, connected) in self.sessions.iter_mut().filter(|(id, _)| on_screen.as_ref() != Some(*id)) {
            connected.shown_terminals.clear();
        }
        let pane = match (info, self.current_mut()) {
            (Some(info), _) => info.into_any_element(),
            (None, current) => match current {
            // macOS traffic lights sit over the header's left end once the sidebar is a rail.
            Some(connected) => workspace_pane(connected, if collapsed { px(72.) } else { px(16.) }, window, cx),
            None => match self.store.read(cx).idle_pane(cx) {
                IdlePane::Splash => splash(cx),
                IdlePane::Failed { error: reason } => centered(
                    v_flex()
                        .gap_2()
                        .max_w(px(600.))
                        .child(div().text_color(error).child("Cannot reach the TreeTerm daemon"))
                        .child(div().text_sm().text_color(secondary).child(reason)),
                ),
                IdlePane::Reconnecting { attempt, error: reason } => centered(
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
                // App.tsx with no active session store: the text at the default 16px.
                IdlePane::SelectWorkspace => centered(
                    div().text_size(px(16.)).text_color(secondary).child("Select a workspace to get started"),
                ),
            },
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
                .bg(if self.resizing { accent } else { border })
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
                if !view.resizing || (width - view.sidebar_width).abs() >= 1. {
                    view.resizing = true;
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
            .children(self.sound_status(cx))
            .children(overlay)
    }
}

impl AppView {
    /// Whether the window may close now; when child workspaces of any connection are
    /// unmerged, asks first (and quits if the user confirms).
    pub fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.ask_before(CloseAnyway::Quit, window, cx)
    }

    /// Whether nothing holds up `close_anyway`: when child workspaces of any connection are
    /// unmerged, asks first, and its Close Anyway does `close_anyway`. While it is open it is
    /// asked again, not stacked.
    fn ask_before(&mut self, close_anyway: CloseAnyway, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let unmerged = self.store.read(cx).unmerged_sub_workspaces(cx);
        if unmerged.is_empty() {
            return true;
        }
        match self.close_confirm.upgrade() {
            Some(open) => open.ask_again(close_anyway),
            None => self.close_confirm = crate::active_processes_view::confirm_close(unmerged, close_anyway, window, cx),
        }
        false
    }

    /// Quit (Cmd+Q): Electron's `app.quit()` closes the window, which asks first.
    pub fn quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.request_close(window, cx) {
            cx.quit();
        }
    }

    /// Exit and Kill Daemon (Cmd+Shift+Q, Ctrl+Shift+Q): asks first as Quit does, then stops
    /// the local daemon and quits (`quitAndKillDaemon`, which stopped it before asking).
    pub fn quit_and_kill_daemon(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kill = CloseAnyway::KillDaemonAndQuit(self.store.clone());
        if self.ask_before(kill.clone(), window, cx) {
            kill.run(cx);
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
        let (settings, keybindings) = (self.settings.clone(), self.keybindings.clone());
        self.settings_dialog.open(settings, keybindings, window, cx);
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
        let session = self.current()?.session.read(cx);
        session.workspace_store(session.active_workspace()?).cloned()
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
                if let Some(id) = self.active.clone() {
                    self.select_workspace(&id, workspace_id, cx);
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
                let current = session
                    .active_workspace()
                    .and_then(|active| ids.iter().position(|w| w == active))
                    .unwrap_or(0);
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

/// "▶ Run" (`RunActionDropdown`): the workspace's detected actions by source, then the
/// custom runners.
fn run_menu(
    connected: &mut Connected,
    workspace: &Entity<treeterm_model::workspace_store::WorkspaceStore>,
    workspace_id: &str,
    cx: &mut Context<AppView>,
) -> AnyElement {
    use gpui_kit::component::menu::DropdownMenu as _;
    use treeterm_model::run_actions::RunActions;
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
                    .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Run action").build(window, cx))
                    .child("▶ Run")
                    .child(crate::ui::icon(IconName::ChevronDown, 12., fg)),
            )
                .dropdown_menu_with_anchor(gpui_kit::Anchor::TopRight, move |menu, window, cx| {
                    // The open menu follows the model: it turns from "Loading..." into the
                    // actions when detection finishes. The refill holds the workspace only as
                    // long as the menu, as its items do.
                    let (workspace, runners) = (menu_workspace.clone(), runners.clone());
                    crate::ui::menu_following(menu, &menu_model, window, cx, move |menu, model, cx| {
                        fill_run_menu(menu, model, &workspace, &runners, cx)
                    })
                }),
        )
        .when_some(error, |d, error| d.child(div().text_xs().text_color(colors(cx).error).child(error)))
        .into_any_element()
}

/// The Run menu's items (`.run-action-menu`): "Loading..." while detecting, then each source's
/// actions under its uppercase 11px header; the custom runners whenever there are any, while
/// detecting too; a rule between groups.
fn fill_run_menu(
    menu: gpui_kit::component::menu::PopupMenu,
    model: &Entity<treeterm_model::run_actions::RunActions>,
    workspace: &Entity<WorkspaceStore>,
    runners: &[(String, String, String)],
    cx: &mut Context<gpui_kit::component::menu::PopupMenu>,
) -> gpui_kit::component::menu::PopupMenu {
    use gpui_kit::component::menu::PopupMenuItem;
    use gpui_kit::component::tooltip::Tooltip;
    use treeterm_model::run_actions::Detection;
    let secondary = colors(cx).text_secondary;
    // Headers and notes are plain text, as Electron's divs: disabled items are neither
    // highlighted, selected by the arrow keys, nor clickable.
    let note = move |text: &'static str| {
        PopupMenuItem::element(move |_, _| div().px(px(0.)).py(px(2.)).text_size(px(12.)).text_color(secondary).child(text))
            .disabled(true)
    };
    let header = move |text: String| {
        PopupMenuItem::element(move |_, _| div().text_size(px(11.)).text_color(secondary).child(text.to_uppercase()))
            .disabled(true)
    };
    // `.run-action-menu-item`: 13px, one line, ellipsised; `title` shows its description.
    let item = |text: SharedString, title: SharedString| {
        PopupMenuItem::element(move |_, _| {
            let title = title.clone();
            div()
                .id("run-action-item")
                .flex_1()
                .min_w_0()
                .py(px(2.))
                .text_size(px(13.))
                .truncate()
                .child(text.clone())
                .when(!title.is_empty(), |d| d.tooltip(move |window, cx| Tooltip::new(title.clone()).build(window, cx)))
        })
    };
    // `.run-action-menu`: 200 to 320px wide, at most 400px high, scrolling.
    let mut menu = menu.min_w(px(200.)).max_w(px(320.)).max_h(px(400.)).scrollable(true);
    let mut groups = 0;
    match model.read(cx).detection().clone() {
        Detection::Detecting => menu = menu.item(note("Loading...")),
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
                let model = model.clone();
                let (label, title) = (SharedString::from(action.name.clone()), SharedString::from(action.description.clone()));
                menu = menu.item(item(label, title).on_click(move |_, _, cx| {
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
        for (app_id, name, icon) in runners {
            let (workspace, app_id) = (workspace.clone(), app_id.clone());
            let label = SharedString::from(format!("{icon}\u{2009}\u{2009}{name}"));
            menu = menu.item(item(label, SharedString::default()).on_click(move |_, _, cx| {
                workspace.update(cx, |w, cx| {
                    w.add_tab(&app_id, cx);
                });
            }));
        }
    }
    menu
}

/// `.workspace-action-btn:disabled`: half opacity and a not-allowed cursor, no click.
fn disabled_action(button: gpui_kit::Stateful<gpui_kit::Div>) -> gpui_kit::Stateful<gpui_kit::Div> {
    button.opacity(0.5).cursor_not_allowed()
}

/// The header's git buttons: status (`GitStatusButton`) and pull (`GitPullButton`).
fn git_status_items(
    controller: &Entity<treeterm_model::git_controller::GitController>,
    cx: &mut Context<AppView>,
) -> Vec<AnyElement> {
    use gpui_kit::component::Sizable as _;
    use treeterm_core::git_status::{ParentConflicts, WorkingTree};
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
            .map(|button| {
                if refreshing {
                    disabled_action(button)
                } else {
                    button.on_click(move |_, _, cx| refresh.update(cx, |git, cx| git.refresh(cx)))
                }
            })
            .into_any_element(),
    ];
    // `.workspace-action-btn-pull`: accent fill while behind upstream.
    let pull_state = controller.read(cx).pull_state();
    if status.behind_count > 0 {
        let pull = controller.clone();
        let behind = status.behind_count;
        let title = format!("Pull {behind} commit{} from remote", if behind > 1 { "s" } else { "" });
        let accent = c.accent;
        items.push(
            crate::ui::action_button("git-pull", cx)
                .bg(accent)
                .border_color(accent)
                .text_color(gpui_kit::white())
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(title.clone()).build(window, cx))
                .child(match pull_state {
                    PullState::Pulling => spinner(14., gpui_kit::white()),
                    PullState::Idle => crate::ui::icon(IconName::ArrowDownToLine, 14., gpui_kit::white()).into_any_element(),
                })
                .child(
                    h_flex()
                        .gap(px(3.))
                        .child("Pull")
                        .child(div().text_size(px(10.)).font_weight(gpui_kit::FontWeight::SEMIBOLD).child(behind.to_string())),
                )
                .map(|button| match pull_state {
                    PullState::Pulling => disabled_action(button),
                    // `handlePull`: `alert(result.error)` when it fails.
                    PullState::Idle => button.on_click(move |_, window, cx| {
                        if let Some(outcome) = pull.update(cx, |git, cx| git.pull(cx)) {
                            crate::ui::alert_failure(outcome, window, cx);
                        }
                    }),
                })
                .into_any_element(),
        );
    }
    items
}

/// The GitHub pair (`GitHubButton`) of a child worktree on a branch: create or open its PR
/// (or address its unresolved comments), and open that page in the browser.
fn github_items(
    controller: &Entity<treeterm_model::git_controller::GitController>,
    workspace: &Entity<treeterm_model::workspace_store::WorkspaceStore>,
    cx: &mut Context<AppView>,
) -> AnyElement {
    use treeterm_core::git_status::PullRequest;
    use treeterm_model::git_controller::OpenGitHubState;
    let (pr, unresolved) = match &controller.read(cx).status().pull_request {
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
        None => colors(cx).text_primary,
    };
    let open = {
        let controller = controller.clone();
        move |window: &mut Window, cx: &mut App| {
            // None: a click in the same frame as the one that started the lookup.
            let Some(task) = controller.update(cx, |git, cx| git.open_github(cx)) else {
                return;
            };
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
    // `disabled={loading}` on both while the URL resolves, the comments path too.
    let open_state = controller.read(cx).open_github_state();
    let (main_open, comments_ws) = (open.clone(), workspace.clone());
    h_flex()
        .child(
            style(crate::ui::action_button("github-main", cx))
                .rounded_r(px(0.))
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(main_title.clone()).build(window, cx))
                .child(crate::ui::icon(IconName::Github, 14., fg))
                .child(label)
                .map(|button| match open_state {
                    OpenGitHubState::Opening => disabled_action(button),
                    OpenGitHubState::Idle => button.on_click(move |_, window, cx| {
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
                }),
        )
        .child(
            style(crate::ui::action_button("github-browser", cx))
                .ml(px(1.))
                .px(px(6.))
                .rounded_l(px(0.))
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(browser_title).build(window, cx))
                .child(crate::ui::icon(IconName::ExternalLink, 14., fg))
                .map(|button| match open_state {
                    OpenGitHubState::Opening => disabled_action(button),
                    OpenGitHubState::Idle => button.on_click(move |_, window, cx| open(window, cx)),
                }),
        )
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

/// `loading.html`, Electron's splash window while the daemon starts, in the pane: "TreeTerm",
/// the spinner and "Connecting to daemon...", centred.
fn splash(cx: &App) -> AnyElement {
    let c = colors(cx);
    centered(
        v_flex()
            .items_center()
            .child(
                div()
                    .mb(px(24.))
                    .text_size(px(18.))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(c.text_primary)
                    .child("TreeTerm"),
            )
            .child(div().mb(px(16.)).child(crate::ui::ring_spinner("splash-spinner", 24., c.border, c.accent)))
            .child(div().text_size(px(13.)).text_color(c.text_secondary).child("Connecting to daemon...")),
    )
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
    cx: &mut Context<AppView>,
) -> AnyElement {
    use gpui_kit::component::Sizable as _;
    let c = colors(cx);
    let (secondary, primary, border, bg, accent) = (c.text_secondary, c.text_primary, c.border, c.bg_secondary, c.accent);
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
            use treeterm_core::analyzer::Refresh;
            // `RefreshTitleButton`: disabled, with a spinner, while the LLM works.
            let refresh = workspace.clone();
            let button = div().id("refresh-title").px(px(4.)).py(px(2.)).flex_shrink_0();
            let button = if workspace.read(cx).is_refreshing(Refresh::TitleAndDescription, cx) {
                button.tooltip(tip("Generating name and description...")).child(
                    Spinner::new()
                        .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
                        .with_size(px(12.))
                        .color(secondary),
                )
            } else {
                button
                    .cursor_pointer()
                    .tooltip(tip("Regenerate name and description with the LLM"))
                    .child(crate::ui::icon(IconName::RefreshCw, 12., secondary))
                    .on_click(move |_, window, cx| {
                        let outcome = refresh.update(cx, |w, cx| w.refresh_labels(Refresh::TitleAndDescription, cx));
                        crate::ui::alert_failure(outcome, window, cx);
                    })
            };
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
            .child(button)
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
        use treeterm_core::analyzer::Refresh;
        // `BranchBadge`'s `renaming`: a branch rename through the LLM is under way.
        let renaming = move |workspace: &Entity<WorkspaceStore>, cx: &App| {
            can_rename && workspace.read(cx).is_refreshing(Refresh::BranchName, cx)
        };
        let label = if renaming(workspace, cx) {
            h_flex()
                .gap(px(3.))
                .child(
                    Spinner::new()
                        .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
                        .with_size(px(10.))
                        .color(fg),
                )
                .child("Renaming...")
                .into_any_element()
        } else if copied {
            "Copied!".into_any_element()
        } else {
            branch.into_any_element()
        };
        actions = actions.child(
            crate::ui::pill(label, fg, badge_bg)
                .id("branch-badge")
                .cursor_pointer()
                .tooltip(tip("Copy branch name"))
                .on_click(cx.listener(move |view, _, _, cx| view.copy_branch(copy.clone(), cx)))
                .context_menu(move |menu, _, cx| {
                    let path = path.clone();
                    let menu = menu.item(crate::ui::menu_item("Copy worktree path").on_click(move |_, _, cx| {
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(path.clone()));
                    }));
                    // Offered while no rename runs, as the menu opens.
                    if can_rename && !renaming(&rename_ws, cx) {
                        let ws = rename_ws.clone();
                        menu.item(crate::ui::menu_item("Auto rename branch").on_click(move |_, window, cx| {
                            let outcome = ws.update(cx, |w, cx| w.refresh_labels(Refresh::BranchName, cx));
                            crate::ui::alert_failure(outcome, window, cx);
                        }))
                    } else {
                        menu
                    }
                }),
        );
    }
    // Electron's order: status, Pull, Run, the GitHub pair, Merge/Abandon, Review, ×.
    let git = connected.session.read(cx).git_controller(id).cloned().filter(|_| file.is_git_repo);
    if let Some(controller) = &git {
        actions = actions.children(git_status_items(controller, cx));
    }
    actions = actions.child(run_menu(connected, workspace, id, cx));
    if let Some(controller) = &git
        && file.is_worktree
        && !root
        && file.git_branch.is_some()
    {
        actions = actions.child(github_items(controller, workspace, cx));
    }
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
            .children(workspace.read(cx).description_prompt().is_some().then(|| prompt_description_buttons(workspace)))
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
        .into_any_element()
}

/// `PromptDescriptionButton` (`.prompt-description-button`): "Prompt Description" (blue)
/// and "Skip" (grey) after the description, 4px apart.
fn prompt_description_buttons(workspace: &Entity<WorkspaceStore>) -> AnyElement {
    // 10px semibold, 1px 6px padding, a 1px border, radius 3, 90% opaque until hovered.
    let button = |id: &'static str, label: &'static str, title: &'static str, [bg, border, hover_bg, hover_border]: [u32; 4]| {
        div()
            .id(id)
            .px(px(6.))
            .py(px(1.))
            .rounded(px(3.))
            .border_1()
            .border_color(gpui_kit::rgb(border))
            .bg(gpui_kit::rgb(bg))
            .text_color(gpui_kit::rgb(0xffffff))
            .text_size(px(10.))
            .line_height(px(14.))
            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
            .whitespace_nowrap()
            .opacity(0.9)
            .cursor_pointer()
            .hover(move |d| d.bg(gpui_kit::rgb(hover_bg)).border_color(gpui_kit::rgb(hover_border)).opacity(1.))
            .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(title).build(window, cx))
            .child(label)
    };
    let (prompt, skip) = (workspace.clone(), workspace.clone());
    h_flex()
        .ml(px(2.))
        .gap(px(4.))
        .flex_shrink_0()
        .child(
            button("prompt-description", "Prompt Description", "Send description to AI agent", [0x2472c8, 0x1a5a9e, 0x3b8eea, 0x2472c8])
                .on_click(move |_, window, cx| {
                    let task = prompt.update(cx, |w, cx| w.prompt_description(cx));
                    notify_failure(task, window, cx);
                }),
        )
        .child(
            button("skip-description-prompt", "Skip", "Skip sending description", [0x6e7681, 0x565c64, 0x8b949e, 0x6e7681])
                .on_click(move |_, _, cx| skip.update(cx, |w, cx| w.dismiss_description_prompt(cx))),
        )
        .into_any_element()
}

/// Show `task`'s error, if it fails, as a notification in `window`.
fn notify_failure(task: gpui_kit::Task<Result<(), String>>, window: &Window, cx: &mut App) {
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

/// A tabset's strip: its tabs, then "+" and the new-tab menu, and its maximize toggle.
fn tabset_strip(
    connected: &mut Connected,
    workspace: &Entity<WorkspaceStore>,
    workspace_id: &str,
    tabset: &LayoutTabSet,
    docks_at_edge: crate::tab_layout_view::DocksAtEdge,
    cx: &mut Context<AppView>,
) -> AnyElement {
    use std::rc::Rc;
    use crate::tab_strip::{NewTabApp, OnId, OnMove, OnPress, StripTab, TabStrip};
    let registry = connected.session.read(cx).registry().clone();
    let store = workspace.read(cx);
    let file = &store.workspace().file;
    let tabs: Vec<StripTab> = tabset
        .tabs
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
            workspace.update(cx, |w, cx| w.select_tab(tab, cx));
            view.update(cx, |view, cx| view.focus_active_tab(cx));
        })
    };
    let path = tabset.path.clone();
    let on_activate: OnPress = {
        let (workspace, path) = (workspace.clone(), path.clone());
        Rc::new(move |_: &mut Window, cx: &mut App| workspace.update(cx, |w, cx| w.activate_tabset(&path, cx)))
    };
    let on_maximize: OnPress = {
        let (workspace, path) = (workspace.clone(), path.clone());
        Rc::new(move |_: &mut Window, cx: &mut App| workspace.update(cx, |w, cx| w.toggle_maximize(&path, cx)))
    };
    let on_close: OnId = {
        let workspace = workspace.clone();
        Rc::new(move |tab: &str, _: &mut Window, cx: &mut App| workspace.update(cx, |w, cx| w.remove_tab(tab, cx)))
    };
    let on_move: OnMove = {
        let (workspace, view) = (workspace.clone(), view.clone());
        Rc::new(move |dragged: &str, target: &str, side, _: &mut Window, cx: &mut App| {
            workspace.update(cx, |w, cx| w.move_tab(dragged, target, side, cx));
            view.update(cx, |view, cx| view.focus_active_tab(cx));
        })
    };
    // The new tab opens in this tabset (Electron's opened it in the active one).
    let on_new_tab: OnId = {
        let (workspace, path) = (workspace.clone(), path.clone());
        Rc::new(move |app: &str, _: &mut Window, cx: &mut App| {
            workspace.update(cx, |w, cx| {
                w.activate_tabset(&path, cx);
                w.add_tab(app, cx)
            });
            view.update(cx, |view, cx| view.focus_active_tab(cx));
        })
    };
    let strip = TabStrip {
        tabs,
        active: tabset.selected.clone(),
        apps,
        on_select,
        on_close,
        on_move,
        on_new_tab,
        on_activate,
        maximize: tabset.maximize,
        on_maximize,
        docks_at_edge,
    };
    let id = format!("{workspace_id}-{}", path.iter().map(usize::to_string).collect::<Vec<_>>().join("-"));
    let state = connected.tab_strips.entry((workspace_id.to_owned(), path)).or_default();
    strip.render(&id, state, cx.entity_id(), cx)
}

/// No workspace's tabs are on screen (none picked, or the one picked was removed, is loading,
/// failed or shows an operation's failure): the session's native webviews are hidden, and
/// its Review and Files views too, so they refresh (Files read `HEAD`) when shown again; Files
/// and Editor tabs save their scroll offset.
fn hide_tab_views(connected: &Connected, cx: &mut Context<AppView>) {
    let browsers: Vec<Entity<crate::browser_view::BrowserView>> = connected.browser_views.values().map(|(_, view)| view.clone()).collect();
    for view in browsers {
        view.update(cx, |v, cx| v.set_shown(false, cx));
    }
    let files: Vec<Entity<crate::file_browser_view::FileBrowserView>> = connected.file_views.values().cloned().collect();
    for view in files {
        view.update(cx, |v, cx| {
            v.set_shown(false, cx);
            v.set_visible(false, cx);
        });
    }
    let editors: Vec<Entity<crate::text_editor_view::TextEditorView>> = connected.editor_views.values().cloned().collect();
    for view in editors {
        view.update(cx, |v, cx| v.set_visible(false, cx));
    }
    let reviews: Vec<Entity<crate::review_view::ReviewView>> = connected.review_views.values().cloned().collect();
    for view in reviews {
        view.update(cx, |v, cx| {
            v.set_shown(false, cx);
            v.set_visible(false, cx);
        });
    }
}

/// `.workspace-empty`: no workspace is shown.
fn workspace_empty(cx: &App) -> AnyElement {
    let c = colors(cx);
    centered(
        v_flex()
            .items_center()
            .child(
                div()
                    .mb(px(8.))
                    .text_size(px(18.))
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .text_color(c.text_primary)
                    .child("No workspace selected"),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(c.text_secondary)
                    .child("Select a workspace from the sidebar or add a new one to get started."),
            ),
    )
}

/// `.workspace-load-error`: the workspace's last operation failed. "Operation failed" and
/// the message, with Force Unlock (`forceUnlock`, which leaves the error) and Cancel
/// (`clearWorkspaceError`, which brings the tabs back).
fn operation_failed(session: &Entity<SessionStore>, id: &str, message: String, cx: &App) -> AnyElement {
    let c = colors(cx);
    let unlock = session.clone();
    let (cancel, cancelled) = (session.clone(), id.to_owned());
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            v_flex()
                .max_w(px(500.))
                .text_center()
                .child(
                    div()
                        .mb(px(12.))
                        .text_size(px(18.))
                        .font_weight(gpui_kit::FontWeight::BOLD)
                        .text_color(c.error)
                        .child("Operation failed"),
                )
                .child(
                    div()
                        .mb(px(16.))
                        .p(px(12.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(c.border)
                        .bg(c.bg_secondary)
                        .font_family(crate::ui::MONO)
                        .text_size(px(12.))
                        .text_color(c.text_secondary)
                        .text_left()
                        .child(message),
                )
                .child(
                    h_flex()
                        .justify_center()
                        .gap(px(8.))
                        .child(
                            crate::ui::action_button("operation-force-unlock", cx)
                                .child("Force Unlock")
                                .on_click(move |_, _, cx| unlock.update(cx, |s, cx| s.force_unlock(cx))),
                        )
                        .child(
                            crate::ui::action_button("operation-cancel", cx)
                                .child("Cancel")
                                .on_click(move |_, _, cx| cancel.update(cx, |s, cx| s.clear_workspace_error(&cancelled, cx))),
                        ),
                ),
        )
        .into_any_element()
}

fn workspace_pane(
    connected: &mut Connected,
    header_inset: gpui_kit::Pixels,
    window: &mut Window,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let error = colors(cx).error;
    prune_terminal_views(connected, cx);
    connected.tool_views.prune(connected.session.read(cx), cx);
    // Set again below for each terminal this render shows.
    let previous_terminals = std::mem::take(&mut connected.shown_terminals);
    let Some(id) = connected.session.read(cx).active_workspace().map(str::to_owned) else {
        hide_tab_views(connected, cx);
        return workspace_empty(cx);
    };
    // `.workspace-terminal` shows only for a Loaded entry.
    if !matches!(connected.session.read(cx).entry(&id), Some(WorkspaceEntry::Loaded(_))) {
        hide_tab_views(connected, cx);
    }
    let workspace = match connected.session.read(cx).entry(&id) {
        // An active workspace with no entry is shown as none (`!activeWorkspace`).
        None => return workspace_empty(cx),
        Some(WorkspaceEntry::Loading { message, output, .. }) => {
            return loading_pane(&id, message, output, &mut connected.loading_output, cx);
        }
        // The reason, and Dismiss (`dismissWorkspace`).
        Some(WorkspaceEntry::Error { error: reason, .. }) => {
            let (session, dismissed) = (connected.session.clone(), id.clone());
            return centered(
                v_flex()
                    .max_w(px(500.))
                    .items_center()
                    .gap(px(16.))
                    .child(div().text_center().text_color(error).child(reason.clone()))
                    .child(
                        crate::ui::action_button("workspace-dismiss", cx)
                            .child("Dismiss")
                            .on_click(move |_, _, cx| session.update(cx, |s, cx| s.dismiss_workspace(&dismissed, cx))),
                    ),
            );
        }
        Some(WorkspaceEntry::Loaded(store) | WorkspaceEntry::OperationError { store, .. }) => store.clone(),
    };
    let operation_error = match connected.session.read(cx).entry(&id) {
        Some(WorkspaceEntry::OperationError { error, .. }) => Some(error.clone()),
        Some(_) | None => None,
    };
    if let Some(message) = operation_error {
        // The header over the "Operation failed" panel, in place of the tabs.
        let header = workspace_header(connected, &workspace, &id, header_inset, cx);
        return v_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .child(header)
            .child(operation_failed(&connected.session, &id, message, cx))
            .into_any_element();
    }
    let header = workspace_header(connected, &workspace, &id, header_inset, cx);

    // The layout's tabsets (flex-layout's `Layout`), each its strip over the tab it shows; a
    // maximized one alone.
    let active = workspace.read(cx).shown_tab();
    let layout = workspace.read(cx).layout();
    let tree = layout.tree();
    let visible: Vec<LayoutTabSet> = match tree.maximized() {
        Some(maximized) => vec![maximized.clone()],
        None => tree.tabsets().into_iter().cloned().collect(),
    };
    let docks_at_edge = connected.tab_layouts.entry(id.clone()).or_default().docks_at_edge();
    let mut panes = HashMap::new();
    for tabset in &visible {
        let strip = tabset_strip(connected, &workspace, &id, tabset, docks_at_edge.clone(), cx);
        let content = match &tabset.selected {
            // An empty tabset: blank under a strip of only "+".
            None => div().size_full().into_any_element(),
            Some(tab_id) => {
                let focus = if active.as_ref() == Some(tab_id) { Focus::Take } else { Focus::Leave };
                let shown = ShownTab { previous_terminals: &previous_terminals, focus };
                tab_content(connected, shown, &workspace, &id, tab_id, window, cx)
            }
        };
        // A press in another tabset's content makes it the active one (flex-layout's `Tab`).
        let mut body = div().flex_1().min_h_0();
        if !tabset.active {
            let (workspace, path) = (workspace.clone(), tabset.path.clone());
            body = body.capture_any_mouse_down(move |_, _, cx| workspace.update(cx, |w, cx| w.activate_tabset(&path, cx)));
        }
        panes.insert(tabset.path.clone(), v_flex().size_full().child(strip).child(body.child(content)).into_any_element());
    }
    let on_resize: crate::tab_layout_view::OnResize = {
        let workspace = workspace.clone();
        std::rc::Rc::new(move |row: &[usize], weights: &[f64], _: &mut Window, cx: &mut App| {
            workspace.update(cx, |w, cx| w.resize_layout_row(row, weights, cx));
        })
    };
    // A tab dropped on a tabset's content or the layout's edge docks there, and is focused.
    let on_dock: crate::tab_layout_view::OnDock = {
        let (workspace, view) = (workspace.clone(), cx.entity());
        std::rc::Rc::new(move |tab: &str, target: &DropTarget, _: &mut Window, cx: &mut App| {
            workspace.update(cx, |w, cx| w.dock_tab(tab, target, cx));
            view.update(cx, |view, cx| view.focus_active_tab(cx));
        })
    };
    let view = cx.entity_id();
    let state = connected.tab_layouts.entry(id.clone()).or_default();
    let tabsets =
        crate::tab_layout_view::TabLayout { tree: &tree, splitters: layout.splitters(), panes, on_resize, on_dock, id: &id, view }
            .render(state, cx);

    // Native webviews draw above GPUI: show only those of the tabs shown, and none under a
    // dialog.
    let dialog_open = {
        use gpui_kit::component::WindowExt as _;
        window.has_active_dialog(cx)
    };
    let shown: Vec<String> = visible.iter().filter_map(|tabset| tabset.selected.clone()).collect();
    // A Web App or Browser tab shows its page only on its Browser sub-tab.
    let shown_pages: HashSet<(String, String)> = shown
        .iter()
        .map(|tab| (id.clone(), tab.clone()))
        .filter(|key| connected.page_subtabs.get(key).is_none_or(|sub| *sub == PageSubTab::Browser))
        .collect();
    let views: Vec<((String, String), Entity<crate::browser_view::BrowserView>)> =
        connected.browser_views.iter().map(|(k, (_, v))| (k.clone(), v.clone())).collect();
    for (key, view) in views {
        let shown = !dialog_open && shown_pages.contains(&key);
        view.update(cx, |v, cx| v.set_shown(shown, cx));
    }
    let contents: Vec<TabContent> = shown.iter().filter_map(|tab| workspace.read(cx).tab_content(tab)).collect();
    let shown_files: HashSet<EntityId> = contents
        .iter()
        .filter_map(|content| if let TabContent::Files(model) = content { Some(model.entity_id()) } else { None })
        .collect();
    let file_views: Vec<(EntityId, Entity<crate::file_browser_view::FileBrowserView>)> =
        connected.file_views.iter().map(|(k, v)| (*k, v.clone())).collect();
    // A Files tab hidden saves its scroll offset, and shown again reads `HEAD` again for its
    // comments (Electron unmounts it, then mounts it again).
    for (model_id, view) in file_views {
        let visible = shown_files.contains(&model_id);
        view.update(cx, |v, cx| {
            v.set_shown(!dialog_open && visible, cx);
            v.set_visible(visible, cx);
        });
    }
    // An Editor tab hidden saves its scroll offset, as a Files tab does (Electron unmounts
    // both then).
    let shown_editors: HashSet<EntityId> = contents
        .iter()
        .filter_map(|content| if let TabContent::Editor(model) = content { Some(model.entity_id()) } else { None })
        .collect();
    let editor_views: Vec<(EntityId, Entity<crate::text_editor_view::TextEditorView>)> =
        connected.editor_views.iter().map(|(k, v)| (*k, v.clone())).collect();
    for (model_id, view) in editor_views {
        view.update(cx, |v, cx| v.set_visible(shown_editors.contains(&model_id), cx));
    }
    // Review shows a favourite's file (a PDF or model in a webview) in place of the diff, and
    // refreshes when it is shown again (whatever switched to it: the tab strip, a shortcut,
    // the header's Review button, another workspace and back, a synced tab change).
    let shown_reviews: HashSet<EntityId> = contents
        .iter()
        .filter_map(|content| if let TabContent::Review(model) = content { Some(model.entity_id()) } else { None })
        .collect();
    let review_views: Vec<(EntityId, Entity<crate::review_view::ReviewView>)> =
        connected.review_views.iter().map(|(k, v)| (*k, v.clone())).collect();
    for (model_id, view) in review_views {
        let visible = shown_reviews.contains(&model_id);
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
        .child(div().flex().flex_1().min_h_0().child(tabsets))
        .into_any_element()
}

/// What the tab `tab_id` shows in its tabset. Only the workspace's active tab takes keyboard
/// focus when a switch asked for it (`shown.focus`).
fn tab_content(
    connected: &mut Connected,
    shown: ShownTab<'_>,
    workspace: &Entity<WorkspaceStore>,
    id: &str,
    tab_id: &str,
    window: &mut Window,
    cx: &mut Context<AppView>,
) -> AnyElement {
    let c = colors(cx);
    let (secondary, error) = (c.text_secondary, c.error);
    let ShownTab { previous_terminals, focus } = shown;
    match workspace.read(cx).tab_content(tab_id) {
        Some(TabContent::Terminal(model)) => {
            let view = connected
                .terminal_views
                .entry(model.entity_id())
                .or_insert_with(|| cx.new(|cx| TerminalView::new(model.clone(), window, cx)))
                .clone();
            show_terminal(connected, previous_terminals, model.entity_id(), &view, focus, window, cx);
            let prompts = workspace
                .read(cx)
                .analyzer(tab_id)
                .is_some()
                .then(|| harness_prompt_buttons(&connected.session, id, cx))
                .flatten();
            v_flex()
                .size_full()
                .child(div().relative().flex_1().min_h_0().child(view).children(prompts))
                .child(terminal_status_bar(workspace, tab_id, cx))
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
            if focus == Focus::Take && std::mem::take(&mut connected.focus_pending) {
                let focus = view.read(cx).focus_handle().clone();
                window.focus(&focus, cx);
            }
            div().size_full().child(view).into_any_element()
        }
        Some(TabContent::Browser { page, forward }) => browser_tab(connected, workspace, id, tab_id, &page, forward, cx),
        Some(TabContent::WebApp { app, server, forward }) => {
            web_app_tab(connected, shown, workspace, id, tab_id, &app, server, forward, window, cx)
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
            let view = connected.tool_views.render(&session, workspace, id, tab_id, tool, window, cx);
            div().size_full().child(view).into_any_element()
        }
        Some(TabContent::StartingTerminal) => {
            centered(div().text_color(secondary).child("Starting terminal…"))
        }
        Some(TabContent::StartingHarness) => harness_message(0x888888, "Starting AI harness..."),
        Some(TabContent::InvalidHarnessState) => harness_message(0xf44747, "Error: Invalid AI harness state"),
        Some(TabContent::Failed(message)) => centered(div().text_color(error).child(message)),
        Some(TabContent::NotPorted(app)) => centered(
            div()
                .text_color(secondary)
                .child(format!("“{app}” tabs are not available in the native app yet")),
        ),
        None => centered(div().text_color(secondary).child("No tabs")),
    }
}

/// `AiHarness`'s messages in place of its terminal: top-left with 16px padding, in the
/// body's 16px text.
fn harness_message(color: u32, text: &'static str) -> AnyElement {
    div()
        .size_full()
        .p(px(16.))
        .text_size(px(16.))
        .line_height(crate::ui::normal_line(16.))
        .text_color(gpui_kit::rgb(color))
        .child(text)
        .into_any_element()
}

/// A tab being drawn in its tabset: the terminals the last render showed, and whether it is
/// the workspace's active tab.
#[derive(Clone, Copy)]
struct ShownTab<'a> {
    previous_terminals: &'a HashSet<EntityId>,
    focus: Focus,
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

/// Which opening of a Web App's or Browser tab's page a view shows: the page model, and its
/// `page_mount`.
type PageMount = (EntityId, u64);

/// The page of a Web App or Browser tab once it is ready (`WebView`, mounted in Ready only):
/// a fresh view each time the page opens anew (`mount`), on the page the tab remembers on top
/// of the start page `home` (page memory).
fn page_view(connected: &mut Connected, key: (String, String), workspace: &Entity<WorkspaceStore>, home: String, mount: PageMount, cx: &mut Context<AppView>) -> AnyElement {
    if let Some((opened, view)) = connected.browser_views.get(&key)
        && *opened == mount
    {
        return div().size_full().child(view.clone()).into_any_element();
    }
    let tab_id = key.1.clone();
    let url = match workspace.read(cx).page_url(&tab_id, &home) {
        Ok(url) => url,
        Err(message) => {
            connected.browser_views.remove(&key);
            return div().p_4().text_color(colors(cx).error).child(message).into_any_element();
        }
    };
    let workspace = workspace.downgrade();
    let view = cx.new(|_| crate::browser_view::BrowserView::new(url, home, workspace, tab_id));
    connected.browser_views.insert(key, (mount, view.clone()));
    div().size_full().child(view).into_any_element()
}

/// Outside Ready the page is closed (`WebView` unmounts): the native view would draw over
/// the waiting or error view shown instead. It opens anew once ready.
fn close_page(connected: &mut Connected, key: &(String, String)) {
    connected.browser_views.remove(key);
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
    let phase = page.read(cx).phase().clone();
    if !matches!(phase, BrowserPhase::Ready { .. }) {
        close_page(connected, &key);
    }
    let browser = match phase {
        BrowserPhase::Ready { home } => {
            let mount = (page.entity_id(), page.read(cx).page_mount());
            page_view(connected, key.clone(), workspace, home, mount, cx)
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
    shown: ShownTab<'_>,
    workspace: &Entity<treeterm_model::workspace_store::WorkspaceStore>,
    workspace_id: &str,
    tab_id: &str,
    app: &Entity<treeterm_model::web_app::WebAppModel>,
    server: treeterm_model::workspace_store::ServerTerminal,
    forward: PageHost,
    window: &mut Window,
    cx: &mut Context<AppView>,
) -> AnyElement {
    use treeterm_model::web_app::WebAppPhase;
    let c = colors(cx);
    let (secondary, error) = (c.text_secondary, c.error);
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
        use treeterm_model::workspace_store::ServerTerminal;
        match server {
            ServerTerminal::Attached(model) => {
                let view = connected
                    .terminal_views
                    .entry(model.entity_id())
                    .or_insert_with(|| cx.new(|cx| TerminalView::new(model.clone(), window, cx)))
                    .clone();
                show_terminal(connected, shown.previous_terminals, model.entity_id(), &view, shown.focus, window, cx);
                v_flex()
                    .size_full()
                    .child(div().flex_1().min_h_0().child(view))
                    .child(terminal_status_bar(workspace, tab_id, cx))
                    .into_any_element()
            }
            // `Terminal` before its tab has a PTY: plain text at the top left. A PTY that
            // failed to start says why (the Browser sub-tab has Retry).
            ServerTerminal::Starting => {
                div().p(px(16.)).text_color(secondary).child("Creating terminal...").into_any_element()
            }
            ServerTerminal::Failed(message) => div().p(px(16.)).text_color(error).child(message).into_any_element(),
        }
    } else {
        let phase = app.read(cx).phase().clone();
        if !matches!(phase, WebAppPhase::Ready { .. }) {
            close_page(connected, &key);
        }
        match phase {
            WebAppPhase::Ready { home } => {
                let mount = (app.entity_id(), app.read(cx).page_mount());
                page_view(connected, key.clone(), workspace, home, mount, cx)
            }
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
                // `.terminal-floating-buttons button` (0.7) outranks the button's own 0.9.
                .opacity(0.7)
                .cursor_pointer()
                .hover(|d| d.bg(gpui_kit::rgb(0xffb820)).border_color(gpui_kit::rgb(0xf0a000)).opacity(1.))
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(title).build(window, cx))
                .child(prompt.label())
                .on_click(move |_, window, cx| {
                    let task = treeterm_model::harness_prompts::send(&prompt, &session, &id, cx);
                    notify_failure(task, window, cx);
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

/// Show the terminal of `model` in its tabset. Electron mounts only the terminals of the tabs
/// its tabsets show, so one not shown in the last render comes into view as a mount does (it
/// fits again and, as the active tab, takes keyboard focus): after its tab is added or picked
/// here or in another window, a workspace switch, its tabset restored, or the tab shown
/// before it closing or its program exiting. A tab or workspace switch that asked for focus
/// also focuses the active tab's terminal when it stays shown.
fn show_terminal(
    connected: &mut Connected,
    previous: &HashSet<EntityId>,
    model: EntityId,
    view: &Entity<TerminalView>,
    focus: Focus,
    window: &mut Window,
    cx: &mut Context<AppView>,
) {
    connected.shown_terminals.insert(model);
    if !previous.contains(&model) {
        view.update(cx, |view, cx| view.on_shown(focus, window, cx));
    }
    if focus == Focus::Take && std::mem::take(&mut connected.focus_pending) {
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
        use treeterm_model::app_store::ConnState;
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
                "settings": self.settings_dialog.is_open(),
                "activeProcesses": active_processes,
                "closeConfirm": self.close_confirm.strong_count() > 0,
            })
        };
        // Whether the window shows the session described (else the idle pane, or a session
        // pane).
        let shown = self.info.is_none() && self.current().is_some();
        match self.automation_target().and_then(|id| self.sessions.get(&id)) {
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
                let active = json!(session.active_workspace());
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
                    "shown": shown,
                    "terminals": terminals,
                    "dialogs": dialogs,
                })
            }
        }
    }

    /// The session automation acts on: the one shown, else (nothing picked yet) the first in
    /// the sidebar, as Electron's `getInitialWorkspace` falls back to the first session.
    fn automation_target(&self) -> Option<ConnectionId> {
        self.active
            .clone()
            .filter(|id| self.sessions.contains_key(id))
            .or_else(|| self.sessions.keys().next().cloned())
    }

    pub fn automation_select_workspace(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(connection) = self.automation_target() {
            self.select_workspace(&connection, id.to_owned(), cx);
        }
    }

    /// The automation target's sidebar section.
    fn automation_tree(&self) -> Option<Entity<TreePane>> {
        let id = self.automation_target()?;
        self.trees.get(&id).map(|tree| tree.pane.clone())
    }

    pub fn automation_select_tab(&mut self, workspace: &str, tab: &str, cx: &mut Context<Self>) {
        let target = self.automation_target();
        if let Some(connected) = target.and_then(|id| self.sessions.get_mut(&id)) {
            if let Some(store) = connected.session.read(cx).workspace_store(workspace).cloned() {
                store.update(cx, |w, cx| w.set_active_tab(tab, cx));
            }
            connected.focus_pending = true;
            cx.notify();
        }
    }

    pub fn automation_new_tab(&mut self, workspace: &str, app: &str, cx: &mut Context<Self>) -> Option<String> {
        let target = self.automation_target()?;
        let connected = self.sessions.get_mut(&target)?;
        let store = connected.session.read(cx).workspace_store(workspace).cloned()?;
        let app = if app.is_empty() {
            connected.session.read(cx).registry().default_app()?.id.clone()
        } else {
            app.to_owned()
        };
        connected.focus_pending = true;
        store.update(cx, |w, cx| w.add_tab(&app, cx))
    }

    /// The automation target's tree forks `id` (its Fork button, past the upstream warning).
    pub fn automation_fork(&mut self, id: &str, cx: &mut Context<Self>) -> bool {
        let Some(tree) = self.automation_tree() else {
            return false;
        };
        tree.update(cx, |tree, cx| tree.quick_fork(id, cx));
        true
    }

    pub fn automation_session(&self) -> Option<Entity<SessionStore>> {
        let target = self.automation_target()?;
        self.sessions.get(&target).map(|connected| connected.session.clone())
    }

    /// "+" on the automation target: the Open Workspace dialog.
    pub fn automation_open_workspace_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(tree) = self.automation_tree() else {
            return false;
        };
        tree.update(cx, |tree, cx| tree.open_workspace(window, cx));
        true
    }

    pub fn automation_open_workspace(&mut self, path: String, cx: &mut Context<Self>) -> Option<String> {
        let connection = self.automation_target()?;
        let session = self.sessions.get(&connection)?.session.clone();
        let id = session.update(cx, |s, cx| s.open_workspace(path, WorktreeSettings::default(), cx));
        self.select_workspace(&connection, id.clone(), cx);
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
        if let Some(connected) = self.automation_target().and_then(|id| self.sessions.get(&id))
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

    fn keybinding_store(cx: &mut VisualTestContext) -> Entity<KeybindingStore> {
        cx.update(|_, cx| cx.new(|_| KeybindingStore::new(treeterm_core::keybinding::KeybindingSettings::default())))
    }

    /// A store over `dir` and the Settings dialog on it.
    fn open_settings(
        cx: &mut VisualTestContext,
        dir: &std::path::Path,
    ) -> (Entity<SettingsStore>, Entity<crate::settings_view::SettingsView>) {
        let dir = dir.to_owned();
        let store = cx.update(|_, cx| cx.new(|cx| SettingsStore::new(Some(dir), cx)));
        let keybindings = keybinding_store(cx);
        let mut dialog = crate::settings_view::SettingsDialog::default();
        cx.update(|window, cx| dialog.open(store.clone(), keybindings, window, cx));
        cx.run_until_parked();
        (store, dialog.view().expect("Settings is open"))
    }

    fn click_save(cx: &mut VisualTestContext, view: &Entity<crate::settings_view::SettingsView>) {
        cx.update(|window, cx| view.update(cx, |view, cx| view.save(window, cx)));
    }

    /// `handleSave`: the dialog closes only once its write landed. A failed write keeps it
    /// open with that write's error and the settings as they were; Save again writes again.
    #[gpui_kit::test]
    fn settings_save_closes_once_written_and_stays_open_on_failure(cx: &mut TestAppContext) {
        use treeterm_core::settings::{self, Settings};
        let cx = window(cx);
        let dir = tempfile::tempdir().expect("temp dir");
        let mut stored = Settings::default();
        stored.terminal.max_cols = 120;
        settings::save(dir.path(), &stored).expect("write settings.json");
        let (store, view) = open_settings(cx, dir.path());

        // settings.json becomes a directory, so every write to it fails (even as root).
        let path = settings::settings_path(dir.path());
        std::fs::remove_file(&path).expect("remove settings.json");
        std::fs::create_dir(&path).expect("block settings.json");
        click_save(cx, &view);
        cx.run_until_parked();
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)), "stays open");
        let error = cx.update(|_, cx| view.read(cx).error().cloned()).expect("says why");
        assert!(error.starts_with("Could not save settings: cannot write"), "{error}");
        assert_eq!(*cx.update(|_, cx| store.read(cx).settings().clone()), stored);

        std::fs::remove_dir(&path).expect("unblock settings.json");
        click_save(cx, &view);
        cx.run_until_parked();
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)), "closed once written");
        assert_eq!(settings::load(dir.path()).expect("written"), stored, "the form's values");
    }

    /// Save with no edits writes even when nothing changed, as Electron's `settings:save`
    /// does, so a file that could not be loaded is replaced and the dialog closes, rather
    /// than showing the load's error.
    #[gpui_kit::test]
    fn settings_save_without_edits_replaces_an_unloadable_file(cx: &mut TestAppContext) {
        use treeterm_core::settings::{self, Settings};
        let cx = window(cx);
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(settings::settings_path(dir.path()), "{ not json").expect("write settings.json");
        let (store, view) = open_settings(cx, dir.path());
        assert!(cx.update(|_, cx| store.read(cx).error().is_some()), "the load failed");

        click_save(cx, &view);
        cx.run_until_parked();
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)), "closed once written");
        assert_eq!(settings::load(dir.path()).expect("written"), Settings::default());
    }

    /// Save clicked again while its write runs is ignored: one write, and the dialog closes
    /// once, leaving the dialog under Settings open.
    #[gpui_kit::test]
    fn settings_save_is_ignored_while_one_is_written(cx: &mut TestAppContext) {
        let cx = window(cx);
        let unmerged = vec![("child".to_owned(), Some("child".to_owned()))];
        let below = cx.update(|window, cx| crate::active_processes_view::confirm_close(unmerged, CloseAnyway::Quit, window, cx));
        let dir = tempfile::tempdir().expect("temp dir");
        let (store, view) = open_settings(cx, dir.path());
        assert_eq!(cx.update(|window, cx| window.active_dialog_count(cx)), 2);
        // The store notifies once per finished write.
        let writes = std::rc::Rc::new(std::cell::Cell::new(0));
        let counted = writes.clone();
        cx.update(|_, cx| cx.observe(&store, move |_, _| counted.set(counted.get() + 1)).detach());

        click_save(cx, &view);
        click_save(cx, &view);
        cx.run_until_parked();
        assert_eq!(writes.get(), 1, "written once");
        assert_eq!(cx.update(|window, cx| window.active_dialog_count(cx)), 1, "Settings closed once");
        assert_eq!(below.strong_count(), 1, "the confirmation stays");
    }

    /// A 200px-tall loading box of `lines` lines, as a loading workspace shows it.
    struct LoadingBox {
        workspace: &'static str,
        lines: usize,
        output: LoadingOutput,
    }

    impl Render for LoadingBox {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.output.follow(self.workspace, self.lines);
            let lines = (0..self.lines).map(|ix| format!("line {ix}")).collect();
            v_flex().h(px(200.)).child(loading_output(lines, &self.output.scroll, cx))
        }
    }

    fn draw(cx: &mut VisualTestContext) {
        for _ in 0..3 {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.draw(cx).clear(cx);
                window.simulate_next_frame(cx);
            });
        }
    }

    /// How far the box is scrolled, and how far it can be.
    fn scrolled(view: &Entity<LoadingBox>, cx: &mut VisualTestContext) -> (gpui_kit::Pixels, gpui_kit::Pixels) {
        cx.update(|_, cx| {
            let scroll = &view.read(cx).output.scroll;
            (-scroll.offset().y, scroll.max_offset().y)
        })
    }

    #[gpui_kit::test]
    fn the_loading_output_follows_its_newest_line(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(crate::theme::install);
        let (view, cx) =
            cx.add_window_view(|_, _| LoadingBox { workspace: "a", lines: 100, output: LoadingOutput::default() });
        draw(cx);
        let (offset, max) = scrolled(&view, cx);
        assert!(max > px(0.), "100 lines overflow the box");
        assert_eq!(offset, max, "shown at its newest line");

        // Scrolled up, it stays while no line arrives.
        view.update(cx, |view, cx| {
            view.output.scroll.set_offset(gpui_kit::point(px(0.), px(0.)));
            cx.notify();
        });
        draw(cx);
        assert_eq!(scrolled(&view, cx).0, px(0.));

        // A new line brings the bottom back.
        view.update(cx, |view, cx| {
            view.lines += 1;
            cx.notify();
        });
        draw(cx);
        let (offset, max) = scrolled(&view, cx);
        assert_eq!(offset, max);

        // So does another workspace's box, scrolled up as this one was.
        view.update(cx, |view, cx| {
            view.output.scroll.set_offset(gpui_kit::point(px(0.), px(0.)));
            view.workspace = "b";
            cx.notify();
        });
        draw(cx);
        let (offset, max) = scrolled(&view, cx);
        assert_eq!(offset, max);
    }

    /// A loading workspace's pane with `lines` lines of git output so far.
    struct LoadingPane {
        lines: usize,
        output: LoadingOutput,
    }

    impl Render for LoadingPane {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let lines: Vec<String> = (0..self.lines).map(|ix| format!("line {ix}")).collect();
            v_flex().h(px(300.)).child(loading_pane("a", "Creating worktree...", &lines, &mut self.output, cx))
        }
    }

    /// `AutoScrollPre` is drawn only once there is output, and shows all of it.
    #[gpui_kit::test]
    fn the_loading_pane_shows_all_of_gits_output_once_there_is_some(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(crate::theme::install);
        let (view, cx) = cx.add_window_view(|_, _| LoadingPane { lines: 0, output: LoadingOutput::default() });
        draw(cx);
        // The box's scroll handle sees each line it lays out.
        let box_lines = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                let scroll = &view.read(cx).output.scroll;
                (scroll.bounds().size.height > px(0.)).then(|| scroll.children_count())
            })
        };
        assert_eq!(box_lines(cx), None, "no box before output");

        view.update(cx, |view, cx| {
            view.lines = 300;
            cx.notify();
        });
        draw(cx);
        assert_eq!(box_lines(cx), Some(300));
    }

    #[gpui_kit::test]
    fn the_close_confirmation_handle_lives_while_its_dialog_is_open(cx: &mut TestAppContext) {
        let cx = window(cx);
        let unmerged = vec![("child".to_owned(), Some("child".to_owned()))];
        let confirm = cx.update(|window, cx| crate::active_processes_view::confirm_close(unmerged, CloseAnyway::Quit, window, cx));
        cx.run_until_parked();
        assert_eq!(confirm.strong_count(), 1, "open");

        // What its Cancel button does.
        cx.update(|window, cx| window.close_dialog(cx));
        cx.run_until_parked();
        assert_eq!(confirm.strong_count(), 0, "closed");
    }

    /// An app store whose local daemon is nowhere (its connection fails at once), on a runtime
    /// that outlives the test.
    fn app_store(cx: &mut App, dir: &std::path::Path) -> Entity<AppStore> {
        static RUNTIME: std::sync::LazyLock<tokio::runtime::Runtime> =
            std::sync::LazyLock::new(|| tokio::runtime::Runtime::new().expect("a tokio runtime"));
        let rt = treeterm_core::rt::Rt::new(RUNTIME.handle().clone());
        let spec = treeterm_core::local_daemon::LocalDaemonSpec {
            socket_path: dir.join("none.sock"),
            binary: dir.join("never-spawned"),
            log_path: dir.join("daemon.log"),
            pid_file: dir.join("daemon.pid"),
        };
        let settings = cx.new(|_| SettingsStore::in_memory(Default::default()));
        cx.new(|cx| AppStore::new(rt, spec, settings, cx))
    }

    /// Exit and Kill Daemon while the close confirmation is open asks it again: its Close
    /// Anyway stops the daemon too, and a Quit after that leaves it so.
    #[gpui_kit::test]
    fn exit_and_kill_daemon_while_the_close_confirmation_is_open_makes_it_stop_the_daemon(cx: &mut TestAppContext) {
        // The store's connection is driven on tokio, which wakes its status task from there.
        cx.executor().allow_parking();
        let cx = window(cx);
        let dir = tempfile::tempdir().expect("temp dir");
        let store = cx.update(|_, cx| app_store(cx, dir.path()));
        let unmerged = vec![("child".to_owned(), Some("child".to_owned()))];
        let confirm = cx.update(|window, cx| crate::active_processes_view::confirm_close(unmerged, CloseAnyway::Quit, window, cx));
        cx.run_until_parked();
        let open = confirm.upgrade().expect("open");
        assert!(matches!(open.close_anyway(), CloseAnyway::Quit));

        open.ask_again(CloseAnyway::KillDaemonAndQuit(store.clone()));
        assert!(matches!(open.close_anyway(), CloseAnyway::KillDaemonAndQuit(kills) if kills == store));
        open.ask_again(CloseAnyway::Quit);
        assert!(matches!(open.close_anyway(), CloseAnyway::KillDaemonAndQuit(kills) if kills == store));
        assert_eq!(cx.update(|window, cx| window.active_dialog_count(cx)), 1, "still the one dialog");
    }

    /// Sessions "a" (0–100px) and "b" (100–200px), stacked as in the sidebar.
    fn two_sections() -> [(ConnectionId, gpui_kit::Bounds<gpui_kit::Pixels>); 2] {
        let section = |id: &str, top: f32| {
            let bounds = gpui_kit::Bounds::new(gpui_kit::point(px(0.), px(top)), gpui_kit::size(px(200.), px(100.)));
            (ConnectionId::new(id), bounds)
        };
        [section("a", 0.), section("b", 100.)]
    }

    /// Every section hears the move to `y` (`onDragOver` fires on the one under the pointer):
    /// whether each one's hint changed.
    fn move_to(drag: &mut SessionDrag, sections: &[(ConnectionId, gpui_kit::Bounds<gpui_kit::Pixels>)], y: f32) -> Vec<bool> {
        sections.iter().map(|(id, bounds)| drag.moved(id, *bounds, gpui_kit::point(px(10.), px(y)))).collect()
    }

    /// `dragState`: the dragged session's section dims; over another session's top half it
    /// would land before it, over its bottom half after it, and over its own section, or
    /// outside every section, nowhere.
    #[test]
    fn a_dragged_session_dims_and_would_land_beside_the_section_under_the_pointer() {
        let sections = two_sections();
        let (a, b) = (sections[0].0.clone(), sections[1].0.clone());
        let mut drag = SessionDrag::Idle;
        assert_eq!(move_to(&mut drag, &sections, 10.), [false, false], "nothing dragged");
        assert!(!drag.dims(&a) && !drag.dims(&b));

        drag = SessionDrag::Dragging { dragged: b.clone(), over: None };
        assert!(drag.dims(&b) && !drag.dims(&a));
        assert_eq!(move_to(&mut drag, &sections, 150.), [false, false], "over itself");
        assert_eq!((drag.hint(&a), drag.hint(&b)), (None, None));

        assert_eq!(move_to(&mut drag, &sections, 49.), [true, false]);
        assert_eq!((drag.hint(&a), drag.hint(&b)), (Some(Side::Before), None));
        assert_eq!(move_to(&mut drag, &sections, 20.), [false, false], "the same side");
        assert_eq!(move_to(&mut drag, &sections, 50.), [true, false]);
        assert_eq!(drag.hint(&a), Some(Side::After));

        // Back over itself, or out of every section: no line.
        assert_eq!(move_to(&mut drag, &sections, 150.), [true, false]);
        assert_eq!(drag.hint(&a), None);
        assert_eq!(move_to(&mut drag, &sections, 60.), [true, false]);
        assert_eq!(move_to(&mut drag, &sections, 250.), [true, false]);
        assert_eq!(drag.hint(&a), None);
        assert!(drag.dims(&b), "still dragged");
    }

    /// `handleSessionDrop`: dropped where the line shows, the session moves to that side;
    /// dropped on itself it stays. Either way the drag ends.
    #[test]
    fn a_dropped_session_lands_where_the_line_showed_and_the_drag_ends() {
        let sections = two_sections();
        let (a, b) = (sections[0].0.clone(), sections[1].0.clone());
        let mut drag = SessionDrag::Dragging { dragged: b.clone(), over: None };
        assert_eq!(move_to(&mut drag, &sections, 80.), [true, false]);
        assert_eq!(drag.drop_on(&a), Some(Side::After));
        assert!(matches!(drag, SessionDrag::Idle));
        assert!(!drag.dims(&b));

        drag = SessionDrag::Dragging { dragged: b.clone(), over: None };
        assert_eq!(move_to(&mut drag, &sections, 150.), [false, false]);
        assert_eq!(drag.drop_on(&b), None, "dropped on itself");
        assert!(matches!(drag, SessionDrag::Idle));

        assert_eq!(drag.drop_on(&a), None, "nothing dragged");
    }

    /// The main window's view over a store whose daemon is nowhere, drawn.
    fn app_view<'a>(cx: &'a mut TestAppContext, dir: &std::path::Path) -> (Entity<AppView>, &'a mut VisualTestContext) {
        let (window, view) = cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::install(cx);
            let store = app_store(cx, dir);
            let keybindings = cx.new(|_| KeybindingStore::new(treeterm_core::keybinding::KeybindingSettings::default()));
            let settings = cx.new(|_| SettingsStore::in_memory(Default::default()));
            let names = cx.new(|_| treeterm_model::session_names::SessionNames::new(None));
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| AppView::new(store, keybindings, settings, names, window, cx))
            })
            .expect("open a window")
        });
        let cx = VisualTestContext::from_window(window, cx).into_mut();
        draw(cx);
        (view, cx)
    }

    /// `isResizing` (`.divider.active`): once dragged, the sidebar divider stays
    /// accent-coloured wherever the pointer goes, past the 400px clamp included, until the
    /// button comes up.
    #[gpui_kit::test]
    fn the_sidebar_divider_stays_active_while_dragged(cx: &mut TestAppContext) {
        // The store's connection is driven on tokio, which wakes its status task from there.
        cx.executor().allow_parking();
        let dir = tempfile::tempdir().expect("temp dir");
        let (view, cx) = app_view(cx, dir.path());
        let state = |cx: &mut VisualTestContext| cx.update(|_, cx| (view.read(cx).resizing, view.read(cx).sidebar_width));
        let (left, none) = (gpui_kit::MouseButton::Left, gpui_kit::Modifiers::default());
        let at = |x: f32| gpui_kit::point(px(x), px(300.));

        // The divider sits right of the 250px sidebar. A press alone is no drag.
        cx.simulate_mouse_move(at(252.), None, none);
        cx.simulate_mouse_down(at(252.), left, none);
        draw(cx);
        assert_eq!(state(cx), (false, 250.), "pressed");

        // The first move past the threshold starts the drag; the next ones resize.
        cx.simulate_mouse_move(at(300.), left, none);
        cx.simulate_mouse_move(at(320.), left, none);
        draw(cx);
        assert_eq!(state(cx), (true, 320.), "dragged");
        cx.simulate_mouse_move(at(600.), left, none);
        draw(cx);
        assert_eq!(state(cx), (true, 400.), "off the divider, past the clamp: still active");

        cx.simulate_mouse_up(at(600.), left, none);
        draw(cx);
        assert_eq!(state(cx), (false, 400.), "let go");
    }
}
