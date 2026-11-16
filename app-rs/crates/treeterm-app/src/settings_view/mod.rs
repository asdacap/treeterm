//! The Settings dialog. Port of `SettingsDialog.tsx` with `WebAppSettingsSection.tsx` and
//! `BrowserSettingsSection.tsx`: the sections listed on the left, one shown at a time.
//!
//! Edits are local until "Save", which validates them and writes `settings.json` through
//! the settings store (other windows and the Electron app pick the change up from there).
//! A save writes only what the dialog shows; the rest of the settings stay as they are by
//! then. Parsing and validation live in [`form`] and [`instance`], free of GPUI.

mod form;
mod instance;
mod instance_list;
mod sections;
mod widgets;

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Context, Entity, FocusHandle, SharedString, Subscription, Task, WeakEntity, Window, canvas, div, px,
};
use treeterm_core::keybinding::Os;
use treeterm_core::settings::{
    AiHarnessInstance, BrowserInstance, CustomRunnerInstance, Settings, TerminalInstance, WebAppInstance,
};
use treeterm_model::keybinding_store::KeybindingStore;
use treeterm_model::settings_store::SettingsStore;

use self::form::{Lists, SandboxSupport, Section, Texts};
use self::instance_list::InstanceList;
use crate::theme::colors;
use crate::ui;

/// Save's write to `settings.json`.
enum Saving {
    Idle,
    /// Waits for the write, which runs on in the settings store if the dialog goes away.
    Waiting { _wait: Task<()> },
}

/// A keybinding button waiting for its key (Electron's `RecordingState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Recording {
    Prefix,
    /// The action's index in `PrefixAction::ALL`.
    Action(usize),
}

/// `.settings-dialog-header` (62px) and `.settings-dialog-actions` (64.5px) around the content.
const CHROME_HEIGHT: f32 = 126.5;
/// The tab list's height (8px padding, 13 tabs of 35.5px): the content is never shorter.
const TABS_HEIGHT: f32 = 477.5;

pub struct SettingsView {
    store: Entity<SettingsStore>,
    /// The shortcuts, which take some keys before the recorder sees them.
    keybindings: Entity<KeybindingStore>,
    /// The section shown, shared with the window's [`SettingsDialog`] so that it outlives
    /// the view (Electron's `activeTab`).
    section: Rc<Cell<Section>>,
    /// The toggles and choices as edited (Electron's `localSettings`). Its instance lists are
    /// empty: they live in the lists below, and the free text in `inputs`; both are read on
    /// save.
    draft: Settings,
    sandbox: SandboxSupport,
    inputs: Inputs,
    profiles: InstanceList<TerminalInstance>,
    harnesses: InstanceList<AiHarnessInstance>,
    runners: InstanceList<CustomRunnerInstance>,
    web_apps: InstanceList<WebAppInstance>,
    browsers: InstanceList<BrowserInstance>,
    error: Option<SharedString>,
    saving: Saving,
    recording: Option<Recording>,
    /// Takes the keystrokes while recording.
    intercept: Option<Subscription>,
    /// Focused while recording, so no input takes the keys too.
    focus: FocusHandle,
    /// The Classifier Provider select's, which its `<label>` focuses.
    provider_focus: FocusHandle,
    /// The shown section's natural height, measured after layout: the dialog grows with it
    /// up to 80% of the window, as Electron's does.
    body_height: f32,
    scroll: gpui_kit::ScrollHandle,
}

/// The single-value text fields; see [`Texts`] for what each holds.
struct Inputs {
    font_size: Entity<InputState>,
    max_cols: Entity<InputState>,
    font_family: Entity<InputState>,
    llm_base_url: Entity<InputState>,
    llm_api_key: Entity<InputState>,
    llm_model: Entity<InputState>,
    analyzer_model: Entity<InputState>,
    analyzer_system_prompt: Entity<TextareaState>,
    criteria: [Entity<TextareaState>; 7],
    title_model: Entity<InputState>,
    title_system_prompt: Entity<TextareaState>,
    safe_paths: Entity<InputState>,
    buffer_lines: Entity<InputState>,
    idle_debounce: Entity<InputState>,
    idle_debounce_unread: Entity<InputState>,
    /// Filled by recording (the buttons show it); never shown as a text field.
    prefix_key: Entity<InputState>,
    prefix_timeout: Entity<InputState>,
    /// As `prefix_key`.
    action_keys: [Entity<InputState>; 6],
    github_pat: Entity<InputState>,
    /// Re-render on typing a criterion, for its "Reset to default".
    _criteria_changes: Vec<Subscription>,
    /// Writes the stored timeout back into its field ([`form::prefix_timeout_field`]).
    _prefix_timeout_changes: Subscription,
}

