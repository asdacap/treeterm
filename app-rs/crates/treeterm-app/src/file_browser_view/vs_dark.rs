//! Monaco's `vs-dark` theme as a GPUI Kit highlight theme, so the code editors (the Files
//! tab's viewer, the Editor tab) are coloured as Electron's Monaco editors are.
//!
//! Monaco tokenises with Monarch grammars and GPUI Kit with tree-sitter, so the capture
//! names below carry the colour of the Monarch token that marks the same construct:
//! keywords and markdown headings `#569CD6`, strings and link targets `#CE9178`, numbers
//! `#B5CEA8`, comments `#6A9955`, types `#3DC9B0`, keys and attributes `#9CDCFE`;
//! identifiers, calls and punctuation keep the foreground (`#D4D4D4`).

use std::sync::Arc;

use gpui_kit::component::highlighter::{HighlightTheme, StatusColors, SyntaxColors};
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
    // Monaco outlines the current line (`editor.lineHighlightBorder`, drawn by
    // `current_line_outline`) where GPUI Kit would fill it and its line number.
    style.editor_active_line = None;
    style.editor_line_number = color(0x858585);
    style.editor_active_line_number = color(0xc6c6c6);
    style.editor_invisible = Some(rgba(0xe3e4e229).into());
    style.editor_gutter_background = color(0x1e1e1e);
    style.syntax = syntax();
    style.status = status();
    Arc::new(theme)
}

/// The language workers' squiggles in `vs-dark`'s `editorError.foreground`,
/// `editorWarning.foreground` and `editorInfo.foreground`; their message on hover in the
/// hover widget's `editorHoverWidget.background` and `.border`.
fn status() -> StatusColors {
    let colors = serde_json::json!({
        "error": "#F14C4C",
        "error.background": "#252526",
        "error.border": "#454545",
        "warning": "#CCA700",
        "warning.background": "#252526",
        "warning.border": "#454545",
        "info": "#3794FF",
        "info.background": "#252526",
        "info.border": "#454545",
    });
    match serde_json::from_value(colors) {
        Ok(status) => status,
        // The table above is fixed; a field GPUI Kit renamed would land here (and in the test).
        Err(error) => {
            tracing::error!(%error, "vs-dark status colours did not parse");
            HighlightTheme::default_dark().style.status.clone()
        }
    }
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
        "namespace.path": { "color": "#D4D4D4" },
        "number": { "color": "#B5CEA8" },
        "operator": { "color": "#D4D4D4" },
        "primary": { "color": "#D4D4D4" },
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
    fn the_status_table_parses() {
        let colors = serde_json::to_value(status()).unwrap();
        assert_eq!(colors["error"], serde_json::to_value(Hsla::from(rgb(0xf14c4c))).unwrap());
        assert_eq!(colors["warning.border"], serde_json::to_value(Hsla::from(rgb(0x454545))).unwrap());
    }

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

    /// Monaco's Rust grammar tokenises numbers as numbers and `self` as a keyword, and its
    /// TypeScript grammar `this` and `super` as keywords.
    #[test]
    fn numbers_self_and_this_are_coloured_as_in_monaco() {
        let rust = "fn f(&self) -> f64 { self.x * 2.5 + 1 as f64 }\n";
        assert_eq!(token_color("a.rs", rust, "self"), Some(rgb(0x569cd6).into()));
        assert_eq!(token_color("a.rs", rust, "2.5"), Some(rgb(0xb5cea8).into()));
        assert_eq!(token_color("a.rs", rust, "1"), Some(rgb(0xb5cea8).into()));
        let ts = "class A extends B { m() { return this.x + super.y } }\n";
        assert_eq!(token_color("a.ts", ts, "this"), Some(rgb(0x569cd6).into()));
        assert_eq!(token_color("a.ts", ts, "super"), Some(rgb(0x569cd6).into()));
    }

    /// Monaco's grammars tokenise control keywords as keywords, which `vs-dark` paints like the
    /// rest; Review's Dark+ paints them apart (`review_view/dark_plus.rs`).
    #[test]
    fn control_keywords_are_coloured_as_other_keywords_as_in_monaco() {
        let cases = [
            ("a.rs", "fn f() { if x { return } }\n", "if"),
            ("a.ts", "import a from 'b'\n", "from"),
            ("a.py", "for x in y: pass\n", "for"),
            ("a.go", "func f() { return }\n", "return"),
        ];
        for (path, text, token) in cases {
            assert_eq!(token_color(path, text, token), Some(rgb(0x569cd6).into()), "{token} in {path}");
        }
    }

    /// Monaco's Rust grammar leaves a path's modules, what a `use` brings in, a module's
    /// name, a macro's name and an attribute plain, where Review colours some of them as
    /// Dark+ does (`review_view/dark_plus.rs`).
    #[test]
    fn rust_paths_uses_modules_and_attributes_are_plain_as_in_monaco() {
        let rust = "use std::sync::Arc;\nuse std::fs;\nuse crate::foo::{bar, Baz};\nmod tests;\n#[cfg(test)]\nfn run() {\n    crate::util::parse();\n    tracing::error!(\"x\");\n}\n";
        for token in ["std", "sync", "fs;", "foo", "bar", "Baz", "tests", "#[", "cfg", "test)", "util", "tracing", "error"] {
            assert_eq!(token_color("a.rs", rust, token), Some(rgb(0xd4d4d4).into()), "{token}");
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
        const ATTRIBUTE: u32 = 0x9cdcfe;
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
            // Monaco's languages GPUI Kit has no grammar for, through the nearest one.
            ("a.vue", "<!-- c1 -->\n<template><p class=\"x1\"></p></template>\n<script>let s = \"x2\"</script>\n", &[("<!-- c1 -->", COMMENT), ("template", KEYWORD), ("class", ATTRIBUTE), ("x1", STRING), ("let", KEYWORD), ("\"x2\"", STRING)]),
            ("a.xml", "<?xml version=\"1.0\"?>\n<!-- c1 -->\n<note id=\"x1\"></note>\n", &[("<!-- c1 -->", COMMENT), ("note", KEYWORD), ("id", ATTRIBUTE), ("x1", STRING)]),
            ("a.less", "/* c1 */\n.a { .b { content: \"x1\"; } }\n", &[("/* c1 */", COMMENT), ("\"x1\"", STRING)]),
            ("a.dockerfile", "# c1\nRUN echo \"x1\"\n", &[("# c1", COMMENT), ("\"x1\"", STRING)]),
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
