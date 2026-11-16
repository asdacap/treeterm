//! Shiki's `dark-plus` (VS Code Dark+) as a GPUI Kit highlight theme, for the Review diff:
//! Electron's Pierre diff registers it as `treeterm-dark` (`pierre-diffs-config.ts`), while
//! the code editors use Monaco's `vs-dark` (`file_browser_view/vs_dark.rs`).
//!
//! Shiki tokenises with TextMate grammars and GPUI Kit with tree-sitter, so each capture
//! below carries the colour Dark+ gives the TextMate scope that marks the same construct:
//! function names `#DCDCAA`, variables, parameters and properties `#9CDCFE`, types
//! `#4EC9B0`, keywords and literals such as `true` `#569CD6`. Dark+ paints control keywords
//! (`if`, `return`, `import`) `#C586C0`, but the grammars' queries capture every keyword as
//! `@keyword`, so they stay `#569CD6`.

use std::sync::{Arc, LazyLock};

use gpui_kit::component::highlighter::{HighlightTheme, SyntaxColors};
use gpui_kit::{Hsla, rgb};

/// Built once: the diff asks for it every frame.
static THEME: LazyLock<Arc<HighlightTheme>> = LazyLock::new(build);

/// Dark+'s token colours and foreground. The diff paints its own background and gutter.
pub fn theme() -> Arc<HighlightTheme> {
    THEME.clone()
}

fn build() -> Arc<HighlightTheme> {
    let mut theme = (*HighlightTheme::default_dark()).clone();
    theme.name = "Dark+".into();
    theme.style.editor_foreground = Some(Hsla::from(rgb(0xd4d4d4)));
    theme.style.syntax = syntax();
    Arc::new(theme)
}

