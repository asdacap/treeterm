//! Window content: daemon start-up screen, then tree pane + workspace pane.

use gpui_kit::component::{h_flex, spinner::Spinner, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Entity, SharedString, Subscription, Window, div, px};
use treeterm_model::app_store::{AppState, AppStore};
use treeterm_model::session_store::{SessionStore, WorkspaceEntry};

use crate::theme::colors;
use crate::tree_pane::{TreePane, WorkspaceSelected};

pub struct AppView {
    /// Owned, not just observed: an observation does not keep the store alive, and dropping
    /// it cancels the daemon start-up and every watch it owns.
    _store: Entity<AppStore>,
    body: Body,
}

enum Body {
    StartingDaemon,
    Failed(SharedString),
    Connected(Connected),
}

/// Per-window state for a connected session.
struct Connected {
    session: Entity<SessionStore>,
    tree: Entity<TreePane>,
    active: ActiveView,
    _selection: Subscription,
}

enum ActiveView {
    Nothing,
    Workspace(String),
}

/// What the app store's state means for this window, read before mutating the view.
enum Next {
    StartingDaemon,
    Failed(SharedString),
    Connected(Entity<SessionStore>),
}

impl AppView {
    pub fn new(store: Entity<AppStore>, cx: &mut Context<Self>) -> Self {
        cx.observe(&store, |view, store, cx| view.sync(&store, cx)).detach();
        let mut view = Self {
            _store: store.clone(),
            body: Body::StartingDaemon,
        };
        view.sync(&store, cx);
        view
    }

    fn sync(&mut self, store: &Entity<AppStore>, cx: &mut Context<Self>) {
        let next = match store.read(cx).state() {
            AppState::StartingDaemon => Next::StartingDaemon,
            AppState::Failed(reason) => Next::Failed(reason.clone().into()),
            AppState::Connected(session) => Next::Connected(session.clone()),
        };
        self.body = match next {
            Next::StartingDaemon => Body::StartingDaemon,
            Next::Failed(reason) => Body::Failed(reason),
            Next::Connected(session) => {
                if let Body::Connected(current) = &self.body
                    && current.session == session
                {
                    return;
                }
                Body::Connected(self.connect(session, cx))
            }
        };
        cx.notify();
    }

    fn connect(&mut self, session: Entity<SessionStore>, cx: &mut Context<Self>) -> Connected {
        cx.observe(&session, |_, _, cx| cx.notify()).detach();
        let tree = cx.new(|cx| TreePane::new(session.clone(), cx));
        let selection = cx.subscribe(&tree, |view, _, WorkspaceSelected(id), cx| {
            if let Body::Connected(connected) = &mut view.body {
                connected.active = ActiveView::Workspace(id.clone());
                cx.notify();
            }
        });
        Connected {
            session,
            tree,
            active: ActiveView::Nothing,
            _selection: selection,
        }
    }
}

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (bg, fg, secondary, error) = (c.bg_primary, c.text_primary, c.text_secondary, c.error);
        let content: AnyElement = match &self.body {
            Body::StartingDaemon => centered(
                h_flex()
                    .gap_2()
                    .text_color(secondary)
                    .child(Spinner::new())
                    .child("Starting TreeTerm daemon…"),
            ),
            Body::Failed(reason) => centered(
                v_flex()
                    .gap_2()
                    .max_w(px(600.))
                    .child(div().text_color(error).child("Cannot reach the TreeTerm daemon"))
                    .child(div().text_sm().text_color(secondary).child(reason.clone())),
            ),
            Body::Connected(connected) => h_flex()
                .size_full()
                .child(connected.tree.clone())
                .child(workspace_pane(connected, cx))
                .into_any_element(),
        };
        div().size_full().bg(bg).text_color(fg).child(content)
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

/// Read-only summary of the active workspace; the dock with live tabs replaces it.
fn workspace_pane(connected: &Connected, cx: &mut Context<AppView>) -> AnyElement {
    let c = colors(cx);
    let (secondary, error, border) = (c.text_secondary, c.error, c.border);
    let ActiveView::Workspace(id) = &connected.active else {
        return centered(div().text_color(secondary).child("Select a workspace"));
    };
    let session = connected.session.read(cx);
    // The selection can outlive its ref when another window removes the workspace.
    let Some(r) = session.refs().iter().find(|r| &r.id == id) else {
        return centered(div().text_color(secondary).child("Workspace was removed"));
    };
    let body = match session.workspace(id) {
        WorkspaceEntry::Loading => div().text_color(secondary).child("Loading…").into_any_element(),
        WorkspaceEntry::Error(reason) => div().text_color(error).child(reason.clone()).into_any_element(),
        WorkspaceEntry::Loaded(file) => {
            let mut tabs = v_flex().gap_1();
            for (tab_id, tab) in &file.app_states {
                let active = file.active_tab_id.as_deref() == Some(tab_id.as_str());
                tabs = tabs.child(
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .child(if active { "●" } else { "○" })
                        .child(tab.title.clone())
                        .child(div().text_color(secondary).child(tab.application_id.clone())),
                );
            }
            v_flex()
                .gap_3()
                .child(div().text_xl().child(file.display_name().to_owned()))
                .child(div().text_sm().text_color(secondary).child(r.path.clone()))
                .child(
                    div()
                        .text_sm()
                        .text_color(secondary)
                        .child(format!("branch: {}", file.git_branch.as_deref().unwrap_or("—"))),
                )
                .child(div().pt_2().border_t_1().border_color(border).child(tabs))
                .into_any_element()
        }
    };
    div().flex_1().h_full().p_4().pt(px(38.)).child(body).into_any_element()
}
