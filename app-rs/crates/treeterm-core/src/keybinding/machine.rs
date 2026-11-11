//! The shortcut state machine (port of `useKeybindingStore` in `store/keybinding.ts`).

use std::time::{Duration, Instant};

use super::key::{Key, KeyPress};
use super::settings::{KeybindingSettings, PrefixAction};

/// Where prefix mode is, with the data each state needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefixState {
    Idle,
    /// The prefix key was pressed: the next key picks an action.
    Active {
        activated_at: Instant,
    },
    /// Up/Down move through the workspace tree, Enter selects, Escape cancels.
    WorkspaceFocus {
        activated_at: Instant,
        cursor: WorkspaceCursor,
    },
}

/// [`PrefixState`] without its data (the TS `PrefixModeState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixMode {
    Idle,
    Active,
    WorkspaceFocus,
}

/// The workspaces offered in workspace focus (flattened tree order) and the focused one.
/// The index is always in range; an empty list (index 0) focuses nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCursor {
    workspace_ids: Vec<String>,
    focused_index: usize,
}

impl WorkspaceCursor {
    fn new(workspace_ids: Vec<String>, focused_index: usize) -> Result<Self, FocusIndexOutOfRange> {
        let len = workspace_ids.len();
        if focused_index < len || (len == 0 && focused_index == 0) {
            Ok(Self { workspace_ids, focused_index })
        } else {
            Err(FocusIndexOutOfRange { index: focused_index, len })
        }
    }

    pub fn workspace_ids(&self) -> &[String] {
        &self.workspace_ids
    }

    pub fn focused_index(&self) -> usize {
        self.focused_index
    }

    /// `None` only for an empty list.
    pub fn focused_workspace(&self) -> Option<&str> {
        self.workspace_ids.get(self.focused_index).map(String::as_str)
    }

    /// Move with wrap-around; nothing to do on an empty list.
    fn navigate(&mut self, direction: FocusDirection) {
        let len = self.workspace_ids.len();
        if len == 0 {
            return;
        }
        self.focused_index = match direction {
            FocusDirection::Up => (self.focused_index + len - 1) % len,
            FocusDirection::Down => (self.focused_index + 1) % len,
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    Up,
    Down,
}

/// When the UI must call [`Keybindings::on_timeout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deadline {
    /// No timer: cancel any pending one.
    Never,
    At(Instant),
}

/// What the UI does with a keystroke.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyOutcome {
    /// Not a shortcut: dispatch the keystroke as usual (to the focused terminal, ...).
    PassThrough,
    /// Swallow the keystroke: the focused view must not see it.
    Consumed,
    /// Swallow the keystroke and perform the action.
    Perform(Action),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// A prefix-mode action. For [`PrefixAction::WorkspaceFocus`] the UI answers with
    /// [`Keybindings::enter_workspace_focus`] (flattened tree order, active workspace index).
    Prefix(PrefixAction),
    /// Cmd/Ctrl+1..9: select the active workspace's tab at `index` (0-based), if it exists.
    SwitchToTab { index: usize },
    /// Enter in workspace focus.
    SetActiveWorkspace { workspace_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("workspace focus index {index} is out of range for {len} workspaces")]
pub struct FocusIndexOutOfRange {
    pub index: usize,
    pub len: usize,
}

const TAB_DIGITS: [char; 9] = ['1', '2', '3', '4', '5', '6', '7', '8', '9'];

/// Keyboard shortcuts as a sans-IO state machine.
///
/// The UI intercepts every keystroke before dispatch and calls [`Self::handle_key`] with the
/// current time, swallowing the keystroke unless the outcome is
/// [`KeyOutcome::PassThrough`]. After every call that takes `now` (or changes the state),
/// it re-renders and replaces its single timer with one for [`Self::deadline`], which calls
/// [`Self::on_timeout`]. Stale or late timers are harmless: the machine compares against
/// its own deadline, and a key arriving after the deadline finds the mode already expired.
#[derive(Debug, Clone)]
pub struct Keybindings {
    settings: KeybindingSettings,
    state: PrefixState,
}

impl Keybindings {
    pub fn new(settings: KeybindingSettings) -> Self {
        Self { settings, state: PrefixState::Idle }
    }

