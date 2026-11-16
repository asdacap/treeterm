//! The sections that edit single settings (the instance lists are in `instance_list`).

use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, Div, Stateful, Window, div, px, rgb, rgba};
use treeterm_core::keybinding::{Os, PrefixAction};
use treeterm_core::settings::{
    ClassifiedState, ClassifierCriteria, ClassifierProvider, CursorStyle, DEFAULT_ANALYZER_SYSTEM_PROMPT,
    DEFAULT_TITLE_SYSTEM_PROMPT, ReasoningEffort, Settings, Theme,
};

use super::widgets::{
    divider, group, heading, hint, hint_with_code, label, label_for, labelled, option_label, reset_button, section,
    select, select_from, select_menu, toggle, unlabeled,
};
use super::{Recording, SettingsView, form};
use crate::number_field::NumberField;
use crate::theme::colors;
use crate::ui;

const CURSOR_STYLES: &[(CursorStyle, &str)] =
    &[(CursorStyle::Block, "Block"), (CursorStyle::Underline, "Underline"), (CursorStyle::Bar, "Bar")];
const PROVIDERS: &[(ClassifierProvider, &str)] =
    &[(ClassifierProvider::ChatCompletions, "Chat Completions"), (ClassifierProvider::Classifier, "Classifier")];
const REASONING: &[(ReasoningEffort, &str)] = &[
    (ReasoningEffort::Off, "Off"),
    (ReasoningEffort::Low, "Low"),
    (ReasoningEffort::Medium, "Medium"),
    (ReasoningEffort::High, "High"),
];
const THEMES: &[(Theme, &str)] = &[(Theme::Dark, "Dark"), (Theme::Light, "Light"), (Theme::System, "System")];
/// The provider select's `<label>`, which also names it.
const PROVIDER_LABEL: &str = "Classifier Provider";
/// The title model input's `<label>`, which also names it.
const TITLE_MODEL_LABEL: &str = "Title Model";

impl SettingsView {
    pub(super) fn render_general(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let mut preview = self.draft.clone();
        self.lists(cx).store_in(&mut preview);
        let options = form::default_app_choices(&preview);
        let selected = self.draft.global_default_application_id.clone();
        let current = form::default_app_label(&options, &selected);
        let view = cx.entity().downgrade();
        let focus = ui::keyed_focus("default-app", window, cx);
        let picker = select_menu(ui::key_select_box("default-app", current, &focus, cx), move |mut menu| {
            for option in &options {
                let (view, id) = (view.clone(), option.id.clone());
                menu = menu.item(ui::menu_item(option.label.clone()).checked(option.id == selected).on_click(
                    move |_, _, cx| {
                        if let Err(error) = view.update(cx, |view, cx| {
                            view.draft.global_default_application_id = id.clone();
                            cx.notify();
                        }) {
                            tracing::debug!(%error, "settings closed");
                        }
                    },
                ));
            }
            menu
        });
        section()
            .child(group(
                "Default Application for New Worktrees",
                picker,
                Some(
                    "The default application to open when creating a new worktree. \
                     Child worktrees can inherit or override this setting.",
                ),
                cx,
            ))
            .into_any_element()
    }

    pub(super) fn render_terminal(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = &self.draft.terminal;
        let focus = ui::keyed_focus("cursor-style", window, cx);
        let cursor =
            select("cursor-style", CURSOR_STYLES, t.cursor_style, |s, style| s.terminal.cursor_style = style, &focus, cx);
        let blink = toggle(
            "cursor-blink",
            "Cursor Blink",
            t.cursor_blink,
            false,
            |s, on| s.terminal.cursor_blink = on,
            window,
            cx,
        );
        let osc52 = toggle(
            "osc52",
            "Allow Terminal Clipboard Writes (OSC 52)",
            t.allow_osc52_clipboard,
            false,
            |s, on| s.terminal.allow_osc52_clipboard = on,
            window,
            cx,
        );
        let raw = toggle(
            "show-raw-chars",
            "Show Raw Characters (Debug)",
            t.show_raw_chars,
            false,
            |s, on| s.terminal.show_raw_chars = on,
            window,
            cx,
        );
        section()
            .child(group("Font Size", NumberField::new(&self.inputs.font_size, form::FONT_SIZE), None, cx))
            .child(group(
                "Max Columns",
                NumberField::new(&self.inputs.max_cols, form::MAX_COLS),
                Some(
                    "Terminals fit to at most this many columns; the \"col limit\" switch in a tab's \
                     status bar lifts it for that tab",
                ),
                cx,
            ))
            .child(group("Font Family", ui::text_input(&self.inputs.font_family, cx), None, cx))
            .child(group("Cursor Style", cursor, None, cx))
            .child(unlabeled(blink, None, cx))
            .child(unlabeled(osc52, Some("Lets local or remote terminal programs copy text to your system clipboard"), cx))
            // Electron's hint names the DevTools console, which the app does not have.
            .child(unlabeled(
                raw,
                Some("Log the last 50 raw terminal characters to the app log (stdout, as [RAW] lines)"),
                cx,
            ))
            .into_any_element()
    }

