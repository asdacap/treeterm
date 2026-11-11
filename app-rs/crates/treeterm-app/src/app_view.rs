//! Window content: daemon start-up screen, then tree pane + workspace pane.

use std::collections::HashMap;

use gpui_kit::component::{h_flex, spinner::Spinner, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Entity, EntityId, SharedString, Subscription, Window, div, px};
use treeterm_model::app_store::{AppState, AppStore};
use treeterm_model::session_store::{SessionStore, TabTerminal, WorkspaceEntry};

use crate::terminal_view::TerminalView;
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
    /// The tab shown per workspace, until this window picks one it is the file's active tab.
    /// Window-local for now: writing `activeTabId` back comes with the workspace store.
    selected_tabs: HashMap<String, String>,
    /// Views of terminal models, by model, so focus and IME state survive re-renders.
    terminal_views: HashMap<EntityId, Entity<TerminalView>>,
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
                connected.focus_pending = true;
                cx.notify();
            }
        });
        Connected {
            session,
            tree,
            active: ActiveView::Nothing,
            selected_tabs: HashMap::new(),
            terminal_views: HashMap::new(),
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

/// The active workspace: header, tab strip and the selected tab. Terminal tabs are live;
/// other application kinds arrive in later milestones.
fn workspace_pane(connected: &mut Connected, window: &mut Window, cx: &mut Context<AppView>) -> AnyElement {
    let c = colors(cx);
    let (secondary, error, border, active_bg, hover) =
        (c.text_secondary, c.error, c.border, c.bg_primary, c.bg_hover);
    let tab_bar_bg = c.bg_secondary;
    let ActiveView::Workspace(id) = &connected.active else {
        return centered(div().text_color(secondary).child("Select a workspace"));
    };
    let id = id.clone();
    let session = connected.session.read(cx);
    // The selection can outlive its ref when another window removes the workspace.
    let Some(r) = session.refs().iter().find(|r| r.id == id) else {
        return centered(div().text_color(secondary).child("Workspace was removed"));
    };
    let path = r.path.clone();
    let file = match session.workspace(&id) {
        WorkspaceEntry::Loading => return centered(div().text_color(secondary).child("Loading…")),
        WorkspaceEntry::Error(reason) => {
            return centered(div().text_color(error).child(reason.clone()));
        }
        WorkspaceEntry::Loaded(file) => file.clone(),
    };

    let selected = connected
        .selected_tabs
        .get(&id)
        .filter(|tab| file.app_states.contains_key(*tab))
        .cloned()
        .or_else(|| file.active_tab_id.clone().filter(|tab| file.app_states.contains_key(tab)))
        .or_else(|| file.app_states.keys().next().cloned());

    let mut tabs = h_flex().h(px(32.)).bg(tab_bar_bg).border_b_1().border_color(border);
    for (tab_id, tab) in &file.app_states {
        let is_selected = selected.as_deref() == Some(tab_id.as_str());
        let ws = id.clone();
        let tab_key = tab_id.clone();
        tabs = tabs.child(
            div()
                .id(SharedString::from(format!("tab-{tab_id}")))
                .h_full()
                .px_3()
                .flex()
                .items_center()
                .text_sm()
                .border_r_1()
                .border_color(border)
                .when(is_selected, |d| d.bg(active_bg))
                .when(!is_selected, |d| d.text_color(secondary).hover(move |s| s.bg(hover)))
                .cursor_pointer()
                .child(tab.title.clone())
                .on_click(cx.listener(move |view, _, _, cx| {
                    if let Body::Connected(connected) = &mut view.body {
                        connected.selected_tabs.insert(ws.clone(), tab_key.clone());
                        connected.focus_pending = true;
                        cx.notify();
                    }
                })),
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
        });

    let content: AnyElement = match &selected {
        None => centered(div().text_color(secondary).child("No tabs")),
        Some(tab_id) => {
            let terminal = connected
                .session
                .update(cx, |session, cx| session.terminal(&id, tab_id, cx));
            match terminal {
                TabTerminal::Live(model) => {
                    let view = connected
                        .terminal_views
                        .entry(model.entity_id())
                        .or_insert_with(|| cx.new(|cx| TerminalView::new(model.clone(), window, cx)))
                        .clone();
                    if std::mem::take(&mut connected.focus_pending) {
                        let focus = view.read(cx).focus_handle().clone();
                        window.focus(&focus, cx);
                    }
                    div().size_full().child(view).into_any_element()
                }
                TabTerminal::NoPty => {
                    centered(div().text_color(error).child("No PTY available for this terminal"))
                }
                TabTerminal::NotATerminal => {
                    let app = file.app_states.get(tab_id).map(|t| t.application_id.clone()).unwrap_or_default();
                    centered(
                        div()
                            .text_color(secondary)
                            .child(format!("“{app}” tabs are not available in the native app yet")),
                    )
                }
            }
        }
    };
    // Views whose model is gone (tab closed) are dropped with it.
    let live: Vec<EntityId> = connected.terminal_views.keys().copied().collect();
    for entity_id in live {
        let alive = connected.terminal_views[&entity_id].read(cx).model().entity_id() == entity_id
            && connected.session.read(cx).has_terminal(entity_id);
        if !alive {
            connected.terminal_views.remove(&entity_id);
        }
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

#[cfg(feature = "automation")]
impl AppView {
    /// A JSON snapshot of what this window shows, for automation.
    pub fn automation_state(&self, window: &Window, cx: &gpui_kit::App) -> serde_json::Value {
        use serde_json::json;
        match &self.body {
            Body::StartingDaemon => json!({ "body": "starting" }),
            Body::Failed(reason) => json!({ "body": "failed", "reason": reason.to_string() }),
            Body::Connected(connected) => {
                let session = connected.session.read(cx);
                let workspaces: Vec<_> = session
                    .refs()
                    .iter()
                    .map(|r| {
                        let entry = match session.workspace(&r.id) {
                            WorkspaceEntry::Loading => json!("loading"),
                            WorkspaceEntry::Error(reason) => json!({ "error": reason }),
                            WorkspaceEntry::Loaded(file) => json!({
                                "name": file.display_name(),
                                "tabs": file.app_states.iter().map(|(id, t)| json!({
                                    "id": id, "app": t.application_id, "title": t.title
                                })).collect::<Vec<_>>(),
                            }),
                        };
                        json!({ "id": r.id, "path": r.path, "entry": entry })
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
                    "workspaces": workspaces,
                    "active": active,
                    "selectedTabs": connected.selected_tabs,
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
            connected.selected_tabs.insert(workspace.to_owned(), tab.to_owned());
            connected.focus_pending = true;
            cx.notify();
        }
    }
}
