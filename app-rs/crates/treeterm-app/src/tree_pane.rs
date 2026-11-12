//! Left pane: the session's workspace tree (read-only so far).

use std::collections::HashSet;

use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Context, Entity, EventEmitter, SharedString, Window, div, px};
use treeterm_model::keybinding_store::KeybindingStore;
use treeterm_core::tree::DropPosition;
use treeterm_model::session_store::{RemoveMode, SessionState, SessionStore, WorkspaceEntry};

use crate::theme::colors;
use crate::ui;
use gpui_kit::assets::IconName;

/// Emitted when the user picks a workspace.
pub struct WorkspaceSelected(pub String);

pub struct TreePane {
    session: Entity<SessionStore>,
    keybindings: Entity<KeybindingStore>,
    /// The session's name: custom, else "Local" or the ssh label.
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
    /// Per-window view state: which workspaces have their children hidden.
    collapsed: HashSet<String>,
    selected: Selection,
    /// Bytes per second received from this connection's daemon.
    traffic: Entity<crate::traffic_badge::TrafficBadge>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Selection {
    Nothing,
    Workspace(String),
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
    pub fn new(
        session: Entity<SessionStore>,
        keybindings: Entity<KeybindingStore>,
        names: Entity<treeterm_model::session_names::SessionNames>,
        target: treeterm_core::connection::ConnectionTarget,
        remote: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&session, |_, _, cx| cx.notify()).detach();
        cx.observe(&keybindings, |_, _, cx| cx.notify()).detach();
        cx.observe(&names, |_, _, cx| cx.notify()).detach();
        let meter = session.read(cx).traffic().clone();
        let traffic = cx.new(|cx| crate::traffic_badge::TrafficBadge::new(meter, "connection-traffic", cx));
        Self {
            traffic,
            session,
            keybindings,
            names,
            target,
            renaming: None,
            remote,
            folded: false,
            drop_hint: None,
            collapsed: HashSet::new(),
            selected: Selection::Nothing,
        }
    }

    /// Edit the session's name in place (`SessionPanel` double-click).
    fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.names.read(cx).name(&self.target);
        let input = cx.new(|cx| InputState::new(window, cx).default_value(current));
        input.update(cx, |input, cx| input.focus(window, cx));
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
        if !self.collapsed.remove(id) {
            self.collapsed.insert(id.to_owned());
        }
        cx.notify();
    }

    /// "+": type or pick a directory and open it as a root workspace (`OpenWorkspaceDialog`).
    pub fn open_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pane = cx.entity().downgrade();
        let location = if self.remote {
            crate::open_workspace_dialog::Location::Remote(self.session.clone())
        } else {
            crate::open_workspace_dialog::Location::Local
        };
        crate::open_workspace_dialog::open(
            location,
            std::rc::Rc::new(move |path, _window, cx| {
                match pane.update(cx, |pane, cx| {
                    let id = pane.session.update(cx, |s, cx| s.open_workspace(path, cx));
                    pane.select(id, cx);
                }) {
                    Ok(()) | Err(_) => {} // Err: the pane closed
                }
            }),
            window,
            cx,
        );
    }

    /// Another session's workspace was picked: this tree shows no selection.
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if self.selected != Selection::Nothing {
            self.selected = Selection::Nothing;
            cx.notify();
        }
    }

    pub fn select(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected = Selection::Workspace(id.clone());
        cx.emit(WorkspaceSelected(id));
        cx.notify();
    }
}