    pub(super) fn render_sandbox(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let unavailable = !self.sandbox.is_available();
        let s = &self.draft.sandbox;
        let enabled = toggle(
            "sandbox-enabled",
            "Enable Sandbox by Default",
            s.enabled_by_default,
            unavailable,
            |s, on| s.sandbox.enabled_by_default = on,
            window,
            cx,
        );
        let network = toggle(
            "sandbox-network",
            "Allow Network Access by Default",
            s.allow_network_by_default,
            unavailable,
            |s, on| s.sandbox.allow_network_by_default = on,
            window,
            cx,
        );
        section()
            // `.settings-warning` has no style of its own: body text.
            .when_some(self.sandbox.warning(), |section, warning| {
                section.child(div().text_size(px(16.)).child(warning))
            })
            .child(unlabeled(enabled, Some("New workspaces will have sandboxing enabled automatically"), cx))
            .child(unlabeled(network, Some("Sandboxed terminals will have network access by default"), cx))
            .into_any_element()
    }

    pub(super) fn render_llm(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let a = &self.draft.terminal_analyzer;
        let set_provider = |s: &mut Settings, provider| s.terminal_analyzer.provider = provider;
        let provider = select_from(self.provider_box(cx), PROVIDERS, a.provider, set_provider, cx);
        // `<label htmlFor="classifier-provider">`: a click on it focuses the select.
        let provider_focus = self.provider_focus.clone();
        let focus_provider = move |window: &mut Window, cx: &mut App| window.focus(&provider_focus, cx);
        let provider_label = label_for("classifier-provider-label", PROVIDER_LABEL, focus_provider, cx);
        // `<label htmlFor="title-model">`.
        let title_model = self.inputs.title_model.clone();
        let title_model_label = label_for(
            "title-model-label",
            TITLE_MODEL_LABEL,
            move |window, cx| title_model.update(cx, |input, cx| input.focus(window, cx)),
            cx,
        );
        let reasoning = plain_select(a.reasoning_effort, window, cx);
        let reset_prompt = reset_button("reset-system-prompt", "Reset to default", false, window, cx).on_click(
            cx.listener(|view, _, window, cx| {
                view.inputs.analyzer_system_prompt.update(cx, |state, cx| {
                    state.set_value(DEFAULT_ANALYZER_SYSTEM_PROMPT, window, cx)
                });
                cx.notify();
            }),
        );
        let reset_title_prompt = reset_button("reset-title-prompt", "Reset to default", false, window, cx).on_click(
            cx.listener(|view, _, window, cx| {
                view.inputs.title_system_prompt.update(cx, |state, cx| {
                    state.set_value(DEFAULT_TITLE_SYSTEM_PROMPT, window, cx)
                });
                cx.notify();
            }),
        );
        // `rows={4}` and `rows={3}`, `resize: vertical`.
        let system_prompt = ui::text_area(&self.inputs.analyzer_system_prompt, cx);
        let system_prompt = ui::tabbing_text_area(ui::rows_text_area("system-prompt", system_prompt, 4, window, cx));
        let title_system_prompt = ui::text_area(&self.inputs.title_system_prompt, cx);
        let title_system_prompt =
            ui::tabbing_text_area(ui::rows_text_area("title-system-prompt", title_system_prompt, 3, window, cx));
        section()
            .child(group(
                "Base URL",
                ui::text_input(&self.inputs.llm_base_url, cx),
                Some("OpenAI-compatible API endpoint. Works with OpenAI, Ollama, LM Studio, etc."),
                cx,
            ))
            .child(group(
                "API Key",
                ui::text_input(&self.inputs.llm_api_key, cx),
                Some("Your API key is stored locally and never sent anywhere except the configured endpoint."),
                cx,
            ))
            .child(group(
                "Model",
                ui::text_input(&self.inputs.llm_model, cx),
                Some("Model name to use for chat completions."),
                cx,
            ))
            .child(heading("Terminal Analyzer", cx))
            .child(labelled(
                provider_label,
                provider,
                Some(
                    "Classifier uses OpenRouter Decisions with the shared API Key and the configured Base URL \
                     origin at /api/alpha/decisions. Reasoning applies only to Chat Completions; title \
                     generation always uses Chat Completions.",
                ),
                cx,
            ))
            .child(group(
                "Model",
                ui::text_input(&self.inputs.analyzer_model, cx).aria_label("Classifier Model"),
                Some("Model name for terminal analysis with the selected provider (e.g. typesafe/jev-1.13)."),
                cx,
            ))
            .child(
                group(
                    "System Prompt",
                    system_prompt,
                    Some(
                        "Context and policy for the analyzer. Supports {{cwd}} and {{safe_paths}} template \
                         variables. Do not list the states here: Chat Completions appends the output format and \
                         the state criteria below, and Classifier sends this text as guidance next to the criteria.",
                    ),
                    cx,
                )
                .child(reset_prompt),
            )
            .child(self.render_criteria(window, cx))
            .child(labelled(
                title_model_label,
                ui::text_input(&self.inputs.title_model, cx).aria_label(TITLE_MODEL_LABEL),
                Some("Chat Completions model for titles, independent of the classifier provider and model."),
                cx,
            ))
            .child(
                group(
                    "Title System Prompt",
                    title_system_prompt,
                    Some("System prompt for auto-generating workspace titles from terminal output."),
                    cx,
                )
                .child(reset_title_prompt),
            )
            .child(group(
                "Safe Paths",
                ui::text_input(&self.inputs.safe_paths, cx),
                Some(
                    "Comma-separated list of paths considered safe. The current working directory is always \
                     included automatically.",
                ),
                cx,
            ))
            .child(group(
                "Buffer Lines",
                NumberField::new(&self.inputs.buffer_lines, form::BUFFER_LINES),
                Some("Number of terminal buffer lines to send to the analyzer."),
                cx,
            ))
            .child(group(
                "Idle Debounce (ms)",
                NumberField::new(&self.inputs.idle_debounce, form::IDLE_DEBOUNCE),
                Some("Milliseconds of quiet terminal output before a tab is considered idle or classified."),
                cx,
            ))
            .child(group(
                "Idle Debounce When Unread (ms)",
                NumberField::new(&self.inputs.idle_debounce_unread, form::IDLE_DEBOUNCE_UNREAD),
                Some("Same, while the workspace already shows an unread marker."),
                cx,
            ))
            .child(
                // The label holds an unstyled `<select>`, 8px after its text.
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(label("Reasoning", cx).flex().items_center().gap(px(8.)).child(reasoning))
                    .child(hint(
                        "Reasoning effort for Chat Completions analysis and title generation. Not sent to \
                         Classifier; still applies to titles when Classifier is selected.",
                        cx,
                    )),
            )
            .into_any_element()
    }

