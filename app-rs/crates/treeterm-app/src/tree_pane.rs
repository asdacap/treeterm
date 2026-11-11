//! Left pane: the session's workspace tree (read-only so far).

use std::collections::HashSet;

use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Context, Entity, EventEmitter, SharedString, Window, div, px};
use treeterm_model::keybinding_store::KeybindingStore;
use treeterm_model::session_store::{AfterMerge, RemoveMode, SessionState, SessionStore, WorkspaceEntry};

use crate::theme::colors;

/// Emitted when the user picks a workspace.
pub struct WorkspaceSelected(pub String);

pub struct TreePane {
    session: Entity<SessionStore>,
    keybindings: Entity<KeybindingStore>,
    /// Per-window view state: which workspaces have their children hidden.
    collapsed: HashSet<String>,
    selected: Selection,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Selection {
    Nothing,
    Workspace(String),
}

impl EventEmitter<WorkspaceSelected> for TreePane {}

impl TreePane {
    pub fn new(session: Entity<SessionStore>, keybindings: Entity<KeybindingStore>, cx: &mut Context<Self>) -> Self {
        cx.observe(&session, |_, _, cx| cx.notify()).detach();
        cx.observe(&keybindings, |_, _, cx| cx.notify()).detach();
        Self {
            session,
            keybindings,
            collapsed: HashSet::new(),
            selected: Selection::Nothing,
        }
    }

    fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(id) {
            self.collapsed.insert(id.to_owned());
        }
        cx.notify();
    }

    /// Pick a directory and open it as a root workspace (`OpenWorkspaceDialog`, local only).
    fn open_workspace(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Workspace".into()),
        });
        cx.spawn(async move |pane, cx| {
            let path = match picked.await {
                Ok(Ok(Some(mut paths))) if !paths.is_empty() => paths.remove(0),
                Ok(Ok(_)) => return, // cancelled
                Ok(Err(error)) => return tracing::error!(%error, "folder picker failed"),
                Err(_canceled) => return,
            };
            let path = path.to_string_lossy().into_owned();
            match pane.update(cx, |pane, cx| {
                let id = pane.session.update(cx, |s, cx| s.open_workspace(path, cx));
                pane.select(id, cx);
            }) {
                Ok(()) | Err(_) => {} // Err: the pane closed
            }
        })
        .detach();
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
        let (bg, text_secondary, error) = (c.bg_secondary, c.text_secondary, c.error);
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
            Ok(rows) => {
                let mut list = v_flex().w_full();
                for row in rows {
                    list = list.child(self.render_row(row, cx));
                }
                list.into_any_element()
            }
        };
        v_flex()
            .h_full()
            .w(px(250.))
            .flex_shrink_0()
            .bg(bg)
            .border_r_1()
            .border_color(cx.theme().border)
            // macOS traffic lights sit over the top 38px, as in Electron's `.tree-pane-content`.
            .pt(px(38.))
            .child(
                h_flex()
                    .px_3()
                    .pb_2()
                    .justify_between()
                    .text_xs()
                    .text_color(text_secondary)
                    .child("WORKSPACES")
                    .child(
                        div()
                            .id("open-workspace")
                            .px_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(|d| d.bg(colors(cx).bg_hover))
                            .child("+")
                            .on_click(cx.listener(|pane, _: &ClickEvent, _, cx| pane.open_workspace(cx))),
                    ),
            )
            .child(div().id("tree-rows").flex_1().overflow_y_scroll().child(body))
    }
}

