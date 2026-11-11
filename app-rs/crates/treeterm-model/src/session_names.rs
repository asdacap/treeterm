//! Custom session names (`sessionNames.ts`): renaming a session in the sidebar, kept in
//! `ui-state.json` by connection target so they survive reconnects and restarts.

use std::path::PathBuf;

use gpui_kit::Context;
use treeterm_core::connection::ConnectionTarget;
use treeterm_core::ui_state::{self, UiState};

pub struct SessionNames {
    /// `None`: nowhere to persist (no config dir); renames last for this run.
    path: Option<PathBuf>,
    state: UiState,
}

/// "Local", or the ssh label / `user@host` (`deriveDefaultSessionName`).
pub fn default_name(target: &ConnectionTarget) -> String {
    match target {
        ConnectionTarget::Local => "Local".to_owned(),
        ConnectionTarget::Remote(config) => config.display_name(),
    }
}

impl SessionNames {
    pub fn new(path: Option<PathBuf>) -> Self {
        let state = match path.as_deref().map(ui_state::load) {
            Some(Ok(state)) => state,
            // Logged, not overwritten: saving would drop whatever the file holds.
            Some(Err(error)) => {
                tracing::error!(%error, "UI state not loaded; session names start empty");
                return Self { path: None, state: UiState::default() };
            }
            None => UiState::default(),
        };
        Self { path, state }
    }

    /// The custom name, else the default.
    pub fn name(&self, target: &ConnectionTarget) -> String {
        self.state
            .session_names
            .get(&ui_state::session_key(target))
            .cloned()
            .unwrap_or_else(|| default_name(target))
    }

    /// A blank name, or the default one, clears the custom name.
    pub fn rename(&mut self, target: &ConnectionTarget, name: &str, cx: &mut Context<Self>) {
        let key = ui_state::session_key(target);
        let name = name.trim();
        if name.is_empty() || name == default_name(target) {
            self.state.session_names.remove(&key);
        } else {
            self.state.session_names.insert(key, name.to_owned());
        }
        if let Some(path) = &self.path
            && let Err(error) = ui_state::save(path, &self.state)
        {
            tracing::error!(%error, "session name not saved");
        }
        cx.notify();
    }
}