    /// The Classifier Provider select's box, named by its `<label>`.
    pub(super) fn provider_box(&self, cx: &App) -> Stateful<Div> {
        let current = option_label(PROVIDERS, &self.draft.terminal_analyzer.provider);
        ui::key_select_box("classifier-provider", current, &self.provider_focus, cx).aria_label(PROVIDER_LABEL)
    }

    /// "State Criteria": one text per classified state, each resettable.
    fn render_criteria(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let defaults = ClassifierCriteria::default();
        let mut list = div().flex().flex_col().gap(px(6.)).child(label("State Criteria", cx)).child(hint(
            "When the classifier should report each state. Used by both providers. Supports {{cwd}} and \
             {{safe_paths}} template variables.",
            cx,
        ));
        for (index, (state, input)) in ClassifiedState::ALL.iter().copied().zip(&self.inputs.criteria).enumerate() {
            let is_default = input.read(cx).value().as_ref() == defaults.get(state);
            let reset = reset_button(("reset-criterion", index), "Reset to default", is_default, window, cx)
                .accessibility_label(format!("Reset {} criterion", state.as_str()))
                .on_click(cx.listener(move |view, _, window, cx| {
                    let default = ClassifierCriteria::default().get(state).to_owned();
                    view.inputs.criteria[index].update(cx, |input, cx| input.set_value(default, window, cx));
                    cx.notify();
                }));
            // `<label htmlFor="criterion-<state>">`: a click on it focuses the textarea, which
            // its `aria-label` names.
            let area = input.clone();
            let name = label_for(
                ("criterion-label", index),
                state.as_str(),
                move |window, cx| area.update(cx, |input, cx| input.focus(window, cx)),
                cx,
            );
            // `rows={2}`, `resize: vertical`.
            let area = ui::text_area(input, cx).aria_label(format!("{} criterion", state.as_str()));
            let area = ui::rows_text_area(("criterion", index), area, 2, window, cx);
            // `.settings-criterion`: 8px 0 4px margins, the reset button at its own width.
            list = list.child(
                div()
                    .mt(px(8.))
                    .mb(px(4.))
                    .flex()
                    .flex_col()
                    .items_start()
                    .child(name.font_family("Menlo").line_height(px(16.)))
                    .child(ui::tabbing_text_area(area).w_full())
                    .child(reset),
            );
        }
        let reset_all = reset_button("reset-all-criteria", "Reset all criteria", false, window, cx).on_click(cx.listener(
            |view, _, window, cx| {
                let defaults = ClassifierCriteria::default();
                for (state, input) in ClassifiedState::ALL.iter().zip(&view.inputs.criteria) {
                    let default = defaults.get(*state).to_owned();
                    input.update(cx, |input, cx| input.set_value(default, window, cx));
                }
                cx.notify();
            },
        ));
        list.child(reset_all).into_any_element()
    }

