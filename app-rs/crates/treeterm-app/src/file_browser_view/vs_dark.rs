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

    /// The colour the viewer paints the first `token` of `text` in, for a file at `path`.
    fn token_color(path: &str, text: &str, token: &str) -> Option<Hsla> {
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
            .and_then(|(_, style)| style.color)
    }

    #[test]
    fn tsx_is_coloured_as_typescript() {
        let text = "// note\nimport { a } from 'b'\nexport function f(): number {\n  const s = \"x\"\n  return <div className=\"c\">{s}</div>\n}\n";
        let ts = text.replace("<div className=\"c\">{s}</div>", "1");
        for (token, hex) in [("// note", 0x6a9955), ("import", 0x569cd6), ("'b'", 0xce9178), ("const", 0x569cd6), ("number", 0x3dc9b0)] {
            let color = Some(rgb(hex).into());
            assert_eq!(token_color("a.ts", &ts, token), color, "{token} in .ts");
            assert_eq!(token_color("a.tsx", text, token), color, "{token} in .tsx");
        }
    }

    /// Every language the viewer maps paints a comment, a keyword and a string as `vs-dark`
    /// does. GPUI Kit shipped C#, Swift, Protobuf, GraphQL and CMake without a highlight
    /// query, C++ with only its additions to C's, and Scala's comments uncoloured.
    #[test]
    fn every_language_colours_comments_keywords_and_strings() {
        const COMMENT: u32 = 0x6a9955;
        const KEYWORD: u32 = 0x569cd6;
        const STRING: u32 = 0xce9178;
        /// A file's path and text, and tokens in it with the colour each is painted in.
        type Case<'a> = (&'a str, &'a str, &'a [(&'a str, u32)]);
        let cases: &[Case] = &[
            ("a.rs", "// c1\nfn f() { let s = \"x1\"; }\n", &[("// c1", COMMENT), ("fn", KEYWORD), ("\"x1\"", STRING)]),
            ("a.ts", "// c1\nconst s = \"x1\"\n", &[("// c1", COMMENT), ("const", KEYWORD), ("\"x1\"", STRING)]),
            ("a.tsx", "// c1\nconst s = \"x1\"\n", &[("// c1", COMMENT), ("const", KEYWORD), ("\"x1\"", STRING)]),
            ("a.js", "// c1\nconst s = \"x1\"\n", &[("// c1", COMMENT), ("const", KEYWORD), ("\"x1\"", STRING)]),
            ("a.json", "{\"k\": \"x1\"}\n", &[("\"x1\"", STRING)]),
            ("a.toml", "# c1\nk = \"x1\"\n", &[("# c1", COMMENT), ("\"x1\"", STRING)]),
            ("a.yaml", "# c1\nk: \"x1\"\n", &[("# c1", COMMENT), ("\"x1\"", STRING)]),
            ("a.py", "# c1\ndef f():\n    return \"x1\"\n", &[("# c1", COMMENT), ("def", KEYWORD), ("\"x1\"", STRING)]),
            ("a.go", "// c1\npackage main\nfunc f() string { return \"x1\" }\n", &[("// c1", COMMENT), ("func", KEYWORD), ("\"x1\"", STRING)]),
            ("a.sh", "# c1\nif true; then echo \"x1\"; fi\n", &[("# c1", COMMENT), ("then", KEYWORD), ("\"x1\"", STRING)]),
            ("a.css", "/* c1 */\na { content: \"x1\"; }\n", &[("/* c1 */", COMMENT), ("\"x1\"", STRING)]),
            ("a.html", "<!-- c1 -->\n<div></div>\n", &[("<!-- c1 -->", COMMENT), ("div", KEYWORD)]),
            ("a.c", "// c1\nint f() { return \"x1\"; }\n", &[("// c1", COMMENT), ("return", KEYWORD), ("\"x1\"", STRING)]),
            // `namespace` is C++'s rule, the rest C's.
            ("a.cpp", "// c1\nnamespace n { int f() { return \"x1\"; } }\n", &[("// c1", COMMENT), ("namespace", KEYWORD), ("return", KEYWORD), ("\"x1\"", STRING)]),
            ("a.cs", "// c1\nclass A { string s = \"x1\"; }\n", &[("// c1", COMMENT), ("class", KEYWORD), ("\"x1\"", STRING)]),
            ("a.java", "// c1\nclass A { String s = \"x1\"; }\n", &[("// c1", COMMENT), ("class", KEYWORD), ("\"x1\"", STRING)]),
            ("a.kt", "// c1\nfun f() { val s = \"x1\" }\n", &[("// c1", COMMENT), ("val", KEYWORD), ("\"x1\"", STRING)]),
            ("a.swift", "// c1\nfunc f() { let s = \"x1\" }\n", &[("// c1", COMMENT), ("func", KEYWORD), ("\"x1\"", STRING)]),
            ("a.rb", "# c1\ndef f\n  \"x1\"\nend\n", &[("# c1", COMMENT), ("def", KEYWORD), ("\"x1\"", STRING)]),
            ("a.php", "<?php\n// c1\nfunction f() { return \"x1\"; }\n", &[("// c1", COMMENT), ("function", KEYWORD), ("\"x1\"", STRING)]),
            ("a.sql", "-- c1\nSELECT 'x1' FROM t;\n", &[("-- c1", COMMENT), ("SELECT", KEYWORD), ("'x1'", STRING)]),
            ("a.zig", "// c1\nconst s = \"x1\";\n", &[("// c1", COMMENT), ("const", KEYWORD), ("\"x1\"", STRING)]),
            ("a.lua", "-- c1\nlocal s = \"x1\"\n", &[("-- c1", COMMENT), ("local", KEYWORD), ("\"x1\"", STRING)]),
            ("a.proto", "// c1\nsyntax = \"proto3\";\nmessage A { string s = 1; }\n", &[("// c1", COMMENT), ("message", KEYWORD), ("\"proto3\"", STRING)]),
            ("a.ex", "# c1\nif x do\n  \"x1\"\nend\n", &[("# c1", COMMENT), ("do", KEYWORD), ("\"x1\"", STRING)]),
            ("a.scala", "// c1\nobject A { val s = \"x1\" }\n", &[("// c1", COMMENT), ("val", KEYWORD), ("\"x1\"", STRING)]),
            ("a.graphql", "# c1\nquery Q { a(b: \"x1\") }\n", &[("# c1", COMMENT), ("query", KEYWORD), ("\"x1\"", STRING)]),
            ("a.svelte", "<!-- c1 -->\n<script>let s = \"x1\"</script>\n", &[("<!-- c1 -->", COMMENT), ("let", KEYWORD), ("\"x1\"", STRING)]),
            ("Makefile", "# c1\nall:\n", &[("# c1", COMMENT)]),
            ("CMakeLists.txt", "# c1\nif(A)\n  set(x \"x1\")\nendif()\n", &[("# c1", COMMENT), ("if", KEYWORD), ("\"x1\"", STRING)]),
        ];
        for (path, text, tokens) in cases {
            for (token, hex) in *tokens {
                assert_eq!(token_color(path, text, token), Some(rgb(*hex).into()), "{token} in {path}");
            }
        }
        // CMake's upper-case-argument rule, whose Neovim-only predicate tree-sitter skipped.
        assert_ne!(token_color("CMakeLists.txt", "set(foo bar)\n", "bar"), Some(rgb(KEYWORD).into()));
    }
}
