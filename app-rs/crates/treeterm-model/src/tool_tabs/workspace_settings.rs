//! The workspace Settings tab (`WorkspaceSettings.tsx`): name, description, the default
//! application for child workspaces (`settings.defaultApplicationId`) and the raw body. The
//! edits are [`WorkspaceStore`](crate::workspace_store::WorkspaceStore) methods
//! (`set_display_name`, `set_description`, `set_default_application`).

use crate::apps::AppRegistry;

/// One choice of "Default Application for Children".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultAppChoice {
    /// Empty: "Use Global Default".
    pub id: String,
    pub label: String,
}

/// "Use Global Default", then every application of the new-tab menu.
pub fn default_app_choices(registry: &AppRegistry) -> Vec<DefaultAppChoice> {
    std::iter::once(DefaultAppChoice { id: String::new(), label: "Use Global Default".to_owned() })
        .chain(registry.new_tab_menu().map(|app| DefaultAppChoice { id: app.id.clone(), label: app.name.clone() }))
        .collect()
}

/// What the choice shows for `id`; an application this app lacks (a variant from settings
/// that was removed, or one only Electron has) is named by its id.
pub fn default_app_label(choices: &[DefaultAppChoice], id: &str) -> String {
    match choices.iter().find(|choice| choice.id == id) {
        Some(choice) => choice.label.clone(),
        None => format!("{id} (not available)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use treeterm_core::settings::Settings;

    #[test]
    fn the_global_default_comes_first_and_unknown_ids_stay_visible() {
        let registry = AppRegistry::from_settings(&Settings::default());
        let choices = default_app_choices(&registry);
        assert_eq!(choices[0].id, "");
        assert_eq!(default_app_label(&choices, ""), "Use Global Default");
        assert_eq!(default_app_label(&choices, "filesystem"), "Files");
        assert_eq!(default_app_label(&choices, "webapp-next"), "webapp-next (not available)");
        assert!(!choices.iter().any(|choice| choice.id == "workspace-settings"));
    }
}
