//! The `prefixMode` and `keybindings` sections of `settings.json` (shared with Electron).

use std::time::Duration;

use serde_json::Value;

use super::key::{Key, KeyPress, Modifiers, Os, ParseError, Shortcut};

/// What a key does after the prefix key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrefixAction {
    NewTab,
    CloseTab,
    NextTab,
    PrevTab,
    OpenSettings,
    /// Start keyboard navigation of the workspace tree.
    WorkspaceFocus,
}

impl PrefixAction {
    /// Settings order, which is also the matching order when two actions share a key.
    pub const ALL: [PrefixAction; 6] = [
        PrefixAction::NewTab,
        PrefixAction::CloseTab,
        PrefixAction::NextTab,
        PrefixAction::PrevTab,
        PrefixAction::OpenSettings,
        PrefixAction::WorkspaceFocus,
    ];

    /// The key under `keybindings` in `settings.json`.
    pub fn settings_key(self) -> &'static str {
        match self {
            PrefixAction::NewTab => "newTab",
            PrefixAction::CloseTab => "closeTab",
            PrefixAction::NextTab => "nextTab",
            PrefixAction::PrevTab => "prevTab",
            PrefixAction::OpenSettings => "openSettings",
            PrefixAction::WorkspaceFocus => "workspaceFocus",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PrefixAction::NewTab => "New Tab",
            PrefixAction::CloseTab => "Close Tab",
            PrefixAction::NextTab => "Next Tab",
            PrefixAction::PrevTab => "Previous Tab",
            PrefixAction::OpenSettings => "Open Settings",
            PrefixAction::WorkspaceFocus => "Workspace Focus",
        }
    }
}

/// The key pressed after the prefix for each action (`settings.keybindings`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionKeys {
    pub new_tab: Key,
    pub close_tab: Key,
    pub next_tab: Key,
    pub prev_tab: Key,
    pub open_settings: Key,
    pub workspace_focus: Key,
}

impl Default for ActionKeys {
    fn default() -> Self {
        Self {
            new_tab: Key::Char('c'),
            close_tab: Key::Char('x'),
            next_tab: Key::Char('n'),
            prev_tab: Key::Char('p'),
            open_settings: Key::Char(','),
            workspace_focus: Key::Char('w'),
        }
    }
}

/// One row of the prefix-mode overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayItem {
    pub key_label: String,
    pub action: PrefixAction,
}

impl ActionKeys {
    pub fn key(&self, action: PrefixAction) -> &Key {
        match action {
            PrefixAction::NewTab => &self.new_tab,
            PrefixAction::CloseTab => &self.close_tab,
            PrefixAction::NextTab => &self.next_tab,
            PrefixAction::PrevTab => &self.prev_tab,
            PrefixAction::OpenSettings => &self.open_settings,
            PrefixAction::WorkspaceFocus => &self.workspace_focus,
        }
    }

    fn key_mut(&mut self, action: PrefixAction) -> &mut Key {
        match action {
            PrefixAction::NewTab => &mut self.new_tab,
            PrefixAction::CloseTab => &mut self.close_tab,
            PrefixAction::NextTab => &mut self.next_tab,
            PrefixAction::PrevTab => &mut self.prev_tab,
            PrefixAction::OpenSettings => &mut self.open_settings,
            PrefixAction::WorkspaceFocus => &mut self.workspace_focus,
        }
    }

    /// The action `press` triggers after the prefix. Like Electron, modifiers are ignored and
    /// the key or the character it types counts ([`KeyPress::is_or_types`]); the first action
    /// in [`PrefixAction::ALL`] order wins.
    pub fn action_for(&self, press: &KeyPress) -> Option<PrefixAction> {
        PrefixAction::ALL.into_iter().find(|action| press.is_or_types(self.key(*action)))
    }

