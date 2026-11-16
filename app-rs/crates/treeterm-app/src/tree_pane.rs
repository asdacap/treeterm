//! Left pane: a connection's section of the sidebar (`SessionPanel`), whatever the
//! connection's state: its header, banner and workspace tree.

use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Context, Entity, EventEmitter, SharedString, Window, div, px};
use treeterm_core::connection::{ConnectPhase, ConnectionId, OutdatedDaemon};
use treeterm_model::app_store::{AppStore, ConnState};
use treeterm_model::keybinding_store::KeybindingStore;
use treeterm_core::tree::DropPosition;
use treeterm_model::session_store::{RemoveMode, SessionState, SessionStore, WorkspaceEntry};
use treeterm_model::tree_view_state::TreeViewState;

use crate::theme::colors;
use crate::ui;
use gpui_kit::assets::IconName;

/// Emitted when the user picks a workspace.
pub struct WorkspaceSelected(pub String);

/// One per connection, from its first connect until it is disconnected: the fold, the
/// workspaces' expand state and a rename in progress outlive the connection's generations.
pub struct TreePane {
    /// The session store the tree shows.
    session: Shown,
    /// The session's connection: the header shows its status, and while it is down the
    /// tree shows its banner and dims the rows.
    app: Entity<AppStore>,
    connection: ConnectionId,
    /// Bytes per second received from the connection's daemon, across its generations.
    traffic: Entity<crate::traffic_badge::TrafficBadge>,
    keybindings: Entity<KeybindingStore>,
    /// The session's name: custom, else "LOCAL" or the ssh label.
    names: Entity<treeterm_model::session_names::SessionNames>,
    target: treeterm_core::connection::ConnectionTarget,
    /// Renaming the session in place (double-click its name); saved on Enter or blur.
    renaming: Option<(Entity<InputState>, gpui_kit::Subscription)>,
    /// A remote session: "+" browses the remote filesystem instead of the native folder picker.
    remote: bool,
    /// The whole session's rows are hidden.
    folded: bool,
    /// Where a workspace being dragged would land.
    drop_hint: Option<(String, DropPosition)>,
    /// The row under the pointer: it and the rows under it are tinted (`.tree-subtree`).
    hovered: Option<String>,
    /// Per-window view state: which workspaces have their children hidden.
    view: TreeViewState,
}

/// The session store a connection's tree shows (`SessionSlot::shown_session`).
enum Shown {
    /// None yet: the connection's first connect is under way or failed.
    Nothing,
    /// The connected generation's store, or the last one's while the connection is down.
    Session {
        store: Entity<SessionStore>,
        _changes: gpui_kit::Subscription,
    },
}

impl EventEmitter<WorkspaceSelected> for TreePane {}

/// A workspace being dragged in the tree (and its drag preview).
#[derive(Clone)]
pub struct DraggedWorkspace {
    id: String,
    label: SharedString,
}

impl Render for DraggedWorkspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        div().px_2().py_1().rounded_sm().bg(c.bg_active).text_sm().child(self.label.clone())
    }
}

/// Top quarter: before; bottom quarter: after; otherwise onto (as a child).
fn drop_zone(y: f32, top: f32, height: f32) -> DropPosition {
    let offset = y - top;
    if offset < height / 4. {
        DropPosition::Before
    } else if offset > height * 3. / 4. {
        DropPosition::After
    } else {
        DropPosition::Onto
    }
}

/// Emitted when the user clicks the session's name: show its session pane.
pub struct SessionInfoRequested;

impl EventEmitter<SessionInfoRequested> for TreePane {}

/// The session header's "Disconnect".
pub struct DisconnectRequested;

impl EventEmitter<DisconnectRequested> for TreePane {}

