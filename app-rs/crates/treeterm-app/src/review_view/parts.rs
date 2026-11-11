//! Small pieces of the Review tab, sized and coloured as `index.css` draws them.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Div, ElementId, Hsla, Stateful, div, px, rgb, rgba};
use treeterm_core::git::{FileChangeStats, FileChangeStatus};

use crate::theme::colors;
use crate::ui::icon;

/// `font-family: monospace` in Chromium on macOS.
pub const MONO: &str = "Menlo";
/// `.diff-stats .additions` / `.deletions`.
pub const ADDED: u32 = 0x3fb950;
pub const DELETED: u32 = 0xf85149;
/// `.review-error`, `.diff-error`.
pub const ERROR_RED: u32 = 0xf14c4c;

/// `getStatusIcon`: `.diff-status` — a 16×16 badge, white bold 10px letter on the status colour.
pub fn status_badge(status: FileChangeStatus) -> Div {
    let (letter, color) = match status {
        FileChangeStatus::Added => ("A", 0x238636),
        FileChangeStatus::Modified => ("M", 0x9e6a03),
        FileChangeStatus::Deleted => ("D", 0xb62324),
        FileChangeStatus::Renamed => ("R", 0x6e7681),
        FileChangeStatus::Untracked => ("?", 0x6e7681),
    };
    div()
        .size(px(16.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(2.))
        .bg(rgb(color))
        .text_color(gpui_kit::white())
        .text_size(px(10.))
        .font_weight(gpui_kit::FontWeight::BOLD)
        .child(letter)
}

/// `FileChangeStats`: `+N` `-N` (green, red), `±N bytes` for binaries, `new` when skipped.
pub fn change_stats(stats: FileChangeStats, cx: &App) -> Vec<AnyElement> {
    let secondary = colors(cx).text_secondary;
    match stats {
        FileChangeStats::Text { additions, deletions } => vec![
            div().text_color(rgb(ADDED)).child(format!("+{additions}")).into_any_element(),
            div().text_color(rgb(DELETED)).child(format!("-{deletions}")).into_any_element(),
        ],
        FileChangeStats::Binary { byte_change } => {
            let color: Hsla = match byte_change.signum() {
                1 => rgb(ADDED).into(),
                -1 => rgb(DELETED).into(),
                _ => secondary,
            };
            let sign = if byte_change > 0 { "+" } else { "" };
            vec![div().text_color(color).child(format!("{sign}{byte_change} bytes")).into_any_element()]
        }
        FileChangeStats::Unknown => vec![div().text_color(secondary).child("new").into_any_element()],
    }
}

/// `+N` `-N` of a directory or a summary.
pub fn totals(additions: u64, deletions: u64) -> Vec<AnyElement> {
    vec![
        div().text_color(rgb(ADDED)).child(format!("+{additions}")).into_any_element(),
        div().text_color(rgb(DELETED)).child(format!("-{deletions}")).into_any_element(),
    ]
}

/// `.review-branch` / `.diff-branch`: a monospace accent pill on a 15% accent tint.
pub fn branch_pill(cx: &App) -> Div {
    div()
        .flex_shrink_0()
        .px(px(8.))
        .py(px(2.))
        .rounded(px(4.))
        .font_family(MONO)
        .text_color(colors(cx).accent)
        .bg(rgba(0x0078d426))
}

/// `.pierre-diff-btn`: a 24×24 icon button, secondary until hovered; `active` tints it
/// accent on the hover fill; disabled fades to 30%.
pub fn toolbar_button(id: impl Into<ElementId>, name: IconName, active: bool, enabled: bool, cx: &App) -> Stateful<Div> {
    let c = colors(cx);
    let (hover, primary, accent, secondary) = (c.bg_hover, c.text_primary, c.accent, c.text_secondary);
    div()
        .id(id)
        .size(px(24.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .text_color(if active { accent } else { secondary })
        .when(active, |d| d.bg(hover))
        .when(!enabled, |d| d.opacity(0.3))
        .when(enabled, |d| d.cursor_pointer().hover(move |d| d.bg(hover).text_color(if active { accent } else { primary })))
        .child(plain_icon(name, 14.))
}

/// A Lucide icon in the surrounding text colour (so a parent's hover recolours it).
pub fn plain_icon(name: IconName, size: f32) -> gpui_kit::component::Icon {
    gpui_kit::component::Icon::new(name).size(px(size))
}

/// A bordered text button: `.diff-file-action`, `.file-diff-stage-btn`, `.diff-action-btn`.
/// Secondary text on transparent; hover fills and brightens.
pub fn outline_button(id: impl Into<ElementId>, font: f32, pad: (f32, f32), radius: f32, enabled: bool, cx: &App) -> Stateful<Div> {
    let c = colors(cx);
    let (hover, primary) = (c.bg_hover, c.text_primary);
    div()
        .id(id)
        .flex_shrink_0()
        .px(px(pad.1))
        .py(px(pad.0))
        .border_1()
        .border_color(c.border)
        .rounded(px(radius))
        .text_size(px(font))
        // Chromium's `normal` line height for the UI font.
        .line_height(px((font * 1.2).round()))
        .text_color(c.text_secondary)
        .whitespace_nowrap()
        .when(!enabled, |d| d.opacity(0.5))
        .when(enabled, |d| d.cursor_pointer().hover(move |d| d.bg(hover).text_color(primary)))
}

/// `.review-action-btn`: 4px 12px, 12px text, rounded 4, no border; `bg` with `hover`.
pub fn action_button(id: impl Into<ElementId>, bg: Hsla, hover: Hsla, fg: Hsla, enabled: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex_shrink_0()
        .flex()
        .items_center()
        .px(px(12.))
        .py(px(4.))
        .rounded(px(4.))
        .text_size(px(12.))
        .text_color(fg)
        .bg(bg)
        .whitespace_nowrap()
        .when(!enabled, |d| d.opacity(0.5))
        .when(enabled, |d| d.cursor_pointer().hover(move |d| d.bg(hover)))
}

/// A Chromium checkbox (13×13, rounded 2px): white with a grey border, or filled with the
/// accent and a white tick when checked.
pub fn checkbox(checked: bool, cx: &App) -> Div {
    let accent = colors(cx).accent;
    div()
        .size(px(13.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(2.))
        .when(checked, |d| d.bg(accent).child(icon(IconName::Check, 11., gpui_kit::white())))
        .when(!checked, |d| d.bg(gpui_kit::white()).border_1().border_color(rgb(0x767676)))
}

/// A text `<input>`: a `height`-tall bordered box (`pad` left and right) on the primary
/// background, the GPUI Kit input inside it bare and centred.
pub fn framed_input(
    input: &gpui_kit::Entity<gpui_kit::component::input::InputState>,
    height: f32,
    pad: (f32, f32),
    font: f32,
    radius: f32,
    border: impl Into<Hsla>,
    cx: &App,
) -> Div {
    div()
        .h(px(height))
        .flex()
        .items_center()
        .pl(px(pad.0))
        .pr(px(pad.1))
        .bg(colors(cx).bg_primary)
        .border_1()
        .border_color(border.into())
        .rounded(px(radius))
        .text_size(px(font))
        .child(
            gpui_kit::component::input::Input::new(input)
                .appearance(false)
                .w_full()
                .h_full()
                .px(px(0.))
                .py(px(0.))
                .text_size(px(font)),
        )
}

/// Chromium's focus ring on a box with a 1px border: 2px of macOS orange over the border
/// edge, a 1px white halo outside it.
pub fn focus_ring(input: Div, radius: f32) -> Div {
    input
        .relative()
        .border_color(rgb(0xe59700))
        .child(
            div()
                .absolute()
                .top(px(-2.))
                .left(px(-2.))
                .right(px(-2.))
                .bottom(px(-2.))
                .rounded(px(radius + 2.))
                .border_1()
                .border_color(gpui_kit::white())
                .child(
                    div()
                        .absolute()
                        .top(px(0.))
                        .left(px(0.))
                        .right(px(0.))
                        .bottom(px(0.))
                        .rounded(px(radius + 1.))
                        .border_1()
                        .border_color(rgb(0xe59700)),
                ),
        )
}

/// A spinning `LoaderCircle` (`.spinning`).
pub fn spinner(size: f32, color: impl Into<Hsla>) -> AnyElement {
    use gpui_kit::component::Sizable as _;
    gpui_kit::component::spinner::Spinner::new()
        .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
        .with_size(px(size))
        .color(color.into())
        .into_any_element()
}
