//! The System Prompt Debugger tab (`SystemPromptDebugger.tsx`).

use gpui_kit::base::SelectableText;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Entity, FocusHandle, FontWeight, Subscription, Window, div, px, rgb};
use treeterm_core::settings::ClassifierProvider;
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::tool_tabs::prompt_debugger::{DebugMode, PromptDebugger, PromptDebuggerEvent, REASONING_CHOICES};

use crate::ui::{FieldStyle, MONO, field, normal_line, pane_select_box, pane_text_area, text_button};

/// The inline-styled controls' colours (`#1e1e1e` fields, `#333` borders, `#aaa` labels).
const FIELD_BG: u32 = 0x1e1e1e;
const FIELD_BORDER: u32 = 0x333333;
const FIELD_TEXT: u32 = 0xd4d4d4;
const LABEL: u32 = 0xaaaaaa;

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
    /// The pane's own focus. A mouse-down anywhere in the pane but a field moves focus here
    /// (Electron: a mousedown on non-editable content blurs the textarea), so after a drag
    /// over the output Cmd/Ctrl+C reaches the Kit root's Copy rather than the field, whose
    /// Copy takes the keystroke even with nothing selected.
    focus: FocusHandle,
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
        // The tab state's `bufferText` replaced the buffer (`set_value` emits no Change).
        subscriptions.push(cx.subscribe_in(&model, window, |view, _, event: &PromptDebuggerEvent, window, cx| match event {
            PromptDebuggerEvent::BufferReplaced(text) => {
                view.buffer.update(cx, |input, cx| input.set_value(text.clone(), window, cx));
            }
            PromptDebuggerEvent::SetActivity { .. } => {}
        }));
        Self { model, settings, analyzer, title, buffer, focus: cx.focus_handle(), _subscriptions: subscriptions }
    }

    fn inputs(&self, mode: DebugMode) -> &ModeInputs {
        match mode {
            DebugMode::Analyzer => &self.analyzer,
            DebugMode::Title => &self.title,
        }
    }

}