impl TreePane {
    /// The section of connection `connection`, showing no session until
    /// [`Self::set_session`] gives it one. `traffic` is the connection's download meter.
    pub fn new(
        app: Entity<AppStore>,
        connection: ConnectionId,
        traffic: Entity<treeterm_model::traffic::TrafficMeter>,
        keybindings: Entity<KeybindingStore>,
        names: Entity<treeterm_model::session_names::SessionNames>,
        target: treeterm_core::connection::ConnectionTarget,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&app, |_, _, cx| cx.notify()).detach();
        cx.observe(&keybindings, |_, _, cx| cx.notify()).detach();
        cx.observe(&names, |_, _, cx| cx.notify()).detach();
        let remote = matches!(target, treeterm_core::connection::ConnectionTarget::Remote(_));
        let traffic = cx.new(|cx| crate::traffic_badge::TrafficBadge::new(traffic, "connection-traffic", cx));
        Self {
            session: Shown::Nothing,
            app,
            connection,
            traffic,
            keybindings,
            names,
            target,
            renaming: None,
            remote,
            folded: false,
            drop_hint: None,
            hovered: None,
            view: TreeViewState::new(Default::default()),
        }
    }

    /// The session store the connection's slot shows now (`SessionSlot::shown_session`).
    /// A new generation's store keeps the fold, what is collapsed and a rename in progress.
    pub fn set_session(&mut self, session: Option<Entity<SessionStore>>, cx: &mut Context<Self>) {
        if self.store() == session.as_ref() {
            return;
        }
        self.drop_hint = None;
        self.view.reload();
        self.session = match session {
            None => Shown::Nothing,
            Some(store) => {
                // A collapsed workspace opens when it gains a child, whoever added it.
                let changes = cx.observe(&store, |pane, store, cx| {
                    let edges = store.read(cx).child_edges(cx);
                    pane.view.observe(edges);
                    cx.notify();
                });
                self.view.observe(store.read(cx).child_edges(cx));
                Shown::Session { store, _changes: changes }
            }
        };
        cx.notify();
    }

    /// The session store shown, if any.
    fn store(&self) -> Option<&Entity<SessionStore>> {
        match &self.session {
            Shown::Nothing => None,
            Shown::Session { store, .. } => Some(store),
        }
    }

    /// Edit the session's name in place (`SessionPanel` double-click), its text selected
    /// (`onFocus` select).
    fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.names.read(cx).name(&self.target);
        let input = cx.new(|cx| InputState::new(window, cx).default_value(current));
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        let subscription = cx.subscribe(&input, |pane, input, event: &InputEvent, cx| match event {
            InputEvent::PressEnter { .. } | InputEvent::Blur => {
                let text = input.read(cx).value().to_string();
                pane.renaming = None;
                let target = pane.target.clone();
                pane.names.update(cx, |names, cx| names.rename(&target, &text, cx));
                cx.notify();
            }
            InputEvent::Change | InputEvent::Focus => {}
        });
        self.renaming = Some((input, subscription));
        cx.notify();
    }

    fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        self.view.toggle(id);
        cx.notify();
    }

    /// A row's Fork (`handleQuickFork`): the new child is shown, Loading first, as soon as
    /// it is added (`createChildWithLoading` makes it the active workspace). A failure is
    /// only logged, as in Electron.
    pub fn quick_fork(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(session) = self.store() else {
            return; // no session: no row to fork
        };
        let forked = session.update(cx, |s, cx| s.quick_fork(id, cx));
        cx.spawn(async move |pane, cx| match forked.await {
            Ok(Ok(child)) => match pane.update(cx, |pane, cx| pane.select(child, cx)) {
                Ok(()) | Err(_) => {} // Err: the pane closed
            },
            Ok(Err(error)) => tracing::error!(%error, "quick fork"),
            Err(_) => tracing::debug!("quick fork dropped with the session store"),
        })
        .detach();
    }

    /// "+": type or pick a directory and open it as a root workspace with the chosen default
    /// application (`OpenWorkspaceDialog`, `handleOpenWorkspaceSubmit`).
    pub fn open_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.store().cloned() else {
            return; // "+" shows only once connected
        };
        let pane = cx.entity().downgrade();
        let location = if self.remote {
            crate::open_workspace_dialog::Location::Remote(session.clone())
        } else {
            crate::open_workspace_dialog::Location::Local(self.app.read(cx).settings().clone())
        };
        let recent = self.names.read(cx).recent_directories(&self.target).to_vec();
        let choices = treeterm_model::tool_tabs::workspace_settings::default_app_choices(session.read(cx).registry());
        crate::open_workspace_dialog::open(
            location,
            recent,
            choices,
            std::rc::Rc::new(move |path, settings, _window, cx| {
                match pane.update(cx, |pane, cx| {
                    // `handleOpen`: remembered for this connection's next Open Workspace.
                    let target = pane.target.clone();
                    pane.names.update(cx, |names, cx| names.add_recent_directory(&target, &path, cx));
                    // The session shown now: a reconnect while the dialog was open replaced
                    // the one it was opened on.
                    let Some(session) = pane.store().cloned() else { return };
                    let id = session.update(cx, |s, cx| s.open_workspace(path, settings, cx));
                    pane.select(id, cx);
                }) {
                    Ok(()) | Err(_) => {} // Err: the pane closed
                }
            }),
            window,
            cx,
        );
    }

    /// Pick workspace `id`: the window shows it. (The row of the session's active workspace
    /// is highlighted while the window shows the session.)
    pub fn select(&mut self, id: String, cx: &mut Context<Self>) {
        cx.emit(WorkspaceSelected(id));
    }
}

