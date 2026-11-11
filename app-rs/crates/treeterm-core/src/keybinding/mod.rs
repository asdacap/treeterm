//! Keyboard shortcuts: tmux-style prefix mode, workspace focus and the direct shortcuts
//! (port of `store/keybinding.ts` and `utils/keybindingConverter.ts`).
//!
//! Sans-IO. The UI intercepts every keystroke before dispatch (GPUI
//! `intercept_keystrokes`), converts it with [`KeyPress::from_gpui`] and feeds it to
//! [`Keybindings::handle_key`] with the current time:
//!
//! - [`KeyOutcome::PassThrough`]: let the keystroke through to the focused view;
//! - [`KeyOutcome::Consumed`]: stop propagation;
//! - [`KeyOutcome::Perform`]: stop propagation and perform the [`Action`]. For
//!   [`PrefixAction::WorkspaceFocus`] that means calling
//!   [`Keybindings::enter_workspace_focus`] with the flattened workspace tree.
//!
//! Prefix mode and workspace focus time out. After each call the UI re-renders and replaces
//! its one timer with [`Keybindings::deadline`], which calls [`Keybindings::on_timeout`].
//!
//! The overlay reads [`Keybindings::state`] (or [`Keybindings::mode`]),
//! [`Keybindings::remaining_fraction`] for the progress bar, [`Shortcut::label`] of the
//! prefix key and [`ActionKeys::overlay_items`]. The tree highlights
//! [`Keybindings::focused_workspace`].
//!
//! Settings come from [`KeybindingSettings::from_settings_json`]; [`Shortcut::to_gpui`]
//! turns an Electron accelerator into a GPUI keybinding string.

mod key;
mod machine;
mod settings;

pub use key::{Key, KeyPress, ModifierKey, Modifiers, Os, ParseError, Shortcut};
pub use machine::{
    Action, Deadline, FocusDirection, FocusIndexOutOfRange, KeyOutcome, Keybindings, PrefixMode, PrefixState,
    WorkspaceCursor,
};
pub use settings::{ActionKeys, KeybindingSettings, KeybindingSettingsError, OverlayItem, PrefixAction};