    pub(super) fn render_appearance(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let focus = ui::keyed_focus("theme", window, cx);
        let set_theme = |s: &mut Settings, theme| s.appearance.theme = theme;
        let theme = select("theme", THEMES, self.draft.appearance.theme, set_theme, &focus, cx);
        section().child(group("Theme", theme, None, cx)).into_any_element()
    }

    pub(super) fn render_keybindings(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let prefix_text = self.inputs.prefix_key.read(cx).value().to_string();
        let prefix_label = match self.recording {
            Some(Recording::Prefix) => "Press keys...".to_owned(),
            _ => form::format_prefix_key(&prefix_text, Os::host()),
        };
        let recording_prefix = self.recording == Some(Recording::Prefix);
        let prefix = keybinding_button("prefix-key", prefix_label, recording_prefix, false, window, cx)
            .on_click(cx.listener(|view, _, window, cx| view.toggle_recording(Recording::Prefix, window, cx)));
        let mut list = section()
            .child(group(
                "Prefix Key",
                prefix,
                Some("Press a prefix key first, then the action key. Like tmux or screen."),
                cx,
            ))
            .child(group(
                "Timeout (ms)",
                NumberField::new(&self.inputs.prefix_timeout, form::PREFIX_TIMEOUT),
                Some("How long to wait for action key after pressing prefix (500-5000ms)"),
                cx,
            ))
            .child(divider(cx));
        for (index, (action, input)) in PrefixAction::ALL.iter().zip(&self.inputs.action_keys).enumerate() {
            let recording = self.recording == Some(Recording::Action(index));
            let text = if recording { "Press key...".to_owned() } else { input.read(cx).value().to_uppercase() };
            let button = keybinding_button(("action-key", index), text, recording, true, window, cx).on_click(cx.listener(
                move |view, _, window, cx| view.toggle_recording(Recording::Action(index), window, cx),
            ));
            // `.keybinding-row`: label and button 8px apart.
            list = list.child(div().flex().flex_col().gap(px(8.)).child(label(action_label(*action), cx)).child(button));
        }
        list.child(hint(
            "Click a keybinding to record a new key. Press the prefix key first, then the action key shown here.",
            cx,
        ))
        .into_any_element()
    }

