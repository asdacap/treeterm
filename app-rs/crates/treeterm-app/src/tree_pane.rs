//! Left pane: the session's workspace tree (read-only so far).

use std::collections::HashSet;

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, ClickEvent, Context, Entity, EventEmitter, SharedString, Window, div, px};
use treeterm_model::session_store::{SessionState, SessionStore, WorkspaceEntry};

use crate::theme::colors;

/// Emitted when the user picks a workspace.
pub struct WorkspaceSelected(pub String);

pub struct TreePane {
    session: Entity<SessionStore>,
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
    pub fn new(session: Entity<SessionStore>, cx: &mut Context<Self>) -> Self {
        cx.observe(&session, |_, _, cx| cx.notify()).detach();
        Self {
            session,
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
            SessionState::Live(_) => Ok(session
                .tree_rows(&self.collapsed)
                .into_iter()
                .map(|row| RowData {
                    label: Label::of(session.workspace(&row.id)),
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
                div()
                    .px_3()
                    .pb_2()
                    .text_xs()
                    .text_color(text_secondary)
                    .child("WORKSPACES"),
            )
            .child(div().id("tree-rows").flex_1().overflow_y_scroll().child(body))
    }
}

impl TreePane {
    fn render_row(&self, row: RowData, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let RowData { id, depth, has_children, label } = row;
        let c = colors(cx);
        let (label, label_color): (SharedString, _) = match label {
            Label::Name(name) => (name, c.text_primary),
            Label::Loading => ("Loading…".into(), c.text_secondary),
            Label::Error(reason) => (format!("⚠ {reason}").into(), c.error),
        };
        let selected = self.selected == Selection::Workspace(id.clone());
        let chevron = match (has_children, self.collapsed.contains(&id)) {
            (false, _) => " ",
            (true, false) => "▾",
            (true, true) => "▸",
        };
        let toggle_id = id.clone();
        let select_id = id.clone();
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
            .child(div().truncate().text_color(label_color).child(label))
    }
}

/// A row's content, copied out of the session so rendering can borrow the context mutably.
struct RowData {
    id: String,
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
    fn of(entry: &WorkspaceEntry) -> Self {
        match entry {
            WorkspaceEntry::Loaded(file) => Label::Name(file.display_name().to_owned().into()),
            WorkspaceEntry::Loading => Label::Loading,
            WorkspaceEntry::Error(reason) => Label::Error(reason.clone().into()),
        }
    }
}

fn status_line(text: impl Into<SharedString>, color: gpui_kit::Hsla) -> AnyElement {
    div()
        .px_3()
        .text_sm()
        .text_color(color)
        .child(text.into())
        .into_any_element()
}
