//! Review comments as `CommentDisplay.tsx` and `CommentInput.tsx` draw them, shared by the
//! Review tab's diff and the Files tab's viewer. What a button does is the caller's.

use std::rc::Rc;

use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Entity, Focusable as _, FontWeight, SharedString, Window, div, px, rgb, rgba};
use treeterm_core::review::{Freshness, ReviewComment};
use treeterm_model::review::GitOp;

use crate::theme::colors;

/// Acts on a comment (by id) or on the comment box's text.
pub type TextAction = Rc<dyn Fn(&str, &mut App)>;
/// Scrolls the viewer to a 1-based line.
pub type LineAction = Rc<dyn Fn(u32, &mut App)>;

/// What the comment buttons do.
#[derive(Clone)]
pub struct CommentActions {
    pub delete: TextAction,
    pub cancel: Rc<dyn Fn(&mut App)>,
    pub submit: TextAction,
}

/// Where a comment is shown.
#[derive(Clone)]
pub enum Placement {
    /// Under its line in a diff (`.pierre-diff-annotation`, `hideLineRef`): a card.
    Diff,
    /// With its line's other comments in the file viewer (`.inline-comment-display-group`):
    /// compact, headed by its line, which `reveal` scrolls the viewer to.
    Viewer { reveal: LineAction },
}

const OUTDATED: u32 = 0xf0a000;
const ERROR_RED: u32 = 0xf85149;