impl Render for TreePane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (text_secondary, error, hover, accent, primary, border) =
            (c.text_secondary, c.error, c.bg_hover, c.accent, c.text_primary, c.border);
        let session = self.session.read(cx);
        let rows: Result<Vec<RowData>, AnyElement> = match session.state() {
            SessionState::Waiting => Err(status_line("Loading session…", text_secondary)),
            SessionState::Failed(reason) => Err(status_line(reason.clone(), error)),
            SessionState::Live => Ok(session
                .tree_rows(&self.collapsed, cx)
                .into_iter()
                .map(|row| RowData {
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
                    favourite: session.workspace_store(&row.id).is_some_and(|w| w.read(cx).is_favourite()),
                    has_parent: session
                        .workspace_store(&row.id)
                        .is_some_and(|w| w.read(cx).file().parent_id.as_deref().is_some_and(|p| !p.is_empty())),
                    pr: session.git_controller(&row.id).and_then(|git| {
                        let status = git.read(cx).status();
                        match &status.pull_request {
                            treeterm_core::git_status::PullRequest::Found(pr) => {
                                Some((pr.number, pr_state_color(pr), treeterm_model::git_controller::pr_signal(status)))
                            }
                            treeterm_core::git_status::PullRequest::None => None,
                        }
                    }),
                    git_repo: session.workspace_store(&row.id).is_some_and(|w| w.read(cx).file().is_git_repo),
                    label: session
                        .entry(&row.id)
                        .map_or(Label::Loading, |entry| Label::of(entry, cx)),
                    id: row.id,
                    depth: row.depth,
                    has_children: row.has_children,
                })
                .collect()),
        };
        let body = match rows {
            Err(status) => status,
            // `.tree-empty`: 16px padding, 12px secondary text.
            Ok(rows) if rows.is_empty() => div()
                .p(px(16.))
                .text_size(px(12.))
                .text_color(text_secondary)
                .child("No workspaces. Click + to add one.")
                .into_any_element(),
            Ok(rows) => {
                let mut list = v_flex().w_full();
                for row in rows {
                    list = list.child(self.render_row(row, cx));
                }
                list.into_any_element()
            }
        };
        let locked = self.session.read(cx).is_locked();
        let title = match &self.renaming {
            Some((input, _)) => div()
                .id("session-rename")
                .flex_1()
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
                .child(div().size(px(8.)).flex_shrink_0().rounded_full().bg(gpui_kit::rgb(0x4caf50)))
                .child(div().truncate().child(self.names.read(cx).name(&self.target).to_uppercase()))
                .child(self.traffic.clone())
                .when(locked, |d| {
                    d.child(
                        div()
                            .id("force-unlock")
                            .p(px(2.))
                            .rounded(px(4.))
                            .cursor_pointer()
                            .hover(move |d| d.bg(hover))
                            .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Force unlock session").build(window, cx))
                            .child(ui::icon(IconName::LockOpen, 14., primary))
                            .on_click(cx.listener(|pane, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                pane.session.update(cx, |s, cx| s.force_unlock(cx));
                            })),
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
                    .child(
                        ui::icon_button("open-workspace", cx)
                            .text_size(px(18.))
                            .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Add workspace").build(window, cx))
                            .child("+")
                            .on_click(cx.listener(move |pane, _: &ClickEvent, window, cx| {
                                cx.stop_propagation();
                                pane.open_workspace(window, cx);
                            })),
                    ),
            )
            .when(!self.folded, |d| d.child(div().pb(px(4.)).child(body)))
    }
}