impl TreePane {
    fn render_row(&self, row: RowData, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let RowData { id, depth, has_children, label, is_worktree, activity } = row;
        let busy = activity != treeterm_model::workspace_store::ActivityState::Idle;
        let c = colors(cx);
        let (label, label_color): (SharedString, _) = match label {
            Label::Name(name) => (name, c.text_primary),
            Label::Loading => ("Loading…".into(), c.text_secondary),
            Label::Error(reason) => (format!("⚠ {reason}").into(), c.error),
        };
        let selected = self.selected == Selection::Workspace(id.clone());
        // Workspace focus mode (prefix + w) moves this cursor with the arrow keys.
        let focus_cursor = self.keybindings.read(cx).machine().focused_workspace() == Some(id.as_str());
        let chevron = match (has_children, self.collapsed.contains(&id)) {
            (false, _) => " ",
            (true, false) => "▾",
            (true, true) => "▸",
        };
        let toggle_id = id.clone();
        let select_id = id.clone();
        let menu_id = id.clone();
        let session = self.session.clone();
        let pane = cx.entity();
        h_flex()
            .id(SharedString::from(format!("ws-row-{id}")))
            .w_full()
            .h(px(24.))
            .pl(px(8. + 12. * depth as f32))
            .pr_2()
            .gap_1()
            .text_sm()
            .when(selected, |row| row.bg(c.bg_active))
            .when(!selected, |row| row.hover(|row| row.bg(c.bg_hover)))
            .when(focus_cursor, |row| row.border_1().border_color(c.accent))
            .on_click(cx.listener(move |pane, _: &ClickEvent, _, cx| {
                pane.select(select_id.clone(), cx);
            }))
            .child(
                div()
                    .id(SharedString::from(format!("ws-toggle-{id}")))
                    .w(px(12.))
                    .text_color(c.text_secondary)
                    .child(chevron)
                    .on_click(cx.listener(move |pane, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        pane.toggle(&toggle_id, cx);
                    })),
            )
            .child(div().flex_1().truncate().text_color(label_color).child(label.clone()))
            .when(busy, |row| {
                row.child(
                    div()
                        .size(px(7.))
                        .flex_shrink_0()
                        .rounded_full()
                        .bg(gpui_kit::rgb(activity.color())),
                )
            })
            .context_menu(move |menu, _window, _cx| {
                let (session, id, label) = (session.clone(), menu_id.clone(), label.clone());
                let (child_session, child_parent, child_label, pane) =
                    (session.clone(), id.clone(), label.clone(), pane.clone());
                let menu = menu.item(PopupMenuItem::new("New child workspace…").on_click(move |_, window, cx| {
                    let pane = pane.clone();
                    let on_created: crate::child_dialog::OnCreated = std::rc::Rc::new(move |id, _, cx| {
                        pane.update(cx, |pane, cx| pane.select(id, cx));
                    });
                    crate::child_dialog::open(
                        child_session.clone(),
                        child_parent.clone(),
                        child_label.clone(),
                        on_created,
                        window,
                        cx,
                    );
                }))
                .separator();
                let menu = {
                    let (session, id) = (session.clone(), id.clone());
                    menu.item(PopupMenuItem::new("Quick fork").on_click(move |_, _, cx| {
                        if let Err(error) = session.update(cx, |s, cx| s.quick_fork(&id, cx)) {
                            tracing::error!(%error, "quick fork");
                        }
                    }))
                    .separator()
                };
                let remove = |menu: gpui_kit::component::menu::PopupMenu, text: &'static str, mode: RemoveMode| {
                    let (session, id, label) = (session.clone(), id.clone(), label.clone());
                    menu.item(PopupMenuItem::new(text).on_click(move |_, window, cx| {
                        confirm_remove(session.clone(), id.clone(), label.clone(), mode, window, cx)
                    }))
                };
                let merge = |menu: gpui_kit::component::menu::PopupMenu, text: &'static str, squash: bool, after: AfterMerge| {
                    let (session, id, label) = (session.clone(), id.clone(), label.clone());
                    menu.item(PopupMenuItem::new(text).on_click(move |_, window, cx| {
                        confirm_merge(session.clone(), id.clone(), label.clone(), squash, after, window, cx)
                    }))
                };
                let menu = if is_worktree {
                    let menu = merge(menu, "Merge into parent and remove…", false, AfterMerge::Remove);
                    let menu = merge(menu, "Squash-merge into parent and remove…", true, AfterMerge::Remove);
                    let menu = merge(menu, "Merge into parent, keep workspace…", false, AfterMerge::Keep).separator();
                    let menu = remove(menu, "Remove (delete worktree and branch)…", RemoveMode::DeleteWorktreeAndBranch);
                    remove(menu, "Remove, keep branch…", RemoveMode::KeepBranch)
                } else {
                    menu
                };
                remove(menu, "Remove from TreeTerm (keep files)…", RemoveMode::KeepWorktreeAndBranch)
            })
    }
}

/// A row's content, copied out of the session so rendering can borrow the context mutably.
struct RowData {
    id: String,
    is_worktree: bool,
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
/// deletes their worktrees and branches, so ask first.
fn confirm_remove(
    session: Entity<SessionStore>,
    id: String,
    label: SharedString,
    mode: RemoveMode,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) {
    let consequence = match mode {
        RemoveMode::DeleteWorktreeAndBranch => {
            "Its worktree directory is deleted along with its branch (unless detached). Uncommitted work is lost."
        }
        RemoveMode::KeepBranch => "Its worktree directory is deleted; the branch stays.",
        RemoveMode::KeepWorktreeAndBranch => "The directory, worktree and branch stay on disk.",
    };
    window.open_alert_dialog(cx, move |alert, _, _| {
        let (session, id) = (session.clone(), id.clone());
        alert
            .title(format!("Remove “{label}”?"))
            .description(format!(
                "Its tabs and those of its child workspaces are closed and their programs killed. {consequence}"
            ))
            .show_cancel(true)
            .on_ok(move |_, _, cx| {
                session.update(cx, |s, cx| s.remove_workspace(&id, mode, cx));
                true
            })
    });
}

fn confirm_merge(
    session: Entity<SessionStore>,
    id: String,
    label: SharedString,
    squash: bool,
    after: AfterMerge,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) {
    let how = if squash { "squash-merged (staged, not committed)" } else { "merged" };
    let then = match after {
        AfterMerge::Remove => {
            " Its uncommitted work is committed first; afterwards its tabs are closed and its worktree and branch deleted."
        }
        AfterMerge::Keep => " Only committed work is merged; the workspace stays.",
    };
    window.open_alert_dialog(cx, move |alert, _, _| {
        let (session, id) = (session.clone(), id.clone());
        alert
            .title(format!("Merge “{label}” into its parent?"))
            .description(format!("Its branch is {how} into the parent's branch.{then}"))
            .show_cancel(true)
            .on_ok(move |_, _, cx| {
                session.update(cx, |s, cx| s.merge_workspace(&id, squash, after, cx));
                true
            })
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
