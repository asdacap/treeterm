//! The Chat tab (`Chat.tsx`); replies render as markdown.

use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    Context, ElementId, Entity, FontWeight, ScrollHandle, SharedString, Subscription, Window, div, px, relative,
    rgb, rgba,
};
use treeterm_model::tool_tabs::chat::{ChatModel, ChatRole, REASONING_CHOICES};

use crate::theme::colors;
use crate::ui::{FieldStyle, native_select, normal_line, pane_text_area};

pub struct ChatView {
    model: Entity<ChatModel>,
    input: Entity<TextareaState>,
    scroll: ScrollHandle,
    /// Clear the input on the next render (the model took it).
    clear_input: bool,
    /// How much text the list showed last time: more scrolls to the bottom.
    shown_len: usize,
    _subscriptions: Vec<Subscription>,
}

impl ChatView {
    pub fn new(model: Entity<ChatModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(3)
                .submit_on_enter(true)
                .placeholder("Type a message... (Enter to send, Shift+Enter for newline)")
        });
        let enter = cx.subscribe(&input, |view, _, event: &InputEvent, cx| match event {
            InputEvent::PressEnter { shift: false, .. } => view.send(cx),
            // The Send button follows what is typed.
            InputEvent::Change => cx.notify(),
            InputEvent::PressEnter { shift: true, .. } | InputEvent::Focus | InputEvent::Blur => {}
        });
        let observe = cx.observe(&model, |_, _, cx| cx.notify());
        Self {
            model,
            input,
            scroll: ScrollHandle::new(),
            clear_input: false,
            shown_len: 0,
            _subscriptions: vec![enter, observe],
        }
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).value().to_string();
        if self.model.update(cx, |m, cx| m.send(&text, cx)) {
            self.clear_input = true;
            cx.notify();
        }
    }
}

/// Plain text that can be selected and copied: your own messages and the error.
///
/// It is a Markdown text view of [`literal_markdown`], as the replies are text views, rather
/// than a `SelectableText`: the window's selection orders the text views a drag spans by where
/// they are drawn, but a `SelectableText` by a fixed number, which would copy it before (or
/// leave it out of) the replies the drag also takes.
fn plain_text(id: impl Into<ElementId>, text: &str) -> TextView {
    TextView::markdown(id, literal_markdown(text)).selectable(true)
}