impl Inputs {
    fn new(texts: Texts, window: &mut Window, cx: &mut Context<SettingsView>) -> Self {
        let mut line = |text: String, placeholder: &'static str, masked: bool| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).masked(masked).default_value(text))
        };
        let font_size = line(texts.font_size, "", false);
        let max_cols = line(texts.max_cols, "", false);
        let font_family = line(texts.font_family, "", false);
        let llm_base_url = line(texts.llm_base_url, "https://openrouter.ai/api/v1", false);
        let llm_api_key = line(texts.llm_api_key, "sk-...", true);
        let llm_model = line(texts.llm_model, "gpt-4o", false);
        let analyzer_model = line(texts.analyzer_model, "openai/gpt-oss-safeguard-20b", false);
        let title_model = line(texts.title_model, "", false);
        let safe_paths = line(texts.safe_paths, "/tmp", false);
        let buffer_lines = line(texts.buffer_lines, "", false);
        let idle_debounce = line(texts.idle_debounce, "", false);
        let idle_debounce_unread = line(texts.idle_debounce_unread, "", false);
        let prefix_key = line(texts.prefix_key, "", false);
        let prefix_timeout = line(texts.prefix_timeout, "", false);
        let action_keys = texts.action_keys.map(|key| line(key, "", false));
        let github_pat = line(texts.github_pat, "ghp_...", true);
        // Shown `rows` lines tall, and resizable (`render_llm`).
        let mut area = |text: String| cx.new(|cx| TextareaState::new(window, cx).default_value(text));
        let analyzer_system_prompt = area(texts.analyzer_system_prompt);
        let criteria = texts.criteria.map(&mut area);
        let title_system_prompt = area(texts.title_system_prompt);
        let _criteria_changes = criteria
            .iter()
            .map(|state| {
                cx.subscribe(state, |_, _, event: &InputEvent, cx| match event {
                    InputEvent::Change => cx.notify(),
                    InputEvent::PressEnter { .. } | InputEvent::Focus | InputEvent::Blur => {}
                })
            })
            .collect();
        let _prefix_timeout_changes = cx.subscribe_in(&prefix_timeout, window, |_, state, event: &InputEvent, window, cx| {
            match event {
                // The field is React-controlled: what it holds is written back at once.
                InputEvent::Change => {
                    if let Some(stored) = form::prefix_timeout_field(&state.read(cx).value()) {
                        state.update(cx, |input, cx| input.set_value(stored, window, cx));
                    }
                }
                InputEvent::PressEnter { .. } | InputEvent::Focus | InputEvent::Blur => {}
            }
        });
        Self {
            font_size,
            max_cols,
            font_family,
            llm_base_url,
            llm_api_key,
            llm_model,
            analyzer_model,
            analyzer_system_prompt,
            criteria,
            title_model,
            title_system_prompt,
            safe_paths,
            buffer_lines,
            idle_debounce,
            idle_debounce_unread,
            prefix_key,
            prefix_timeout,
            action_keys,
            github_pat,
            _criteria_changes,
            _prefix_timeout_changes,
        }
    }

    fn read(&self, cx: &App) -> Texts {
        let line = |state: &Entity<InputState>| state.read(cx).value().to_string();
        let area = |state: &Entity<TextareaState>| state.read(cx).value().to_string();
        Texts {
            font_size: line(&self.font_size),
            max_cols: line(&self.max_cols),
            font_family: line(&self.font_family),
            llm_base_url: line(&self.llm_base_url),
            llm_api_key: line(&self.llm_api_key),
            llm_model: line(&self.llm_model),
            analyzer_model: line(&self.analyzer_model),
            analyzer_system_prompt: area(&self.analyzer_system_prompt),
            criteria: self.criteria.each_ref().map(area),
            title_model: line(&self.title_model),
            title_system_prompt: area(&self.title_system_prompt),
            safe_paths: line(&self.safe_paths),
            buffer_lines: line(&self.buffer_lines),
            idle_debounce: line(&self.idle_debounce),
            idle_debounce_unread: line(&self.idle_debounce_unread),
            prefix_key: line(&self.prefix_key),
            prefix_timeout: line(&self.prefix_timeout),
            action_keys: self.action_keys.each_ref().map(line),
            github_pat: line(&self.github_pat),
        }
    }
}

impl SettingsView {
    pub fn new(
        store: Entity<SettingsStore>,
        keybindings: Entity<KeybindingStore>,
        section: Rc<Cell<Section>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut draft = (**store.read(cx).settings()).clone();
        let inputs = Inputs::new(Texts::from_settings(&draft), window, cx);
        let profiles = InstanceList::new(std::mem::take(&mut draft.terminal.instances), window, cx);
        let harnesses = InstanceList::new(std::mem::take(&mut draft.ai_harness.instances), window, cx);
        let runners = InstanceList::new(std::mem::take(&mut draft.custom_runner.instances), window, cx);
        let web_apps = InstanceList::new(std::mem::take(&mut draft.web_app.instances), window, cx);
        let browsers = InstanceList::new(std::mem::take(&mut draft.browser.instances), window, cx);
        Self {
            store,
            keybindings,
            section,
            draft,
            sandbox: SandboxSupport::detect(Os::host(), std::env::var_os("PATH").as_deref()),
            inputs,
            profiles,
            harnesses,
            runners,
            web_apps,
            browsers,
            error: None,
            saving: Saving::Idle,
            recording: None,
            intercept: None,
            focus: cx.focus_handle(),
            provider_focus: cx.focus_handle(),
            body_height: 0.,
            scroll: gpui_kit::ScrollHandle::new(),
        }
    }

    /// The instance lists with what is typed now.
    fn lists(&self, cx: &App) -> Lists {
        Lists {
            profiles: self.profiles.current(cx),
            harnesses: self.harnesses.current(cx),
            runners: self.runners.current(cx),
            web_apps: self.web_apps.current(cx),
            browsers: self.browsers.current(cx),
        }
    }

