//! Syntax errors in TypeScript and JavaScript files, as Monaco's TypeScript worker marks
//! them (its syntactic diagnostics), read from the tree-sitter grammars GPUI Kit highlights
//! these files with. The worker's semantic diagnostics need the TypeScript language service,
//! which TreeTerm does not run.
//!
//! tree-sitter tells where the text stops parsing, not which TypeScript diagnostic that is:
//! - a token the grammar had to assume is marked where it is missing: `';' expected.`,
//!   `Identifier expected.`, or `Expression expected.` for another missing node;
//! - text it had to skip is marked `Unexpected token.`, on the line it starts on.

use super::{Problem, Severity};

/// The grammar a file is read with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grammar {
    TypeScript,
    /// TypeScript with JSX (`.tsx`).
    Tsx,
    /// JavaScript, JSX included.
    JavaScript,
}

impl Grammar {
    /// The grammar of a file at `path` of Monaco's `language`; `None` for a language the
    /// TypeScript worker does not read.
    pub fn for_file(language: &str, path: &str) -> Option<Self> {
        let tsx = path.rsplit_once('.').is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("tsx"));
        match language {
            "typescript" if tsx => Some(Self::Tsx),
            "typescript" => Some(Self::TypeScript),
            "javascript" => Some(Self::JavaScript),
            _ => None,
        }
    }

    fn language(self) -> tree_sitter::Language {
        match self {
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        }
    }
}

/// The message for a missing node of `kind`, in TypeScript's words.
fn missing_message(node: tree_sitter::Node) -> String {
    let kind = node.kind();
    if !node.is_named() {
        return format!("'{kind}' expected.");
    }
    if kind.ends_with("identifier") {
        return "Identifier expected.".to_owned();
    }
    "Expression expected.".to_owned()
}

/// The syntax errors in `text`, by start (one per start, as TypeScript reports them).
pub fn problems(grammar: Grammar, text: &str) -> Vec<Problem> {
    let mut parser = tree_sitter::Parser::new();
    if let Err(error) = parser.set_language(&grammar.language()) {
        // The grammars are linked in at the ABI tree-sitter was built for.
        tracing::error!("{grammar:?} grammar: {error}");
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else {
        tracing::error!("{grammar:?}: tree-sitter parsed nothing");
        return Vec::new();
    };
    let mut problems: Vec<Problem> = Vec::new();
    let mut cursor = tree.walk();
    loop {
        let node = cursor.node();
        let found = if node.is_missing() {
            Some(Problem { range: node.start_byte()..node.start_byte(), severity: Severity::Error, message: missing_message(node) })
        } else if node.is_error() {
            let range = node.byte_range();
            let end = text[range.clone()].find(['\n', '\r']).map_or(range.end, |line_end| range.start + line_end);
            Some(Problem { range: range.start..end, severity: Severity::Error, message: "Unexpected token.".to_owned() })
        } else {
            None
        };
        if let Some(problem) = found
            && problems.last().is_none_or(|last| last.range.start != problem.range.start)
        {
            problems.push(problem);
        }
        // Into a subtree with an error, but not into the skipped text itself.
        if node.has_error() && !node.is_error() && cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                problems.sort_by_key(|problem| problem.range.start);
                return problems;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The problems as `(text marked, message)`, by start.
    fn marked(grammar: Grammar, text: &str) -> Vec<(String, String)> {
        problems(grammar, text).into_iter().map(|problem| (text[problem.range].to_owned(), problem.message)).collect()
    }

    fn mark(at: &str, message: &str) -> (String, String) {
        (at.to_owned(), message.to_owned())
    }

    #[test]
    fn valid_files_have_no_problems() {
        let typescript = "import { a } from './a';\n\
            export default function f<T extends object = {}>(x: T): T { return x; }\n\
            enum E { A = 1, B }\n\
            abstract class Q implements I { private readonly x?: number; constructor(public y: number) { super(); } }\n\
            let v = a?.b ?? (c as string);\n";
        assert_eq!(marked(Grammar::TypeScript, typescript), []);
        assert_eq!(marked(Grammar::Tsx, "const el = <div className=\"x\">{a}</div>;\n"), []);
        assert_eq!(marked(Grammar::JavaScript, "const el = <div>hi</div>;\nasync function* g() { yield await 1; }\n"), []);
    }

    #[test]
    fn skipped_text_is_an_unexpected_token() {
        assert_eq!(marked(Grammar::TypeScript, "const x = ;"), [mark("=", "Unexpected token.")]);
        assert_eq!(marked(Grammar::TypeScript, "let x = 1 +;"), [mark("+", "Unexpected token.")]);
        // Marked on the line it starts on.
        assert_eq!(marked(Grammar::JavaScript, "function f( {\n  return 1\n"), [mark("function f( {", "Unexpected token.")]);
    }

    #[test]
    fn a_missing_token_is_expected_where_it_is_missing() {
        // An empty range here; `code_intel::problems` marks the word or character there.
        assert_eq!(marked(Grammar::TypeScript, "if (x { y() }"), [mark("", "')' expected.")]);
        let found = problems(Grammar::TypeScript, "if (x { y() }");
        assert_eq!(found[0].range, 5..5);
        assert_eq!(marked(Grammar::TypeScript, "class A { m() { }"), [mark("", "'}' expected.")]);
    }

    #[test]
    fn jsx_is_read_in_tsx_and_javascript_files_only() {
        let text = "const el = <div className=\"x\">hi</div>;";
        assert_eq!(marked(Grammar::Tsx, text), []);
        assert_eq!(marked(Grammar::JavaScript, text), []);
        assert!(!marked(Grammar::TypeScript, text).is_empty());
    }

    #[test]
    fn the_grammar_follows_the_language_and_extension() {
        assert_eq!(Grammar::for_file("typescript", "/w/a.ts"), Some(Grammar::TypeScript));
        assert_eq!(Grammar::for_file("typescript", "/w/a.TSX"), Some(Grammar::Tsx));
        assert_eq!(Grammar::for_file("javascript", "/w/a.jsx"), Some(Grammar::JavaScript));
        assert_eq!(Grammar::for_file("css", "/w/a.css"), None);
    }
}