impl Render for PromptDebuggerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let debugger = self.model.read(cx);
        let (mode, running, output, reasoning, provider) =
            (debugger.mode(), debugger.is_running(), debugger.output().clone(), debugger.reasoning(), debugger.provider(cx));
        let classifier = provider == ClassifierProvider::Classifier;
        let inputs = self.inputs(mode);
        let (model_input, prompt_input) = (inputs.model.clone(), inputs.prompt.clone());
        let label = |text: String| div().text_size(px(12.)).line_height(normal_line(12.)).text_color(rgb(LABEL)).child(text);
        let field_style = |height: f32, pad: (f32, f32)| FieldStyle {
            bg: rgb(FIELD_BG).into(),
            border: rgb(FIELD_BORDER).into(),
            radius: 4.,
            pad,
            size: 12.,
            mono: true,
            height,
        };
        // The `<select>`s: 12px, padded 2px 4px, 22px high.
        let select = |id: &'static str, text: &'static str, width: f32, disabled: bool| {
            pane_select_box(id, text, FieldStyle { mono: false, ..field_style(22., (2., 4.)) }, disabled, cx)
                .w(px(width))
                .text_color(rgb(FIELD_TEXT))
        };
        let model = self.model.clone();
        let mode_picker = select("debugger-mode", mode.label(), 126., false).dropdown_menu(move |mut menu, _, _| {
            for target in [DebugMode::Analyzer, DebugMode::Title] {
                let model = model.clone();
                menu = menu.item(
                    PopupMenuItem::new(target.label())
                        .checked(target == mode)
                        .on_click(move |_, _, cx| model.update(cx, |d, cx| d.set_mode(target, cx))),
                );
            }
            menu
        });
        let reasoning_label = REASONING_CHOICES.iter().find(|(value, _)| *value == reasoning).map_or("Off", |(_, text)| text);
        let model = self.model.clone();
        let reasoning_picker: AnyElement = if classifier {
            select("debugger-reasoning", reasoning_label, 73., true).into_any_element()
        } else {
            select("debugger-reasoning", reasoning_label, 73., false)
                .dropdown_menu(move |mut menu, _, _| {
                    for (value, text) in REASONING_CHOICES {
                        let model = model.clone();
                        menu = menu.item(
                            PopupMenuItem::new(text)
                                .checked(value == reasoning)
                                .on_click(move |_, _, cx| model.update(cx, |d, cx| d.set_reasoning(value, cx))),
                        );
                    }
                    menu
                })
                .into_any_element()
        };
        let labelled = |text: &'static str, control: AnyElement| {
            h_flex()
                .flex_shrink_0()
                .gap(px(4.))
                .text_size(px(12.))
                .line_height(normal_line(12.))
                .text_color(rgb(LABEL))
                .child(text)
                .child(control)
        };
        v_flex()
            .id("prompt-debugger")
            .track_focus(&self.focus)
            .size_full()
            .p(px(16.))
            .gap(px(12.))
            .overflow_y_scroll()
            .child(
                div()
                    .text_size(px(18.72))
                    .line_height(px(22.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(0xcccccc))
                    .child("System Prompt Debugger"),
            )
            .child(
                v_flex()
                    .text_size(px(12.))
                    .line_height(normal_line(12.))
                    .text_color(rgb(LABEL))
                    .child(format!("Provider: {}", if classifier { "Classifier" } else { "Chat Completions" }))
                    .when(classifier, |d| {
                        d.child(
                            "Classifier uses OpenRouter Decisions with the shared API Key and configured Base URL origin at /api/alpha/decisions. Results include the decision and summary. Reasoning is not sent to Classifier.",
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap(px(12.))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            // The label sits on the 16px body font's line.
                            .child(div().h(px(19.)).flex().items_end().pb(px(1.)).child(label("Model".into())))
                            .child(field(&model_input, field_style(24., (4., 8.)), window, cx).text_color(rgb(FIELD_TEXT))),
                    )
                    .child(labelled("Mode", mode_picker.into_any_element()))
                    .child(labelled("Reasoning", reasoning_picker)),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(label(format!("System Prompt ({})", mode.label())))
                    .child(
                        text_button("debugger-update", "Update", 11., (2., 8.), rgb(0xcccccc), rgb(0x333333), rgb(0x555555), 4.)
                            .on_click(cx.listener(|view, _, _, cx| {
                                let settings = view.settings.clone();
                                view.model.update(cx, |d, cx| d.save_prompt(&settings, cx));
                            })),
                    ),
            )
            .child(pane_text_area(&prompt_input, field_style(120., (8., 8.)), window, cx).text_color(rgb(FIELD_TEXT)))
            .child(label("Buffer (paste from debug console)".into()))
            .child(pane_text_area(&self.buffer, field_style(120., (8., 8.)), window, cx).text_color(rgb(FIELD_TEXT)))
            .child(
                h_flex()
                    .gap(px(12.))
                    .child(
                        div()
                            .id("debugger-test")
                            .px(px(16.))
                            .py(px(6.))
                            .rounded(px(4.))
                            .bg(rgb(if running { 0x333333 } else { 0x2472c8 }))
                            .text_size(px(13.))
                            .line_height(normal_line(13.))
                            .text_color(gpui_kit::white())
                            .child(if running { "Testing..." } else { "Test" })
                            .when(running, |d| d.cursor_not_allowed())
                            .when(!running, |d| {
                                d.cursor_pointer().on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |d, cx| d.test(cx))))
                            }),
                    )
                    .when_some(output.duration(), |d, duration| {
                        d.child(
                            div()
                                .text_size(px(12.))
                                .line_height(normal_line(12.))
                                .text_color(rgb(0x888888))
                                .child(format!("{}ms", duration.as_millis())),
                        )
                    }),
            )
            // The error above the result, both selectable (Cmd/Ctrl+C copies).
            .when_some(output.error().map(str::to_owned), |d, message| {
                d.child(
                    div()
                        .font_family(MONO)
                        .text_size(px(12.))
                        .line_height(normal_line(12.))
                        .text_color(rgb(0xf14c4c))
                        .child(SelectableText::new("debugger-error", message)),
                )
            })
            .when_some(output.result().map(str::to_owned), |d, result| {
                d.child(
                    div()
                        .p(px(8.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(rgb(FIELD_BORDER))
                        .bg(rgb(FIELD_BG))
                        .font_family(MONO)
                        .text_size(px(12.))
                        .line_height(normal_line(12.))
                        .text_color(rgb(0x23d18b))
                        .child(SelectableText::new("debugger-result", result)),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::base::TextSelection;
    use gpui_kit::test::TestQueryExt as _;
    use gpui_kit::{
        Focusable as _, Modifiers, MouseButton, Pixels, Point, TestAppContext, VisualTestContext, WeakEntity, WindowOptions, point,
    };
    use treeterm_core::settings::Settings;

    use super::*;

    const COPY: &str = if cfg!(target_os = "macos") { "cmd-c" } else { "ctrl-c" };
    const PASTE: &str = if cfg!(target_os = "macos") { "cmd-v" } else { "ctrl-v" };

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    /// The left end of the first line below `top` that a drag selects text in (hit testing
    /// follows the pointer, so the pointer moves down until it is over text).
    fn first_selectable_line(top: Pixels, cx: &mut VisualTestContext) -> Point<Pixels> {
        (0..200)
            .map(|dy| point(px(16.5), top + px(dy as f32)))
            .find(|at| {
                cx.simulate_mouse_move(*at, None, Modifiers::none());
                cx.update(|window, cx| TextSelection::is_selectable_at(*at, window, cx))
            })
            .expect("selectable text below the buffer")
    }

    fn buffer_focused(view: &Entity<PromptDebuggerView>, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| view.read(cx).buffer.read(cx).focus_handle(cx).is_focused(window))
    }

    /// Electron: a mousedown on the output blurs the textarea, so Cmd/Ctrl+C copies the
    /// selected output. Here a field's own Copy takes the keystroke while it has focus,
    /// even with nothing selected.
    #[gpui_kit::test]
    fn a_drag_over_the_output_takes_focus_from_the_fields_so_copy_copies_it(cx: &mut TestAppContext) {
        let (debugger, window, view) = cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::install(cx);
            let settings = cx.new(|_| SettingsStore::in_memory(Settings::default()));
            let debugger =
                cx.new(|_| PromptDebugger::new(None, &Settings::default(), &serde_json::json!({}), WeakEntity::new_invalid()));
            let model = debugger.clone();
            let (window, view) = gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| PromptDebuggerView::new(model, settings, window, cx))
            })
            .expect("open a window");
            (debugger, window, view)
        });
        let cx = VisualTestContext::from_window(window, cx).into_mut();
        draw(cx);
        let buffer = cx.update(|window, _| {
            window
                .elements()
                .into_iter()
                .find(|element| element.placeholder() == Some("Paste terminal buffer lines here..."))
                .expect("the buffer field")
                .bounds()
        });

        // Type in the buffer; a test comes back while the buffer still has focus.
        cx.simulate_click(buffer.center(), Modifiers::none());
        cx.simulate_input("ls -la");
        draw(cx);
        assert!(buffer_focused(&view, cx), "a click focuses the buffer");
        cx.update(|_, cx| {
            debugger.update(cx, |d, cx| {
                d.set_model(DebugMode::Analyzer, "");
                d.test(cx);
            })
        });
        draw(cx);
        let error = "Model must be configured.";
        assert_eq!(cx.update(|_, cx| debugger.read(cx).output().error().map(str::to_owned)), Some(error.to_owned()));

        // Drag over the output, then Copy.
        let from = first_selectable_line(buffer.bottom(), cx);
        let to = point(from.x + px(400.), from.y);
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
        draw(cx);
        assert!(!buffer_focused(&view, cx), "the drag took focus from the buffer");
        cx.simulate_keystrokes(COPY);
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some(error));

        // A click in the field still focuses it, and the copied text pastes there.
        cx.simulate_click(buffer.center(), Modifiers::none());
        draw(cx);
        assert!(buffer_focused(&view, cx), "a click focuses the buffer again");
        cx.simulate_keystrokes(PASTE);
        draw(cx);
        assert_eq!(cx.update(|_, cx| debugger.read(cx).buffer().to_owned()), format!("ls -la{error}"));
    }
}