impl Render for TreePane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (text_secondary, hover, accent, primary, border) = (c.text_secondary, c.bg_hover, c.accent, c.text_primary, c.border);
        // The connection (`SessionPanel`'s `connection.status`); `None` only while this view
        // is being dropped with its connection.
        let link = self.app.read(cx).session_slot(&self.connection).map(|slot| slot.state.clone());
        let body = match (&link, &self.session) {
            // `renderStatusContent`: the connect phase in place of the rows, a `.tree-empty`
            // line (centred) with 12px text and 4px 8px padding.
            (Some(ConnState::Connecting(phase)), _) => Some(
                div()
                    .px(px(8.))
                    .py(px(4.))
                    .text_size(px(12.))
                    .text_color(text_secondary)
                    .text_center()
                    .child(connect_phase_label(*phase))
                    .into_any_element(),
            ),
            // No session yet (its first connect failed): the banner only.
            (_, Shown::Nothing) => None,
            (_, Shown::Session { store, .. }) => Some(self.tree(&store.clone(), link.as_ref(), window, cx)),
        };
        let banner = self.connection_banner(link.as_ref(), cx);
        let status_icon = status_icon(link.as_ref(), cx);
        let connected = matches!(link, Some(ConnState::Connected(_)));
        let locked = self.store().filter(|session| session.read(cx).is_locked()).cloned();
        let title = match &self.renaming {
            Some((input, _)) => div()
                .id("session-rename")
                .flex_1()
                // Selecting text must not drag the session.
                .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_: &ClickEvent, _, cx| cx.stop_propagation())
                .on_key_down(cx.listener(|pane, event: &gpui_kit::KeyDownEvent, _, cx| {
                    if event.keystroke.key == "escape" {
                        pane.renaming = None; // dropping the subscription discards the edit
                        cx.notify();
                    }
                }))
                .child(Input::new(input).xsmall())
                .into_any_element(),
            // `.tree-title`: status dot, name, traffic, force-unlock.
            None => h_flex()
                .id("session-title")
                .flex_1()
                .min_w_0()
                .gap(px(6.))
                .items_center()
                .text_size(px(ui::SMALL))
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(text_secondary)
                .child(status_icon)
                .child(div().truncate().child(self.names.read(cx).name(&self.target).to_uppercase()))
                .child(self.traffic.clone())
                .when_some(locked, |d, session| {
                    d.child(
                        div()
                            .id("force-unlock")
                            .p(px(2.))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .hover(move |d| d.bg(hover))
                            .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Force unlock session").build(window, cx))
                            .child(ui::icon(IconName::LockOpen, 14., primary))
                            .on_click(move |_: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                session.update(cx, |s, cx| s.force_unlock(cx));
                            }),
                    )
                })
                .on_click(cx.listener(|pane, event: &ClickEvent, window, cx| {
                    if event.click_count() == 2 {
                        cx.stop_propagation();
                        pane.start_rename(window, cx);
                    }
                }))
                .into_any_element(),
        };
        v_flex()
            .w_full()
            .border_b_1()
            .border_color(border)
            .child(
                // `.session-panel-header`: 40px, 8px 16px padding, 3px accent left border.
                h_flex()
                    .id("session-header")
                    .h(px(40.))
                    .pl(px(16.))
                    .pr(px(16.))
                    .gap(px(4.))
                    .items_center()
                    .border_l(px(3.))
                    .border_color(accent)
                    .cursor_pointer()
                    .hover(move |d| d.bg(hover))
                    .child(
                        div()
                            .id("session-fold")
                            .size(px(16.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .opacity(0.6)
                            .hover(|d| d.opacity(1.))
                            .child(ui::icon(if self.folded { IconName::ChevronRight } else { IconName::ChevronDown }, 16., primary))
                            .on_click(cx.listener(|pane, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                pane.folded = !pane.folded;
                                cx.notify();
                            })),
                    )
                    .child(title)
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(SessionInfoRequested)))
                    .context_menu({
                        let pane = cx.entity().downgrade();
                        move |menu, _, _| {
                            let pane = pane.clone();
                            menu.item(danger_item("Disconnect").on_click(move |_, _, cx| {
                                match pane.update(cx, |_, cx| cx.emit(DisconnectRequested)) {
                                    Ok(()) | Err(_) => {}
                                }
                            }))
                        }
                    })
                    .when(connected, |d| {
                        d.child(
                            ui::icon_button("open-workspace", cx)
                                .text_size(px(18.))
                                .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Add workspace").build(window, cx))
                                .child("+")
                                .on_click(cx.listener(move |pane, _: &ClickEvent, window, cx| {
                                    cx.stop_propagation();
                                    pane.open_workspace(window, cx);
                                })),
                        )
                    }),
            )
            // The banner folds with the rows (`.tree-list`).
            .when(!self.folded, |d| d.child(div().pb(px(4.)).children(banner).children(body)))
    }
}

impl TreePane {
    /// The session's rows, its loading line or its empty state.
    fn tree(&self, entity: &Entity<SessionStore>, link: Option<&ConnState>, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (text_secondary, error) = (c.text_secondary, c.error);
        let now = cx.background_executor().now();
        let session = entity.read(cx);
        let rows: Result<Vec<RowData>, AnyElement> = match session.state() {
            SessionState::Waiting => Err(status_line("Loading session…", text_secondary)),
            SessionState::Failed(reason) => Err(status_line(reason.clone(), error)),
            SessionState::Live => {
                let rows = session.tree_rows(self.view.collapsed(), cx);
                let hovered = self.hovered.as_deref().map_or(0..0, |id| treeterm_core::tree::subtree(&rows, id));
                Ok(rows
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, row)| Some(RowData {
                        hover: if !hovered.contains(&index) {
                            RowHover::Outside
                        } else if index == hovered.start {
                            RowHover::Hovered
                        } else {
                            RowHover::Under
                        },
                        activity: session
                            .workspace_store(&row.id)
                            .map(|w| w.read(cx).overall_activity())
                            .unwrap_or_default(),
                        is_worktree: session
                            .workspace_store(&row.id)
                            .is_some_and(|w| w.read(cx).file().is_worktree),
                        unread: session
                            .workspace_store(&row.id)
                            .is_some_and(|w| w.read(cx).has_unread_attention()),
                        attention_pending: session.workspace_store(&row.id).is_some_and(|w| w.read(cx).attention_pending()),
                        flash: session.workspace_store(&row.id).and_then(|w| w.read(cx).attention_flash(now)),
                        favourite: session.workspace_store(&row.id).is_some_and(|w| w.read(cx).is_favourite()),
                        has_parent: session
                            .workspace_store(&row.id)
                            .is_some_and(|w| w.read(cx).file().parent_id.as_deref().is_some_and(|p| !p.is_empty())),
                        pr: session.git_controller(&row.id).map_or_else(Vec::new, |git| pr_indicators(git.read(cx).status())),
                        git_repo: session.workspace_store(&row.id).is_some_and(|w| w.read(cx).file().is_git_repo),
                        label: Label::of(session.entry(&row.id)?, cx),
                        id: row.id,
                        depth: row.depth,
                        has_children: row.has_children,
                    }))
                    .collect())
            }
        };
        match rows {
            Err(status) => status,
            // `.tree-empty`: 16px padding, 12px secondary text, centred.
            Ok(rows) if rows.is_empty() => div()
                .p(px(16.))
                .text_size(px(12.))
                .text_color(text_secondary)
                .text_center()
                .child("No workspaces. Click + to add one.")
                .into_any_element(),
            Ok(rows) => {
                // A flash fades frame by frame until it is over.
                if rows.iter().any(|row| row.flash.is_some()) {
                    window.request_animation_frame();
                }
                let pointer = match link {
                    Some(ConnState::Reconnecting { .. } | ConnState::Failed { .. }) => Pointer::Inert,
                    Some(ConnState::Connected(_) | ConnState::Connecting(_)) | None => Pointer::Live,
                };
                let mut list = v_flex().w_full();
                for row in rows {
                    list = list.child(self.render_row(entity, row, pointer, cx));
                }
                match pointer {
                    Pointer::Live => list.into_any_element(),
                    // `isDegraded`: the rows stay, dimmed (`opacity: 0.6`).
                    Pointer::Inert => list.opacity(0.6).into_any_element(),
                }
            }
        }
    }
}

