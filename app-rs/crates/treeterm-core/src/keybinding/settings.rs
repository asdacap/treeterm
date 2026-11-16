//! The `prefixMode` and `keybindings` sections of `settings.json` (shared with Electron).

use std::time::Duration;

use super::key::{Key, KeyPress, Os, ParseError, Shortcut};
use crate::locale;
use crate::settings::{Keybindings as StoredKeys, Settings};

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

    /// This action's key as `settings.keybindings` holds it.
    pub fn stored_key(self, keys: &StoredKeys) -> &str {
        match self {
            PrefixAction::NewTab => &keys.new_tab,
            PrefixAction::CloseTab => &keys.close_tab,
            PrefixAction::NextTab => &keys.next_tab,
            PrefixAction::PrevTab => &keys.prev_tab,
            PrefixAction::OpenSettings => &keys.open_settings,
            PrefixAction::WorkspaceFocus => &keys.workspace_focus,
        }
    }
}

/// A key binding as `settings.json` holds it, and what it matches. As in Electron, a
/// binding that can match no key press only disables itself: it is still shown by its text,
/// and every other binding keeps working.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding<T> {
    text: String,
    parsed: Result<T, ParseError>,
}

impl<T> Binding<T> {
    /// The binding as written.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// What the binding matches, or why it can match nothing.
    pub fn parsed(&self) -> &Result<T, ParseError> {
        &self.parsed
    }
}

impl Binding<Shortcut> {
    /// `prefixMode.prefixKey`. Like tinykeys' `parseKeybinding`, only the first of
    /// space-separated presses counts (`Control+A Control+B` is `Control+A`), and an empty
    /// one matches nothing: prefix mode is then off.
    pub fn prefix(text: impl Into<String>, os: Os) -> Self {
        let text = text.into();
        let trimmed = text.trim();
        let first = trimmed.split_once(' ').map_or(trimmed, |(first, _)| first);
        let parsed = Shortcut::parse(first, os);
        Self { text, parsed }
    }
}

impl Binding<Key> {
    /// A key pressed after the prefix. Electron compares it with the pressed key as written,
    /// untrimmed: ` ` is Space.
    pub fn action(text: impl Into<String>) -> Self {
        let text = text.into();
        let parsed = Key::parse(&text);
        Self { text, parsed }
    }
}

/// The key pressed after the prefix for each action (`settings.keybindings`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionKeys {
    pub new_tab: Binding<Key>,
    pub close_tab: Binding<Key>,
    pub next_tab: Binding<Key>,
    pub prev_tab: Binding<Key>,
    pub open_settings: Binding<Key>,
    pub workspace_focus: Binding<Key>,
}

impl Default for ActionKeys {
    /// Electron's: `c`, `x`, `n`, `p`, `,`, `w`.
    fn default() -> Self {
        Self::from_settings(&StoredKeys::default())
    }
}

/// One row of the prefix-mode overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayItem {
    pub key_label: String,
    pub action: PrefixAction,
}

impl ActionKeys {
    pub fn from_settings(keys: &StoredKeys) -> Self {
        let binding = |action: PrefixAction| Binding::action(action.stored_key(keys));
        Self {
            new_tab: binding(PrefixAction::NewTab),
            close_tab: binding(PrefixAction::CloseTab),
            next_tab: binding(PrefixAction::NextTab),
            prev_tab: binding(PrefixAction::PrevTab),
            open_settings: binding(PrefixAction::OpenSettings),
            workspace_focus: binding(PrefixAction::WorkspaceFocus),
        }
    }

    pub fn key(&self, action: PrefixAction) -> &Binding<Key> {
        match action {
            PrefixAction::NewTab => &self.new_tab,
            PrefixAction::CloseTab => &self.close_tab,
            PrefixAction::NextTab => &self.next_tab,
            PrefixAction::PrevTab => &self.prev_tab,
            PrefixAction::OpenSettings => &self.open_settings,
            PrefixAction::WorkspaceFocus => &self.workspace_focus,
        }
    }

