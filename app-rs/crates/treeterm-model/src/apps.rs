//! The applications a tab can run. Port of the registry in `src/renderer/store/app.ts`.
//!
//! Ids are shared with the Electron app (`terminal`, `terminal-<instance>`, ...), so a tab
//! created by either app opens in the other. Applications the native app does not have yet
//! are absent from the registry: their tabs are kept untouched (never closed or rewritten).

use serde_json::{Value, json};
use treeterm_core::settings::Settings;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppKind {
    /// A shell (or a startup command) in a daemon PTY.
    Terminal { startup_command: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDef {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub kind: AppKind,
    pub can_close: bool,
    pub show_in_new_tab_menu: bool,
    /// Opened automatically in new workspaces.
    pub is_default: bool,
}

impl AppDef {
    /// `createInitialState` of the application.
    pub fn initial_state(&self) -> Value {
        match &self.kind {
            AppKind::Terminal { .. } => json!({
                "ptyId": null,
                "ptyHandle": uuid::Uuid::new_v4().to_string(),
                "keepOnExit": false,
                "idleDetectorDisabled": false,
                "widthLimitDisabled": false,
            }),
        }
    }
}

pub struct AppRegistry {
    apps: Vec<AppDef>,
}

impl AppRegistry {
    pub fn from_settings(_settings: &Settings) -> Self {
        let terminal = |id: &str, show: bool| AppDef {
            id: id.to_owned(),
            name: "Terminal".to_owned(),
            icon: ">".to_owned(),
            kind: AppKind::Terminal {
                startup_command: None,
            },
            can_close: true,
            show_in_new_tab_menu: show,
            is_default: show,
        };
        Self {
            apps: vec![
                terminal("terminal", true),
                // The Electron app's alternate terminal frontend; natively it is the same terminal.
                terminal("ghostty-terminal", false),
            ],
        }
    }

    pub fn get(&self, id: &str) -> Option<&AppDef> {
        self.apps.iter().find(|app| app.id == id)
    }

    /// `getDefaultApp`: the first application marked default, else the plain terminal.
    pub fn default_app(&self) -> Option<&AppDef> {
        self.apps
            .iter()
            .find(|app| app.is_default)
            .or_else(|| self.get("terminal"))
    }

    pub fn new_tab_menu(&self) -> impl Iterator<Item = &AppDef> {
        self.apps.iter().filter(|app| app.show_in_new_tab_menu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_tabs_start_without_a_pty_and_with_a_fresh_handle() {
        let registry = AppRegistry::from_settings(&Settings::default());
        let app = registry.default_app().unwrap();
        assert_eq!(app.id, "terminal");
        let a = app.initial_state();
        let b = app.initial_state();
        assert!(a["ptyId"].is_null());
        assert_ne!(a["ptyHandle"], b["ptyHandle"]);
        assert_eq!(a["keepOnExit"], false);
    }

    #[test]
    fn the_ghostty_alias_is_hidden_from_the_menu() {
        let registry = AppRegistry::from_settings(&Settings::default());
        assert!(registry.get("ghostty-terminal").is_some());
        let menu: Vec<_> = registry.new_tab_menu().map(|app| app.id.as_str()).collect();
        assert_eq!(menu, vec!["terminal"]);
    }
}
