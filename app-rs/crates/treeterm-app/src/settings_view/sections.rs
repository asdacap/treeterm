//! The sections that edit single settings (the instance lists are in `instance_list`).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, div, px};
use treeterm_core::keybinding::PrefixAction;
use treeterm_core::settings::{
    ClassifiedState, ClassifierCriteria, ClassifierProvider, CursorStyle, DEFAULT_ANALYZER_SYSTEM_PROMPT,
    DEFAULT_TITLE_SYSTEM_PROMPT, ReasoningEffort, Theme,
};

use super::widgets::{choices, group, heading, hint, section, toggle, unlabeled};
use super::{SettingsView, form};
use crate::theme::colors;

impl SettingsView {
    pub(super) fn render_general(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut preview = self.draft.clone();
        self.lists(cx).store_in(&mut preview);
        let options = form::default_app_choices(&preview);
        let selected = self.draft.global_default_application_id.clone();
        let label = form::default_app_label(&options, &selected);
        let view = cx.entity();
        let picker = Button::new("default-app").label(label).small().outline().dropdown_menu(move |mut menu, _, _| {
            for option in &options {
                let (view, id) = (view.clone(), option.id.clone());
                menu = menu.item(PopupMenuItem::new(option.label.clone()).checked(option.id == selected).on_click(
                    move |_, _, cx| {
                        view.update(cx, |view, cx| {
                            view.draft.global_default_application_id = id.clone();
                            cx.notify();
                        })
                    },
                ));
            }
            menu
        });
        section()
            .child(group(
                "Default Application for New Worktrees",
                div().child(picker),
                Some(
                    "The default application to open when creating a new worktree. \
                     Child worktrees can inherit or override this setting.",
                ),
                cx,
            ))
            .into_any_element()
    }

    pub(super) fn render_terminal(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = &self.draft.terminal;
        let cursor = choices(
            "cursor-style",
            &[(CursorStyle::Block, "Block"), (CursorStyle::Underline, "Underline"), (CursorStyle::Bar, "Bar")],
            t.cursor_style,
            |s, style| s.terminal.cursor_style = style,
            cx,
        );
        let blink = toggle("cursor-blink", "Cursor Blink", t.cursor_blink, false, |s, on| s.terminal.cursor_blink = on, cx);
        let osc52 = toggle(
            "osc52",
            "Allow Terminal Clipboard Writes (OSC 52)",
            t.allow_osc52_clipboard,
            false,
            |s, on| s.terminal.allow_osc52_clipboard = on,
            cx,
        );
        let raw = toggle(
            "show-raw-chars",
            "Show Raw Characters (Debug)",
            t.show_raw_chars,
            false,
            |s, on| s.terminal.show_raw_chars = on,
            cx,
        );
        section()
            .child(group("Font Size", Input::new(&self.inputs.font_size).small(), None, cx))
            .child(group(
                "Max Columns",
                Input::new(&self.inputs.max_cols).small(),
                Some(
                    "Terminals fit to at most this many columns; the \"col limit\" switch in a tab's \
                     status bar lifts it for that tab",
                ),
                cx,
            ))
            .child(group("Font Family", Input::new(&self.inputs.font_family).small(), None, cx))
            .child(group("Cursor Style", cursor, None, cx))
            .child(unlabeled(blink, None, cx))
            .child(unlabeled(osc52, Some("Lets local or remote terminal programs copy text to your system clipboard"), cx))
            .child(unlabeled(raw, Some("Log the last 50 raw terminal characters to the console (DevTools)"), cx))
            .into_any_element()
    }

    pub(super) fn render_sandbox(&self, cx: &mut Context<Self>) -> AnyElement {
        let unavailable = !self.sandbox.is_available();
        let s = &self.draft.sandbox;
        let enabled = toggle(
            "sandbox-enabled",
            "Enable Sandbox by Default",
            s.enabled_by_default,
            unavailable,
            |s, on| s.sandbox.enabled_by_default = on,
            cx,
        );
        let network = toggle(
            "sandbox-network",
            "Allow Network Access by Default",
            s.allow_network_by_default,
            unavailable,
            |s, on| s.sandbox.allow_network_by_default = on,
            cx,
        );
        let c = colors(cx);
        section()
            .when_some(self.sandbox.warning(), |section, warning| {
                section.child(
                    div().p_2().border_1().border_color(c.error).rounded_md().text_sm().text_color(c.error).child(warning),
                )
            })
            .child(unlabeled(enabled, Some("New workspaces will have sandboxing enabled automatically"), cx))
            .child(unlabeled(network, Some("Sandboxed terminals will have network access by default"), cx))
            .into_any_element()
    }