/// Markdown that renders `text` as it is. ASCII punctuation is escaped, so nothing reads as
/// markup (emphasis, code, links, HTML, headings, lists, math, ...). Spaces, tabs and carriage
/// returns are character references, which Markdown keeps where it would drop or read typed
/// ones (spaces starting a line, a carriage return as a line end).
/// Each line break is a hard break; those ending the text are left out, since a hard break
/// cannot end a paragraph.
fn literal_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for c in text.trim_end_matches('\n').chars() {
        match c {
            '\n' => out.push_str("\\\n"),
            ' ' => out.push_str("&#32;"),
            '\t' => out.push_str("&#9;"),
            '\r' => out.push_str("&#13;"),
            c if c.is_ascii_punctuation() => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.clear_input) {
            self.input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        let c = colors(cx);
        let (secondary, primary, border) = (c.text_secondary, c.text_primary, c.border);
        let model = self.model.read(cx);
        let (messages, streaming, failure, reasoning) =
            (model.messages().to_vec(), model.is_streaming(), model.error().map(str::to_owned), model.reasoning());
        let endpoint = model.llm_settings(cx).map(|llm| format!("Using {} at {}", llm.model, llm.base_url));
        let shown_len: usize = messages.iter().map(|m| m.content.len() + 1).sum();
        if shown_len != self.shown_len {
            self.shown_len = shown_len;
            self.scroll.scroll_to_bottom();
        }
        let mut list = v_flex()
            .id("chat-messages")
            .track_scroll(&self.scroll)
            .flex_1()
            .min_h_0()
            .p(px(16.))
            .gap(px(12.))
            .overflow_y_scroll();
        if messages.is_empty() {
            list = list.child(
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .text_color(secondary)
                    .child(div().text_size(px(16.)).line_height(normal_line(16.)).child("Start a conversation"))
                    .children(endpoint.map(|text| {
                        div().mt(px(4.)).text_size(px(12.)).line_height(normal_line(12.)).opacity(0.7).child(text)
                    })),
            );
        }
        for message in messages {
            let user = message.role == ChatRole::User;
            let content = match (message.role, message.content.is_empty()) {
                (ChatRole::Assistant, false) => {
                    TextView::markdown(SharedString::from(format!("chat-{}", message.id)), message.content.clone())
                        .selectable(true)
                        .into_any_element()
                }
                (ChatRole::Assistant, true) if streaming => {
                    div().italic().text_color(secondary).child("Thinking...").into_any_element()
                }
                (ChatRole::Assistant, true) => div().into_any_element(),
                (ChatRole::User, _) => {
                    plain_text(SharedString::from(format!("chat-{}", message.id)), &message.content).into_any_element()
                }
            };
            list = list.child(
                v_flex()
                    .max_w(relative(0.8))
                    .gap(px(4.))
                    .when(user, |d| d.self_end())
                    .when(!user, |d| d.self_start())
                    .child(
                        div()
                            .text_size(px(11.))
                            .line_height(normal_line(11.))
                            .text_color(secondary)
                            .child(if user { "YOU" } else { "ASSISTANT" }),
                    )
                    .child(
                        div()
                            .px(px(12.))
                            .py(px(8.))
                            .rounded(px(8.))
                            .text_size(px(14.))
                            .line_height(px(21.))
                            .when(user, |d| d.bg(c.accent).text_color(gpui_kit::white()))
                            .when(!user, |d| d.bg(c.bg_secondary).text_color(primary))
                            .child(content),
                    ),
            );
        }
        if let Some(text) = failure {
            list = list.child(
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(rgba(0xff50504d))
                    .bg(rgba(0xff505026))
                    .text_size(px(13.))
                    .text_color(rgb(0xff5050))
                    .child(plain_text("chat-error", &text)),
            );
        }
        let label = REASONING_CHOICES.iter().find(|(value, _)| *value == reasoning).map_or("Off", |(_, label)| label);
        let model = self.model.clone();
        let picker = native_select("chat-reasoning", label, 70., streaming);
        let picker = if streaming {
            picker.into_any_element()
        } else {
            picker
                .dropdown_menu(move |mut menu, _, _| {
                    for (value, text) in REASONING_CHOICES {
                        let model = model.clone();
                        menu = menu.item(
                            PopupMenuItem::new(text)
                                .checked(value == reasoning)
                                .on_click(move |_, _, cx| model.update(cx, |m, cx| m.set_reasoning(value, cx))),
                        );
                    }
                    menu
                })
                .into_any_element()
        };
        let empty = self.input.read(cx).value().trim().is_empty();
        // `.chat-btn`: 13px medium, 8px 16px, rounded 6px.
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .px(px(16.))
                .py(px(8.))
                .rounded(px(6.))
                .text_size(px(13.))
                .line_height(normal_line(13.))
                .font_weight(FontWeight::MEDIUM)
                .child(label)
        };
        let action = if streaming {
            button("chat-cancel", "Cancel")
                .bg(rgba(0xff505033))
                .text_color(rgb(0xff5050))
                .cursor_pointer()
                .on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.cancel(cx))))
        } else {
            button("chat-send", "Send")
                .bg(c.accent)
                .text_color(gpui_kit::white())
                .when(empty, |d| d.opacity(0.5))
                .when(!empty, |d| d.cursor_pointer().on_click(cx.listener(|view, _, _, cx| view.send(cx))))
        };
        let style = FieldStyle {
            bg: c.bg_primary,
            border,
            radius: 6.,
            pad: (8., 12.),
            size: 14.,
            mono: false,
            height: 68.,
        };
        v_flex()
            .size_full()
            .bg(c.bg_primary)
            .child(list)
            .child(
                v_flex()
                    .flex_shrink_0()
                    .gap(px(8.))
                    .px(px(16.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(border)
                    .bg(c.bg_secondary)
                    .child(
                        h_flex()
                            .gap(px(4.))
                            .text_size(px(12.))
                            .line_height(normal_line(12.))
                            .text_color(secondary)
                            .child("Reasoning")
                            .child(picker),
                    )
                    .child(
                        h_flex()
                            .items_end()
                            .gap(px(8.))
                            .child(div().flex_1().min_w_0().when(streaming, |d| d.opacity(0.6)).child(
                                pane_text_area(&self.input, style, window, cx).disabled(streaming),
                            ))
                            .child(action),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::base::TextSelection;
    use gpui_kit::test::TestQueryExt as _;
    use gpui_kit::{
        Bounds, Modifiers, MouseButton, Pixels, Point, TestAppContext, VisualTestContext, WeakEntity, WindowOptions, point,
    };
    use serde_json::{Value, json};

    use super::*;

    const COPY: &str = if cfg!(target_os = "macos") { "cmd-c" } else { "ctrl-c" };

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    /// Where a chat tab shows, or not: Electron renders nothing for a hidden tab.
    struct Pane {
        chat: Entity<ChatView>,
        shown: bool,
    }

    impl Render for Pane {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().when(self.shown, |d| d.child(self.chat.clone()))
        }
    }

    /// A chat tab over `state` (the tab's saved `ChatState`), with no LLM and no workspace.
    fn open(state: Value, cx: &mut TestAppContext) -> (Entity<ChatModel>, Entity<Pane>, &mut VisualTestContext) {
        let (chat, window, pane) = cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::install(cx);
            let chat = cx.new(|_| ChatModel::new(None, &state, WeakEntity::new_invalid()));
            let model = chat.clone();
            let (window, pane) = gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let chat = cx.new(|cx| ChatView::new(model, window, cx));
                cx.new(|_| Pane { chat, shown: true })
            })
            .expect("open a window");
            (chat, window, pane)
        });
        let cx = VisualTestContext::from_window(window, cx).into_mut();
        draw(cx);
        (chat, pane, cx)
    }

    /// Saved messages, the user's and the assistant's in turn.
    fn conversation(texts: &[&str]) -> Value {
        let messages: Vec<Value> = texts
            .iter()
            .enumerate()
            .map(|(ix, text)| {
                let role = if ix % 2 == 0 { "user" } else { "assistant" };
                json!({ "id": format!("m{ix}"), "role": role, "content": text })
            })
            .collect();
        json!({ "messages": messages })
    }

    fn input(cx: &mut VisualTestContext) -> Bounds<Pixels> {
        cx.update(|window, _| {
            window
                .elements()
                .into_iter()
                .find(|element| element.placeholder() == Some("Type a message... (Enter to send, Shift+Enter for newline)"))
                .expect("the input")
                .bounds()
        })
    }

    /// The selectable points of the window's line at `y`, every 2px, left to right.
    fn selectable_points(y: Pixels, cx: &mut VisualTestContext) -> Vec<Point<Pixels>> {
        let width = cx.update(|window, _| window.viewport_size().width);
        (0..)
            .map(|x| point(px(x as f32 * 2.), y))
            .take_while(|at| at.x < width)
            .filter(|at| {
                cx.simulate_mouse_move(*at, None, Modifiers::none());
                cx.update(|window, cx| TextSelection::is_selectable_at(*at, window, cx))
            })
            .collect()
    }

    /// The start of the first line of text at or below `top`.
    fn first_text(top: Pixels, cx: &mut VisualTestContext) -> Point<Pixels> {
        (0..400)
            .find_map(|dy| selectable_points(top + px(dy as f32), cx).first().copied())
            .expect("text below")
    }

    /// Drag from `from` to `to`, then Cmd/Ctrl+C: what the clipboard holds.
    fn drag_and_copy(from: Point<Pixels>, to: Point<Pixels>, cx: &mut VisualTestContext) -> Option<String> {
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
        draw(cx);
        cx.simulate_keystrokes(COPY);
        cx.read_from_clipboard().and_then(|item| item.text())
    }

    /// Electron's user messages are plain `<p>` text, which a drag selects and Cmd/Ctrl+C
    /// copies, together with the replies around it in reading order, also right after typing
    /// in the input (a mousedown on the messages blurs it).
    #[gpui_kit::test]
    fn your_messages_can_be_selected_and_copied_with_the_replies(cx: &mut TestAppContext) {
        // Kept as typed: spacing, blank lines, and text that would be Markdown in a reply.
        let typed = "a  b\n\n  <b>c</b> *d* `e` # f &amp; $5 and $10 https://example.com\n- g\n1. h\tend";
        let (_, pane, cx) = open(conversation(&["first question", "**first** answer", typed]), cx);
        let field = input(cx).center();
        cx.simulate_click(field, Modifiers::none());
        cx.simulate_input("draft");
        draw(cx);
        let typed_in = cx.update(|_, cx| pane.read(cx).chat.read(cx).input.read(cx).value().to_string());
        assert_eq!(typed_in, "draft", "the input had focus");

        let first = first_text(px(0.), cx);
        let below = point(px(1.), input(cx).top() - px(20.));
        assert_eq!(
            drag_and_copy(first, below, cx).as_deref(),
            Some(format!("first question\n\nfirst answer\n\n{typed}").as_str()),
        );

        // A drag that starts in a reply and ends in a message of yours copies them in order.
        let reply = first_text(first.y + px(30.), cx);
        assert!(reply.x < first.x, "the reply is on the left, the question on the right");
        let question = first_text(reply.y + px(30.), cx);
        let copied = drag_and_copy(reply, point(question.x + px(16.), question.y), cx).expect("copied text");
        assert!(copied.starts_with("first answer\n\na"), "{copied:?}");
    }

    #[gpui_kit::test]
    fn the_error_can_be_selected_and_copied(cx: &mut TestAppContext) {
        let (chat, _, cx) = open(json!({ "messages": "not a list" }), cx);
        let error = cx.update(|_, cx| chat.read(cx).error().map(str::to_owned)).expect("an error");
        // Under "Start a conversation", which fills the list.
        let from = first_text(input(cx).top() - px(80.), cx);
        let width = cx.update(|window, _| window.viewport_size().width);
        assert_eq!(drag_and_copy(from, point(width - px(20.), from.y), cx), Some(error));
    }

    /// Markdown cannot end a paragraph on a hard break: line breaks ending the text are left
    /// out (Electron's `<p>` does not show them either).
    #[test]
    fn your_text_is_escaped_and_loses_only_its_trailing_line_breaks() {
        assert_eq!(literal_markdown("a\nb\n\n"), "a\\\nb");
        assert_eq!(literal_markdown("x y\t*"), "x&#32;y&#9;\\*");
    }
}
