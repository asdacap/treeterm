//! The System Prompt Debugger tab (`SystemPromptDebugger.tsx`).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, Subscription, Window, div, px, rgb};
use treeterm_core::settings::ClassifierProvider;
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::tool_tabs::prompt_debugger::{DebugMode, DebugRun, PromptDebugger, REASONING_CHOICES};

use crate::theme::colors;

/// The editable fields of one mode.
struct ModeInputs {
    model: Entity<InputState>,
    prompt: Entity<TextareaState>,
}

pub struct PromptDebuggerView {
    model: Entity<PromptDebugger>,
    settings: Entity<SettingsStore>,
    analyzer: ModeInputs,
    title: ModeInputs,
    buffer: Entity<TextareaState>,
    _subscriptions: Vec<Subscription>,
}

impl PromptDebuggerView {
    pub fn new(model: Entity<PromptDebugger>, settings: Entity<SettingsStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subscriptions = vec![cx.observe(&model, |_, _, cx| cx.notify())];
        let mut inputs = |mode: DebugMode, window: &mut Window, cx: &mut Context<Self>| {
            let (model_text, prompt_text) = {
                let debugger = model.read(cx);
                (debugger.model_for(mode).to_owned(), debugger.prompt(mode).to_owned())
            };
            let model_input = cx.new(|cx| InputState::new(window, cx).default_value(model_text));
            let prompt = cx.new(|cx| TextareaState::new(window, cx).rows(8).default_value(prompt_text));
            let debugger = model.clone();
            subscriptions.push(cx.subscribe(&model_input, move |_, input, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    let text = input.read(cx).value().to_string();
                    debugger.update(cx, |d, _| d.set_model(mode, &text));
                }
            }));
            let debugger = model.clone();
            subscriptions.push(cx.subscribe(&prompt, move |_, input, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    let text = input.read(cx).value().to_string();
                    debugger.update(cx, |d, _| d.set_prompt(mode, &text));
                }
            }));
            ModeInputs { model: model_input, prompt }
        };
        let analyzer = inputs(DebugMode::Analyzer, window, cx);
        let title = inputs(DebugMode::Title, window, cx);
        let buffer_text = model.read(cx).buffer().to_owned();
        let buffer = cx.new(|cx| {
            TextareaState::new(window, cx).rows(8).placeholder("Paste terminal buffer lines here...").default_value(buffer_text)
        });
        let debugger = model.clone();
        subscriptions.push(cx.subscribe(&buffer, move |_, input, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                let text = input.read(cx).value().to_string();
                debugger.update(cx, |d, _| d.set_buffer(&text));
            }
        }));
        Self { model, settings, analyzer, title, buffer, _subscriptions: subscriptions }
    }

    fn inputs(&self, mode: DebugMode) -> &ModeInputs {
        match mode {
            DebugMode::Analyzer => &self.analyzer,
            DebugMode::Title => &self.title,
        }
    }

}

impl Render for PromptDebuggerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (secondary, error, border, code_bg) = (c.text_secondary, c.error, c.border, c.bg_secondary);
        let debugger = self.model.read(cx);
        let (mode, run, reasoning, provider) = (debugger.mode(), debugger.run().clone(), debugger.reasoning(), debugger.provider(cx));
        let classifier = provider == ClassifierProvider::Classifier;
        let inputs = self.inputs(mode);
        let (model_input, prompt_input) = (inputs.model.clone(), inputs.prompt.clone());
        let label = |text: String| div().text_xs().text_color(secondary).child(text);

        let mut modes = h_flex().gap_1().items_center().child(label("Mode".into()));
        for (index, target) in [DebugMode::Analyzer, DebugMode::Title].into_iter().enumerate() {
            let button = Button::new(("debugger-mode", index)).xsmall().label(target.label());
            let button = if target == mode { button.primary() } else { button.ghost() };
            modes = modes.child(button.on_click(cx.listener(move |view, _, _, cx| {
                view.model.update(cx, |d, cx| d.set_mode(target, cx));
            })));
        }
        let mut reasonings = h_flex().gap_1().items_center().child(label("Reasoning".into()));
        for (index, (value, text)) in REASONING_CHOICES.into_iter().enumerate() {
            let button = Button::new(("debugger-reasoning", index)).xsmall().label(text).disabled(classifier);
            let button = if value == reasoning { button.primary() } else { button.ghost() };
            reasonings = reasonings.child(button.on_click(cx.listener(move |view, _, _, cx| {
                view.model.update(cx, |d, cx| d.set_reasoning(value, cx));
            })));
        }
        let running = run == DebugRun::Running;
        let (duration, outcome) = match run {
            DebugRun::Idle | DebugRun::Running => (None, None),
            DebugRun::Finished { duration, outcome } => (duration, Some(outcome)),
        };
        v_flex()
            .id("prompt-debugger")
            .size_full()
            .p_4()
            .gap_3()
            .overflow_y_scroll()
            .child(div().text_base().child("System Prompt Debugger"))
            .child(
                v_flex()
                    .gap_1()
                    .text_xs()
                    .text_color(secondary)
                    .child(format!("Provider: {}", if classifier { "Classifier" } else { "Chat Completions" }))
                    .when(classifier, |d| {
                        d.child(
                            "Classifier uses OpenRouter Decisions with the shared API Key and configured Base URL origin at /api/alpha/decisions. Results include the decision and summary. Reasoning is not sent to Classifier.",
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_end()
                    .child(v_flex().flex_1().gap_1().child(label("Model".into())).child(Input::new(&model_input).small()))
                    .child(modes)
                    .child(reasonings),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(label(format!("System Prompt ({})", mode.label())))
                    .child(Button::new("debugger-update").xsmall().outline().label("Update").on_click(cx.listener(
                        |view, _, _, cx| {
                            let settings = view.settings.clone();
                            view.model.update(cx, |d, cx| d.save_prompt(&settings, cx));
                        },
                    ))),
            )
            .child(Textarea::new(&prompt_input).h(px(160.)))
            .child(label("Buffer (paste from debug console)".into()))
            .child(Textarea::new(&self.buffer).h(px(160.)))
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        Button::new("debugger-test")
                            .small()
                            .primary()
                            .disabled(running)
                            .label(if running { "Testing..." } else { "Test" })
                            .on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |d, cx| d.test(cx)))),
                    )
                    .when_some(duration, |d, duration| {
                        d.child(div().text_xs().text_color(secondary).child(format!("{}ms", duration.as_millis())))
                    }),
            )
            .when_some(outcome, |d, outcome| match outcome {
                Ok(result) => d.child(
                    div()
                        .p_2()
                        .rounded_md()
                        .border_1()
                        .border_color(border)
                        .bg(code_bg)
                        .font_family("Menlo")
                        .text_xs()
                        .text_color(rgb(0x23d18b))
                        .child(result),
                ),
                Err(message) => d.child(div().font_family("Menlo").text_xs().text_color(error).child(message)),
            })
    }
}