impl TreePane {
    /// `renderConnectionBanner`: while the connection is down, what happened and what to do.
    fn connection_banner(&self, link: Option<&ConnState>, cx: &mut Context<Self>) -> Option<AnyElement> {
        match link? {
            ConnState::Reconnecting { attempt, error } => Some(connection_banner(
                BannerKind::Reconnecting,
                format!("Reconnecting (attempt {attempt})... {error}"),
                vec![
                    ("Retry now", Box::new(|pane: &mut Self, cx: &mut Context<Self>| pane.app.read(cx).reconnect_now(&pane.connection))),
                    ("Stop", Box::new(|pane: &mut Self, cx: &mut Context<Self>| pane.app.read(cx).cancel_reconnect(&pane.connection))),
                ],
                cx,
            )),
            ConnState::Failed { error, hash_mismatch } => {
                let mut buttons: Vec<(&'static str, BannerAction)> = vec![
                    ("Reconnect", Box::new(|pane: &mut Self, cx: &mut Context<Self>| pane.app.read(cx).reconnect(&pane.connection))),
                    ("Disconnect", Box::new(|_: &mut Self, cx: &mut Context<Self>| cx.emit(DisconnectRequested))),
                ];
                if *hash_mismatch {
                    // Electron offers these in the session pane's Daemon Mismatch tab.
                    for (label, outdated) in [("Refresh daemon", OutdatedDaemon::Refresh), ("Connect anyway", OutdatedDaemon::Accept)] {
                        buttons.push((
                            label,
                            Box::new(move |pane: &mut Self, cx: &mut Context<Self>| {
                                let (app, id) = (pane.app.clone(), pane.connection.clone());
                                app.update(cx, |s, cx| s.reconnect_outdated(&id, outdated, cx));
                            }),
                        ));
                    }
                }
                Some(connection_banner(BannerKind::Error, error.clone(), buttons, cx))
            }
            ConnState::Connected(_) | ConnState::Connecting(_) => None,
        }
    }
}

/// `renderStatusIcon`: an 8px green dot once connected; 14px, a spinner while connecting
/// (orange while reconnecting) and a red alert once failed.
fn status_icon(link: Option<&ConnState>, cx: &gpui_kit::App) -> AnyElement {
    let spinner = |color: gpui_kit::Hsla| {
        Spinner::new()
            .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
            .with_size(px(14.))
            .color(color)
            .into_any_element()
    };
    match link {
        Some(ConnState::Connected(_)) | None => {
            div().size(px(8.)).flex_shrink_0().rounded_full().bg(gpui_kit::rgb(0x4caf50)).into_any_element()
        }
        Some(ConnState::Connecting(_)) => spinner(colors(cx).text_secondary),
        Some(ConnState::Reconnecting { .. }) => spinner(gpui_kit::rgb(0xff9800).into()),
        Some(ConnState::Failed { .. }) => ui::icon(IconName::CircleAlert, 14., gpui_kit::rgb(0xf44336)).into_any_element(),
    }
}

/// What a connection that is still connecting shows in place of its tree.
fn connect_phase_label(phase: ConnectPhase) -> &'static str {
    match phase {
        ConnectPhase::Bootstrap => "Bootstrapping...",
        ConnectPhase::Tunnel => "Establishing tunnel...",
        ConnectPhase::Daemon => "Connecting to daemon...",
    }
}

enum BannerKind {
    Reconnecting,
    Error,
}

type BannerAction = Box<dyn Fn(&mut TreePane, &mut Context<TreePane>)>;

/// `.connection-banner`: an 11px coloured strip with small outlined buttons.
fn connection_banner(
    kind: BannerKind,
    text: String,
    buttons: Vec<(&'static str, BannerAction)>,
    cx: &mut Context<TreePane>,
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
        BannerKind::Error => (0xf44336, 0xf4433614, ui::icon(IconName::CircleAlert, 12., gpui_kit::rgb(0xf44336)).into_any_element()),
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
                .on_click(cx.listener(move |pane, _, _, cx| action(pane, cx))),
        );
    }
    banner.into_any_element()
}

