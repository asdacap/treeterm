//! The app's keyboard shortcut state (prefix mode, workspace focus) as an entity, so the
//! overlay and the tree can observe it. Logic is `treeterm_core::keybinding`; this owns its
//! one timeout timer.

use std::time::Instant;

use gpui_kit::{Context, Task};
use treeterm_core::keybinding::{
    Deadline, FocusIndexOutOfRange, KeyOutcome, KeyPress, KeybindingSettings, Keybindings,
};

pub struct KeybindingStore {
    machine: Keybindings,
    timer: Option<Task<()>>,
    /// What [`Self::handle_key`] made of the keystroke being dispatched (the last one fed).
    last_outcome: KeyOutcome,
}

impl KeybindingStore {
    pub fn new(settings: KeybindingSettings) -> Self {
        Self {
            machine: Keybindings::new(settings),
            timer: None,
            last_outcome: KeyOutcome::PassThrough,
        }
    }

    /// New prefix key, timeout or action keys (settings changed).
    pub fn set_settings(&mut self, settings: KeybindingSettings, cx: &mut Context<Self>) {
        self.machine.set_settings(settings);
        self.reschedule(cx);
        cx.notify();
    }

    pub fn machine(&self) -> &Keybindings {
        &self.machine
    }

    /// Feed a keystroke the window is about to dispatch.
    pub fn handle_key(&mut self, press: &KeyPress, cx: &mut Context<Self>) -> KeyOutcome {
        let before = self.machine.mode();
        let outcome = self.machine.handle_key(press, Instant::now());
        if self.machine.mode() != before || !matches!(outcome, KeyOutcome::PassThrough) {
            self.reschedule(cx);
            cx.notify();
        }
        self.last_outcome = outcome.clone();
        outcome
    }

    /// Whether the keystroke being dispatched was taken by the shortcuts in a way that
    /// keeps it from other key handlers ([`KeyOutcome::claims_key`]). GPUI runs every
    /// keystroke interceptor even after one stops propagation; the shortcut interceptor
    /// runs first, so later ones (the settings dialog's key recorder) ask this.
    pub fn claimed_last_key(&self) -> bool {
        self.last_outcome.claims_key()
    }

    /// Answer to `PrefixAction::WorkspaceFocus`: the flattened tree and the active index.
    pub fn enter_workspace_focus(
        &mut self,
        workspace_ids: Vec<String>,
        focused_index: usize,
        cx: &mut Context<Self>,
    ) -> Result<(), FocusIndexOutOfRange> {
        self.machine
            .enter_workspace_focus(workspace_ids, focused_index, Instant::now())?;
        self.reschedule(cx);
        cx.notify();
        Ok(())
    }

    fn reschedule(&mut self, cx: &mut Context<Self>) {
        self.timer = match self.machine.deadline() {
            Deadline::Never => None,
            Deadline::At(at) => {
                let wait = at.saturating_duration_since(Instant::now());
                let executor = cx.background_executor().clone();
                Some(cx.spawn(async move |this, cx| {
                    executor.timer(wait).await;
                    match this.update(cx, |store, cx| {
                        store.machine.on_timeout(Instant::now());
                        store.reschedule(cx);
                        cx.notify();
                    }) {
                        Ok(()) | Err(_) => {} // Err: store dropped
                    }
                }))
            }
        };
    }
}