/// `.comment-delete-btn`.
fn delete_button(id: &str, actions: &CommentActions, cx: &App) -> AnyElement {
    let c = colors(cx);
    let (secondary, hover) = (c.text_secondary, c.bg_hover);
    let ui_font = gpui_kit::component::ActiveTheme::theme(cx).font_family.clone();
    let (delete, owned) = (actions.delete.clone(), id.to_owned());
    div()
        .id(SharedString::from(format!("delete-comment-{id}")))
        .font_family(ui_font)
        .size(px(20.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .text_size(px(18.))
        .text_color(secondary)
        .cursor_pointer()
        .hover(move |d| d.bg(hover).text_color(rgb(0xf14c4c)))
        .child("×")
        .on_click(move |_, _, cx| delete(&owned, cx))
        .into_any_element()
}

/// `CommentDisplay`: the text, its time, and × to delete. The header carries the line
/// (`Placement::Viewer`) and the "Outdated" badge; with neither, × sits beside the text.
pub fn comment_display(comment: &ReviewComment, placement: &Placement, actions: &CommentActions, cx: &App) -> AnyElement {
    let c = colors(cx);
    let (secondary, primary, tertiary, border, accent) = (c.text_secondary, c.text_primary, c.bg_tertiary, c.border, c.accent);
    let outdated = comment.freshness == Freshness::Outdated;
    let reveal = match placement {
        Placement::Diff => None,
        Placement::Viewer { reveal } => Some(reveal.clone()),
    };
    let header = reveal.is_some() || outdated;
    let card = match placement {
        // `.comment-display` (`.outdated`).
        Placement::Diff => v_flex()
            .mb(px(8.))
            .p(px(12.))
            .rounded(px(4.))
            .border_1()
            .border_color(if outdated { rgb(OUTDATED).into() } else { border })
            .bg(if outdated { rgba(0xf0a0000d).into() } else { tertiary }),
        // `.inline-comment-display-group .comment-display`: no border, 4px 8px.
        Placement::Viewer { .. } => v_flex().px(px(8.)).py(px(4.)).when(outdated, |d| d.bg(rgba(0xf0a0000d))),
    };
    let gap = match placement {
        Placement::Diff => px(8.),
        Placement::Viewer { .. } => px(2.),
    };
    let line = comment.line_number;
    card.when(header, |d| {
        d.child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .mb(gap)
                .text_size(px(12.))
                .when_some(reveal, |d, reveal| {
                    // `.comment-line-ref`.
                    d.child(
                        div()
                            .id(SharedString::from(format!("comment-line-{}", comment.id)))
                            .font_family(crate::ui::MONO)
                            .text_color(accent)
                            .cursor_pointer()
                            .hover(|d| d.underline())
                            .child(format!("Line {} ({})", comment.line_number, comment.side.as_str()))
                            .on_click(move |_, _, cx| reveal(line, cx)),
                    )
                })
                .when(outdated, |d| {
                    // `.comment-outdated-badge`.
                    d.child(
                        div()
                            .px(px(6.))
                            .py(px(2.))
                            .rounded(px(3.))
                            .bg(rgb(OUTDATED))
                            .text_color(rgb(0x1e1e1e))
                            .text_size(px(10.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("OUTDATED"),
                    )
                })
                .child(div().ml_auto().child(delete_button(&comment.id, actions, cx))),
        )
    })
    .child(
        h_flex()
            .items_start()
            .mb(gap)
            .text_size(px(13.))
            .line_height(gpui_kit::relative(1.5))
            .text_color(primary)
            .child(div().flex_1().min_w_0().child(comment.text.clone()))
            .when(!header, |d| d.child(div().ml(px(8.)).child(delete_button(&comment.id, actions, cx)))),
    )
    .child(div().text_size(px(11.)).text_color(secondary).child(treeterm_core::clock::locale_date_time(comment.created_at)))
    .into_any_element()
}

/// `CommentInput`: "Comment on line N", the text (Cmd/Ctrl+Enter submits, Esc cancels),
/// Cancel and Add Comment, and why the last save failed.
pub fn comment_input(
    input: &Entity<TextareaState>,
    has_text: bool,
    label: String,
    adding: &GitOp,
    actions: &CommentActions,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let c = colors(cx);
    let (secondary, primary, tertiary, border, hover, accent, bg_primary) =
        (c.text_secondary, c.text_primary, c.bg_tertiary, c.border, c.bg_hover, c.accent, c.bg_primary);
    let ui_font = gpui_kit::component::ActiveTheme::theme(cx).font_family.clone();
    let saving = *adding == GitOp::Running;
    let enabled = has_text && !saving;
    let focused = input.read(cx).focus_handle(cx).is_focused(window);
    let (on_escape, on_cancel, on_submit, text) =
        (actions.cancel.clone(), actions.cancel.clone(), actions.submit.clone(), input.clone());
    v_flex()
        .child(div().mb(px(8.)).text_size(px(13.)).text_color(secondary).child(label))
        .child(
            div()
                .on_key_down(move |event: &gpui_kit::KeyDownEvent, _, cx| {
                    if event.keystroke.key == "escape" {
                        on_escape(cx);
                    }
                })
                .child(
                    Textarea::new(input)
                        .appearance(false)
                        .w_full()
                        .min_h(px(60.))
                        .px(px(12.))
                        .py(px(8.))
                        .text_size(px(13.))
                        .bg(bg_primary)
                        .border_1()
                        .border_color(if focused { accent } else { border })
                        .rounded(px(4.)),
                ),
        )
        .child(
            h_flex()
                .gap(px(8.))
                .mt(px(12.))
                .child(
                    div()
                        .id("comment-cancel")
                        .font_family(ui_font.clone())
                        .px(px(16.))
                        .py(px(6.))
                        .rounded(px(4.))
                        .text_size(px(13.))
                        .bg(tertiary)
                        .text_color(primary)
                        .cursor_pointer()
                        .hover(move |d| d.bg(hover))
                        .child("Cancel")
                        .on_click(move |_, _, cx| on_cancel(cx)),
                )
                .child(
                    div()
                        .id("comment-submit")
                        .font_family(ui_font)
                        .px(px(16.))
                        .py(px(6.))
                        .rounded(px(4.))
                        .text_size(px(13.))
                        .bg(accent)
                        .text_color(gpui_kit::white())
                        .when(!enabled, |d| d.opacity(0.5))
                        .when(enabled, |d| d.cursor_pointer().hover(|d| d.bg(rgb(0x006cbf))))
                        .child("Add Comment")
                        .when(enabled, |d| {
                            d.on_click(move |_, _, cx| {
                                let value = text.read(cx).value().to_string();
                                on_submit(&value, cx);
                            })
                        }),
                ),
        )
        .when_some(
            match adding {
                GitOp::Failed(message) => Some(message.clone()),
                GitOp::Idle | GitOp::Running => None,
            },
            |d, message| d.child(div().mt(px(8.)).text_size(px(12.)).text_color(rgb(ERROR_RED)).child(message)),
        )
        .into_any_element()
}