impl TreePane {
    fn render_row(
        &self,
        session: &Entity<SessionStore>,
        row: RowData,
        pointer: Pointer,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let RowData {
            id,
            depth,
            has_children,
            label,
            is_worktree,
            unread,
            attention_pending,
            flash,
            favourite,
            has_parent,
            pr,
            git_repo,
            activity,
            hover,
        } = row;
        let busy = activity != treeterm_model::workspace_store::ActivityState::Idle;
        let c = colors(cx);
        let (secondary, primary) = (c.text_secondary, c.text_primary);
        // `WorkspaceIcon`: spinner while loading, alert on error, the activity glyph while
        // busy, else a branch (worktree) or folder. Only a loaded row has a tooltip
        // (`title={… ws?.path}`).
        let (icon, label, tooltip): (AnyElement, SharedString, Option<SharedString>) = match label {
            Label::Name { name, tooltip } if busy => (ui::activity_glyph(activity, 14., secondary), name, Some(tooltip)),
            Label::Name { name, tooltip } => (
                ui::icon(if is_worktree { IconName::GitBranch } else { IconName::Folder }, 16., secondary).into_any_element(),
                name,
                Some(tooltip),
            ),
            Label::Loading(name) => (
                gpui_kit::component::spinner::Spinner::new()
                    .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
                    .with_size(px(16.))
                    .color(secondary)
                    .into_any_element(),
                name,
                None,
            ),
            Label::Error(name) => (ui::icon(IconName::CircleAlert, 16., c.error).into_any_element(), name, None),
        };
        let selected = session.read(cx).viewed_workspace() == Some(id.as_str());
        // Workspace focus mode (prefix + w) moves this cursor with the arrow keys.
        let focus_cursor = self.keybindings.read(cx).machine().focused_workspace() == Some(id.as_str());
        let live = pointer == Pointer::Live;
        // An inert row is never under the pointer, whatever row it last left.
        let hover = if live { hover } else { RowHover::Outside };
        // `.tree-item:hover`, and `.tree-subtree:has(> .tree-item:hover) .tree-item:not(.active):not(.focused)`
        // (which outranks it): the row under the pointer and the rows under it at half the
        // hover colour. The active row keeps its own; the focus cursor's row is tinted, in
        // full, only while under the pointer itself.
        let background = match (selected, focus_cursor, hover) {
            (true, _, _) => Some(c.bg_active),
            (false, false, RowHover::Hovered | RowHover::Under) => Some(c.bg_hover.opacity(0.5)),
            (false, true, RowHover::Hovered) => Some(c.bg_hover),
            (false, true, RowHover::Under) | (false, _, RowHover::Outside) => None,
        };
        let expand = match (has_children, self.view.is_collapsed(&id)) {
            (false, _) => None,
            (true, false) => Some(IconName::ChevronDown),
            (true, true) => Some(IconName::ChevronRight),
        };
        let toggle_id = id.clone();
        let hover_id = id.clone();
        let select_id = id.clone();
        let fork_id = id.clone();
        let (drag_value, move_id, drop_id) = (
            DraggedWorkspace { id: id.clone(), label: label.clone() },
            id.clone(),
            id.clone(),
        );
        let hint = self.drop_hint.as_ref().filter(|(target, _)| *target == id).map(|(_, position)| *position);
        let menu_id = id.clone();
        let (session, drop_session, fork_session) = (session.clone(), session.clone(), session.clone());
        let pane = cx.entity();
        let group = SharedString::from(format!("ws-row-group-{id}"));
        let root = depth == 0;
        // `.tree-item`: 36px rows, 6px 4px padding plus 8px per level.
        let row = h_flex()
            .id(SharedString::from(format!("ws-row-{id}")))
            .group(group.clone())
            .relative()
            .w_full()
            .min_h(px(36.))
            .py(px(6.))
            .pl(px(4. + 8. * depth as f32))
            .pr(px(4.))
            .items_center()
            .when_some(background, |row, background| row.bg(background))
            // `.workspace-unread`: a 2px accent rule on the left, also under the focus outline.
            .when(unread, |row| row.border_l(px(2.)).border_color(c.accent))
            .when(live, |row| {
                row.cursor_pointer()
                    .on_hover(cx.listener(move |pane, hovered: &bool, _, cx| {
                        let next = match (*hovered, &pane.hovered) {
                            (true, _) => Some(hover_id.clone()),
                            // Left for another row, which may have said so first.
                            (false, Some(current)) if *current == hover_id => None,
                            (false, Some(_) | None) => return,
                        };
                        if pane.hovered != next {
                            pane.hovered = next;
                            cx.notify();
                        }
                    }))
                    .when_some(tooltip, |row, tooltip| row.tooltip(ui::title_tooltip(tooltip)))
                    .on_drag(drag_value, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
                    .on_drag_move(cx.listener(move |pane, event: &gpui_kit::DragMoveEvent<DraggedWorkspace>, _, cx| {
                        let position = event.event.position;
                        let next = if event.bounds.contains(&position) && event.drag(cx).id != move_id {
                            let (top, height) = (f32::from(event.bounds.top()), f32::from(event.bounds.size.height));
                            Some((move_id.clone(), drop_zone(f32::from(position.y), top, height)))
                        } else {
                            None
                        };
                        let changed = match (&pane.drop_hint, &next) {
                            (Some((old, _)), None) => *old == move_id,
                            (old, Some(_)) => old.as_ref() != next.as_ref(),
                            (None, None) => false,
                        };
                        if changed {
                            pane.drop_hint = next;
                            cx.notify();
                        }
                    }))
                    .on_drop(cx.listener(move |pane, dragged: &DraggedWorkspace, _, cx| {
                        let position = match pane.drop_hint.take() {
                            Some((target, position)) if target == drop_id => position,
                            Some(_) | None => DropPosition::Onto,
                        };
                        let (drag, target) = (dragged.id.clone(), drop_id.clone());
                        drop_session.update(cx, |s, cx| s.move_workspace(&drag, &target, position, cx));
                        if position == DropPosition::Onto {
                            pane.view.expand(&target);
                        }
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |pane, _: &ClickEvent, _, cx| {
                        pane.select(select_id.clone(), cx);
                    }))
            })
            .when_some(hint, |row, position| match position {
                DropPosition::Before => row.border_t_2().border_color(c.accent),
                DropPosition::After => row.border_b_2().border_color(c.accent),
                DropPosition::Onto => row.bg(c.bg_hover).border_1().border_color(c.accent),
            })
            // The chevron toggles; a row without children keeps the 16px slot, and a click
            // there selects the row (`.tree-item-expand-placeholder`).
            .child(
                div()
                    .id(SharedString::from(format!("ws-toggle-{id}")))
                    .w(px(16.))
                    .flex_shrink_0()
                    .flex()
                    .justify_center()
                    .text_color(secondary)
                    .children(expand.map(|name| ui::icon(name, 16., secondary)))
                    .when(has_children && live, |slot| {
                        slot.hover(move |d| d.text_color(primary)).on_click(cx.listener(
                            move |pane, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                pane.toggle(&toggle_id, cx);
                            },
                        ))
                    }),
            )
            .child(div().mr(px(8.)).flex_shrink_0().flex().items_center().child(icon))
            .children(pr)
            .child(div().flex_1().min_w_0().truncate().text_size(px(ui::BODY)).text_color(primary).child(label.clone()))
            .when(unread, |row| {
                let marker = ui::unread_marker(SharedString::from(format!("ws-unread-{id}")), c.accent);
                row.child(marker.when(live, |marker| marker.tooltip(ui::unread_hint)))
            })            // "Saving workspace attention" while an unread-marker edit is not yet saved.
            .when(attention_pending, |row| row.child(ui::attention_saving(primary)))
            // `.tree-item-favourite`: a filled star when marked.
            .child(
                div()
                    .size(px(24.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(favourite, |d| d.text_size(px(14.)).text_color(gpui_kit::rgb(0xf5c84a)).child("★")),
            )
            // `.tree-item-actions`: always on root rows, on hover otherwise.
            .when(git_repo, |row| match pointer {
                Pointer::Inert => row.child(
                    ui::item_action_inert(cx)
                        .when(!root, |d| d.invisible())
                        .child(ui::icon(IconName::GitFork, 16., secondary)),
                ),
                Pointer::Live => row.child(
                    ui::item_action(SharedString::from(format!("ws-fork-{id}")), cx)
                        .when(!root, |d| d.invisible().group_hover(group.clone(), |d| d.visible()))
                        .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Fork").build(window, cx))
                        .child(ui::icon(IconName::GitFork, 16., secondary))
                        .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| {
                            cx.stop_propagation();
                            let (session, id) = (fork_session.clone(), fork_id.clone());
                            let label = session
                                .read(cx)
                                .workspace_store(&id)
                                .map(|w| SharedString::from(w.read(cx).file().display_name().to_owned()))
                                .unwrap_or_default();
                            let behind = behind_upstream(&session, &id, cx);
                            let tree = cx.entity().downgrade();
                            upstream_guard(behind, label, window, cx, move |_, cx| {
                                // Not behind upstream, this runs at once, inside this
                                // pane's own update: fork once that is done.
                                let (tree, id) = (tree.clone(), id.clone());
                                cx.defer(move |cx| match tree.update(cx, |tree, cx| tree.quick_fork(&id, cx)) {
                                    Ok(()) | Err(_) => {} // Err: the pane closed
                                });
                            });
                        })),
                ),
            })
            // `WorkspaceAttentionFlash`, over the row's content.
            .children(flash.map(|flash| ui::attention_flash(flash, 0.)))
            // `.focused`: the focus cursor is a 2px accent outline just inside the row's edge
            // (`outline-offset: -2px`), drawn over the row without moving its content, and
            // over the unread rule too.
            .when(focus_cursor, |row| {
                row.child(
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .bottom_0()
                        // An absolute child sits inside the row's borders: reach over the rule.
                        .left(px(if unread { -2. } else { 0. }))
                        .border_2()
                        .border_color(c.accent),
                )
            });
        if pointer == Pointer::Inert {
            return row.into_any_element();
        }
        // `SessionPanel`'s row menu: the same items, conditions and order.
        row.context_menu(move |menu, _window, cx| {
            let store = session.read(cx).workspace_store(&menu_id).cloned();
            let Some(store) = store else {
                // Not loaded: only Dismiss, at once (`dismissWorkspace`).
                let (session, id) = (session.clone(), menu_id.clone());
                return menu.item(
                    danger_item("Dismiss")
                        .on_click(move |_, _, cx| session.update(cx, |s, cx| s.dismiss_workspace(&id, cx))),
                );
            };
            let file = store.read(cx).file().clone();
            let child = file.is_worktree && has_parent;
            let (settings_store, settings_pane, settings_id) = (store.clone(), pane.clone(), menu_id.clone());
            let mut menu = menu
                .item(ui::menu_item("Settings").on_click(move |_, _, cx| {
                    if settings_store.update(cx, |w, cx| w.open_or_focus_tab("workspace-settings", cx)).is_some() {
                        settings_pane.update(cx, |pane, cx| pane.select(settings_id.clone(), cx));
                    }
                }));
            let favourite_store = store.clone();
            menu = menu.item(
                ui::menu_item(if favourite { "Unmark as Favourite" } else { "Mark as Favourite" })
                    .on_click(move |_, _, cx| favourite_store.update(cx, |w, cx| w.toggle_favourite(cx))),
            );
            if child {
                use treeterm_core::analyzer::Refresh;
                for (text, refresh) in
                    [("Refresh Title & Description", Refresh::TitleAndDescription), ("Refresh Branch Name", Refresh::BranchName)]
                {
                    let store = store.clone();
                    menu = menu.item(ui::menu_item(text).on_click(move |_, window, cx| {
                        // `reportRefreshOutcome`
                        let outcome = store.update(cx, |w, cx| w.refresh_labels(refresh, cx));
                        ui::alert_failure(outcome, window, cx);
                    }));
                }
            }
            if file.is_git_repo {
                let (pane, session, parent, label) = (pane.clone(), session.clone(), menu_id.clone(), label.clone());
                menu = menu.item(ui::menu_item("Open Existing Branch").on_click(move |_, window, cx| {
                    let (pane, session, parent, label) = (pane.clone(), session.clone(), parent.clone(), label.clone());
                    let behind = behind_upstream(&session, &parent, cx);
                    upstream_guard(behind, label.clone(), window, cx, move |window, cx| {
                        let pane = pane.clone();
                        let on_created: crate::child_dialog::OnCreated = std::rc::Rc::new(move |id, _, cx| {
                            pane.update(cx, |pane, cx| pane.select(id, cx));
                        });
                        crate::child_dialog::open(session.clone(), parent.clone(), label.clone(), on_created, window, cx);
                    });
                }));
            }
            if !file.is_worktree && file.is_git_repo {
                let (session, id) = (session.clone(), menu_id.clone());
                menu = menu.item(ui::menu_item("Auto Open All Recent Worktrees").on_click(move |_, window, cx| {
                    crate::auto_open_dialog::open(session.clone(), id.clone(), window, cx);
                }));
            }
            if child {
                // Review & Merge: show the workspace with its Review tab.
                let (pane, id) = (pane.clone(), menu_id.clone());
                menu = menu.item(ui::menu_item("Review & Merge").on_click(move |_, _, cx| {
                    if store.update(cx, |w, cx| w.open_or_focus_tab("review", cx)).is_some() {
                        pane.update(cx, |pane, cx| pane.select(id.clone(), cx));
                    }
                }));
            }
            if !file.is_worktree {
                let (session, id, label) = (session.clone(), menu_id.clone(), label.clone());
                menu = menu.item(danger_item("Remove").on_click(move |_, window, cx| {
                    confirm_remove(session.clone(), id.clone(), label.clone(), RemoveMode::KeepWorktreeAndBranch, window, cx)
                }));
            }
            menu
        })
        .into_any_element()
    }
}

/// `.context-menu-item.danger`: red text.
fn danger_item(label: &'static str) -> PopupMenuItem {
    PopupMenuItem::element(move |_, _| {
        div().h(px(29.5)).flex().items_center().text_size(px(ui::BODY)).text_color(gpui_kit::rgb(0xf14c4c)).child(label)
    })
}

/// Whether the rows take the pointer.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pointer {
    Live,
    /// While the connection is down (`isDegraded`: `pointer-events: none`): no hover,
    /// tooltip, click, drag, drop or menu. The pointer reaches what is under the rows
    /// instead: a wheel scrolls the sidebar, a drag moves the session, a dragged session
    /// drops on it.
    Inert,
}