    /// `handleSave`: validate, save, and close once written. An invalid form shows its error
    /// and opens its section; a failed write shows why and keeps the dialog open, with the
    /// settings unchanged, so the next Save writes again. Ignored while a save is running.
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Saving::Waiting { .. } = self.saving {
            return;
        }
        let texts = self.inputs.read(cx);
        let lists = self.lists(cx);
        let edited = match form::build(&self.draft, &texts, lists) {
            Ok(edited) => edited,
            Err(error) => {
                self.section.set(error.section);
                self.error = Some(error.message.into());
                cx.notify();
                return;
            }
        };
        let saved = self.store.update(cx, |store, cx| store.save(move |current| form::apply_owned(edited, current), cx));
        let wait = cx.spawn_in(window, async move |view, cx| {
            let outcome = saved.await.unwrap_or_else(|_| Err("settings store closed".to_owned()));
            match view.update_in(cx, |view, window, cx| {
                view.saving = Saving::Idle;
                match outcome {
                    Ok(()) => window.close_dialog(cx),
                    Err(error) => view.error = Some(format!("Could not save settings: {error}").into()),
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: the dialog closed
            }
        });
        self.saving = Saving::Waiting { _wait: wait };
    }

    /// The error shown above the buttons.
    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    /// Start (or, clicked again, stop) recording a keybinding button's key. While recording,
    /// every keystroke goes to the recorder first (before key bindings such as Tab's focus
    /// move), as Electron's `preventDefault` + `stopPropagation` do.
    /// Only the keys the shortcuts claim come before it: Electron's prefix handler runs in
    /// the capture phase, so recording the current prefix key takes a second press.
    fn toggle_recording(&mut self, target: Recording, window: &mut Window, cx: &mut Context<Self>) {
        self.recording = if self.recording == Some(target) { None } else { Some(target) };
        self.intercept = self.recording.map(|_| {
            let (view, keybindings) = (cx.entity().downgrade(), self.keybindings.clone());
            cx.intercept_keystrokes(move |event, window, cx| {
                // GPUI runs every interceptor, the shortcuts' (registered at start-up) first.
                if event.keystroke.is_modifier_key() || keybindings.read(cx).claimed_last_key() {
                    return;
                }
                let keystroke = event.keystroke.clone();
                match view.update(cx, |view, cx| view.record_key(&keystroke, window, cx)) {
                    Ok(true) => cx.stop_propagation(),
                    Ok(false) => {}
                    Err(error) => tracing::debug!(%error, "settings closed"),
                }
            })
        });
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// `handleKeyDown`: a key pressed while recording becomes the binding. Returns whether
    /// it was taken.
    fn record_key(&mut self, keystroke: &gpui_kit::Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(target) = self.recording.take() else {
            return false;
        };
        let key = form::dom_key(&keystroke.key, keystroke.key_char.as_deref());
        let (state, value) = match target {
            Recording::Prefix => {
                let m = keystroke.modifiers;
                (&self.inputs.prefix_key, form::recorded_prefix_key(&key, m.platform || m.control, m.alt, m.shift))
            }
            Recording::Action(index) => (&self.inputs.action_keys[index], form::recorded_action_key(&key)),
        };
        state.update(cx, |input, cx| input.set_value(value, window, cx));
        self.intercept = None;
        cx.notify();
        true
    }

    /// `.settings-tabs`: 140px, 8px vertical padding, a right border; `.settings-tab` buttons
    /// of 13px text with 10px 16px padding, the active one bg-active.
    fn sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let c = colors(cx);
        let (active_bg, hover_bg, primary, secondary) = (c.bg_active, c.bg_hover, c.text_primary, c.text_secondary);
        let mut list = div()
            .flex()
            .flex_col()
            .w(px(140.))
            .h_full()
            .flex_none()
            .py(px(8.))
            .border_r_1()
            .border_color(c.border);
        for section in Section::ALL {
            let active = section == self.section.get();
            list = list.child(
                ui::key_button(section.label(), 0., window, cx)
                    .justify_start()
                    .px(px(16.))
                    .py(px(10.))
                    .text_size(px(ui::BODY))
                    .line_height(px(15.5))
                    .cursor_pointer()
                    .when(active, |item| item.bg(active_bg).text_color(primary))
                    .when(!active, |item| item.text_color(secondary).hover(move |s| s.bg(hover_bg).text_color(primary)))
                    .child(section.label())
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.section.set(section);
                        cx.notify();
                    })),
            );
        }
        list
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.section.get() {
            Section::General => self.render_general(window, cx),
            Section::Terminal => self.render_terminal(window, cx),
            Section::TerminalProfiles => {
                instance_list::render(&self.profiles, |v| &mut v.profiles, self.sandbox, window, cx)
            }
            Section::AiHarness => instance_list::render(&self.harnesses, |v| &mut v.harnesses, self.sandbox, window, cx),
            Section::CustomRunners => instance_list::render(&self.runners, |v| &mut v.runners, self.sandbox, window, cx),
            Section::WebApps => instance_list::render(&self.web_apps, |v| &mut v.web_apps, self.sandbox, window, cx),
            Section::Browsers => instance_list::render(&self.browsers, |v| &mut v.browsers, self.sandbox, window, cx),
            Section::Llm => self.render_llm(window, cx),
            Section::Sandbox => self.render_sandbox(window, cx),
            Section::Appearance => self.render_appearance(window, cx),
            Section::Keybindings => self.render_keybindings(window, cx),
            Section::Github => self.render_github(window, cx),
            Section::Debug => self.render_debug(window, cx),
        };
        // `.settings-dialog`: max-height 80vh, the content (flex: 1) at least as tall as the
        // tab list; past that the panel scrolls.
        let max_content = f32::from(window.viewport_size().height) * 0.8 - CHROME_HEIGHT;
        let natural = self.body_height + 32.;
        let content_height = natural.max(TABS_HEIGHT).min(max_content);
        let overflows = natural > content_height + 0.5;
        let measured = cx.entity().downgrade();
        let measure = canvas(
            move |bounds, _, cx| {
                let height = f32::from(bounds.size.height);
                if let Err(error) = measured.update(cx, |view, cx| {
                    if (view.body_height - height).abs() > 0.5 {
                        view.body_height = height;
                        cx.notify();
                    }
                }) {
                    tracing::debug!(%error, "settings closed");
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let error = colors(cx).error;
        let header = ui::dialog_key_header("Settings", "×", colors(cx).text_primary, window, cx);
        let sidebar = self.sidebar(window, cx);
        let cancel = ui::dialog_key_button("cancel-settings", "Cancel", ui::ButtonKind::Secondary, false, window, cx)
            .on_click(|_, window, cx| window.close_dialog(cx));
        let save = ui::dialog_key_button("save-settings", "Save", ui::ButtonKind::Primary, false, window, cx)
            .on_click(cx.listener(|view, _, window, cx| view.save(window, cx)));
        ui::dialog_root(cx)
            .track_focus(&self.focus)
            .child(header)
            .child(
                div().flex().w_full().h(px(content_height)).overflow_hidden().child(sidebar).child(
                    div()
                        .relative()
                        .flex_1()
                        // Without a zero minimum, wide inputs and long help lines size the
                        // panel past the dialog's edge.
                        .min_w_0()
                        .h_full()
                        .child(
                            div()
                                .id("settings-panel")
                                .size_full()
                                .overflow_y_scroll()
                                .track_scroll(&self.scroll)
                                .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
                                .pl(px(20.))
                                // The scrollbar's 10px gutter when the section overflows.
                                .pr(px(if overflows { 30. } else { 20. }))
                                .py(px(16.))
                                .child(div().relative().child(body).child(measure)),
                        )
                        .when(overflows, |panel| {
                            panel.child(ui::scrollbar_thumb(&self.scroll, content_height, natural, cx))
                        }),
                ),
            )
            .child(
                ui::dialog_actions(cx)
                    // Not in Electron (it does not validate): why Save did not close.
                    .when_some(self.error.clone(), |d, message| {
                        d.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .self_center()
                                .text_size(px(ui::BODY))
                                .text_color(error)
                                .child(message),
                        )
                    })
                    .child(cancel)
                    .child(save),
            )
    }
}

