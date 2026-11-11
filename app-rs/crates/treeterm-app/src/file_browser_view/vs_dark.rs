//! Monaco's `vs-dark` theme as a GPUI Kit highlight theme, so the code editors (the Files
//! tab's viewer, the Editor tab) are coloured as Electron's Monaco editors are.
//!
//! Monaco tokenises with Monarch grammars and GPUI Kit with tree-sitter, so the capture
//! names below carry the colour of the Monarch token that marks the same construct:
//! keywords and markdown headings `#569CD6`, strings and link targets `#CE9178`, numbers
//! `#B5CEA8`, comments `#6A9955`, types `#3DC9B0`, keys and attributes `#9CDCFE`;
//! identifiers, calls and punctuation keep the foreground (`#D4D4D4`).

use std::sync::Arc;

use gpui_kit::component::highlighter::{HighlightTheme, SyntaxColors};
use gpui_kit::{Hsla, rgb, rgba};

fn color(hex: u32) -> Option<Hsla> {
    Some(rgb(hex).into())
}

/// `vs-dark`'s editor colours over GPUI Kit's dark highlight theme.
pub fn theme() -> Arc<HighlightTheme> {
    let mut theme = (*HighlightTheme::default_dark()).clone();
    theme.name = "VS Dark".into();
    let style = &mut theme.style;
    style.editor_background = color(0x1e1e1e);
    style.editor_foreground = color(0xd4d4d4);
    // `editor.lineHighlightBorder`: Monaco outlines the line; GPUI Kit fills it.
    style.editor_active_line = color(0x242424);
    style.editor_line_number = color(0x858585);
    style.editor_active_line_number = color(0xc6c6c6);
    style.editor_invisible = Some(rgba(0xe3e4e229).into());
    style.editor_gutter_background = color(0x1e1e1e);
    style.syntax = syntax();
    Arc::new(theme)
}

fn syntax() -> SyntaxColors {
    let colors = serde_json::json!({
        "attribute": { "color": "#9CDCFE" },
        "boolean": { "color": "#569CD6" },
        "comment": { "color": "#6A9955" },
        "comment_doc": { "color": "#6A9955" },
        "constant": { "color": "#569CD6" },
        "constructor": { "color": "#D4D4D4" },
        "embedded": { "color": "#D4D4D4" },
        "emphasis": { "font_style": "italic" },
        "emphasis.strong": { "font_weight": 700 },
        "enum": { "color": "#3DC9B0" },
        "function": { "color": "#D4D4D4" },
        "keyword": { "color": "#569CD6" },
        "label": { "color": "#D4D4D4" },
        "link_text": { "color": "#D4D4D4", "font_style": "normal" },
        "link_uri": { "color": "#CE9178", "font_style": "normal" },
        "number": { "color": "#B5CEA8" },
        "operator": { "color": "#D4D4D4" },
        "property": { "color": "#D4D4D4" },
        "punctuation": { "color": "#D4D4D4" },
        "punctuation.bracket": { "color": "#D4D4D4" },
        "punctuation.delimiter": { "color": "#D4D4D4" },
        "punctuation.list_marker": { "color": "#D4D4D4" },
        "punctuation.special": { "color": "#D4D4D4" },
        "string": { "color": "#CE9178" },
        "string.escape": { "color": "#CE9178" },
        "string.regex": { "color": "#D16969" },
        "string.special": { "color": "#CE9178" },
        "string.special.symbol": { "color": "#CE9178" },
        "tag": { "color": "#569CD6" },
        "text.literal": { "color": "#CE9178" },
        "text.code.span": { "color": "#CE9178" },
        "title": { "color": "#569CD6", "font_weight": 400 },
        "type": { "color": "#3DC9B0" },
        "variable": { "color": "#D4D4D4" },
        "variable.special": { "color": "#569CD6" },
        "variant": { "color": "#D4D4D4" },
    });
    match serde_json::from_value(colors) {
        Ok(syntax) => syntax,
        // The table above is fixed; a field GPUI Kit renamed would land here (and in the test).
        Err(error) => {
            tracing::error!(%error, "vs-dark syntax colours did not parse");
            HighlightTheme::default_dark().style.syntax.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_syntax_table_parses() {
        let colors = serde_json::to_value(syntax()).unwrap();
        assert_eq!(colors["keyword"]["color"], serde_json::to_value(Hsla::from(rgb(0x569cd6))).unwrap());
        assert!(colors["title"]["font_weight"].is_number());
    }
}