/// A row's content, copied out of the session so rendering can borrow the context mutably.
struct RowData {
    id: String,
    is_worktree: bool,
    /// Background work finished while the workspace was not shown.
    unread: bool,
    /// An unread-marker edit is not yet saved.
    attention_pending: bool,
    /// The attention flash's state and opacity now.
    flash: Option<(treeterm_model::workspace_store::ActivityState, f32)>,
    favourite: bool,
    /// A child worktree (it has a parent): its title and branch can be re-suggested.
    has_parent: bool,
    /// `PrIndicators`: the PR's number and what it needs.
    pr: Vec<AnyElement>,
    /// Shows the Fork button.
    git_repo: bool,
    activity: treeterm_model::workspace_store::ActivityState,
    depth: usize,
    has_children: bool,
    label: Label,
    hover: RowHover,
}

/// Where a row is relative to the row under the pointer (`.tree-subtree`).
#[derive(Clone, Copy)]
enum RowHover {
    Outside,
    /// The row under the pointer.
    Hovered,
    /// Shown under the row under the pointer.
    Under,
}

/// A row's name and what its icon shows (`WorkspaceTreeItem`): a placeholder or failed
/// workspace is named too; why it failed shows in the main pane.
enum Label {
    /// A loaded workspace, with its path and description on hover.
    Name { name: SharedString, tooltip: SharedString },
    Loading(SharedString),
    Error(SharedString),
}