    pub fn settings(&self) -> &KeybindingSettings {
        &self.settings
    }

    /// Apply edited settings. A running mode keeps its start time, so a new timeout moves
    /// its deadline: reschedule.
    pub fn set_settings(&mut self, settings: KeybindingSettings) {
        self.settings = settings;
    }

    pub fn state(&self) -> &PrefixState {
        &self.state
    }

    pub fn mode(&self) -> PrefixMode {
        match self.state {
            PrefixState::Idle => PrefixMode::Idle,
            PrefixState::Active { .. } => PrefixMode::Active,
            PrefixState::WorkspaceFocus { .. } => PrefixMode::WorkspaceFocus,
        }
    }

    /// The workspace highlighted in workspace focus (TS `selectFocusedWorkspace`).
    pub fn focused_workspace(&self) -> Option<&str> {
        match &self.state {
            PrefixState::WorkspaceFocus { cursor, .. } => cursor.focused_workspace(),
            PrefixState::Idle | PrefixState::Active { .. } => None,
        }
    }

    pub fn deadline(&self) -> Deadline {
        match &self.state {
            PrefixState::Idle => Deadline::Never,
            PrefixState::Active { activated_at } | PrefixState::WorkspaceFocus { activated_at, .. } => {
                activated_at.checked_add(self.settings.timeout).map_or(Deadline::Never, Deadline::At)
            }
        }
    }

    /// Fraction of the timeout left, for the overlay's progress bar: 1.0 just activated (and
    /// while idle), 0.0 at the deadline.
    pub fn remaining_fraction(&self, now: Instant) -> f32 {
        match &self.state {
            PrefixState::Idle => 1.0,
            PrefixState::Active { activated_at } | PrefixState::WorkspaceFocus { activated_at, .. } => {
                let timeout = self.settings.timeout;
                if timeout == Duration::ZERO {
                    return 0.0;
                }
                let elapsed = now.saturating_duration_since(*activated_at);
                (1.0 - elapsed.as_secs_f32() / timeout.as_secs_f32()).clamp(0.0, 1.0)
            }
        }
    }

    pub fn activate(&mut self, now: Instant) {
        self.state = PrefixState::Active { activated_at: now };
    }

    pub fn deactivate(&mut self) {
        self.state = PrefixState::Idle;
    }

    /// Start workspace focus on `workspace_ids` (flattened tree order) with
    /// `focused_index` highlighted, for the same timeout as prefix mode.
    pub fn enter_workspace_focus(
        &mut self,
        workspace_ids: Vec<String>,
        focused_index: usize,
        now: Instant,
    ) -> Result<(), FocusIndexOutOfRange> {
        let cursor = WorkspaceCursor::new(workspace_ids, focused_index)?;
        self.state = PrefixState::WorkspaceFocus { activated_at: now, cursor };
        Ok(())
    }

    /// Move the highlight in workspace focus. Does not extend the timeout (as in Electron).
    pub fn navigate_workspace(&mut self, direction: FocusDirection) {
        match &mut self.state {
            PrefixState::WorkspaceFocus { cursor, .. } => cursor.navigate(direction),
            PrefixState::Idle | PrefixState::Active { .. } => {}
        }
    }

    /// The prefix timer fired (or any time the UI wants to settle the clock).
    pub fn on_timeout(&mut self, now: Instant) {
        if let Deadline::At(deadline) = self.deadline()
            && now >= deadline
        {
            self.deactivate();
        }
    }

