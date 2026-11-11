//! Views of the tool tabs (`treeterm_model::tool_tabs`): one view per (workspace, tab),
//! kept across re-renders so inputs, scroll and expansion state survive.

mod analyzer_history_view;
mod chat_view;
mod comments_view;
mod github_view;
mod prompt_debugger_view;
mod ssh_upload_view;
mod tty_list_view;
mod workspace_settings_view;

use std::collections::HashMap;

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Entity, EntityId, Window, div};
use treeterm_model::session_store::SessionStore;
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::workspace_store::{TabContent, WorkspaceStore};

use crate::app_view::AppView;

enum ToolView {
    GitHub(Entity<github_view::GitHubView>),
    WorkspaceSettings(Entity<workspace_settings_view::WorkspaceSettingsView>),
    AnalyzerHistory(Entity<analyzer_history_view::AnalyzerHistoryView>),
    Comments(Entity<comments_view::CommentsView>),
    TtyList(Entity<tty_list_view::TtyListView>),
    Chat(Entity<chat_view::ChatView>),
    PromptDebugger(Entity<prompt_debugger_view::PromptDebuggerView>),
    SshUpload(Entity<ssh_upload_view::SshUploadView>),
}

impl ToolView {
    fn element(&self) -> AnyElement {
        match self {
            ToolView::GitHub(view) => view.clone().into_any_element(),
            ToolView::WorkspaceSettings(view) => view.clone().into_any_element(),
            ToolView::AnalyzerHistory(view) => view.clone().into_any_element(),
            ToolView::Comments(view) => view.clone().into_any_element(),
            ToolView::TtyList(view) => view.clone().into_any_element(),
            ToolView::Chat(view) => view.clone().into_any_element(),
            ToolView::PromptDebugger(view) => view.clone().into_any_element(),
            ToolView::SshUpload(view) => view.clone().into_any_element(),
        }
    }
}

/// The tool-tab views of one connected session.
pub struct ToolViews {
    settings: Entity<SettingsStore>,
    /// By (workspace, tab), with the entity the view was made for (its model, git
    /// controller or workspace store): a re-created one gets a new view.
    views: HashMap<(String, String), (Option<EntityId>, ToolView)>,
}

impl ToolViews {
    pub fn new(settings: Entity<SettingsStore>) -> Self {
        Self { settings, views: HashMap::new() }
    }

    /// Drop the views of tabs that are gone: a view holds its model, and a chat kept alive
    /// here would keep streaming.
    pub fn prune(&mut self, session: &SessionStore, cx: &gpui_kit::App) {
        self.views.retain(|(workspace, tab), _| {
            session
                .workspace_store(workspace)
                .is_some_and(|store| store.read(cx).file().app_states.contains_key(tab))
        });
    }

    /// The tool tab `tab_id` of `workspace` showing `content`, which must be a tool tab's.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        session: &Entity<SessionStore>,
        workspace: &Entity<WorkspaceStore>,
        workspace_id: &str,
        tab_id: &str,
        content: TabContent,
        window: &mut Window,
        cx: &mut Context<AppView>,
    ) -> AnyElement {
        let model_id = match &content {
            TabContent::AnalyzerHistory(model) => Some(model.entity_id()),
            TabContent::Comments(model) => Some(model.entity_id()),
            TabContent::TtyList(model) => Some(model.entity_id()),
            TabContent::Chat(model) => Some(model.entity_id()),
            TabContent::PromptDebugger(model) => Some(model.entity_id()),
            TabContent::SshUpload(model) => Some(model.entity_id()),
            TabContent::GitHub => session.read(cx).git_controller(workspace_id).map(|c| c.entity_id()),
            TabContent::WorkspaceSettings => Some(workspace.entity_id()),
            TabContent::Terminal(_)
            | TabContent::Files(_)
            | TabContent::Editor(_)
            | TabContent::Review(_)
            | TabContent::Browser { .. }
            | TabContent::StartingTerminal
            | TabContent::Failed(_)
            | TabContent::NotPorted(_)
            | TabContent::WebApp { .. } => None,
        };
        let key = (workspace_id.to_owned(), tab_id.to_owned());
        if let Some((made_for, view)) = self.views.get(&key)
            && *made_for == model_id
        {
            return view.element();
        }
        let view = match content {
            TabContent::GitHub => {
                let controller = session.read(cx).git_controller(workspace_id).cloned();
                ToolView::GitHub(cx.new(|cx| github_view::GitHubView::new(controller, cx)))
            }
            TabContent::WorkspaceSettings => {
                let workspace = workspace.clone();
                ToolView::WorkspaceSettings(
                    cx.new(|cx| workspace_settings_view::WorkspaceSettingsView::new(workspace, window, cx)),
                )
            }
            TabContent::AnalyzerHistory(model) => {
                ToolView::AnalyzerHistory(cx.new(|cx| analyzer_history_view::AnalyzerHistoryView::new(model, cx)))
            }
            TabContent::Comments(model) => {
                let (session, id) = (session.clone(), workspace_id.to_owned());
                ToolView::Comments(cx.new(|cx| comments_view::CommentsView::new(model, session, id, cx)))
            }
            TabContent::TtyList(model) => {
                let session = session.clone();
                ToolView::TtyList(cx.new(|cx| tty_list_view::TtyListView::new(model, session, cx)))
            }
            TabContent::Chat(model) => ToolView::Chat(cx.new(|cx| chat_view::ChatView::new(model, window, cx))),
            TabContent::PromptDebugger(model) => {
                let settings = self.settings.clone();
                ToolView::PromptDebugger(
                    cx.new(|cx| prompt_debugger_view::PromptDebuggerView::new(model, settings, window, cx)),
                )
            }
            TabContent::SshUpload(model) => {
                let session = session.clone();
                ToolView::SshUpload(cx.new(|cx| ssh_upload_view::SshUploadView::new(model, session, window, cx)))
            }
            TabContent::Terminal(_)
            | TabContent::Files(_)
            | TabContent::Editor(_)
            | TabContent::Review(_)
            | TabContent::Browser { .. }
            | TabContent::StartingTerminal
            | TabContent::Failed(_)
            | TabContent::NotPorted(_)
            | TabContent::WebApp { .. } => {
                return div().into_any_element(); // not a tool tab: the caller renders it
            }
        };
        let element = view.element();
        self.views.insert(key, (model_id, view));
        element
    }
}

/// A section heading (`h3`) with an optional count beside it.
pub(crate) fn heading(title: &'static str, count: Option<String>, secondary: gpui_kit::Hsla) -> gpui_kit::Div {
    use gpui_kit::component::h_flex;
    h_flex()
        .gap_3()
        .items_center()
        .child(div().text_base().child(title))
        .when_some(count, |row, count| row.child(div().text_xs().text_color(secondary).child(count)))
}

/// A small coloured label (`pillStyle`).
pub(crate) fn pill(text: impl Into<gpui_kit::SharedString>, background: gpui_kit::Hsla) -> gpui_kit::Div {
    div().px(gpui_kit::px(6.)).rounded_sm().bg(background).text_color(gpui_kit::white()).text_xs().child(text.into())
}
