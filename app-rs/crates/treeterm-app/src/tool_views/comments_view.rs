//! The Comments tab (`CommentsList.tsx`).
//!
//! Several of its CSS rules use `var(--border)`, which is not defined: those borders are not
//! drawn in Electron, so they are not drawn here either.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{ClipboardItem, Context, Entity, FontWeight, Hsla, SharedString, Subscription, Window, div, px, rgb, rgba};
use treeterm_core::review::{Freshness, Resolution};
use treeterm_model::session_store::SessionStore;
use treeterm_model::tool_tabs::comments::{CommentsModel, PushState, created_at};

use crate::theme::colors;
use crate::ui::{MONO, normal_line};

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

/// The tab's borderless text buttons (`.comments-push-btn`, `.comments-reprompt-btn`, …):
/// secondary, filled and primary on hover.
fn flat_button(id: impl Into<gpui_kit::ElementId>, label: impl Into<SharedString>, size: f32, pad: (f32, f32), cx: &gpui_kit::App) -> gpui_kit::Stateful<gpui_kit::Div> {
    let c = colors(cx);
    let (hover, primary) = (c.bg_hover, c.text_primary);
    div()
        .id(id)
        .flex_shrink_0()
        .px(px(pad.1))
        .py(px(pad.0))
        .rounded(px(3.))
        .text_size(px(size))
        .line_height(normal_line(size))
        .font_weight(FontWeight::NORMAL)
        .text_color(c.text_secondary)
        .cursor_pointer()
        .hover(move |d| d.bg(hover).text_color(primary))
        .child(label.into())
}