impl TreePane {
    fn render_row(&self, row: RowData, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let RowData { id, depth, has_children, label, is_worktree, unread, favourite, has_parent, pr, git_repo, activity } = row;
        let busy = activity != treeterm_model::workspace_store::ActivityState::Idle;
        let c = colors(cx);
        let (secondary, primary) = (c.text_secondary, c.text_primary);
        // `WorkspaceIcon`: spinner while loading, alert on error, the activity glyph while
        // busy, else a branch (worktree) or folder.
        let (icon, label): (AnyElement, SharedString) = match label {
            Label::Name(name) if busy => (ui::activity_glyph(activity, 14., secondary), name),
            Label::Name(name) => (
                ui::icon(if is_worktree { IconName::GitBranch } else { IconName::Folder }, 16., secondary).into_any_element(),
                name,
            ),
            Label::Loading => (
                gpui_kit::component::spinner::Spinner::new()
                    .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
                    .with_size(px(16.))
                    .color(secondary)
                    .into_any_element(),
                "Loading…".into(),
            ),
            Label::Error(reason) => (ui::icon(IconName::CircleAlert, 16., c.error).into_any_element(), reason),
        };
        let selected = self.selected == Selection::Workspace(id.clone());
        // Workspace focus mode (prefix + w) moves this cursor with the arrow keys.
        let focus_cursor = self.keybindings.read(cx).machine().focused_workspace() == Some(id.as_str());
        let expand = match (has_children, self.collapsed.contains(&id)) {
            (false, _) => None,
            (true, false) => Some(IconName::ChevronDown),
            (true, true) => Some(IconName::ChevronRight),
        };
        let toggle_id = id.clone();
        let select_id = id.clone();
        let fork_id = id.clone();
        let (drag_value, move_id, drop_id) = (
            DraggedWorkspace { id: id.clone(), label: label.clone() },
            id.clone(),
            id.clone(),
        );
        let hint = self.drop_hint.as_ref().filter(|(target, _)| *target == id).map(|(_, position)| *position);
        let menu_id = id.clone();
        let session = self.session.clone();
        let pane = cx.entity();
        let group = SharedString::from(format!("ws-row-group-{id}"));
        let root = depth == 0;
        // `.tree-item`: 36px rows, 6px 4px padding plus 8px per level.
        h_flex()
            .id(SharedString::from(format!("ws-row-{id}")))
            .group(group.clone())
            .w_full()
            .min_h(px(36.))
            .py(px(6.))
            .pl(px(4. + 8. * depth as f32))
            .pr(px(4.))
            .items_center()
            .cursor_pointer()
            .when(selected, |row| row.bg(c.bg_active))
            .when(!selected, |row| row.hover(|row| row.bg(c.bg_hover)))
            .when(focus_cursor, |row| row.border_2().border_color(c.accent))
            // `.workspace-unread`: a 2px accent rule on the left.
            .when(unread && !focus_cursor, |row| row.border_l(px(2.)).border_color(c.accent))
            .on_drag(drag_value, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
            .on_drag_move(cx.listener(move |pane, event: &gpui_kit::DragMoveEvent<DraggedWorkspace>, _, cx| {
                let position = event.event.position;
                let next = if event.bounds.contains(&position) && event.drag(cx).id != move_id {
                    let zone = drop_zone(f32::from(position.y), f32::from(event.bounds.top()), f32::from(event.bounds.size.height));
                    Some((move_id.clone(), zone))
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
                pane.session.update(cx, |s, cx| s.move_workspace(&drag, &target, position, cx));
                if position == DropPosition::Onto {
                    pane.collapsed.remove(&target);
                }
                cx.notify();
            }))
            .when_some(hint, |row, position| match position {
                DropPosition::Before => row.border_t_2().border_color(c.accent),
                DropPosition::After => row.border_b_2().border_color(c.accent),
                DropPosition::Onto => row.bg(c.bg_hover).border_1().border_color(c.accent),
            })
            .on_click(cx.listener(move |pane, _: &ClickEvent, _, cx| {
                pane.select(select_id.clone(), cx);
            }))
            .child(
                div()
                    .id(SharedString::from(format!("ws-toggle-{id}")))
                    .w(px(16.))
                    .flex_shrink_0()
                    .flex()
                    .justify_center()
                    .text_color(secondary)
                    .hover(move |d| d.text_color(primary))
                    .children(expand.map(|name| ui::icon(name, 16., secondary)))
                    .on_click(cx.listener(move |pane, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        pane.toggle(&toggle_id, cx);
                    })),
            )
            .child(div().mr(px(8.)).flex_shrink_0().flex().items_center().child(icon))
            .when_some(pr, |row, (number, state_color, signal)| {
                use treeterm_model::git_controller::PrSignal;
                let signal = match signal {
                    PrSignal::None => None,
                    PrSignal::MergeConflict => Some(ui::icon(IconName::TriangleAlert, 12., gpui_kit::rgb(0xd29922)).into_any_element()),
                    PrSignal::CiFailure => Some(ui::icon(IconName::CircleX, 12., gpui_kit::rgb(0xb62324)).into_any_element()),
                    PrSignal::CiRunning => Some(
                        gpui_kit::component::spinner::Spinner::new()
                            .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
                            .with_size(px(12.))
                            .color(gpui_kit::rgb(0xd29922).into())
                            .into_any_element(),
                    ),
                    PrSignal::ReadyToMerge => Some(ui::icon(IconName::CircleCheck, 12., gpui_kit::rgb(0x238636)).into_any_element()),
                };
                row.child(
                    div()
                        .mr(px(4.))
                        .flex_shrink_0()
                        .text_size(px(12.))
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(gpui_kit::rgb(state_color))
                        .child(format!("#{number}")),
                )
                .children(signal.map(|signal| div().mr(px(4.)).flex_shrink_0().child(signal)))
            })
            .child(div().flex_1().min_w_0().truncate().text_size(px(ui::BODY)).text_color(primary).child(label.clone()))
            // "New activity — open workspace to mark as read".
            .when(unread, |row| row.child(div().mx(px(4.)).flex_shrink_0().text_size(px(10.)).text_color(c.accent).child("●")))
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
            .when(git_repo, |row| {
                row.child(
                    ui::item_action(SharedString::from(format!("ws-fork-{id}")), cx)
                        .when(!root, |d| d.invisible().group_hover(group.clone(), |d| d.visible()))
                        .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Fork").build(window, cx))
                        .child(ui::icon(IconName::GitFork, 16., secondary))
                        .on_click(cx.listener(move |pane, _: &ClickEvent, window, cx| {
                            cx.stop_propagation();
                            let (session, id) = (pane.session.clone(), fork_id.clone());
                            let label = session
                                .read(cx)
                                .workspace_store(&id)
                                .map(|w| SharedString::from(w.read(cx).file().display_name().to_owned()))
                                .unwrap_or_default();
                            let behind = behind_upstream(&session, &id, cx);
                            upstream_guard(behind, label, window, cx, move |_, cx| {
                                if let Err(error) = session.update(cx, |s, cx| s.quick_fork(&id, cx)) {
                                    tracing::error!(%error, "quick fork");
                                }
                            });
                        })),
                )
            })
            // `SessionPanel`'s row menu: the same items, conditions and order.
            .context_menu(move |menu, _window, cx| {
                let store = session.read(cx).workspace_store(&menu_id).cloned();
                let Some(store) = store else {
                    // Not loaded: only Dismiss.
                    let (session, id, label) = (session.clone(), menu_id.clone(), label.clone());
                    return menu.item(danger_item("Dismiss").on_click(move |_, window, cx| {
                        confirm_remove(session.clone(), id.clone(), label.clone(), RemoveMode::KeepWorktreeAndBranch, window, cx)
                    }));
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
                    for (text, branch) in [("Refresh Title & Description", false), ("Refresh Branch Name", true)] {
                        let store = store.clone();
                        menu = menu.item(ui::menu_item(text).on_click(move |_, window, cx| {
                            if let Err(error) = store.update(cx, |w, cx| w.refresh_labels(branch, cx)) {
                                window.push_notification(error, cx);
                            }
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
    }
}

/// `.context-menu-item.danger`: red text.
fn danger_item(label: &'static str) -> PopupMenuItem {
    PopupMenuItem::element(move |_, _| {
        div().h(px(29.5)).flex().items_center().text_size(px(ui::BODY)).text_color(gpui_kit::rgb(0xf14c4c)).child(label)
    })
}

/// A row's content, copied out of the session so rendering can borrow the context mutably.
struct RowData {
    id: String,
    is_worktree: bool,
    /// Background work finished while the workspace was not shown.
    unread: bool,
    favourite: bool,
    /// A child worktree (it has a parent): its title and branch can be re-suggested.
    has_parent: bool,
    /// The PR's number, its state's colour and what it needs (`PrIndicators`).
    pr: Option<(u64, u32, treeterm_model::git_controller::PrSignal)>,
    /// Shows the Fork button.
    git_repo: bool,
    activity: treeterm_model::workspace_store::ActivityState,
    depth: usize,
    has_children: bool,
    label: Label,
}

enum Label {
    Name(SharedString),
    Loading,
    Error(SharedString),
}

impl Label {
    fn of(entry: &WorkspaceEntry, cx: &gpui_kit::App) -> Self {
        match entry {
            WorkspaceEntry::Loaded(store) | WorkspaceEntry::OperationError { store, .. } => {
                Label::Name(store.read(cx).file().display_name().to_owned().into())
            }
            WorkspaceEntry::Loading { .. } => Label::Loading,
            WorkspaceEntry::Error { error, .. } => Label::Error(error.clone().into()),
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