    /// The prefix-mode cheat sheet: every action with its key, sorted by key label.
    pub fn overlay_items(&self, os: Os) -> Vec<OverlayItem> {
        let mut items: Vec<OverlayItem> = PrefixAction::ALL
            .into_iter()
            .map(|action| OverlayItem { key_label: self.key(action).label(os).into_owned(), action })
            .collect();
        items.sort_by(|a, b| a.key_label.cmp(&b.key_label));
        items
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeybindingSettings {
    /// `prefixMode.prefixKey`.
    pub prefix_key: Shortcut,
    /// `prefixMode.timeout`: how long prefix mode (and workspace focus) waits for a key.
    pub timeout: Duration,
    pub actions: ActionKeys,
}

impl Default for KeybindingSettings {
    /// Electron's defaults: `Control+B` (Control on every OS), 1500 ms.
    fn default() -> Self {
        Self {
            prefix_key: Shortcut::new(Key::Char('b'), Modifiers { control: true, ..Modifiers::NONE }),
            timeout: Duration::from_millis(1500),
            actions: ActionKeys::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeybindingSettingsError {
    #[error("prefixMode.prefixKey: {0}")]
    PrefixKey(#[source] ParseError),
    #[error("keybindings.{}: {source}", .action.settings_key())]
    ActionKey {
        action: PrefixAction,
        #[source]
        source: ParseError,
    },
}

impl KeybindingSettings {
    /// Read from the whole `settings.json` value. Like `settings::from_value`, a missing key
    /// or one of the wrong type takes the Electron default; an empty action key does too (as
    /// in Electron's `migrateKeybindings`), and so does the legacy `{ prefixMode: "c" }`
    /// action form. A key string that does not parse is an error to show, not a silent
    /// default. `prefixMode.enabled` is not read: Electron ignores it as well.
    pub fn from_settings_json(value: &Value, os: Os) -> Result<Self, KeybindingSettingsError> {
        let defaults = Self::default();
        let prefix = &value["prefixMode"];
        let prefix_key = match prefix["prefixKey"].as_str() {
            Some(accelerator) => {
                Shortcut::parse(accelerator, os).map_err(KeybindingSettingsError::PrefixKey)?
            }
            None => defaults.prefix_key,
        };
        let timeout = prefix["timeout"].as_u64().map_or(defaults.timeout, Duration::from_millis);
        let mut actions = defaults.actions;
        for action in PrefixAction::ALL {
            let raw = &value["keybindings"][action.settings_key()];
            let stored = match raw {
                Value::String(key) => key.as_str(),
                Value::Object(legacy) => legacy.get("prefixMode").and_then(Value::as_str).unwrap_or(""),
                _ => "",
            };
            if !stored.is_empty() {
                *actions.key_mut(action) = Key::parse(stored)
                    .map_err(|source| KeybindingSettingsError::ActionKey { action, source })?;
            }
        }
        Ok(Self { prefix_key, timeout, actions })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_sections_take_electron_defaults() {
        assert_eq!(
            KeybindingSettings::from_settings_json(&json!({}), Os::MacOs),
            Ok(KeybindingSettings::default())
        );
    }

    #[test]
    fn reads_the_prefix_and_action_keys() {
        let settings = KeybindingSettings::from_settings_json(
            &json!({
                "prefixMode": { "enabled": true, "prefixKey": "CommandOrControl+A", "timeout": 900 },
                "keybindings": {
                    "newTab": "t", "closeTab": "Q", "nextTab": "ArrowRight", "prevTab": "ArrowLeft",
                    "openSettings": " ", "workspaceFocus": "F2"
                }
            }),
            Os::MacOs,
        )
        .unwrap();
        assert_eq!(settings.prefix_key.to_gpui(Os::MacOs), "cmd-a");
        assert_eq!(settings.timeout, Duration::from_millis(900));
        assert_eq!(
            settings.actions,
            ActionKeys {
                new_tab: Key::Char('t'),
                close_tab: Key::Char('q'),
                next_tab: Key::Right,
                prev_tab: Key::Left,
                open_settings: Key::Space,
                // Electron's loader drops this one (see `migrateKeybindings`); read it anyway.
                workspace_focus: Key::F(2),
            }
        );
    }

    #[test]
    fn wrong_types_empty_strings_and_legacy_objects_fall_back_per_key() {
        let settings = KeybindingSettings::from_settings_json(
            &json!({
                "prefixMode": { "prefixKey": 7, "timeout": "slow" },
                "keybindings": {
                    "newTab": { "direct": "CommandOrControl+T", "prefixMode": "t" },
                    "closeTab": { "direct": "CommandOrControl+W" },
                    "nextTab": "",
                    "prevTab": null,
                    "openSettings": 3
                }
            }),
            Os::Linux,
        )
        .unwrap();
        let defaults = KeybindingSettings::default();
        assert_eq!(settings.prefix_key, defaults.prefix_key);
        assert_eq!(settings.timeout, defaults.timeout);
        assert_eq!(settings.actions, ActionKeys { new_tab: Key::Char('t'), ..ActionKeys::default() });
    }

    #[test]
    fn unparsable_keys_are_errors() {
        let error = KeybindingSettings::from_settings_json(
            &json!({ "prefixMode": { "prefixKey": "Hyper+B" } }),
            Os::Linux,
        )
        .unwrap_err();
        assert_eq!(
            error,
            KeybindingSettingsError::PrefixKey(ParseError::UnknownModifier {
                binding: "Hyper+B".to_owned(),
                modifier: "Hyper".to_owned(),
            })
        );
        assert_eq!(
            error.to_string(),
            "prefixMode.prefixKey: key binding \"Hyper+B\": unknown modifier \"Hyper\""
        );

        let error = KeybindingSettings::from_settings_json(
            &json!({ "keybindings": { "prevTab": "Shift" } }),
            Os::Linux,
        )
        .unwrap_err();
        assert_eq!(
            error,
            KeybindingSettingsError::ActionKey {
                action: PrefixAction::PrevTab,
                source: ParseError::ModifierAsKey { binding: "Shift".to_owned(), key: "Shift".to_owned() },
            }
        );
        assert_eq!(
            error.to_string(),
            "keybindings.prevTab: key binding \"Shift\": \"Shift\" is a modifier, not a key"
        );
    }

    #[test]
    fn overlay_items_are_sorted_by_key_label() {
        let items = ActionKeys::default().overlay_items(Os::MacOs);
        let rows: Vec<(&str, &str)> =
            items.iter().map(|item| (item.key_label.as_str(), item.action.label())).collect();
        assert_eq!(
            rows,
            vec![
                (",", "Open Settings"),
                ("C", "New Tab"),
                ("N", "Next Tab"),
                ("P", "Previous Tab"),
                ("W", "Workspace Focus"),
                ("X", "Close Tab"),
            ]
        );
    }

    #[test]
    fn the_first_action_in_settings_order_wins_a_shared_key() {
        let actions = ActionKeys { open_settings: Key::Char('c'), ..ActionKeys::default() };
        let press = KeyPress::from_gpui("c", Some("c"), Modifiers::NONE);
        assert_eq!(actions.action_for(&press), Some(PrefixAction::NewTab));
    }
}