impl Label {
    fn of(entry: &WorkspaceEntry, cx: &gpui_kit::App) -> Self {
        match entry {
            WorkspaceEntry::Loaded(store) | WorkspaceEntry::OperationError { store, .. } => {
                let workspace = store.read(cx).workspace();
                Label::Name {
                    name: workspace.file.display_name().to_owned().into(),
                    tooltip: workspace.row_tooltip().into(),
                }
            }
            WorkspaceEntry::Loading { name, .. } => Label::Loading(name.clone().into()),
            WorkspaceEntry::Error { name, .. } => Label::Error(name.clone().into()),
        }
    }
}

/// Removing kills the workspace's (and its children's) running programs, and possibly
/// deletes their worktrees and branches, so ask first — with Electron's `confirm()` texts
/// (`WorkspacePane`'s abandon menu for a child workspace, `SessionPanel`'s Remove for a
/// root one), in the platform's message box as `confirm()` shows them.
pub(crate) fn confirm_remove(
    session: Entity<SessionStore>,
    id: String,
    label: SharedString,
    mode: RemoveMode,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) {
    let (is_child, name) = session.read(cx).workspace_store(&id).map_or((false, label.to_string()), |store| {
        let file = store.read(cx).file();
        (file.is_worktree && file.parent_id.as_deref().is_some_and(|p| !p.is_empty()), file.name.clone())
    });
    let message = match (is_child, mode) {
        (true, RemoveMode::DeleteWorktreeAndBranch) => {
            "Are you sure you want to abandon this workspace? All changes will be discarded.".to_owned()
        }
        (true, RemoveMode::KeepBranch) => {
            "Abandon this workspace but keep the branch? The worktree will be removed but the branch will be kept."
                .to_owned()
        }
        (true, RemoveMode::KeepWorktreeAndBranch) => "Abandon this workspace but keep both the worktree and branch? \
             They will remain but will no longer be tracked in TreeTerm."
            .to_owned(),
        (false, _) => format!("Remove workspace \"{name}\"?"),
    };
    ui::confirm(message, window, cx, move |_, cx| {
        session.update(cx, |s, cx| s.remove_workspace(&id, mode, cx));
    });
}