    pub fn handle_key(&mut self, press: &KeyPress, now: Instant) -> KeyOutcome {
        // A key can beat a late timer; an expired mode must not take it.
        self.on_timeout(now);

        if self.settings.prefix_key.matches(press) {
            return match self.state {
                PrefixState::Idle => {
                    self.activate(now);
                    KeyOutcome::Consumed
                }
                // The prefix twice sends the prefix key itself to the terminal (tmux `C-b C-b`).
                PrefixState::Active { .. } => {
                    self.deactivate();
                    KeyOutcome::PassThrough
                }
                PrefixState::WorkspaceFocus { .. } => {
                    self.deactivate();
                    KeyOutcome::Consumed
                }
            };
        }

        match &mut self.state {
            PrefixState::Idle => direct_shortcut(press),
            PrefixState::Active { .. } => {
                if *press.key() == Key::Escape {
                    self.deactivate();
                    return KeyOutcome::Consumed;
                }
                if let Some(action) = self.settings.actions.action_for(press) {
                    self.deactivate();
                    return KeyOutcome::Perform(Action::Prefix(action));
                }
                match press.key() {
                    // Holding a modifier on the way to the action key keeps prefix mode.
                    Key::Modifier(_) => KeyOutcome::PassThrough,
                    // Any other key cancels prefix mode and still reaches the terminal.
                    _ => {
                        self.deactivate();
                        KeyOutcome::PassThrough
                    }
                }
            }
            PrefixState::WorkspaceFocus { cursor, .. } => match press.key() {
                Key::Escape => {
                    self.deactivate();
                    KeyOutcome::Consumed
                }
                Key::Enter => {
                    let selected = cursor.focused_workspace().map(str::to_owned);
                    self.deactivate();
                    match selected {
                        Some(workspace_id) => {
                            KeyOutcome::Perform(Action::SetActiveWorkspace { workspace_id })
                        }
                        None => KeyOutcome::Consumed,
                    }
                }
                Key::Up => {
                    cursor.navigate(FocusDirection::Up);
                    KeyOutcome::Consumed
                }
                Key::Down => {
                    cursor.navigate(FocusDirection::Down);
                    KeyOutcome::Consumed
                }
                // Other keys reach the terminal and leave workspace focus running.
                _ => KeyOutcome::PassThrough,
            },
        }
    }
}

/// Shortcuts outside prefix mode: Cmd/Ctrl+1..9 switch tabs.
fn direct_shortcut(press: &KeyPress) -> KeyOutcome {
    let modifiers = press.modifiers();
    match press.key() {
        Key::Char(c) if modifiers.control || modifiers.platform => {
            match TAB_DIGITS.iter().position(|d| d == c) {
                Some(index) => KeyOutcome::Perform(Action::SwitchToTab { index }),
                None => KeyOutcome::PassThrough,
            }
        }
        _ => KeyOutcome::PassThrough,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keybinding::key::{Modifiers, Os, Shortcut};

    const CTRL: Modifiers = Modifiers { control: true, ..Modifiers::NONE };
    const META: Modifiers = Modifiers { platform: true, ..Modifiers::NONE };
    const SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::NONE };
    const TIMEOUT: Duration = Duration::from_millis(1500);

    /// The TS tests' settings: `Control+B`, 1500 ms, the default action keys.
    fn machine() -> Keybindings {
        Keybindings::new(KeybindingSettings::default())
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn ctrl_b() -> KeyPress {
        KeyPress::from_gpui("b", None, CTRL)
    }

    /// A key pressed without modifiers, typing itself when printable (as GPUI reports it).
    fn key(name: &str) -> KeyPress {
        let typed = match Key::from_gpui(name) {
            Key::Char(_) => Some(name),
            _ => None,
        };
        KeyPress::from_gpui(name, typed, Modifiers::NONE)
    }

    fn ids(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    fn in_focus(workspaces: &[&str], index: usize, now: Instant) -> Keybindings {
        let mut kb = machine();
        kb.enter_workspace_focus(ids(workspaces), index, now).unwrap();
        kb
    }

    fn focused_index(kb: &Keybindings) -> usize {
        match kb.state() {
            PrefixState::WorkspaceFocus { cursor, .. } => cursor.focused_index(),
            other => panic!("not in workspace focus: {other:?}"),
        }
    }

    mod use_keybinding_store {
        use super::*;

        mod initial_state {
            use super::*;

            #[test]
            fn starts_in_idle_state_with_no_handlers() {
                let kb = machine();
                assert_eq!(kb.state(), &PrefixState::Idle);
                assert_eq!(kb.mode(), PrefixMode::Idle);
                assert_eq!(kb.deadline(), Deadline::Never);
                assert_eq!(kb.focused_workspace(), None);
            }
        }

        mod activate_deactivate {
            use super::*;

            #[test]
            fn activates_and_deactivates_prefix_mode() {
                let t0 = Instant::now();
                let mut kb = machine();
                kb.activate(t0);
                assert_eq!(kb.state(), &PrefixState::Active { activated_at: t0 });
                assert_eq!(kb.deadline(), Deadline::At(t0 + TIMEOUT));

                kb.deactivate();
                assert_eq!(kb.state(), &PrefixState::Idle);
                assert_eq!(kb.deadline(), Deadline::Never);
            }
        }

        mod enter_workspace_focus {
            use super::*;

            #[test]
            fn sets_state_to_workspace_focus_with_workspace_data() {
                let t0 = Instant::now();
                let kb = in_focus(&["ws1", "ws2", "ws3"], 1, t0);
                match kb.state() {
                    PrefixState::WorkspaceFocus { activated_at, cursor } => {
                        assert_eq!(*activated_at, t0);
                        assert_eq!(cursor.workspace_ids(), ids(&["ws1", "ws2", "ws3"]));
                        assert_eq!(cursor.focused_index(), 1);
                    }
                    other => panic!("not in workspace focus: {other:?}"),
                }
                assert_eq!(kb.mode(), PrefixMode::WorkspaceFocus);
                assert_eq!(kb.deadline(), Deadline::At(t0 + TIMEOUT));
            }

            #[test]
            fn rejects_an_out_of_range_index_and_keeps_the_state() {
                let t0 = Instant::now();
                let mut kb = machine();
                kb.activate(t0);
                assert_eq!(
                    kb.enter_workspace_focus(ids(&["ws1"]), 1, t0),
                    Err(FocusIndexOutOfRange { index: 1, len: 1 })
                );
                assert_eq!(
                    kb.enter_workspace_focus(Vec::new(), 1, t0),
                    Err(FocusIndexOutOfRange { index: 1, len: 0 })
                );
                assert_eq!(kb.mode(), PrefixMode::Active);
            }

            #[test]
            fn an_empty_list_focuses_nothing() {
                let kb = in_focus(&[], 0, Instant::now());
                assert_eq!(kb.mode(), PrefixMode::WorkspaceFocus);
                assert_eq!(kb.focused_workspace(), None);
            }
        }

        mod navigate_workspace {
            use super::*;

            fn navigate(start: usize, direction: FocusDirection) -> usize {
                let mut kb = in_focus(&["ws1", "ws2", "ws3"], start, Instant::now());
                kb.navigate_workspace(direction);
                focused_index(&kb)
            }

            #[test]
            fn moves_down() {
                assert_eq!(navigate(0, FocusDirection::Down), 1);
            }

            #[test]
            fn wraps_down_from_last() {
                assert_eq!(navigate(2, FocusDirection::Down), 0);
            }

            #[test]
            fn moves_up() {
                assert_eq!(navigate(2, FocusDirection::Up), 1);
            }

            #[test]
            fn wraps_up_from_first() {
                assert_eq!(navigate(0, FocusDirection::Up), 2);
            }

            #[test]
            fn does_nothing_when_no_workspaces() {
                let t0 = Instant::now();
                let mut kb = machine();
                kb.activate(t0);
                kb.navigate_workspace(FocusDirection::Down);
                assert_eq!(kb.state(), &PrefixState::Active { activated_at: t0 });

                let mut kb = in_focus(&[], 0, t0);
                kb.navigate_workspace(FocusDirection::Down);
                assert_eq!(focused_index(&kb), 0);
            }

            #[test]
            fn handles_single_workspace() {
                let mut kb = in_focus(&["ws1"], 0, Instant::now());
                kb.navigate_workspace(FocusDirection::Down);
                kb.navigate_workspace(FocusDirection::Up);
                assert_eq!(focused_index(&kb), 0);
            }
        }

        mod select_focused_workspace {
            use super::*;

            #[test]
            fn returns_null_when_no_workspaces() {
                assert_eq!(machine().focused_workspace(), None);
            }

            #[test]
            fn returns_focused_workspace() {
                let kb = in_focus(&["ws1", "ws2", "ws3"], 1, Instant::now());
                assert_eq!(kb.focused_workspace(), Some("ws2"));
            }

            /// The TS forced the index out of bounds through `setState`; here that state
            /// cannot be built, so the out-of-range index is refused up front.
            #[test]
            fn returns_null_when_out_of_bounds() {
                let mut kb = machine();
                assert!(kb.enter_workspace_focus(ids(&["ws1"]), 5, Instant::now()).is_err());
                assert_eq!(kb.focused_workspace(), None);
            }
        }

        mod handle_key_down {
            use super::*;

            #[test]
            fn pressing_prefix_key_in_idle_mode_activates() {
                let t0 = Instant::now();
                let mut kb = machine();
                assert_eq!(kb.handle_key(&ctrl_b(), t0), KeyOutcome::Consumed);
                assert_eq!(kb.state(), &PrefixState::Active { activated_at: t0 });
            }

            #[test]
            fn pressing_prefix_key_in_active_mode_deactivates_and_passes_the_key_through() {
                let t0 = Instant::now();
                let mut kb = machine();
                assert_eq!(kb.handle_key(&ctrl_b(), t0), KeyOutcome::Consumed);
                assert_eq!(kb.mode(), PrefixMode::Active);

                assert_eq!(kb.handle_key(&ctrl_b(), t0 + ms(100)), KeyOutcome::PassThrough);
                assert_eq!(kb.state(), &PrefixState::Idle);

                assert_eq!(kb.handle_key(&ctrl_b(), t0 + ms(200)), KeyOutcome::Consumed);
                assert_eq!(kb.mode(), PrefixMode::Active);
            }

            #[test]
            fn pressing_prefix_key_in_workspace_focus_consumes_the_key_and_cancels_navigation() {
                let t0 = Instant::now();
                let mut kb = in_focus(&["ws1"], 0, t0);
                assert_eq!(kb.handle_key(&ctrl_b(), t0), KeyOutcome::Consumed);
                assert_eq!(kb.state(), &PrefixState::Idle);
            }

            #[test]
            fn prefix_key_prevent_default_and_stop_propagation() {
                assert_eq!(machine().handle_key(&ctrl_b(), Instant::now()), KeyOutcome::Consumed);
            }

            mod in_active_prefix_mode {
                use super::*;

                fn active(t0: Instant) -> Keybindings {
                    let mut kb = machine();
                    assert_eq!(kb.handle_key(&ctrl_b(), t0), KeyOutcome::Consumed);
                    kb
                }

                #[test]
                fn pressing_key_calls_handler_and_deactivates() {
                    for (name, action) in [
                        ("c", PrefixAction::NewTab),
                        ("x", PrefixAction::CloseTab),
                        ("n", PrefixAction::NextTab),
                        ("p", PrefixAction::PrevTab),
                        (",", PrefixAction::OpenSettings),
                        ("w", PrefixAction::WorkspaceFocus),
                    ] {
                        let t0 = Instant::now();
                        let mut kb = active(t0);
                        assert_eq!(
                            kb.handle_key(&key(name), t0 + ms(10)),
                            KeyOutcome::Perform(Action::Prefix(action)),
                            "{name}"
                        );
                        assert_eq!(kb.state(), &PrefixState::Idle, "{name}");
                    }
                }

                #[test]
                fn escape_deactivates_without_calling_handlers() {
                    let t0 = Instant::now();
                    let mut kb = active(t0);
                    assert_eq!(kb.handle_key(&key("escape"), t0), KeyOutcome::Consumed);
                    assert_eq!(kb.state(), &PrefixState::Idle);
                }

                #[test]
                fn unknown_non_modifier_key_deactivates() {
                    let t0 = Instant::now();
                    let mut kb = active(t0);
                    // Not swallowed: the key still reaches the terminal, as in Electron.
                    assert_eq!(kb.handle_key(&key("z"), t0), KeyOutcome::PassThrough);
                    assert_eq!(kb.state(), &PrefixState::Idle);
                }

                #[test]
                fn modifier_only_key_shift_does_not_deactivate() {
                    let t0 = Instant::now();
                    let mut kb = active(t0);
                    let shift = KeyPress::from_gpui("shift", None, SHIFT);
                    assert_eq!(kb.handle_key(&shift, t0), KeyOutcome::PassThrough);
                    assert_eq!(kb.state(), &PrefixState::Active { activated_at: t0 });
                }

                #[test]
                fn modifier_only_key_meta_does_not_deactivate() {
                    let t0 = Instant::now();
                    let mut kb = active(t0);
                    let meta = KeyPress::from_gpui("platform", None, META);
                    assert_eq!(kb.handle_key(&meta, t0), KeyOutcome::PassThrough);
                    assert_eq!(kb.state(), &PrefixState::Active { activated_at: t0 });
                }

                #[test]
                fn action_keys_ignore_modifiers_and_match_the_typed_character() {
                    let t0 = Instant::now();
                    let mut kb = active(t0);
                    let shift_c = KeyPress::from_gpui("c", Some("C"), SHIFT);
                    assert_eq!(
                        kb.handle_key(&shift_c, t0),
                        KeyOutcome::Perform(Action::Prefix(PrefixAction::NewTab))
                    );

                    let mut kb = active(t0);
                    assert_eq!(
                        kb.handle_key(&KeyPress::from_gpui("x", None, CTRL), t0),
                        KeyOutcome::Perform(Action::Prefix(PrefixAction::CloseTab))
                    );

                    let mut kb = machine();
                    kb.set_settings(KeybindingSettings {
                        actions: crate::keybinding::ActionKeys {
                            open_settings: Key::Char('?'),
                            ..Default::default()
                        },
                        ..KeybindingSettings::default()
                    });
                    assert_eq!(kb.handle_key(&ctrl_b(), t0), KeyOutcome::Consumed);
                    // GPUI reports Shift+/ as `?` without Shift.
                    let question = KeyPress::from_gpui("?", Some("?"), Modifiers::NONE);
                    assert_eq!(
                        kb.handle_key(&question, t0),
                        KeyOutcome::Perform(Action::Prefix(PrefixAction::OpenSettings))
                    );
                }

                #[test]
                fn digits_are_not_tab_shortcuts_after_the_prefix() {
                    let t0 = Instant::now();
                    let mut kb = active(t0);
                    let cmd_1 = KeyPress::from_gpui("1", None, META);
                    assert_eq!(kb.handle_key(&cmd_1, t0), KeyOutcome::PassThrough);
                    assert_eq!(kb.state(), &PrefixState::Idle);
                }
            }

            mod in_workspace_focus_mode {
                use super::*;

                fn focus(t0: Instant) -> Keybindings {
                    in_focus(&["ws1", "ws2", "ws3"], 1, t0)
                }

                #[test]
                fn escape_deactivates() {
                    let t0 = Instant::now();
                    let mut kb = focus(t0);
                    assert_eq!(kb.handle_key(&key("escape"), t0), KeyOutcome::Consumed);
                    assert_eq!(kb.state(), &PrefixState::Idle);
                }

                #[test]
                fn enter_selects_workspace_and_calls_set_active_workspace() {
                    let t0 = Instant::now();
                    let mut kb = focus(t0);
                    assert_eq!(
                        kb.handle_key(&KeyPress::from_gpui("enter", Some("\n"), Modifiers::NONE), t0),
                        KeyOutcome::Perform(Action::SetActiveWorkspace { workspace_id: "ws2".to_owned() })
                    );
                    assert_eq!(kb.state(), &PrefixState::Idle);
                }

                #[test]
                fn arrow_up_navigates_up() {
                    let t0 = Instant::now();
                    let mut kb = focus(t0);
                    assert_eq!(kb.handle_key(&key("up"), t0), KeyOutcome::Consumed);
                    assert_eq!(focused_index(&kb), 0);
                }

                #[test]
                fn arrow_down_navigates_down() {
                    let t0 = Instant::now();
                    let mut kb = focus(t0);
                    assert_eq!(kb.handle_key(&key("down"), t0), KeyOutcome::Consumed);
                    assert_eq!(focused_index(&kb), 2);
                }

                #[test]
                fn other_keys_pass_through_and_keep_focus() {
                    let t0 = Instant::now();
                    let mut kb = focus(t0);
                    assert_eq!(kb.handle_key(&key("j"), t0), KeyOutcome::PassThrough);
                    assert_eq!(focused_index(&kb), 1);
                    assert_eq!(kb.mode(), PrefixMode::WorkspaceFocus);
                }

                #[test]
                fn enter_without_workspaces_only_deactivates() {
                    let t0 = Instant::now();
                    let mut kb = in_focus(&[], 0, t0);
                    assert_eq!(kb.handle_key(&key("enter"), t0), KeyOutcome::Consumed);
                    assert_eq!(kb.state(), &PrefixState::Idle);
                }
            }

            mod in_idle_mode {
                use super::*;

                #[test]
                fn cmd_1_9_calls_switch_to_tab_with_index() {
                    let mut kb = machine();
                    let cmd_3 = KeyPress::from_gpui("3", None, META);
                    assert_eq!(
                        kb.handle_key(&cmd_3, Instant::now()),
                        KeyOutcome::Perform(Action::SwitchToTab { index: 2 })
                    );
                    assert_eq!(kb.state(), &PrefixState::Idle);
                }

                #[test]
                fn ctrl_1_9_calls_switch_to_tab_with_index() {
                    let mut kb = machine();
                    let ctrl_1 = KeyPress::from_gpui("1", None, CTRL);
                    assert_eq!(
                        kb.handle_key(&ctrl_1, Instant::now()),
                        KeyOutcome::Perform(Action::SwitchToTab { index: 0 })
                    );
                    let ctrl_9 = KeyPress::from_gpui("9", None, CTRL);
                    assert_eq!(
                        kb.handle_key(&ctrl_9, Instant::now()),
                        KeyOutcome::Perform(Action::SwitchToTab { index: 8 })
                    );
                }

                #[test]
                fn other_keys_pass_through() {
                    let t0 = Instant::now();
                    let mut kb = machine();
                    for press in [
                        key("3"),
                        key("escape"),
                        key("enter"),
                        KeyPress::from_gpui("0", None, META),
                        KeyPress::from_gpui("a", None, CTRL),
                        KeyPress::from_gpui("up", None, CTRL),
                        // Shift+Cmd+1 is `!` to GPUI, like `KeyboardEvent.key` in Electron.
                        KeyPress::from_gpui("!", None, META),
                    ] {
                        assert_eq!(kb.handle_key(&press, t0), KeyOutcome::PassThrough, "{press:?}");
                        assert_eq!(kb.state(), &PrefixState::Idle);
                    }
                }
            }

            #[test]
            fn a_changed_prefix_key_applies_to_the_next_press() {
                let t0 = Instant::now();
                let mut kb = machine();
                kb.set_settings(KeybindingSettings {
                    prefix_key: Shortcut::parse("CommandOrControl+A", Os::MacOs).unwrap(),
                    ..KeybindingSettings::default()
                });
                assert_eq!(kb.handle_key(&ctrl_b(), t0), KeyOutcome::PassThrough);
                let cmd_a = KeyPress::from_gpui("a", None, META);
                assert_eq!(kb.handle_key(&cmd_a, t0), KeyOutcome::Consumed);
                assert_eq!(kb.mode(), PrefixMode::Active);
            }
        }

        mod timeout {
            use super::*;

            #[test]
            fn double_prefix_clears_the_timeout() {
                let t0 = Instant::now();
                let mut kb = machine();
                assert_eq!(kb.handle_key(&ctrl_b(), t0), KeyOutcome::Consumed);
                assert_eq!(kb.deadline(), Deadline::At(t0 + TIMEOUT));
                assert_eq!(kb.handle_key(&ctrl_b(), t0 + ms(10)), KeyOutcome::PassThrough);
                assert_eq!(kb.deadline(), Deadline::Never);
            }

            #[test]
            fn prefix_after_timeout_is_consumed_as_a_new_activation() {
                let t0 = Instant::now();
                let mut kb = machine();
                assert_eq!(kb.handle_key(&ctrl_b(), t0), KeyOutcome::Consumed);
                kb.on_timeout(t0 + TIMEOUT);
                assert_eq!(kb.state(), &PrefixState::Idle);
                let t1 = t0 + TIMEOUT + ms(1);
                assert_eq!(kb.handle_key(&ctrl_b(), t1), KeyOutcome::Consumed);
                assert_eq!(kb.state(), &PrefixState::Active { activated_at: t1 });
            }

            #[test]
            fn a_key_after_the_deadline_beats_a_late_timer() {
                let t0 = Instant::now();
                let mut kb = machine();
                assert_eq!(kb.handle_key(&ctrl_b(), t0), KeyOutcome::Consumed);
                // The timer has not fired yet, but prefix mode is over: `c` is just typing
                // and the prefix starts afresh.
                assert_eq!(kb.handle_key(&key("c"), t0 + TIMEOUT), KeyOutcome::PassThrough);
                assert_eq!(kb.state(), &PrefixState::Idle);
                assert_eq!(kb.handle_key(&ctrl_b(), t0 + TIMEOUT), KeyOutcome::Consumed);
                assert_eq!(kb.mode(), PrefixMode::Active);
            }

            #[test]
            fn activate_auto_deactivates_after_timeout() {
                let t0 = Instant::now();
                let mut kb = machine();
                kb.activate(t0);
                kb.on_timeout(t0 + TIMEOUT - ms(1));
                assert_eq!(kb.mode(), PrefixMode::Active);
                kb.on_timeout(t0 + TIMEOUT);
                assert_eq!(kb.state(), &PrefixState::Idle);
                assert_eq!(kb.deadline(), Deadline::Never);
            }

            #[test]
            fn enter_workspace_focus_auto_deactivates_after_timeout() {
                let t0 = Instant::now();
                let mut kb = in_focus(&["ws1"], 0, t0);
                kb.on_timeout(t0 + TIMEOUT);
                assert_eq!(kb.state(), &PrefixState::Idle);
            }

            /// There is no listener or timer to dispose here; the equivalent hazard is an
            /// earlier activation's timer firing during a later activation.
            #[test]
            fn dispose_clears_timeout_so_it_does_not_fire() {
                let t0 = Instant::now();
                let mut kb = machine();
                kb.activate(t0);
                kb.deactivate();
                let t1 = t0 + ms(1000);
                kb.activate(t1);
                kb.on_timeout(t0 + TIMEOUT);
                assert_eq!(kb.state(), &PrefixState::Active { activated_at: t1 });
                assert_eq!(kb.deadline(), Deadline::At(t1 + TIMEOUT));
            }

            #[test]
            fn navigation_does_not_extend_workspace_focus() {
                let t0 = Instant::now();
                let mut kb = in_focus(&["ws1", "ws2"], 0, t0);
                assert_eq!(kb.handle_key(&key("down"), t0 + ms(1000)), KeyOutcome::Consumed);
                assert_eq!(kb.deadline(), Deadline::At(t0 + TIMEOUT));
                assert_eq!(kb.handle_key(&key("enter"), t0 + TIMEOUT), KeyOutcome::PassThrough);
                assert_eq!(kb.state(), &PrefixState::Idle);
            }

            #[test]
            fn remaining_fraction_counts_down_for_the_overlay() {
                let t0 = Instant::now();
                let mut kb = machine();
                assert_eq!(kb.remaining_fraction(t0), 1.0);
                kb.activate(t0);
                assert_eq!(kb.remaining_fraction(t0), 1.0);
                assert!((kb.remaining_fraction(t0 + ms(375)) - 0.75).abs() < 1e-6);
                assert_eq!(kb.remaining_fraction(t0 + TIMEOUT + ms(500)), 0.0);

                kb.set_settings(KeybindingSettings {
                    timeout: Duration::ZERO,
                    ..KeybindingSettings::default()
                });
                assert_eq!(kb.remaining_fraction(t0), 0.0);
            }

            #[test]
            fn a_timeout_too_large_for_the_clock_never_expires() {
                let t0 = Instant::now();
                let mut kb = Keybindings::new(KeybindingSettings {
                    timeout: Duration::MAX,
                    ..KeybindingSettings::default()
                });
                kb.activate(t0);
                assert_eq!(kb.deadline(), Deadline::Never);
                kb.on_timeout(t0 + ms(10_000));
                assert_eq!(kb.mode(), PrefixMode::Active);
            }
        }
    }

    /// Port of `prefixKey.integration.test.ts`: the terminal side (Ctrl-B encoding to 0x02)
    /// lives in `treeterm-term`; what this layer decides is that exactly one of the two
    /// presses reaches it.
    mod prefix_key_integration {
        use super::*;

        #[test]
        fn receives_exactly_one_ctrl_b_for_two_prefix_presses() {
            let t0 = Instant::now();
            let mut kb = machine();
            let outcomes = [kb.handle_key(&ctrl_b(), t0), kb.handle_key(&ctrl_b(), t0 + ms(50))];
            assert_eq!(outcomes, [KeyOutcome::Consumed, KeyOutcome::PassThrough]);
            assert_eq!(kb.state(), &PrefixState::Idle);
        }
    }
}
