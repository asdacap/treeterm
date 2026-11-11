//! Tool tabs ([`crate::apps::ToolApp`]): views over the workspace, its session or the LLM,
//! with no PTY or page of their own. Ports of the Electron applications `github`,
//! `analyzer-history`, `comments`, `tty-list`, `workspace-settings`, `chat`,
//! `system-prompt-debugger` and `ssh-upload`; tab ids, application ids and tab state are
//! Electron's, so the same tabs open in both apps.

pub mod analyzer_history;
pub mod chat;
pub mod comments;
pub mod github;
pub mod prompt_debugger;
pub mod ssh_upload;
pub mod tty_list;
pub mod workspace_settings;

use gpui_kit::{Entity, Subscription};

use crate::workspace_store::TabContent;

/// The runtime side of a tool tab, owned by its workspace store; never persisted.
pub(crate) enum ToolTab {
    /// Reads the session's git controller for this workspace.
    GitHub,
    /// Edits the workspace store itself.
    WorkspaceSettings,
    AnalyzerHistory(Entity<analyzer_history::AnalyzerHistory>),
    Comments(Entity<comments::CommentsModel>),
    TtyList(Entity<tty_list::TtyList>),
    Chat {
        model: Entity<chat::ChatModel>,
        _events: Subscription,
    },
    PromptDebugger {
        model: Entity<prompt_debugger::PromptDebugger>,
        _events: Subscription,
    },
    SshUpload(Entity<ssh_upload::SshUpload>),
}

impl ToolTab {
    pub(crate) fn content(&self) -> TabContent {
        match self {
            ToolTab::GitHub => TabContent::GitHub,
            ToolTab::WorkspaceSettings => TabContent::WorkspaceSettings,
            ToolTab::AnalyzerHistory(model) => TabContent::AnalyzerHistory(model.clone()),
            ToolTab::Comments(model) => TabContent::Comments(model.clone()),
            ToolTab::TtyList(model) => TabContent::TtyList(model.clone()),
            ToolTab::Chat { model, .. } => TabContent::Chat(model.clone()),
            ToolTab::PromptDebugger { model, .. } => TabContent::PromptDebugger(model.clone()),
            ToolTab::SshUpload(model) => TabContent::SshUpload(model.clone()),
        }
    }
}