    pub(super) fn render_llm(&self, cx: &mut Context<Self>) -> AnyElement {
        let a = &self.draft.terminal_analyzer;
        let provider = choices(
            "classifier-provider",
            &[(ClassifierProvider::ChatCompletions, "Chat Completions"), (ClassifierProvider::Classifier, "Classifier")],
            a.provider,
            |s, provider| s.terminal_analyzer.provider = provider,
            cx,
        );
        let reasoning = choices(
            "reasoning-effort",
            &[
                (ReasoningEffort::Off, "Off"),
                (ReasoningEffort::Low, "Low"),
                (ReasoningEffort::Medium, "Medium"),
                (ReasoningEffort::High, "High"),
            ],
            a.reasoning_effort,
            |s, effort| s.terminal_analyzer.reasoning_effort = effort,
            cx,
        );
        let reset_prompt = Button::new("reset-system-prompt").label("Reset to default").xsmall().ghost().on_click(
            cx.listener(|view, _, window, cx| {
                view.inputs.analyzer_system_prompt.update(cx, |state, cx| {
                    state.set_value(DEFAULT_ANALYZER_SYSTEM_PROMPT, window, cx)
                });
                cx.notify();
            }),
        );
        let reset_title_prompt = Button::new("reset-title-prompt").label("Reset to default").xsmall().ghost().on_click(
            cx.listener(|view, _, window, cx| {
                view.inputs.title_system_prompt.update(cx, |state, cx| {
                    state.set_value(DEFAULT_TITLE_SYSTEM_PROMPT, window, cx)
                });
                cx.notify();
            }),
        );
        section()
            .child(group(
                "Base URL",
                Input::new(&self.inputs.llm_base_url).small(),
                Some("OpenAI-compatible API endpoint. Works with OpenAI, Ollama, LM Studio, etc."),
                cx,
            ))
            .child(group(
                "API Key",
                Input::new(&self.inputs.llm_api_key).small().mask_toggle(),
                Some("Your API key is stored locally and never sent anywhere except the configured endpoint."),
                cx,
            ))
            .child(group(
                "Model",
                Input::new(&self.inputs.llm_model).small(),
                Some("Model name to use for chat completions."),
                cx,
            ))
            .child(heading("Terminal Analyzer", cx))
            .child(group(
                "Classifier Provider",
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
                Input::new(&self.inputs.analyzer_model).small(),
                Some("Model name for terminal analysis with the selected provider (e.g. typesafe/jev-1.13)."),
                cx,
            ))
            .child(group(
                "System Prompt",
                Textarea::new(&self.inputs.analyzer_system_prompt).small(),
                Some(
                    "Context and policy for the analyzer. Supports {{cwd}} and {{safe_paths}} template \
                     variables. Do not list the states here: Chat Completions appends the output format and \
                     the state criteria below, and Classifier sends this text as guidance next to the criteria.",
                ),
                cx,
            ))
            .child(div().child(reset_prompt))
            .child(self.render_criteria(cx))
            .child(group(
                "Title Model",
                Input::new(&self.inputs.title_model).small(),
                Some("Chat Completions model for titles, independent of the classifier provider and model."),
                cx,
            ))
            .child(group(
                "Title System Prompt",
                Textarea::new(&self.inputs.title_system_prompt).small(),
                Some("System prompt for auto-generating workspace titles from terminal output."),
                cx,
            ))
            .child(div().child(reset_title_prompt))
            .child(group(
                "Safe Paths",
                Input::new(&self.inputs.safe_paths).small(),
                Some(
                    "Comma-separated list of paths considered safe. The current working directory is always \
                     included automatically.",
                ),
                cx,
            ))
            .child(group(
                "Buffer Lines",
                Input::new(&self.inputs.buffer_lines).small(),
                Some("Number of terminal buffer lines to send to the analyzer."),
                cx,
            ))
            .child(group(
                "Idle Debounce (ms)",
                Input::new(&self.inputs.idle_debounce).small(),
                Some("Milliseconds of quiet terminal output before a tab is considered idle or classified."),
                cx,
            ))
            .child(group(
                "Idle Debounce When Unread (ms)",
                Input::new(&self.inputs.idle_debounce_unread).small(),
                Some("Same, while the workspace already shows an unread marker."),
                cx,
            ))
            .child(group(
                "Reasoning",
                reasoning,
                Some(
                    "Reasoning effort for Chat Completions analysis and title generation. Not sent to \
                     Classifier; still applies to titles when Classifier is selected.",
                ),
                cx,
            ))
            .into_any_element()
    }