    pub(super) fn render_github(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let autodetect = self.draft.github.autodetect_via_gh;
        let detect = toggle(
            "github-autodetect",
            "Autodetect via gh CLI",
            autodetect,
            false,
            |s, on| s.github.autodetect_via_gh = on,
            window,
            cx,
        );
        let detect_hint = hint_with_code(
            "When enabled, uses ",
            "gh auth token",
            " to get the GitHub token automatically. Requires the GitHub CLI to be installed and authenticated.",
            window,
            cx,
        );
        section()
            .child(unlabeled(detect, None, cx).child(detect_hint))
            .child(group(
                "Personal Access Token",
                ui::text_input(&self.inputs.github_pat, cx).disabled(autodetect),
                Some(
                    "Used when \"Autodetect via gh CLI\" is off. Generate a token at GitHub → Settings → \
                     Developer Settings → Personal Access Tokens.",
                ),
                cx,
            ))
            .into_any_element()
    }

    pub(super) fn render_debug(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let badge = toggle(
            "debug-badge",
            "Show Debug Badge",
            self.draft.debug.show_badge,
            false,
            |s, on| s.debug.show_badge = on,
            window,
            cx,
        );
        section()
            .child(unlabeled(
                badge,
                Some("Shows a purple DEBUG badge on all terminals. Useful for testing settings reactivity."),
                cx,
            ))
            .into_any_element()
    }
}

/// `formatKeybindingLabel`: Electron names five actions and shows the sixth by its key.
fn action_label(action: PrefixAction) -> &'static str {
    match action {
        PrefixAction::WorkspaceFocus => "workspaceFocus",
        PrefixAction::NewTab
        | PrefixAction::CloseTab
        | PrefixAction::NextTab
        | PrefixAction::PrevTab
        | PrefixAction::OpenSettings => action.label(),
    }
}

/// `.settings-keybinding`: a monospace button showing the binding (`.prefix-key-btn`
/// centres it); `.recording` turns it accent.
fn keybinding_button(
    id: impl Into<gpui_kit::ElementId>,
    text: String,
    recording: bool,
    centred: bool,
    window: &mut Window,
    cx: &mut App,
) -> gpui_kit::base::Button {
    let button = ui::key_button(id, 4., window, cx);
    let c = colors(cx);
    let hover = c.bg_hover;
    button
        .when(!centred, |d| d.justify_start())
        .px(px(12.))
        .py(px(8.))
        .border_1()
        .text_size(px(ui::BODY))
        .line_height(px(15.))
        .font_family("Menlo")
        .cursor_pointer()
        .when(recording, |d| d.border_color(c.accent).bg(rgba(0x0078d41a)).text_color(c.accent))
        .when(!recording, |d| d.border_color(c.border).bg(c.bg_tertiary).text_color(c.text_primary).hover(move |d| d.bg(hover)))
        .child(text)
}

/// The Reasoning `<select>`, which has no class: Chromium's own light control (white, a
/// #767676 border, black 13px text), with its focus ring while focused from the keyboard.
fn plain_select(
    selected: ReasoningEffort,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement + use<> {
    let current = option_label(REASONING, &selected);
    let focus = ui::keyed_focus("reasoning-effort", window, cx);
    let boxed = div()
        .id("reasoning-effort")
        .flex()
        .items_center()
        .gap(px(4.))
        .h(px(19.))
        .pl(px(4.))
        .pr(px(3.))
        .rounded(px(3.))
        .bg(rgb(0xffffff))
        .border_1()
        .border_color(rgb(0x767676))
        .text_size(px(ui::BODY))
        .line_height(px(15.))
        .font_weight(gpui_kit::FontWeight::NORMAL)
        .text_color(rgb(0x000000))
        .cursor_pointer()
        .child(current)
        .child(ui::icon(gpui_kit::assets::IconName::ChevronDown, 10., rgb(0x000000)));
    let boxed = ui::with_focus_ring(ui::key_select(boxed, &focus), &focus, 3., window);
    select_from(boxed, REASONING, selected, |s, effort| s.terminal_analyzer.reasoning_effort = effort, cx)
}
