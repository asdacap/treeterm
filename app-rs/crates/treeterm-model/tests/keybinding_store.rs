//! KeybindingStore: which keystrokes the shortcuts keep from other key handlers.

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use treeterm_core::keybinding::{
    Action, KeyOutcome, KeyPress, KeybindingSettings, Modifiers, PrefixAction, PrefixMode,
};
use treeterm_model::keybinding_store::KeybindingStore;

const CTRL: Modifiers = Modifiers {
    control: true,
    ..Modifiers::NONE
};

fn store(cx: &mut TestAppContext) -> Entity<KeybindingStore> {
    cx.new(|_| KeybindingStore::new(KeybindingSettings::default()))
}

/// Feed `press` and report the outcome and whether the store says it claimed the key.
fn press(
    cx: &mut TestAppContext,
    store: &Entity<KeybindingStore>,
    press: KeyPress,
) -> (KeyOutcome, bool) {
    store.update(cx, |kb, cx| {
        let outcome = kb.handle_key(&press, cx);
        (outcome, kb.claimed_last_key())
    })
}

fn ctrl(key: &str) -> KeyPress {
    KeyPress::from_gpui(key, None, CTRL)
}

fn plain(key: &str) -> KeyPress {
    KeyPress::from_gpui(key, Some(key), Modifiers::NONE)
}

/// Electron's capture-phase prefix handler stops the prefix key from idle and the action
/// keys: the settings dialog's recorder never sees them. It lets the prefix key pressed in
/// prefix mode, Cmd/Ctrl+1–9 and every other key through.
#[gpui_kit::test]
fn claims_the_keys_electrons_prefix_handler_stopped(cx: &mut TestAppContext) {
    let store = store(cx);
    assert!(
        store.read_with(cx, |kb, _| !kb.claimed_last_key()),
        "nothing fed yet"
    );

    assert_eq!(
        press(cx, &store, plain("a")),
        (KeyOutcome::PassThrough, false)
    );
    assert_eq!(
        press(cx, &store, ctrl("1")),
        (KeyOutcome::Perform(Action::SwitchToTab { index: 0 }), false)
    );

    assert_eq!(
        press(cx, &store, ctrl("b")),
        (KeyOutcome::Consumed, true),
        "prefix from idle"
    );
    assert_eq!(
        press(cx, &store, ctrl("b")),
        (KeyOutcome::PassThrough, false),
        "prefix in prefix mode"
    );
    assert_eq!(
        store.read_with(cx, |kb, _| kb.machine().mode()),
        PrefixMode::Idle
    );

    assert_eq!(press(cx, &store, ctrl("b")), (KeyOutcome::Consumed, true));
    assert_eq!(
        press(cx, &store, plain("c")),
        (
            KeyOutcome::Perform(Action::Prefix(PrefixAction::NewTab)),
            true
        ),
        "action key"
    );
    assert_eq!(
        press(cx, &store, plain("c")),
        (KeyOutcome::PassThrough, false),
        "the same key, idle again"
    );

    assert_eq!(press(cx, &store, ctrl("b")), (KeyOutcome::Consumed, true));
    assert_eq!(
        press(cx, &store, plain("escape")),
        (KeyOutcome::Consumed, true),
        "Escape in prefix mode"
    );
}