impl Render for CommentsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (error, tertiary, card_code, hover_bg) = (c.error, c.bg_tertiary, c.bg_secondary, c.bg_hover);
        let (secondary, primary, accent, border) = (c.text_secondary, c.text_primary, c.accent, c.border);
        let root = v_flex().id("comments-tab").size_full().p(px(8.)).overflow_y_scroll();
        let empty = |text: String| {
            div()
                .p(px(24.))
                .flex()
                .justify_center()
                .text_size(px(13.))
                .line_height(normal_line(13.))
                .text_color(secondary)
                .child(text)
        };
        let model = self.model.read(cx);
        let comments = match model.comments(cx) {
            Ok(comments) => comments,
            Err(message) => return root.child(empty(message.to_string()).text_color(error)),
        };
        if comments.is_empty() {
            return root.child(empty("No review comments yet".to_owned()));
        }
        let filter = model.filter();
        let (push, failure, prompt, expanded) =
            (model.push_state().clone(), model.error().map(str::to_owned), model.prompt(cx), model.prompt_expanded());
        let unprompted = comments.iter().filter(|c| c.resolution == Resolution::Open).count();
        let prompted = comments.len() - unprompted;
        let shown: Vec<_> = comments.iter().filter(|c| c.resolution == filter).cloned().collect();
        let contexts: Vec<_> = shown.iter().map(|comment| model.code_context(comment)).collect();

        let tab = |id: &'static str, label: String, target: Resolution, cx: &mut Context<Self>| {
            let active = filter == target;
            flat_button(id, label, 12., (3., 10.), cx)
                .font_weight(FontWeight::MEDIUM)
                .when(active, |d| d.bg(accent).text_color(primary))
                .on_click(cx.listener(move |view, _, _, cx| view.model.update(cx, |m, cx| m.set_filter(target, cx))))
        };
        let pushing = push == PushState::Pushing;
        let can_push = unprompted > 0 && !pushing;
        let header = h_flex()
            .justify_between()
            .gap(px(8.))
            .px(px(4.))
            .py(px(8.))
            .mb(px(4.))
            .child(
                h_flex()
                    .gap(px(4.))
                    .child(tab("comments-unprompted", format!("Unprompted ({unprompted})"), Resolution::Open, cx))
                    .child(tab("comments-prompted", format!("Prompted ({prompted})"), Resolution::Addressed, cx)),
            )
            .child(
                flat_button(
                    "comments-push",
                    if pushing { "Pushing…".to_owned() } else { format!("Push to GitHub ({unprompted})") },
                    12.,
                    (3., 10.),
                    cx,
                )
                .when(!can_push, |d| d.opacity(0.5).cursor_default())
                .tooltip(|window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new("Push unprompted comments to GitHub as inline PR review comments")
                        .build(window, cx)
                })
                .when(can_push, |d| {
                    d.on_click(cx.listener(|view, _, _, cx| {
                        let controller = view.session.read(cx).git_controller(&view.workspace_id).cloned();
                        view.model.update(cx, |m, cx| m.push_to_github(controller, cx));
                    }))
                }),
            );
        let status = |text: String, color: Hsla| {
            div().px(px(8.)).py(px(4.)).mb(px(4.)).text_size(px(12.)).line_height(normal_line(12.)).text_color(color).child(text)
        };
        let mut cards = v_flex().gap(px(8.));
        if shown.is_empty() {
            let which = match filter {
                Resolution::Open => "unprompted",
                Resolution::Addressed => "prompted",
            };
            cards = cards.child(empty(format!("No {which} comments")));
        }
        for (comment, context) in shown.into_iter().zip(contexts) {
            let addressed = comment.resolution == Resolution::Addressed;
            let outdated = comment.freshness == Freshness::Outdated;
            let (delete_id, prompt_id) = (comment.id.clone(), comment.id.clone());
            let goto = comment.clone();
            let mut card = v_flex()
                .p(px(12.))
                .rounded(px(4.))
                .border_1()
                .border_color(if outdated { rgb(0xf0a000).into() } else { border })
                .bg(if outdated { rgba(0xf0a0000d).into() } else { tertiary })
                .when(addressed, |d| d.opacity(0.7))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .mb(px(8.))
                        .text_size(px(12.))
                        .line_height(normal_line(12.))
                        .child(div().flex_1().min_w_0().truncate().font_family(MONO).text_color(primary).child(comment.file_path.clone()))
                        .child(
                            div()
                                .font_family(MONO)
                                .text_color(accent)
                                .child(format!("L{} ({})", comment.line_number, comment.side.as_str())),
                        )
                        .when(outdated, |d| {
                            d.child(
                                div()
                                    .px(px(6.))
                                    .py(px(2.))
                                    .rounded(px(3.))
                                    .bg(rgb(0xf0a000))
                                    .text_size(px(10.))
                                    .line_height(normal_line(10.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0x1e1e1e))
                                    .child("OUTDATED"),
                            )
                        })
                        .child({
                            let hover = hover_bg;
                            div()
                                .id(SharedString::from(format!("comment-delete-{}", comment.id)))
                                .ml_auto()
                                .size(px(20.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .text_size(px(18.))
                                .text_color(secondary)
                                .cursor_pointer()
                                .hover(move |d| d.bg(hover).text_color(rgb(0xf14c4c)))
                                .child("×")
                                .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Delete comment").build(window, cx))
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.model.update(cx, |m, cx| m.delete(&delete_id, cx));
                                }))
                        }),
                );
            if let Some(lines) = context {
                let mut code = v_flex()
                    .py(px(4.))
                    .mb(px(8.))
                    .rounded(px(4.))
                    .bg(card_code)
                    .font_family(MONO)
                    .text_size(px(12.))
                    .line_height(px(18.));
                for line in lines {
                    code = code.child(
                        h_flex()
                            .px(px(8.))
                            .when(line.target, |d| d.bg(rgba(0xffff001a)))
                            .child(
                                div()
                                    .min_w(px(40.))
                                    .pr(px(12.))
                                    .flex()
                                    .justify_end()
                                    .flex_shrink_0()
                                    .text_color(secondary)
                                    .child(line.number.to_string()),
                            )
                            .child(div().whitespace_nowrap().child(line.text)),
                    );
                }
                card = card.child(code);
            }
            card = card
                .child(div().mb(px(8.)).text_size(px(13.)).line_height(px(19.5)).text_color(primary).child(comment.text.clone()))
                .child(
                    h_flex()
                        .gap(px(8.))
                        .text_size(px(11.))
                        .line_height(normal_line(11.))
                        .child(div().flex_1().min_w_0().text_color(secondary).child(created_at(&comment)))
                        .child(
                            flat_button(
                                SharedString::from(format!("comment-prompt-{}", comment.id)),
                                if addressed { "Re-prompt" } else { "Prompt" },
                                11.,
                                (3., 8.),
                                cx,
                            )
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.model.update(cx, |m, cx| m.prompt_comment(&prompt_id, cx));
                            })),
                        )
                        .child(
                            flat_button(SharedString::from(format!("comment-goto-{}", comment.id)), "Go to file", 11., (3., 8.), cx)
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
                .mt(px(12.))
                .rounded(px(4.))
                .bg(tertiary)
                .overflow_hidden()
                .child(
                    h_flex()
                        .id("comments-prompt-toggle")
                        .justify_between()
                        .px(px(12.))
                        .py(px(8.))
                        .border_b_1()
                        // `border-bottom-color: var(--border)` is invalid, so an open preview's
                        // rule takes the text colour.
                        .border_color(if expanded { secondary } else { gpui_kit::transparent_black() })
                        .text_size(px(12.))
                        .line_height(normal_line(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(secondary)
                        .cursor_pointer()
                        .hover(move |d| {
                            let d = d.text_color(primary);
                            if expanded { d.border_color(primary) } else { d }
                        })
                        .child(format!("{} Generated Prompt", if expanded { "▼" } else { "▶" }))
                        .child(
                            flat_button("comments-copy", "Copy to Clipboard", 11., (2., 8.), cx)
                                .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Copy to clipboard").build(window, cx))
                                .on_click(move |_, _, cx| {
                                    cx.stop_propagation();
                                    cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                                }),
                        )
                        .on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.toggle_prompt(cx)))),
                )
                .when(expanded, |d| {
                    d.child(
                        div()
                            .id("comments-prompt")
                            .max_h(px(300.))
                            .overflow_y_scroll()
                            .p(px(12.))
                            .font_family(MONO)
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .text_color(secondary)
                            .child(prompt),
                    )
                })
        });
        root.child(header)
            .when_some(failure, |d, text| d.child(status(text, error)))
            .when_some(
                match push {
                    PushState::Done(message) => Some(message),
                    PushState::Idle | PushState::Pushing => None,
                },
                |d, message| d.child(status(message, secondary)),
            )
            .child(cards)
            .children(prompt_panel)
    }
}