    /// "State Criteria": one text per classified state, each resettable.
    fn render_criteria(&self, cx: &mut Context<Self>) -> AnyElement {
        let defaults = ClassifierCriteria::default();
        let mut list = v_flex()
            .gap_2()
            .child(div().text_sm().child("State Criteria"))
            .child(hint(
                "When the classifier should report each state. Used by both providers. Supports {{cwd}} and \
                 {{safe_paths}} template variables.",
                cx,
            ));
        for (index, (state, input)) in ClassifiedState::ALL.iter().copied().zip(&self.inputs.criteria).enumerate() {
            let is_default = input.read(cx).value().as_ref() == defaults.get(state);
            let reset = Button::new(("reset-criterion", index))
                .label("Reset to default")
                .xsmall()
                .ghost()
                .disabled(is_default)
                .on_click(cx.listener(move |view, _, window, cx| {
                    let default = ClassifierCriteria::default().get(state).to_owned();
                    view.inputs.criteria[index].update(cx, |input, cx| input.set_value(default, window, cx));
                    cx.notify();
                }));
            list = list.child(
                v_flex()
                    .gap_1()
                    .child(div().text_xs().font_family("Menlo").child(state.as_str()))
                    .child(Textarea::new(input).small())
                    .child(div().child(reset)),
            );
        }
        let reset_all = Button::new("reset-all-criteria").label("Reset all criteria").xsmall().ghost().on_click(
            cx.listener(|view, _, window, cx| {
                let defaults = ClassifierCriteria::default();
                for (state, input) in ClassifiedState::ALL.iter().zip(&view.inputs.criteria) {
                    let default = defaults.get(*state).to_owned();
                    input.update(cx, |input, cx| input.set_value(default, window, cx));
                }
                cx.notify();
            }),
        );
        list.child(div().child(reset_all)).into_any_element()
    }

    pub(super) fn render_appearance(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = choices(
            "theme",
            &[(Theme::Dark, "Dark"), (Theme::Light, "Light"), (Theme::System, "System")],
            self.draft.appearance.theme,
            |s, theme| s.appearance.theme = theme,
            cx,
        );
        section().child(group("Theme", theme, None, cx)).into_any_element()
    }

    pub(super) fn render_keybindings(&self, cx: &mut Context<Self>) -> AnyElement {
        let border = colors(cx).border;
        let mut actions = v_flex().gap_2();
        for (action, input) in PrefixAction::ALL.iter().zip(&self.inputs.action_keys) {
            actions = actions.child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(div().w(px(150.)).text_sm().child(action.label()))
                    .child(div().w(px(120.)).child(Input::new(input).small())),
            );
        }
        section()
            .child(group(
                "Prefix Key",
                Input::new(&self.inputs.prefix_key).small(),
                Some(
                    "Press a prefix key first, then the action key. Like tmux or screen. Written as an \
                     accelerator, e.g. Control+B or CommandOrControl+Shift+K.",
                ),
                cx,
            ))
            .child(group(
                "Timeout (ms)",
                Input::new(&self.inputs.prefix_timeout).small(),
                Some("How long to wait for action key after pressing prefix (500-5000ms)"),
                cx,
            ))
            .child(div().h(px(1.)).bg(border))
            .child(actions)
            .child(hint(
                "The key pressed after the prefix for each action, e.g. c, Tab or F2.",
                cx,
            ))
            .into_any_element()
    }

    pub(super) fn render_github(&self, cx: &mut Context<Self>) -> AnyElement {
        let autodetect = self.draft.github.autodetect_via_gh;
        let detect = toggle(
            "github-autodetect",
            "Autodetect via gh CLI",
            autodetect,
            false,
            |s, on| s.github.autodetect_via_gh = on,
            cx,
        );
        section()
            .child(unlabeled(
                detect,
                Some(
                    "When enabled, uses `gh auth token` to get the GitHub token automatically. Requires the \
                     GitHub CLI to be installed and authenticated.",
                ),
                cx,
            ))
            .child(group(
                "Personal Access Token",
                Input::new(&self.inputs.github_pat).small().mask_toggle().disabled(autodetect),
                Some(
                    "Used when \"Autodetect via gh CLI\" is off. Generate a token at GitHub → Settings → \
                     Developer Settings → Personal Access Tokens.",
                ),
                cx,
            ))
            .into_any_element()
    }

    pub(super) fn render_debug(&self, cx: &mut Context<Self>) -> AnyElement {
        let badge = toggle(
            "debug-badge",
            "Show Debug Badge",
            self.draft.debug.show_badge,
            false,
            |s, on| s.debug.show_badge = on,
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
