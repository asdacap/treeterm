//! The Comments tab (`CommentsList.tsx`).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{ClipboardItem, Context, Entity, SharedString, Subscription, Window, div, rgb};
use treeterm_core::review::{Freshness, Resolution};
use treeterm_model::session_store::SessionStore;
use treeterm_model::tool_tabs::comments::{CommentsModel, PushState, created_at};

use crate::theme::colors;

pub struct CommentsView {
    model: Entity<CommentsModel>,
    /// For the workspace's git controller ("Push to GitHub").
    session: Entity<SessionStore>,
    workspace_id: String,
    _observe: Subscription,
}

impl CommentsView {
    pub fn new(model: Entity<CommentsModel>, session: Entity<SessionStore>, workspace_id: String, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&model, |_, _, cx| cx.notify());
        Self { model, session, workspace_id, _observe: observe }
    }
}

impl Render for CommentsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (secondary, error, border, card, accent, code_bg) =
            (c.text_secondary, c.error, c.border, c.bg_secondary, c.accent, c.bg_primary);
        let root = v_flex().id("comments-tab").size_full().p_3().gap_3().overflow_y_scroll();
        let model = self.model.read(cx);
        let comments = match model.comments(cx) {
            Ok(comments) => comments,
            Err(message) => return root.child(div().text_sm().text_color(error).child(message.to_string())),
        };
        if comments.is_empty() {
            return root.child(div().p_4().text_color(secondary).child("No review comments yet"));
        }
        let filter = model.filter();
        let (push, failure, prompt, expanded) =
            (model.push_state().clone(), model.error().map(str::to_owned), model.prompt(cx), model.prompt_expanded());
        let unprompted = comments.iter().filter(|c| c.resolution == Resolution::Open).count();
        let prompted = comments.len() - unprompted;
        let shown: Vec<_> = comments.iter().filter(|c| c.resolution == filter).cloned().collect();
        let contexts: Vec<_> = shown.iter().map(|comment| model.code_context(comment)).collect();

        let tab = |id: &'static str, label: String, target: Resolution, cx: &mut Context<Self>| {
            let button = Button::new(id).small().label(label);
            let button = if filter == target { button.primary() } else { button.ghost() };
            button.on_click(cx.listener(move |view, _, _, cx| view.model.update(cx, |m, cx| m.set_filter(target, cx))))
        };
        let pushing = push == PushState::Pushing;
        let header = h_flex()
            .gap_1()
            .items_center()
            .child(tab("comments-unprompted", format!("Unprompted ({unprompted})"), Resolution::Open, cx))
            .child(tab("comments-prompted", format!("Prompted ({prompted})"), Resolution::Addressed, cx))
            .child(div().flex_1())
            .child(
                Button::new("comments-push")
                    .small()
                    .outline()
                    .disabled(unprompted == 0 || pushing)
                    .label(if pushing { "Pushing…".to_owned() } else { format!("Push to GitHub ({unprompted})") })
                    .on_click(cx.listener(|view, _, _, cx| {
                        let controller = view.session.read(cx).git_controller(&view.workspace_id).cloned();
                        view.model.update(cx, |m, cx| m.push_to_github(controller, cx));
                    })),
            );
        let mut cards = v_flex().gap_2();
        if shown.is_empty() {
            let which = match filter {
                Resolution::Open => "unprompted",
                Resolution::Addressed => "prompted",
            };
            cards = cards.child(div().text_sm().text_color(secondary).child(format!("No {which} comments")));
        }
        for (comment, context) in shown.into_iter().zip(contexts) {
            let addressed = comment.resolution == Resolution::Addressed;
            let (delete_id, prompt_id) = (comment.id.clone(), comment.id.clone());
            let goto = comment.clone();
            let mut card = v_flex()
                .p_2()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_color(border)
                .bg(card)
                .when(addressed || comment.freshness == Freshness::Outdated, |d| d.opacity(0.75))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .text_xs()
                        .child(div().flex_1().truncate().text_color(accent).child(comment.file_path.clone()))
                        .child(div().text_color(secondary).child(format!("L{} ({})", comment.line_number, comment.side.as_str())))
                        .when(comment.freshness == Freshness::Outdated, |d| {
                            d.child(div().px_1().rounded_sm().bg(rgb(0x5a4a1a)).child("Outdated"))
                        })
                        .child(
                            Button::new(SharedString::from(format!("comment-delete-{}", comment.id)))
                                .xsmall()
                                .ghost()
                                .label("×")
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.model.update(cx, |m, cx| m.delete(&delete_id, cx));
                                })),
                        ),
                );
            if let Some(lines) = context {
                let mut code = v_flex().p_1().rounded_sm().bg(code_bg).font_family("Menlo").text_xs();
                for line in lines {
                    code = code.child(
                        h_flex()
                            .gap_2()
                            .when(line.target, |d| d.bg(rgb(0x264f78)))
                            .child(div().w(gpui_kit::px(36.)).text_color(secondary).child(line.number.to_string()))
                            .child(div().child(line.text)),
                    );
                }
                card = card.child(code);
            }
            card = card.child(div().text_sm().child(comment.text.clone())).child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .text_xs()
                    .child(div().flex_1().text_color(secondary).child(created_at(&comment)))
                    .child(
                        Button::new(SharedString::from(format!("comment-prompt-{}", comment.id)))
                            .xsmall()
                            .ghost()
                            .label(if addressed { "Re-prompt" } else { "Prompt" })
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.model.update(cx, |m, cx| m.prompt_comment(&prompt_id, cx));
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("comment-goto-{}", comment.id)))
                            .xsmall()
                            .ghost()
                            .label("Go to file")
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.model.update(cx, |m, cx| m.go_to_file(&goto, cx));
                            })),
                    ),
            );
            cards = cards.child(card);
        }
        let prompt_panel = (!prompt.is_empty()).then(|| {
            let copy = prompt.clone();
            v_flex()
                .gap_1()
                .border_t_1()
                .border_color(border)
                .pt_2()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .id("comments-prompt-toggle")
                                .flex_1()
                                .text_sm()
                                .cursor_pointer()
                                .child(format!("{} Generated Prompt", if expanded { "▼" } else { "▶" }))
                                .on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.toggle_prompt(cx)))),
                        )
                        .child(Button::new("comments-copy").xsmall().outline().label("Copy to Clipboard").on_click(
                            move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone())),
                        )),
                )
                .when(expanded, |d| {
                    d.child(
                        div()
                            .p_2()
                            .rounded_sm()
                            .bg(code_bg)
                            .font_family("Menlo")
                            .text_xs()
                            .children(prompt.lines().map(|line| div().child(line.to_owned())).collect::<Vec<_>>()),
                    )
                })
        });
        root.child(header)
            .when_some(failure, |d, text| d.child(div().text_sm().text_color(error).child(text)))
            .when_some(
                match push {
                    PushState::Done(message) => Some(message),
                    PushState::Idle | PushState::Pushing => None,
                },
                |d, message| d.child(div().text_sm().text_color(secondary).child(message)),
            )
            .child(cards)
            .children(prompt_panel)
    }
}