fn syntax() -> SyntaxColors {
    let colors = serde_json::json!({
        // HTML attribute names (`entity.other.attribute-name`).
        "attribute": { "color": "#9CDCFE" },
        "boolean": { "color": "#569CD6" },
        "comment": { "color": "#6A9955" },
        "comment_doc": { "color": "#6A9955" },
        // `true`, `null`, `nil` (`constant.language`).
        "constant": { "color": "#569CD6" },
        "constructor": { "color": "#4EC9B0" },
        // A template string's `${…}` (`meta.template.expression`).
        "embedded": { "color": "#D4D4D4" },
        "emphasis": { "font_style": "italic" },
        // `markup.bold`.
        "emphasis.strong": { "color": "#569CD6", "font_weight": 700 },
        "enum": { "color": "#4EC9B0" },
        "function": { "color": "#DCDCAA" },
        "keyword": { "color": "#569CD6" },
        // Rust lifetimes (`entity.name.type.lifetime`).
        "label": { "color": "#4EC9B0" },
        // Markdown: `string.other.link.title`, then `markup.underline.link`.
        "link_text": { "color": "#CE9178", "font_style": "normal" },
        "link_uri": { "color": "#D4D4D4", "font_style": "normal" },
        "number": { "color": "#B5CEA8" },
        "operator": { "color": "#D4D4D4" },
        "property": { "color": "#9CDCFE" },
        "punctuation": { "color": "#D4D4D4" },
        "punctuation.bracket": { "color": "#D4D4D4" },
        "punctuation.delimiter": { "color": "#D4D4D4" },
        // `punctuation.definition.list.begin.markdown`.
        "punctuation.list_marker": { "color": "#6796E6" },
        // A template string's `${` and `}`, a markdown heading's `#`.
        "punctuation.special": { "color": "#569CD6" },
        "string": { "color": "#CE9178" },
        "string.escape": { "color": "#D7BA7D" },
        "string.regex": { "color": "#D16969" },
        // JavaScript regular expressions.
        "string.special": { "color": "#D16969" },
        // Ruby symbols.
        "string.special.symbol": { "color": "#569CD6" },
        "tag": { "color": "#569CD6" },
        // Markdown code blocks (`markup.fenced_code.block`, `markup.raw.block`), which Dark+
        // leaves plain.
        "text.literal": { "color": "#D4D4D4" },
        // `markup.inline.raw`.
        "text.code.span": { "color": "#CE9178" },
        // `markup.heading`.
        "title": { "color": "#569CD6", "font_weight": 700 },
        "type": { "color": "#4EC9B0" },
        "variable": { "color": "#9CDCFE" },
        // `this`, `self` (`variable.language`).
        "variable.special": { "color": "#569CD6" },
        // `variable.other.enummember`.
        "variant": { "color": "#4FC1FF" },
    });
    match serde_json::from_value(colors) {
        Ok(syntax) => syntax,
        // The table above is fixed; a field GPUI Kit renamed would land here (and in the test).
        Err(error) => {
            tracing::error!(%error, "Dark+ syntax colours did not parse");
            HighlightTheme::default_dark().style.syntax.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{FontWeight, HighlightStyle};

    #[test]
    fn the_syntax_table_parses() {
        let colors = serde_json::to_value(syntax()).unwrap();
        assert_eq!(
            colors["function"]["color"],
            serde_json::to_value(Hsla::from(rgb(0xdcdcaa))).unwrap()
        );
        assert!(colors["title"]["font_weight"].is_number());
    }

    #[test]
    fn the_theme_is_built_once_with_dark_plus_foreground() {
        assert!(Arc::ptr_eq(&theme(), &theme()));
        assert_eq!(theme().style.editor_foreground, Some(rgb(0xd4d4d4).into()));
    }

    /// The style the Review diff paints the first `token` of `text` in, for a file at `path`.
    fn token_style(path: &str, text: &str, token: &str) -> Option<HighlightStyle> {
        use gpui_kit::component::highlighter::SyntaxHighlighter;
        use gpui_kit::component::input::Rope;
        let mut highlighter = SyntaxHighlighter::new(crate::file_browser_view::language_for(path));
        highlighter.update(None, &Rope::from(text), None);
        let start = text.find(token).unwrap();
        let theme = theme();
        highlighter
            .styles(&(0..text.len()), theme.as_ref())
            .into_iter()
            .find(|(range, _)| range.contains(&start))
            .map(|(_, style)| style)
    }

    fn assert_colors(path: &str, text: &str, tokens: &[(&str, u32)]) {
        for (token, hex) in tokens {
            let color = token_style(path, text, token).and_then(|style| style.color);
            assert_eq!(color, Some(rgb(*hex).into()), "{token} in {path}");
        }
    }

    /// Shiki's `dark-plus` colours for these tokens, as Electron's Review paints them.
    #[test]
    fn typescript_is_coloured_as_dark_plus() {
        let text = "// note\nimport { a } from 'b'\nexport function find(param: Shape): number {\n  const total = obj.prop + param\n  if (total) return this.size(`t${total}`, /rx+/, true)\n  return 1\n}\n";
        let tokens = [
            ("// note", 0x6a9955),
            ("'b'", 0xce9178),
            ("function", 0x569cd6),
            ("find", 0xdcdcaa),
            ("param", 0x9cdcfe),
            ("Shape", 0x4ec9b0),
            ("number", 0x4ec9b0),
            ("const", 0x569cd6),
            ("total", 0x9cdcfe),
            ("prop", 0x9cdcfe),
            ("this", 0x569cd6),
            ("size", 0xdcdcaa),
            ("${", 0x569cd6),
            ("rx+", 0xd16969),
            ("true", 0x569cd6),
            ("1", 0xb5cea8),
            // Dark+ paints control keywords #C586C0; the query captures them as `@keyword`.
            ("import", 0x569cd6),
            ("return", 0x569cd6),
        ];
        assert_colors("a.ts", text, &tokens);
        assert_colors("a.tsx", text, &tokens);
    }

    #[test]
    fn rust_is_coloured_as_dark_plus() {
        let text = "/// Docs.\nstruct Item<'life> { count: u32 }\nimpl Item<'_> {\n    fn price(&self, rate: f64) -> f64 {\n        let total = self.count as f64 * rate * 2.5;\n        println!(\"{total}\\n\");\n        total\n    }\n}\n";
        assert_colors(
            "a.rs",
            text,
            &[
                ("/// Docs.", 0x6a9955),
                ("struct", 0x569cd6),
                ("Item", 0x4ec9b0),
                ("life", 0x4ec9b0),
                ("count", 0x9cdcfe),
                ("u32", 0x4ec9b0),
                ("fn", 0x569cd6),
                ("price", 0xdcdcaa),
                ("self", 0x569cd6),
                ("rate", 0x9cdcfe),
                ("total", 0x9cdcfe),
                ("2.5", 0xb5cea8),
                ("println", 0xdcdcaa),
                ("\"{total}", 0xce9178),
                ("\\n", 0xd7ba7d),
            ],
        );
    }

    #[test]
    fn markdown_headings_are_bold() {
        let text = "# Heading\n\n- item\n";
        assert_colors(
            "a.md",
            text,
            &[("#", 0x569cd6), ("Heading", 0x569cd6), ("-", 0x6796e6)],
        );
        let heading = token_style("a.md", text, "Heading").unwrap();
        assert_eq!(heading.font_weight, Some(FontWeight::BOLD));
    }
}