/// A window's Settings dialog. Electron's stays mounted for the window's life: asked for
/// while open it stays as it is (`isSettingsOpen`), and it reopens on the section shown last
/// (`activeTab`), its edits read afresh.
pub struct SettingsDialog {
    /// The dialog while it is open. Only the dialog keeps the view, so this dies on close.
    open: WeakEntity<SettingsView>,
    /// The section shown, kept while the dialog is closed.
    section: Rc<Cell<Section>>,
}

impl Default for SettingsDialog {
    fn default() -> Self {
        // Electron's first opens on Terminal.
        Self { open: WeakEntity::new_invalid(), section: Rc::new(Cell::new(Section::Terminal)) }
    }
}

impl SettingsDialog {
    pub fn is_open(&self) -> bool {
        self.open.upgrade().is_some()
    }

    /// Open the dialog in `window`, unless it is open: then its edits stay as they are.
    pub fn open(
        &mut self,
        store: Entity<SettingsStore>,
        keybindings: Entity<KeybindingStore>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.is_open() {
            return;
        }
        let section = self.section.clone();
        let view = cx.new(|cx| SettingsView::new(store, keybindings, section, window, cx));
        self.open = view.downgrade();
        window.open_dialog(cx, move |dialog, _window, cx| {
            // Electron's has no Escape handler: Escape would close it and drop the edits
            // unasked. The backdrop, × and Cancel close it.
            ui::dialog_frame(dialog, 600., cx).keyboard(false).child(view.clone())
        });
    }

    /// The dialog's view while it is open.
    #[cfg(test)]
    pub(crate) fn view(&self) -> Option<Entity<SettingsView>> {
        self.open.upgrade()
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::base::test_support;
    use gpui_kit::{ElementId, Focusable, Role, TestAppContext, VisualTestContext, WindowOptions};
    use treeterm_core::keybinding::{KeyOutcome, KeybindingSettings, PrefixMode};
    use treeterm_core::settings::{ClassifiedState, CursorStyle};

    use super::*;

    struct Empty;

    impl Render for Empty {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// The dialog over a window whose keystrokes pass the shortcuts first, as `main` sets up.
    fn dialog(cx: &mut TestAppContext) -> (&mut VisualTestContext, Entity<KeybindingStore>, Entity<SettingsView>) {
        dialog_in(WindowOptions::default(), cx)
    }

    /// [`dialog`] in a window opened with `options`, with the app's key bindings.
    fn dialog_in(
        options: WindowOptions,
        cx: &mut TestAppContext,
    ) -> (&mut VisualTestContext, Entity<KeybindingStore>, Entity<SettingsView>) {
        let (cx, store, keybindings) = settings_window(options, cx);
        let mut dialog = SettingsDialog::default();
        let view = open(&mut dialog, &store, &keybindings, cx);
        (cx, keybindings, view)
    }

    /// A window opened with `options` whose keystrokes pass the shortcuts first, as `main`
    /// sets up, with the app's key bindings, and the settings for its dialog.
    fn settings_window(
        options: WindowOptions,
        cx: &mut TestAppContext,
    ) -> (&mut VisualTestContext, Entity<SettingsStore>, Entity<KeybindingStore>) {
        let window = cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::install(cx);
            ui::bind_keys(cx);
            gpui_kit::open_window(options, cx, |_, cx| cx.new(|_| Empty)).expect("open a window").0
        });
        let cx = VisualTestContext::from_window(window, cx).into_mut();
        let keybindings = cx.update(|_, cx| cx.new(|_| KeybindingStore::new(KeybindingSettings::default())));
        let shortcuts = keybindings.clone();
        cx.update(|_, cx| {
            cx.intercept_keystrokes(move |event, _, cx| {
                let press = crate::key_press(&event.keystroke);
                if !matches!(shortcuts.update(cx, |kb, cx| kb.handle_key(&press, cx)), KeyOutcome::PassThrough) {
                    cx.stop_propagation();
                }
            })
            .detach();
        });
        let store = cx.update(|_, cx| cx.new(|_| SettingsStore::in_memory(Default::default())));
        (cx, store, keybindings)
    }

    /// Ask `dialog` to open, and the view it then shows.
    fn open(
        dialog: &mut SettingsDialog,
        store: &Entity<SettingsStore>,
        keybindings: &Entity<KeybindingStore>,
        cx: &mut VisualTestContext,
    ) -> Entity<SettingsView> {
        cx.update(|window, cx| dialog.open(store.clone(), keybindings.clone(), window, cx));
        cx.run_until_parked();
        dialog.view().expect("Settings is open")
    }

    fn texts(cx: &mut VisualTestContext, view: &Entity<SettingsView>) -> Texts {
        cx.update(|_, cx| view.read(cx).inputs.read(cx))
    }

    fn recording(cx: &mut VisualTestContext, view: &Entity<SettingsView>) -> Option<Recording> {
        cx.update(|_, cx| view.read(cx).recording)
    }

    fn mode(cx: &mut VisualTestContext, keybindings: &Entity<KeybindingStore>) -> PrefixMode {
        cx.update(|_, cx| keybindings.read(cx).machine().mode())
    }

