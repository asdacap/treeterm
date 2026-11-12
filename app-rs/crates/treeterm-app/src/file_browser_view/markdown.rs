//! `MarkdownPreview`: GitHub-flavoured markdown styled as `.markdown-preview` in
//! `index.css` (16px text at 1.6 line height in 20px of padding; h1/h2 underlined; tertiary
//! code; accent links and quote bars; bordered tables).
//!
//! GPUI Kit's text view leaves a paragraph gap below each block and none above, where CSS
//! collapses `margin: 1em 0` between blocks: the 1em above the first block becomes extra
//! top padding, and a heading's `margin-top: 1.5em` becomes padding above it less the gap
//! the block before already left.

use gpui_kit::base::{TextView, TextViewStyle};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, FontWeight, HighlightStyle, Hsla, ScrollHandle, SharedString, StyleRefinement, div, px, relative, rems, rgb,
};

use crate::theme::colors;

/// The body text size (`html` default) and the 1em margin around blocks.
const BODY: f32 = 16.;
/// `.markdown-preview { padding: 20px }`.
const PADDING: f32 = 20.;
/// `code { font-family: monospace; font-size: 0.9em }`: Chromium sizes the generic
/// monospace family from 13px, not 16px.
const CODE: f32 = 13. * 0.9;
const MONO: &str = "Menlo";

/// `h1`..`h6` at 2em, 1.5em, 1.25em, then the browser's 1em, 0.83em and 0.67em.
fn heading_size(level: u8) -> f32 {
    match level {
        1 => 32.,
        2 => 24.,
        3 => 20.,
        4 => 16.,
        5 => 13.28,
        _ => 10.72,
    }
}

/// A heading: semibold at its size; 1.5em above (less the gap the block before left),
/// 0.5em below (at least the next block's 1em); h1 and h2 underlined 0.3em below the text.
fn heading(level: u8, border: Hsla) -> StyleRefinement {
    let size = heading_size(level);
    let underlined = level <= 2;
    let style = StyleRefinement::default()
        .text_size(px(size))
        .line_height(relative(1.6))
        .font_weight(FontWeight::SEMIBOLD)
        .pt(px((1.5 * size - BODY).max(0.)))
        .pb(px(if underlined { 0.3 * size } else { 0. }))
        .mb(px((0.5 * size).max(BODY)));
    if underlined { style.border_b_1().border_color(border) } else { style }
}

fn style(cx: &App) -> TextViewStyle {
    let c = colors(cx);
    let (border, tertiary, secondary, accent) = (c.border, c.bg_tertiary, c.text_secondary, c.accent);
    TextViewStyle::default()
        .with_foreground(c.text_primary)
        .with_muted_foreground(c.text_secondary)
        .with_link(c.accent)
        .with_selection(rgb(0x264f78).into())
        .with_code_background(tertiary)
        .with_border(border)
        .with_paragraph_gap(rems(BODY / 16.))
        .with_heading(move |level| heading(level, border))
        // `pre`: 13px rows (the monospace default) at 1.6, the code in them at 0.9em of 13px.
        .with_code_block(
            StyleRefinement::default()
                .bg(tertiary)
                .border_0()
                .p(px(16.))
                .rounded(px(6.))
                .font_family(MONO)
                .text_size(px(CODE))
                .line_height(px(13. * 1.6)),
        )
        .with_inline_code(HighlightStyle { background_color: Some(tertiary), ..Default::default() })
        .with_inline_code_style(
            StyleRefinement::default()
                .font_family(MONO)
                .text_size(px(CODE))
                .bg(tertiary)
                .px(px(6.))
                .py(px(2.))
                .rounded(px(4.)),
        )
        // `ul { padding-left: 2em }`: an outside marker hangs in the first 32px.
        .with_list_style(StyleRefinement::default().min_w(px(2. * BODY)).pr(px(6.)).text_color(c.text_primary))
        .with_blockquote_style(StyleRefinement::default().border_l(px(4.)).border_color(accent).pl(px(16.)).text_color(secondary))
        .with_table(StyleRefinement::default().border_1().border_color(border))
        .with_table_head(StyleRefinement::default().bg(c.bg_secondary).font_weight(FontWeight::SEMIBOLD))
        .with_table_cell(StyleRefinement::default().px(px(12.)).py(px(8.)).border_color(border))
        .with_dark(true)
}

/// Where a link click goes instead of the system browser.
pub(crate) type LinkHandler = std::sync::Arc<dyn Fn(&str, &mut gpui_kit::Window, &mut App) + Send + Sync>;

/// The preview of `text`, scrolling in its own box (with Electron's classic scrollbar).
/// Links open in the system browser unless `on_link` takes them.
pub(crate) fn preview(id: SharedString, text: String, scroll: &ScrollHandle, on_link: Option<LinkHandler>, cx: &App) -> AnyElement {
    let c = colors(cx);
    let view = TextView::markdown(id, text)
        .style(style(cx))
        // react-markdown neither highlights code blocks nor labels their language.
        .code_block_highlighter(|_| Vec::new())
        .code_block_actions(|_, _, _| div());
    let view = match on_link {
        Some(on_link) => view.on_link_click(move |href, _, window, cx| on_link(href.as_ref(), window, cx)),
        None => view,
    };
    let content = div()
        .w_full()
        .px(px(PADDING))
        .py(px(PADDING + BODY))
        .text_color(c.text_primary)
        .text_size(px(BODY))
        .line_height(relative(1.6))
        .child(view);
    div().size_full().bg(c.bg_primary).child(crate::ui::scroll_area("markdown-preview", scroll, content, cx)).into_any_element()
}
