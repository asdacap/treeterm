//! The Chat tab (`Chat.tsx`); replies render as markdown.

use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, FontWeight, ScrollHandle, SharedString, Subscription, Window, div, px, relative, rgb, rgba};
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
                (ChatRole::User, _) => div().child(message.content.clone()).into_any_element(),
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
                    .child(text),
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
