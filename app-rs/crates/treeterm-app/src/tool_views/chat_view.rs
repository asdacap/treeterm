//! The Chat tab (`Chat.tsx`); replies render as markdown.

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, ScrollHandle, SharedString, Subscription, Window, div, px};
use treeterm_model::tool_tabs::chat::{ChatModel, ChatRole, REASONING_CHOICES};

use crate::theme::colors;

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
            InputEvent::PressEnter { shift: true, .. } | InputEvent::Change | InputEvent::Focus | InputEvent::Blur => {}
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
        let (secondary, error, border, user_bg, assistant_bg) =
            (c.text_secondary, c.error, c.border, c.bg_tertiary, c.bg_secondary);
        let model = self.model.read(cx);
        let (messages, streaming, failure, reasoning) =
            (model.messages().to_vec(), model.is_streaming(), model.error().map(str::to_owned), model.reasoning());
        let endpoint = model.llm_settings(cx).map(|llm| format!("Using {} at {}", llm.model, llm.base_url));
        let shown_len: usize = messages.iter().map(|m| m.content.len() + 1).sum();
        if shown_len != self.shown_len {
            self.shown_len = shown_len;
            self.scroll.scroll_to_bottom();
        }
        let mut list = v_flex().id("chat-messages").track_scroll(&self.scroll).flex_1().min_h_0().p_3().gap_3().overflow_y_scroll();
        if messages.is_empty() {
            list = list.child(
                v_flex()
                    .pt_8()
                    .gap_1()
                    .items_center()
                    .text_color(secondary)
                    .child("Start a conversation")
                    .children(endpoint.map(|text| div().text_xs().child(text))),
            );
        }
        for message in messages {
            let (who, bg) = match message.role {
                ChatRole::User => ("You", user_bg),
                ChatRole::Assistant => ("Assistant", assistant_bg),
            };
            let content = match (message.role, message.content.is_empty()) {
                (ChatRole::Assistant, false) => {
                    TextView::markdown(SharedString::from(format!("chat-{}", message.id)), message.content.clone())
                        .selectable(true)
                        .into_any_element()
                }
                (ChatRole::Assistant, true) if streaming => {
                    div().text_sm().text_color(secondary).child("Thinking...").into_any_element()
                }
                (ChatRole::Assistant, true) => div().into_any_element(),
                (ChatRole::User, _) => div()
                    .text_sm()
                    .children(message.content.lines().map(|line| div().child(line.to_owned())).collect::<Vec<_>>())
                    .into_any_element(),
            };
            list = list.child(
                v_flex()
                    .p_2()
                    .gap_1()
                    .rounded_md()
                    .bg(bg)
                    .child(div().text_xs().text_color(secondary).child(who))
                    .child(content),
            );
        }
        if let Some(text) = failure {
            list = list.child(div().text_sm().text_color(error).child(text));
        }
        let mut reasoning_row = h_flex().gap_1().items_center().text_xs().text_color(secondary).child("Reasoning");
        for (index, (value, label)) in REASONING_CHOICES.into_iter().enumerate() {
            let button = Button::new(("chat-reasoning", index)).xsmall().label(label).disabled(streaming);
            let button = if value == reasoning { button.primary() } else { button.ghost() };
            reasoning_row = reasoning_row.child(button.on_click(cx.listener(move |view, _, _, cx| {
                view.model.update(cx, |m, cx| m.set_reasoning(value, cx));
            })));
        }
        let action = if streaming {
            Button::new("chat-cancel")
                .small()
                .with_variant(ButtonVariant::Danger)
                .label("Cancel")
                .on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.cancel(cx))))
        } else {
            Button::new("chat-send").small().primary().label("Send").on_click(cx.listener(|view, _, _, cx| view.send(cx)))
        };
        v_flex()
            .size_full()
            .child(list)
            .child(
                v_flex()
                    .p_2()
                    .gap_2()
                    .border_t_1()
                    .border_color(border)
                    .child(reasoning_row)
                    .child(Textarea::new(&self.input).disabled(streaming).h(px(72.)))
                    .child(h_flex().justify_end().child(action)),
            )
    }
}