/// Commits `id`'s branch is behind its upstream (from its git status; 0 if unknown).
fn behind_upstream(session: &Entity<SessionStore>, id: &str, cx: &gpui_kit::App) -> u64 {
    session.read(cx).git_controller(id).map_or(0, |git| git.read(cx).status().behind_count)
}

/// `UpstreamWarningDialog`: forking a branch that is behind upstream asks first.
fn upstream_guard(
    behind: u64,
    name: SharedString,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
    proceed: impl Fn(&mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
) {
    if behind == 0 {
        proceed(window, cx);
        return;
    }
    let proceed = std::rc::Rc::new(proceed);
    window.open_dialog(cx, move |dialog, _, cx| {
        let proceed = proceed.clone();
        let secondary = colors(cx).text_secondary;
        // `.upstream-warning-text`: 14px secondary text, the name in bold.
        let text = format!(
            "{name} is {behind} commit{} behind upstream. Forking now means the new worktree will be based on an \
             outdated branch.",
            if behind > 1 { "s" } else { "" }
        );
        let bold = gpui_kit::HighlightStyle { font_weight: Some(gpui_kit::FontWeight::BOLD), ..Default::default() };
        let body = div()
            .p(px(20.))
            .child(
                div()
                    .mb(px(12.))
                    .text_size(px(14.))
                    .text_color(secondary)
                    .child(gpui_kit::StyledText::new(text).with_highlights([(0..name.len(), bold)])),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .italic()
                    .text_color(secondary)
                    .child("Consider pulling the latest changes before forking to avoid potential merge conflicts."),
            );
        let actions = ui::dialog_actions(cx)
            .child(
                ui::dialog_button("upstream-cancel", "Cancel", ui::ButtonKind::Secondary, false, cx)
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(ui::dialog_button("upstream-fork", "Fork Anyway", ui::ButtonKind::Warning, false, cx).on_click(
                move |_, window, cx| {
                    window.close_dialog(cx);
                    // After this dialog closes, so a second dialog can open.
                    let proceed = proceed.clone();
                    window.defer(cx, move |window, cx| proceed(window, cx));
                },
            ));
        ui::dialog_frame(dialog, 450., cx).child(
            ui::dialog_root(cx)
                .child(ui::dialog_header("Upstream Updates Available", "x", gpui_kit::rgb(0xf0a000), cx))
                .child(body)
                .child(actions),
        )
    });
}

fn status_line(text: impl Into<SharedString>, color: gpui_kit::Hsla) -> AnyElement {
    div()
        .px_3()
        .text_sm()
        .text_color(color)
        .child(text.into())
        .into_any_element()
}

/// `PrIndicators`, for a tree or favourite row: the PR's number in its state's colour when
/// there is a PR, then the icon of what it needs (`getPrSignal`), which shows without a PR
/// too when the workspace conflicts with its parent.
pub(crate) fn pr_indicators(status: &treeterm_core::git_status::GitStatus) -> Vec<AnyElement> {
    use treeterm_core::git_status::PullRequest;
    use treeterm_model::git_controller::PrSignal;
    let number = match &status.pull_request {
        PullRequest::Found(pr) => Some(
            div()
                .mr(px(4.))
                .flex_shrink_0()
                .text_size(px(12.))
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(gpui_kit::rgb(pr_state_color(pr)))
                .child(format!("#{}", pr.number))
                .into_any_element(),
        ),
        PullRequest::None => None,
    };
    let signal = match treeterm_model::git_controller::pr_signal(status) {
        PrSignal::None => None,
        PrSignal::MergeConflict => Some(ui::icon(IconName::TriangleAlert, 12., gpui_kit::rgb(0xd29922)).into_any_element()),
        PrSignal::CiFailure => Some(ui::icon(IconName::CircleX, 12., gpui_kit::rgb(0xb62324)).into_any_element()),
        PrSignal::CiRunning => Some(
            Spinner::new()
                .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
                .with_size(px(12.))
                .color(gpui_kit::rgb(0xd29922).into())
                .into_any_element(),
        ),
        PrSignal::ReadyToMerge => Some(ui::icon(IconName::CircleCheck, 12., gpui_kit::rgb(0x238636)).into_any_element()),
    };
    let signal = signal.map(|signal| div().mr(px(4.)).flex_shrink_0().child(signal).into_any_element());
    number.into_iter().chain(signal).collect()
}

/// `PR_STATE_CLASS`: open green, closed red, merged purple.
fn pr_state_color(pr: &treeterm_core::github::PrInfo) -> u32 {
    use treeterm_core::github::{PrDetails, PrState};
    match &pr.details {
        PrDetails::Loaded(status) => match status.state {
            PrState::Open => 0x238636,
            PrState::Closed => 0xb62324,
            PrState::Merged => 0x8250df,
            PrState::Other(_) => 0x858585,
        },
        PrDetails::Unavailable { .. } => 0x858585,
    }
}
