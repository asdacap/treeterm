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

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, FocusHandle, SharedString, Subscription, Window, canvas, div, px};
use treeterm_core::keybinding::Os;
use treeterm_core::settings::{
    AiHarnessInstance, BrowserInstance, CustomRunnerInstance, Settings, TerminalInstance, WebAppInstance,
};
use treeterm_model::settings_store::SettingsStore;

use self::form::{Lists, SandboxSupport, Section, Texts};
use self::instance_list::InstanceList;
use crate::theme::colors;
use crate::ui;

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
    section: Section,
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
    recording: Option<Recording>,
    /// Takes the keystrokes while recording.
    intercept: Option<Subscription>,
    /// Focused while recording, so no input takes the keys too.
    focus: FocusHandle,
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
        // `rows`: 4, 2 and 3 lines, growing as Electron's `resize: vertical` lets one drag.
        let mut area = |text: String, rows: usize| {
            cx.new(|cx| TextareaState::new(window, cx).auto_grow(rows, 12).default_value(text))
        };
        let analyzer_system_prompt = area(texts.analyzer_system_prompt, 4);
        let criteria = texts.criteria.map(|text| area(text, 2));
        let title_system_prompt = area(texts.title_system_prompt, 3);
        let _criteria_changes = criteria
            .iter()
            .map(|state| {
                cx.subscribe(state, |_, _, event: &InputEvent, cx| match event {
                    InputEvent::Change => cx.notify(),
                    InputEvent::PressEnter { .. } | InputEvent::Focus | InputEvent::Blur => {}
                })
            })
            .collect();
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
    pub fn new(store: Entity<SettingsStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut draft = (**store.read(cx).settings()).clone();
        let inputs = Inputs::new(Texts::from_settings(&draft), window, cx);
        let profiles = InstanceList::new(std::mem::take(&mut draft.terminal.instances), window, cx);
        let harnesses = InstanceList::new(std::mem::take(&mut draft.ai_harness.instances), window, cx);
        let runners = InstanceList::new(std::mem::take(&mut draft.custom_runner.instances), window, cx);
        let web_apps = InstanceList::new(std::mem::take(&mut draft.web_app.instances), window, cx);
        let browsers = InstanceList::new(std::mem::take(&mut draft.browser.instances), window, cx);
        Self {
            store,
            // Electron opens on Terminal too.
            section: Section::Terminal,
            draft,
            sandbox: SandboxSupport::detect(Os::host(), std::env::var_os("PATH").as_deref()),
            inputs,
            profiles,
            harnesses,
            runners,
            web_apps,
            browsers,
            error: None,
            recording: None,
            intercept: None,
            focus: cx.focus_handle(),
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

    /// Validate and save. Returns whether the dialog may close; if not, the error is shown
    /// and its section opened.
    pub fn save(&mut self, cx: &mut Context<Self>) -> bool {
        let texts = self.inputs.read(cx);
        let lists = self.lists(cx);
        let edited = match form::build(&self.draft, &texts, lists, Os::host()) {
            Ok(edited) => edited,
            Err(error) => {
                self.section = error.section;
                self.error = Some(error.message.into());
                cx.notify();
                return false;
            }
        };
        self.store.update(cx, |store, cx| store.update(|current| form::apply_owned(edited, current), cx));
        let saved_error = self.store.read(cx).error().map(str::to_owned);
        match saved_error {
            Some(error) => {
                self.error = Some(format!("Could not save settings: {error}").into());
                cx.notify();
                false
            }
            None => true,
        }
    }

    /// Start (or, clicked again, stop) recording a keybinding button's key. While recording,
    /// every keystroke goes to the recorder first (before key bindings such as Tab's focus
    /// move or Escape's close), as Electron's `preventDefault` + `stopPropagation` do.
    fn toggle_recording(&mut self, target: Recording, window: &mut Window, cx: &mut Context<Self>) {
        self.recording = if self.recording == Some(target) { None } else { Some(target) };
        self.intercept = self.recording.map(|_| {
            let view = cx.entity().downgrade();
            cx.intercept_keystrokes(move |event, window, cx| {
                if event.keystroke.is_modifier_key() {
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

    /// `.settings-tabs`: 140px, 8px vertical padding, a right border; `.settings-tab` rows of
    /// 13px text with 10px 16px padding, the active one bg-active.
    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
            let active = section == self.section;
            list = list.child(
                div()
                    .id(section.label())
                    .px(px(16.))
                    .py(px(10.))
                    .text_size(px(ui::BODY))
                    .line_height(px(15.5))
                    .cursor_pointer()
                    .when(active, |item| item.bg(active_bg).text_color(primary))
                    .when(!active, |item| item.text_color(secondary).hover(move |s| s.bg(hover_bg).text_color(primary)))
                    .child(section.label())
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.section = section;
                        cx.notify();
                    })),
            );
        }
        list
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.section {
            Section::General => self.render_general(cx),
            Section::Terminal => self.render_terminal(cx),
            Section::TerminalProfiles => instance_list::render(&self.profiles, |v| &mut v.profiles, self.sandbox, cx),
            Section::AiHarness => instance_list::render(&self.harnesses, |v| &mut v.harnesses, self.sandbox, cx),
            Section::CustomRunners => instance_list::render(&self.runners, |v| &mut v.runners, self.sandbox, cx),
            Section::WebApps => instance_list::render(&self.web_apps, |v| &mut v.web_apps, self.sandbox, cx),
            Section::Browsers => instance_list::render(&self.browsers, |v| &mut v.browsers, self.sandbox, cx),
            Section::Llm => self.render_llm(cx),
            Section::Sandbox => self.render_sandbox(cx),
            Section::Appearance => self.render_appearance(cx),
            Section::Keybindings => self.render_keybindings(cx),
            Section::Github => self.render_github(cx),
            Section::Debug => self.render_debug(cx),
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
        ui::dialog_root(cx)
            .track_focus(&self.focus)
            .child(ui::dialog_header("Settings", "×", colors(cx).text_primary, cx))
            .child(
                div().flex().w_full().h(px(content_height)).overflow_hidden().child(self.sidebar(cx)).child(
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
                    .child(
                        ui::dialog_button("cancel-settings", "Cancel", ui::ButtonKind::Secondary, false, cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(ui::dialog_button("save-settings", "Save", ui::ButtonKind::Primary, false, cx).on_click(
                        cx.listener(|view, _, window, cx| {
                            if view.save(cx) {
                                window.close_dialog(cx);
                            }
                        }),
                    )),
            )
    }
}

/// Open the dialog in `window`.
pub fn open(store: Entity<SettingsStore>, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| SettingsView::new(store, window, cx));
    window.open_dialog(cx, move |dialog, _window, cx| ui::dialog_frame(dialog, 600., cx).child(view.clone()));
}