    /// Electron's prefix handler takes the prefix key from idle in the capture phase, so the
    /// recorder sees the current prefix key only when it is pressed again in prefix mode.
    #[gpui_kit::test]
    fn recording_the_current_prefix_key_takes_a_second_press(cx: &mut TestAppContext) {
        let (cx, keybindings, view) = dialog(cx);
        cx.update(|window, cx| view.update(cx, |view, cx| view.toggle_recording(Recording::Prefix, window, cx)));

        cx.simulate_keystrokes("ctrl-b");
        assert_eq!(mode(cx, &keybindings), PrefixMode::Active);
        assert_eq!(recording(cx, &view), Some(Recording::Prefix), "still recording");
        assert_eq!(texts(cx, &view).prefix_key, "Control+B");

        cx.simulate_keystrokes("ctrl-b");
        assert_eq!(mode(cx, &keybindings), PrefixMode::Idle);
        assert_eq!(recording(cx, &view), None);
        assert_eq!(texts(cx, &view).prefix_key, "CommandOrControl+B");
    }

    /// `<input type="number" min=500 max=5000 step=100>` under React: ArrowUp/ArrowDown step
    /// it within its bounds, and whatever is typed is replaced by the timeout it holds.
    #[gpui_kit::test]
    fn the_prefix_timeout_steps_and_shows_what_it_holds(cx: &mut TestAppContext) {
        let (cx, _, view) = dialog(cx);
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.section.set(Section::Keybindings);
                cx.notify();
            })
        });
        cx.run_until_parked();
        let field = cx.update(|_, cx| view.read(cx).inputs.prefix_timeout.clone());
        cx.update(|window, cx| field.update(cx, |input, cx| input.focus(window, cx)));
        let text = |cx: &mut VisualTestContext| cx.update(|_, cx| field.read(cx).value().to_string());

        cx.simulate_keystrokes("up");
        assert_eq!(text(cx), "1600");
        cx.simulate_keystrokes("down down");
        assert_eq!(text(cx), "1400");

        // Emptied, it shows the 1500 it then holds; a fraction is cut to whole milliseconds.
        cx.update(|window, cx| field.update(cx, |input, cx| input.replace_all("", window, cx)));
        cx.run_until_parked();
        assert_eq!(text(cx), "1500");
        cx.update(|window, cx| field.update(cx, |input, cx| input.replace_all("1500.7", window, cx)));
        cx.run_until_parked();
        assert_eq!(text(cx), "1500");
        // Typed values are not clamped; stepping brings them back in range.
        cx.update(|window, cx| field.update(cx, |input, cx| input.replace_all("100", window, cx)));
        cx.run_until_parked();
        assert_eq!(text(cx), "100");
        cx.simulate_keystrokes("down");
        assert_eq!(text(cx), "100");
        cx.simulate_keystrokes("up");
        assert_eq!(text(cx), "500");
    }

    /// What `field` holds once it is focused holding `text` and `keys` are pressed.
    fn stepped(cx: &mut VisualTestContext, field: &Entity<InputState>, text: &str, keys: &str) -> String {
        let text = text.to_owned();
        cx.update(|window, cx| {
            field.update(cx, |input, cx| {
                input.set_value(text, window, cx);
                input.focus(window, cx);
            })
        });
        cx.simulate_keystrokes(keys);
        cx.update(|_, cx| field.read(cx).value().to_string())
    }

    /// Font Size, Max Columns, Buffer Lines and both Idle Debounces are Electron's
    /// `<input type="number">`s: ArrowUp/ArrowDown step each by its own step within its own
    /// bounds, and letters cannot be typed.
    #[gpui_kit::test]
    fn the_number_fields_step_within_their_bounds(cx: &mut TestAppContext) {
        let (cx, _, view) = dialog(cx);
        let [font_size, max_cols, buffer_lines, idle_debounce, idle_debounce_unread] = cx.update(|_, cx| {
            let inputs = &view.read(cx).inputs;
            [
                &inputs.font_size,
                &inputs.max_cols,
                &inputs.buffer_lines,
                &inputs.idle_debounce,
                &inputs.idle_debounce_unread,
            ]
            .map(Entity::clone)
        });

        assert_eq!(stepped(cx, &font_size, "14", "up up up"), "17");
        assert_eq!(stepped(cx, &font_size, "32", "up"), "32");
        assert_eq!(stepped(cx, &font_size, "8", "down"), "8");
        assert_eq!(stepped(cx, &max_cols, "40", "down"), "40");
        assert_eq!(stepped(cx, &max_cols, "2000", "down"), "1000");
        cx.simulate_input("a");
        assert_eq!(texts(cx, &view).max_cols, "1000", "letters are not typed");

        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.section.set(Section::Llm);
                cx.notify();
            })
        });
        cx.run_until_parked();
        assert_eq!(stepped(cx, &buffer_lines, "100", "up"), "100");
        assert_eq!(stepped(cx, &buffer_lines, "1", "down"), "1");
        assert_eq!(stepped(cx, &buffer_lines, "30", "down"), "29");
        assert_eq!(stepped(cx, &idle_debounce, "2050", "up"), "2100");
        assert_eq!(stepped(cx, &idle_debounce, "2000", "down down"), "1800");
        assert_eq!(stepped(cx, &idle_debounce_unread, "0", "down"), "0");
        assert_eq!(stepped(cx, &idle_debounce_unread, "15000", "up"), "15100");
    }

    /// An action key in prefix mode runs its action, not the recorder; Cmd/Ctrl+1–9 (only
    /// `preventDefault`ed) reach both.
    #[gpui_kit::test]
    fn action_keys_in_prefix_mode_are_not_recorded(cx: &mut TestAppContext) {
        let (cx, keybindings, view) = dialog(cx);
        cx.update(|window, cx| view.update(cx, |view, cx| view.toggle_recording(Recording::Action(1), window, cx)));

        cx.simulate_keystrokes("ctrl-b c");
        assert_eq!(mode(cx, &keybindings), PrefixMode::Idle);
        assert_eq!(recording(cx, &view), Some(Recording::Action(1)));
        assert_eq!(texts(cx, &view).action_keys[1], "x");

        cx.simulate_keystrokes("ctrl-1");
        assert_eq!(recording(cx, &view), None);
        assert_eq!(texts(cx, &view).action_keys[1], "1");
    }

    fn show(section: Section, cx: &mut VisualTestContext, view: &Entity<SettingsView>) {
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.section.set(section);
                cx.notify();
            })
        });
        cx.run_until_parked();
    }

    /// The control holding the focus, as GPUI Kit's test observation sees it after a frame: a
    /// button or checkbox by its element id, a text field as "text field", and "" when none of
    /// those has it (a select box is not observed).
    fn focused_control(cx: &mut VisualTestContext) -> String {
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            let mut focused = test_support::snapshots(window).into_iter().filter_map(|control| {
                let name = match control.role()? {
                    Role::Button | Role::CheckBox => control.path().last()?.to_string(),
                    Role::TextInput | Role::MultilineTextInput | Role::PasswordInput => "text field".to_owned(),
                    _ => return None,
                };
                (control.focused() == Some(true)).then_some(name)
            });
            let control = focused.next().unwrap_or_default();
            assert_eq!(focused.next(), None, "one control has the focus");
            control
        })
    }

    /// Press and release `key`: a focused button or checkbox acts on the release.
    fn press(key: &str, cx: &mut VisualTestContext) {
        let keystroke = cx.update(|_, cx| gpui_kit::Keystroke::parse_with_os(key, cx.platform_info().os).expect("a key"));
        let down = gpui_kit::KeyDownEvent { keystroke: keystroke.clone(), is_held: false, prefer_character_input: false };
        cx.simulate_event(down);
        cx.simulate_event(gpui_kit::KeyUpEvent { keystroke });
    }

    /// Press Tab until the control `id` has the focus.
    fn tab_to(id: &str, cx: &mut VisualTestContext) {
        for _ in 0..80 {
            cx.simulate_keystrokes("tab");
            if focused_control(cx) == id {
                return;
            }
        }
        panic!("Tab never reached {id}");
    }

    fn focus_field<S: Focusable + 'static>(field: &Entity<S>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
        cx.run_until_parked();
    }

    fn is_focused<S: Focusable + 'static>(field: &Entity<S>, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| field.focus_handle(cx).is_focused(window))
    }

    /// Whether a select's menu is open.
    fn menu_open(cx: &mut VisualTestContext) -> bool {
        cx.update(|_, cx| gpui_kit::base::GlobalState::is_in_deferred_context(cx))
    }

    /// Tab and Shift+Tab move through the controls in document order, as Electron's native
    /// controls do: the close button, the section tabs, the section's fields, Cancel and Save,
    /// and round again.
    #[gpui_kit::test]
    fn tab_moves_through_the_controls_in_order(cx: &mut TestAppContext) {
        let (cx, _, _view) = dialog(cx);
        let mut expected = vec!["dialog-close"];
        expected.extend(Section::ALL.map(Section::label));
        // Font Size, Max Columns, Font Family, the Cursor Style select, the checkboxes.
        expected.extend(["text field", "text field", "text field", "", "cursor-blink", "osc52", "show-raw-chars"]);
        expected.extend(["cancel-settings", "save-settings", "dialog-close"]);
        let mut reached = Vec::new();
        for _ in &expected {
            cx.simulate_keystrokes("tab");
            reached.push(focused_control(cx));
        }
        assert_eq!(reached, expected);
        cx.simulate_keystrokes("shift-tab");
        assert_eq!(focused_control(cx), "save-settings");
    }

    /// Disabled controls are not tab stops: the sandbox checkboxes without bubblewrap, and a
    /// criterion's "Reset to default" while the criterion is the default.
    #[gpui_kit::test]
    fn tab_skips_disabled_controls(cx: &mut TestAppContext) {
        let (cx, _, view) = dialog(cx);
        cx.update(|_, cx| view.update(cx, |view, _| view.sandbox = SandboxSupport::MissingBwrap));
        show(Section::Sandbox, cx, &view);
        tab_to("Debug", cx);
        cx.simulate_keystrokes("tab");
        assert_eq!(focused_control(cx), "cancel-settings");

        show(Section::Llm, cx, &view);
        let [working, next] = cx.update(|_, cx| [0, 1].map(|i| view.read(cx).inputs.criteria[i].clone()));
        focus_field(&working, cx);
        cx.simulate_keystrokes("tab");
        assert!(is_focused(&next, cx), "the default criterion's reset is skipped");
        focus_field(&working, cx);
        cx.simulate_input("!");
        cx.simulate_keystrokes("tab");
        assert_eq!(focused_control(cx), "reset-criterion-0");
    }

    /// Space toggles a focused checkbox; Enter does not, as with a native one.
    #[gpui_kit::test]
    fn space_toggles_a_focused_checkbox_and_enter_does_not(cx: &mut TestAppContext) {
        let (cx, _, view) = dialog(cx);
        let blink = |cx: &mut VisualTestContext| cx.update(|_, cx| view.read(cx).draft.terminal.cursor_blink);
        let before = blink(cx);
        tab_to("cursor-blink", cx);
        press("space", cx);
        assert_eq!(blink(cx), !before);
        press("enter", cx);
        assert_eq!(blink(cx), !before);
        assert_eq!(focused_control(cx), "cursor-blink");
    }

    /// Space or Enter presses a focused button: a section tab, "+ Add", Delete and the close
    /// button.
    #[gpui_kit::test]
    fn space_or_enter_presses_a_focused_button(cx: &mut TestAppContext) {
        let (cx, _, view) = dialog(cx);
        let section = |cx: &mut VisualTestContext| cx.update(|_, cx| view.read(cx).section.get());
        tab_to("LLM", cx);
        press("space", cx);
        assert_eq!(section(cx), Section::Llm);
        tab_to("Terminal Profiles", cx);
        press("enter", cx);
        assert_eq!(section(cx), Section::TerminalProfiles);

        let profiles = |cx: &mut VisualTestContext| cx.update(|_, cx| view.read(cx).profiles.current(cx).len());
        let before = profiles(cx);
        tab_to("term-add", cx);
        press("space", cx);
        press("enter", cx);
        assert_eq!(profiles(cx), before + 2);
        tab_to("term-0-delete", cx);
        press("space", cx);
        assert_eq!(profiles(cx), before + 1);

        tab_to("dialog-close", cx);
        press("enter", cx);
        cx.run_until_parked();
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)), "the dialog closed");
    }

    /// A focused select opens its menu on Space, ArrowDown or ArrowUp (as on macOS), not on
    /// Enter. In the menu the arrows and Enter pick an option; Escape closes it. Either way the
    /// select keeps the focus.
    #[gpui_kit::test]
    fn a_focused_select_opens_its_menu_from_the_keyboard(cx: &mut TestAppContext) {
        let (cx, _, view) = dialog(cx);
        let font_family = cx.update(|_, cx| view.read(cx).inputs.font_family.clone());
        focus_field(&font_family, cx);
        cx.simulate_keystrokes("tab");
        assert_eq!(focused_control(cx), "", "the Cursor Style select");

        press("enter", cx);
        assert!(!menu_open(cx));
        for key in ["space", "up", "down"] {
            press(key, cx);
            assert!(menu_open(cx), "{key} opens the menu");
            cx.simulate_keystrokes("escape");
            assert!(!menu_open(cx), "Escape closes it");
        }
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)), "and not the dialog");

        cx.simulate_keystrokes("space down down enter");
        assert!(!menu_open(cx));
        assert_eq!(cx.update(|_, cx| view.read(cx).draft.terminal.cursor_style), CursorStyle::Underline);
        cx.simulate_keystrokes("tab");
        assert_eq!(focused_control(cx), "cursor-blink", "the select had the focus back");
    }

    /// Tab in a textarea moves the focus on, and Shift+Tab back, without typing anything, as in
    /// Chromium.
    #[gpui_kit::test]
    fn tab_in_a_textarea_moves_the_focus(cx: &mut TestAppContext) {
        let (cx, _, view) = dialog(cx);
        show(Section::Llm, cx, &view);
        let prompt = cx.update(|_, cx| view.read(cx).inputs.analyzer_system_prompt.clone());
        let text = |cx: &mut VisualTestContext| cx.update(|_, cx| prompt.read(cx).value().to_string());
        let before = text(cx);
        focus_field(&prompt, cx);
        cx.simulate_keystrokes("tab");
        assert_eq!(focused_control(cx), "reset-system-prompt");
        cx.simulate_keystrokes("shift-tab");
        assert!(is_focused(&prompt, cx));
        cx.simulate_keystrokes("shift-tab");
        assert!(!is_focused(&prompt, cx));
        assert_eq!(text(cx), before);
    }

    /// A click on a `<label htmlFor>` focuses its control: "Classifier Provider" its select
    /// (without opening it), a criterion's state its textarea, "Title Model" its field.
    #[gpui_kit::test]
    fn a_click_on_a_linked_label_focuses_its_control(cx: &mut TestAppContext) {
        let tall = gpui_kit::Bounds::new(gpui_kit::point(px(0.), px(0.)), gpui_kit::size(px(1000.), px(4000.)));
        let options = WindowOptions { window_bounds: Some(gpui_kit::WindowBounds::Windowed(tall)), ..Default::default() };
        let (cx, _, view) = dialog_in(options, cx);
        show(Section::Llm, cx, &view);
        let click = |selector: &'static str, cx: &mut VisualTestContext| {
            // The dialog takes the section's height, measured as it is drawn.
            for _ in 0..2 {
                cx.update(|window, cx| window.draw(cx).clear(cx));
                cx.run_until_parked();
            }
            let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
            cx.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
        };
        let (title_model, criterion) =
            cx.update(|_, cx| (view.read(cx).inputs.title_model.clone(), view.read(cx).inputs.criteria[2].clone()));

        click("title-model-label", cx);
        assert!(is_focused(&title_model, cx));
        click("criterion-label-2", cx);
        assert!(is_focused(&criterion, cx));
        click("classifier-provider-label", cx);
        assert!(cx.update(|window, cx| view.read(cx).provider_focus.is_focused(window)));
        assert!(!menu_open(cx));
    }

    /// The analyzer's textareas are Electron's `rows` tall (System Prompt 4 lines, each
    /// criterion 2, Title System Prompt 3) and resize from their corner. Electron mounts a
    /// section afresh each time it is shown, so one shown again is back to its rows.
    #[gpui_kit::test]
    fn the_analyzer_textareas_show_their_rows_and_resize_until_the_section_changes(cx: &mut TestAppContext) {
        use gpui_kit::{Modifiers, MouseButton, point};
        let tall = gpui_kit::Bounds::new(point(px(0.), px(0.)), gpui_kit::size(px(1000.), px(4000.)));
        let options = WindowOptions { window_bounds: Some(gpui_kit::WindowBounds::Windowed(tall)), ..Default::default() };
        let (cx, _, view) = dialog_in(options, cx);
        show(Section::Llm, cx, &view);
        let draw = |cx: &mut VisualTestContext| {
            // The dialog takes the section's height, measured as it is drawn.
            for _ in 0..2 {
                cx.update(|window, cx| window.draw(cx).clear(cx));
                cx.run_until_parked();
            }
        };
        let bounds = |selector: &'static str, cx: &mut VisualTestContext| {
            draw(cx);
            cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"))
        };
        let height = |selector: &'static str, cx: &mut VisualTestContext| f32::from(bounds(selector, cx).size.height);
        assert_eq!(height("system-prompt", cx), 78.);
        assert_eq!(height("criterion-0", cx), 48.);
        assert_eq!(height("criterion-6", cx), 48.);
        assert_eq!(height("title-system-prompt", cx), 63.);

        let from = bounds("system-prompt", cx).bottom_right() - point(px(5.), px(5.));
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(from + point(px(0.), px(10.)), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(from + point(px(0.), px(40.)), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(from + point(px(0.), px(40.)), MouseButton::Left, Modifiers::none());
        assert_eq!(height("system-prompt", cx), 118.);
        assert_eq!(height("criterion-0", cx), 48.);

        show(Section::General, cx, &view);
        draw(cx);
        assert!(cx.debug_bounds("system-prompt").is_none(), "the LLM section is not drawn");
        show(Section::Llm, cx, &view);
        assert_eq!(height("system-prompt", cx), 78.);
    }

    /// The analyzer's controls carry Electron's accessible names: its `aria-label`s and the
    /// labels linked with `htmlFor`.
    #[gpui_kit::test]
    fn the_analyzer_controls_have_electrons_accessible_names(cx: &mut TestAppContext) {
        let (cx, _, view) = dialog(cx);
        show(Section::Llm, cx, &view);
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            let observed = test_support::snapshots(window);
            let field_name = |field: gpui_kit::EntityId| {
                let field = observed
                    .iter()
                    .find(|s| {
                        s.path().contains(&ElementId::View(field))
                            && matches!(s.role(), Some(Role::TextInput | Role::MultilineTextInput))
                    })
                    .expect("the field is drawn");
                field.label().map(str::to_owned)
            };
            let inputs = &view.read(cx).inputs;
            assert_eq!(field_name(inputs.analyzer_model.entity_id()).as_deref(), Some("Classifier Model"));
            assert_eq!(field_name(inputs.title_model.entity_id()).as_deref(), Some("Title Model"));
            for (index, (state, field)) in ClassifiedState::ALL.iter().zip(&inputs.criteria).enumerate() {
                assert_eq!(field_name(field.entity_id()), Some(format!("{} criterion", state.as_str())));
                let reset = test_support::find(window, &[], &ElementId::from(("reset-criterion", index)))
                    .expect("the reset button is drawn");
                assert_eq!(reset.label(), Some(format!("Reset {} criterion", state.as_str()).as_str()));
            }

            let provider = view.read(cx).provider_box(cx).into_element();
            let mut node = gpui_kit::accesskit::Node::new(Role::ComboBox);
            provider.write_a11y_info(&mut node);
            assert_eq!(node.label(), Some("Classifier Provider"));
        });
    }

    /// Click Cancel.
    fn cancel(cx: &mut VisualTestContext) {
        tab_to("cancel-settings", cx);
        press("enter", cx);
        cx.run_until_parked();
    }

    fn shown(view: &Entity<SettingsView>, cx: &mut VisualTestContext) -> Section {
        cx.update(|_, cx| view.read(cx).section.get())
    }

    /// Settings asked for while open stays as it is, its edits kept (Electron's
    /// `isSettingsOpen`); once closed its view goes, and it opens with the settings afresh.
    #[gpui_kit::test]
    fn settings_opens_once_until_it_is_closed(cx: &mut TestAppContext) {
        let (cx, store, keybindings) = settings_window(WindowOptions::default(), cx);
        let mut dialog = SettingsDialog::default();
        assert!(!dialog.is_open());
        let view = open(&mut dialog, &store, &keybindings, cx);
        let font_size = cx.update(|_, cx| view.read(cx).inputs.font_size.clone());
        cx.update(|window, cx| font_size.update(cx, |input, cx| input.set_value("20", window, cx)));

        assert!(open(&mut dialog, &store, &keybindings, cx) == view, "the open dialog");
        assert_eq!(cx.update(|window, cx| window.active_dialog_count(cx)), 1);
        assert_eq!(texts(cx, &view).font_size, "20", "with its edits");

        let closed = view.downgrade();
        drop(view);
        cancel(cx);
        assert!(!dialog.is_open());
        assert!(closed.upgrade().is_none(), "released with its dialog");
        let view = open(&mut dialog, &store, &keybindings, cx);
        assert_eq!(cx.update(|window, cx| window.active_dialog_count(cx)), 1);
        assert_eq!(texts(cx, &view).font_size, Texts::from_settings(&Settings::default()).font_size);
    }

    /// Settings first opens on Terminal, then on the section shown last, as Electron's
    /// `activeTab` lives as long as the window: one picked in the sidebar, or the one Save
    /// opened for a field to fix.
    #[gpui_kit::test]
    fn settings_reopens_on_the_section_shown_last(cx: &mut TestAppContext) {
        let (cx, store, keybindings) = settings_window(WindowOptions::default(), cx);
        let mut dialog = SettingsDialog::default();
        let view = open(&mut dialog, &store, &keybindings, cx);
        assert_eq!(shown(&view, cx), Section::Terminal);
        tab_to("General", cx);
        press("space", cx);
        drop(view);
        cancel(cx);

        let view = open(&mut dialog, &store, &keybindings, cx);
        assert_eq!(shown(&view, cx), Section::General);
        let buffer_lines = cx.update(|_, cx| view.read(cx).inputs.buffer_lines.clone());
        cx.update(|window, cx| buffer_lines.update(cx, |input, cx| input.set_value("", window, cx)));
        cx.update(|window, cx| view.update(cx, |view, cx| view.save(window, cx)));
        assert_eq!(shown(&view, cx), Section::Llm, "the field to fix");
        drop(view);
        cancel(cx);

        let view = open(&mut dialog, &store, &keybindings, cx);
        assert_eq!(shown(&view, cx), Section::Llm);
        assert_eq!(cx.update(|_, cx| view.read(cx).error().cloned()), None, "the error went with the dialog");
    }

    /// Escape leaves Settings open with its edits, wherever the focus is: Electron's dialog
    /// has no Escape handler. While recording, Escape is the key recorded.
    #[gpui_kit::test]
    fn escape_leaves_settings_open_with_its_edits(cx: &mut TestAppContext) {
        let (cx, _, view) = dialog(cx);
        let font_size = cx.update(|_, cx| view.read(cx).inputs.font_size.clone());
        focus_field(&font_size, cx);
        cx.update(|window, cx| font_size.update(cx, |input, cx| input.set_value("20", window, cx)));
        cx.simulate_keystrokes("escape");
        tab_to("save-settings", cx);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)), "still open");
        assert_eq!(texts(cx, &view).font_size, "20");

        cx.update(|window, cx| view.update(cx, |view, cx| view.toggle_recording(Recording::Action(1), window, cx)));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(texts(cx, &view).action_keys[1], "Escape");
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)), "still open");
    }
}