    /// The action `press` triggers after the prefix. Like Electron, modifiers are ignored and
    /// the key or the character it types counts ([`KeyPress::is_or_types`]); the first action
    /// in [`PrefixAction::ALL`] order wins. A key that matches nothing triggers nothing.
    pub fn action_for(&self, press: &KeyPress) -> Option<PrefixAction> {
        PrefixAction::ALL
            .into_iter()
            .find(|action| matches!(self.key(*action).parsed(), Ok(key) if press.is_or_types(key)))
    }

    /// The prefix-mode cheat sheet (`getAvailableKeybindings`): every action with its key as
    /// written, upper-cased (`ARROWRIGHT`, ` `), in `localeCompare` order of those labels.
    pub fn overlay_items(&self) -> Vec<OverlayItem> {
        let mut items: Vec<OverlayItem> = PrefixAction::ALL
            .into_iter()
            .map(|action| OverlayItem { key_label: self.key(action).text().to_uppercase(), action })
            .collect();
        items.sort_by(|a, b| locale::compare(&a.key_label, &b.key_label));
        items
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeybindingSettings {
    /// `prefixMode.prefixKey`. While it matches nothing, prefix mode cannot start; Cmd/Ctrl+1–9
    /// still work.
    pub prefix_key: Binding<Shortcut>,
    /// `prefixMode.timeout`: how long prefix mode (and workspace focus) waits for a key. Zero
    /// for a negative one: Electron's `setTimeout` runs a negative delay at once.
    pub timeout: Duration,
    pub actions: ActionKeys,
}

impl Default for KeybindingSettings {
    /// Electron's defaults: `Control+B` (Control on every OS), 1500 ms, [`ActionKeys`]'.
    fn default() -> Self {
        Self::from_settings(&Settings::default(), Os::host())
    }
}

/// A key binding that can match no key press, so does nothing.
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
    /// From the loaded settings, whose reader already gave a missing or mistyped key, an
    /// empty action key and the legacy `{ prefixMode: "c" }` action form their Electron
    /// meaning. Never fails: as in Electron, a key that can match nothing disables only
    /// itself ([`Self::problems`] lists those). `prefixMode.enabled` is not read: Electron
    /// ignores it as well.
    pub fn from_settings(settings: &Settings, os: Os) -> Self {
        Self {
            prefix_key: Binding::prefix(settings.prefix_mode.prefix_key.as_str(), os),
            timeout: Duration::from_millis(settings.prefix_mode.timeout_ms.max(0).unsigned_abs()),
            actions: ActionKeys::from_settings(&settings.keybindings),
        }
    }

    /// Every binding that can match no key press.
    pub fn problems(&self) -> Vec<KeybindingSettingsError> {
        let prefix = match self.prefix_key.parsed() {
            Ok(_) => None,
            Err(error) => Some(KeybindingSettingsError::PrefixKey(error.clone())),
        };
        let actions = PrefixAction::ALL.into_iter().filter_map(|action| match self.actions.key(action).parsed() {
            Ok(_) => None,
            Err(source) => Some(KeybindingSettingsError::ActionKey { action, source: source.clone() }),
        });
        prefix.into_iter().chain(actions).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keybinding::Modifiers;
    use crate::settings::from_value;
    use serde_json::json;

    fn read(value: serde_json::Value, os: Os) -> KeybindingSettings {
        KeybindingSettings::from_settings(&from_value(value).unwrap(), os)
    }

    #[test]
    fn missing_sections_take_electron_defaults() {
        let settings = read(json!({}), Os::MacOs);
        assert_eq!(settings, KeybindingSettings::default());
        assert_eq!(settings.prefix_key.text(), "Control+B");
        assert_eq!(settings.prefix_key.parsed().as_ref().unwrap().to_gpui(Os::MacOs), "ctrl-b");
        assert_eq!(settings.timeout, Duration::from_millis(1500));
        let keys: Vec<&str> = PrefixAction::ALL.iter().map(|a| settings.actions.key(*a).text()).collect();
        assert_eq!(keys, ["c", "x", "n", "p", ",", "w"]);
        assert_eq!(settings.problems(), []);
    }

    #[test]
    fn reads_the_prefix_and_action_keys() {
        let settings = read(
            json!({
                "prefixMode": { "enabled": true, "prefixKey": "CommandOrControl+A", "timeout": 900 },
                "keybindings": {
                    "newTab": "t", "closeTab": "Q", "nextTab": "ArrowRight", "prevTab": "ArrowLeft",
                    "openSettings": " ", "workspaceFocus": "F2"
                }
            }),
            Os::MacOs,
        );
        assert_eq!(settings.prefix_key.text(), "CommandOrControl+A");
        assert_eq!(settings.prefix_key.parsed().as_ref().unwrap().to_gpui(Os::MacOs), "cmd-a");
        assert_eq!(settings.timeout, Duration::from_millis(900));
        let keys: Vec<(&str, Key)> = PrefixAction::ALL
            .iter()
            .map(|a| (settings.actions.key(*a).text(), settings.actions.key(*a).parsed().clone().unwrap()))
            .collect();
        assert_eq!(
            keys,
            [
                ("t", Key::Char('t')),
                ("Q", Key::Char('q')),
                ("ArrowRight", Key::Right),
                ("ArrowLeft", Key::Left),
                (" ", Key::Space),
                // Electron's loader drops this one (see `migrateKeybindings`); read it anyway.
                ("F2", Key::F(2)),
            ]
        );
        assert_eq!(settings.problems(), []);
    }

    /// Electron stores a pasted negative timeout as typed, and its `setTimeout` ends prefix
    /// mode at once.
    #[test]
    fn a_negative_timeout_is_no_time() {
        for timeout in [-5, 0] {
            let settings = read(json!({ "prefixMode": { "timeout": timeout } }), Os::MacOs);
            assert_eq!(settings.timeout, Duration::ZERO, "{timeout}");
        }
    }

    #[test]
    fn wrong_types_empty_strings_and_legacy_objects_fall_back_per_key() {
        let settings = read(
            json!({
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
        );
        let defaults = KeybindingSettings::default();
        assert_eq!(settings.prefix_key, defaults.prefix_key);
        assert_eq!(settings.timeout, defaults.timeout);
        assert_eq!(settings.actions, ActionKeys { new_tab: Binding::action("t"), ..ActionKeys::default() });
    }

    /// `parseKeybinding(prefixKey)[0]`: an empty prefix key matches nothing.
    #[test]
    fn an_empty_prefix_key_matches_nothing_and_keeps_the_rest() {
        let settings = read(
            json!({ "prefixMode": { "prefixKey": "", "timeout": 900 }, "keybindings": { "newTab": "t" } }),
            Os::Linux,
        );
        assert_eq!(settings.prefix_key.text(), "");
        assert_eq!(settings.prefix_key.parsed(), &Err(ParseError::Empty));
        assert_eq!(settings.timeout, Duration::from_millis(900));
        assert_eq!(settings.actions.new_tab, Binding::action("t"));
        assert_eq!(settings.problems(), [KeybindingSettingsError::PrefixKey(ParseError::Empty)]);
        assert_eq!(Binding::prefix("   ", Os::Linux).parsed(), &Err(ParseError::Empty));
    }

    /// tinykeys splits a sequence on spaces; only its first press is the prefix.
    #[test]
    fn only_the_first_press_of_a_sequence_is_the_prefix() {
        let prefix = Binding::prefix(" Control+A Control+B ", Os::Linux);
        assert_eq!(prefix.text(), " Control+A Control+B ");
        assert_eq!(prefix.parsed().as_ref().unwrap().to_gpui(Os::Linux), "ctrl-a");
        // Recorded Ctrl+Space (`CommandOrControl+ `) trims to a press with no key.
        let space = Binding::prefix("CommandOrControl+ ", Os::Linux);
        assert_eq!(space.parsed(), &Err(ParseError::EmptyPart { binding: "CommandOrControl+".to_owned() }));
    }

    #[test]
    fn an_unparsable_prefix_key_disables_only_itself() {
        let settings = read(
            json!({
                "prefixMode": { "prefixKey": "Hyper+B", "timeout": 900 },
                "keybindings": { "newTab": "t" }
            }),
            Os::Linux,
        );
        let error = ParseError::UnknownModifier { binding: "Hyper+B".to_owned(), modifier: "Hyper".to_owned() };
        assert_eq!(settings.prefix_key.parsed(), &Err(error.clone()));
        assert_eq!(settings.timeout, Duration::from_millis(900));
        assert_eq!(settings.actions.new_tab, Binding::action("t"));
        let problems = settings.problems();
        assert_eq!(problems, [KeybindingSettingsError::PrefixKey(error)]);
        assert_eq!(
            problems[0].to_string(),
            "prefixMode.prefixKey: key binding \"Hyper+B\": unknown modifier \"Hyper\""
        );
    }

    /// Electron's recorder stores DOM key names such as `CapsLock` that no GPUI key press
    /// spells: that action does nothing, the rest keep working.
    #[test]
    fn an_unparsable_action_key_disables_only_that_action() {
        let settings = read(
            json!({
                "prefixMode": { "prefixKey": "Alt+A", "timeout": 900 },
                "keybindings": { "newTab": "CapsLock", "prevTab": "Shift", "closeTab": " c" }
            }),
            Os::Linux,
        );
        assert_eq!(settings.prefix_key.parsed().as_ref().unwrap().to_gpui(Os::Linux), "alt-a");
        assert_eq!(settings.timeout, Duration::from_millis(900));
        assert_eq!(settings.actions.next_tab, Binding::action("n"));
        let problems = settings.problems();
        assert_eq!(
            problems,
            [
                KeybindingSettingsError::ActionKey {
                    action: PrefixAction::NewTab,
                    source: ParseError::UnknownKey { binding: "CapsLock".to_owned(), key: "CapsLock".to_owned() },
                },
                KeybindingSettingsError::ActionKey {
                    action: PrefixAction::CloseTab,
                    source: ParseError::UnknownKey { binding: " c".to_owned(), key: " c".to_owned() },
                },
                KeybindingSettingsError::ActionKey {
                    action: PrefixAction::PrevTab,
                    source: ParseError::ModifierAsKey { binding: "Shift".to_owned(), key: "Shift".to_owned() },
                },
            ]
        );
        assert_eq!(problems[2].to_string(), "keybindings.prevTab: key binding \"Shift\": \"Shift\" is a modifier, not a key");

        let n = KeyPress::from_gpui("n", Some("n"), Modifiers::NONE);
        assert_eq!(settings.actions.action_for(&n), Some(PrefixAction::NextTab));
        let c = KeyPress::from_gpui("c", Some("c"), Modifiers::NONE);
        assert_eq!(settings.actions.action_for(&c), None);
    }

    #[test]
    fn overlay_items_are_the_keys_as_written_upper_cased_in_locale_order() {
        let rows = |actions: &ActionKeys| -> Vec<(String, &str)> {
            actions.overlay_items().into_iter().map(|item| (item.key_label, item.action.label())).collect()
        };
        assert_eq!(
            rows(&ActionKeys::default()),
            [
                (",".to_owned(), "Open Settings"),
                ("C".to_owned(), "New Tab"),
                ("N".to_owned(), "Next Tab"),
                ("P".to_owned(), "Previous Tab"),
                ("W".to_owned(), "Workspace Focus"),
                ("X".to_owned(), "Close Tab"),
            ]
        );
        let actions = ActionKeys {
            new_tab: Binding::action("ArrowRight"),
            close_tab: Binding::action(" "),
            next_tab: Binding::action("_"),
            prev_tab: Binding::action("["),
            open_settings: Binding::action("0"),
            workspace_focus: Binding::action("CapsLock"),
        };
        // `localeCompare`, not code point order (`0` < `C` < `[` < `_`).
        assert_eq!(
            rows(&actions),
            [
                (" ".to_owned(), "Close Tab"),
                ("_".to_owned(), "Next Tab"),
                ("[".to_owned(), "Previous Tab"),
                ("0".to_owned(), "Open Settings"),
                ("ARROWRIGHT".to_owned(), "New Tab"),
                ("CAPSLOCK".to_owned(), "Workspace Focus"),
            ]
        );
    }

    #[test]
    fn the_first_action_in_settings_order_wins_a_shared_key() {
        let actions = ActionKeys { open_settings: Binding::action("c"), ..ActionKeys::default() };
        let press = KeyPress::from_gpui("c", Some("c"), Modifiers::NONE);
        assert_eq!(actions.action_for(&press), Some(PrefixAction::NewTab));
    }
}
