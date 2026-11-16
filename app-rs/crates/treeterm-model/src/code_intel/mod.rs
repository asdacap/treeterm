//! What Monaco's language workers add to the code editors, without a language server: the
//! problems they mark (squiggles, with a message on hover), CSS colours, and links. Pure functions over a file's text; the code editors install them through GPUI
//! Kit's LSP hooks.
//!
//! - JSON ([`json`]): the JSON worker's problems, ported.
//! - TypeScript and JavaScript ([`syntax`]): syntax errors, read from the tree-sitter
//!   grammars GPUI Kit highlights them with.
//! - CSS, SCSS and Less ([`css`]): colours, and (CSS) the lint warnings about rules and
//!   declarations.
//! - Every language ([`links`]): the URLs the base editor worker links.

pub mod css;
pub mod json;
pub mod links;
pub mod syntax;
mod web_data;

use std::ops::Range;

/// How serious a problem is: Monaco draws errors red and warnings yellow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// A problem a language worker marks in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The bytes marked: empty only where there is nothing to mark (an empty line).
    pub range: Range<usize>,
    pub severity: Severity,
    pub message: String,
}

/// Monaco marks an empty range (`MarkerDecorations._createDecorationRange`) as the word there;
/// past the line's last non-blank character, or where there is no word, as one character
/// wide. GPUI Kit cannot draw past the end of a line, so there it marks the character before.
fn widen(text: &str, range: Range<usize>) -> Range<usize> {
    if !range.is_empty() {
        return range;
    }
    let at = range.start;
    let line_start = text[..at].rfind('\n').map_or(0, |index| index + 1);
    let line_end = text[at..].find('\n').map_or(text.len(), |index| at + index);
    let line_end = if text[..line_end].ends_with('\r') && line_end > at { line_end - 1 } else { line_end };
    let last_non_blank = line_start + text[line_start..line_end].trim_end().len();
    let next = || text[at..line_end].chars().next().map(|next| at..at + next.len_utf8());
    if at >= last_non_blank {
        return next()
            .or_else(|| text[line_start..at].chars().next_back().map(|previous| at - previous.len_utf8()..at))
            .unwrap_or(range);
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let start = text[..at].char_indices().rev().take_while(|(_, c)| is_word(*c)).last().map_or(at, |(index, _)| index);
    let end = text[at..].char_indices().find(|(_, c)| !is_word(*c)).map_or(text.len(), |(index, _)| at + index);
    if start < end { start..end } else { next().unwrap_or(range) }
}

/// The problems Monaco's workers mark in `text`, a file at `path` of Monaco's `language` (as
/// `detect_monaco_language` names it), by start; none for a language without a worker that
/// reports them.
pub fn problems(language: &str, path: &str, text: &str) -> Vec<Problem> {
    let found = match language {
        "json" => json::problems(text),
        "css" => css::problems(text),
        "typescript" | "javascript" => match syntax::Grammar::for_file(language, path) {
            Some(grammar) => syntax::problems(grammar, text),
            None => Vec::new(),
        },
        _ => Vec::new(),
    };
    found
        .into_iter()
        .map(|problem| Problem { range: widen(text, problem.range), ..problem })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_range_is_marked_as_the_word_or_character_there() {
        let text = "let foo = (x  \nbar\n\n";
        assert_eq!(widen(text, 5..5), 4..7);
        // No word: the character there.
        assert_eq!(widen(text, 8..8), 8..9);
        // Past the last non-blank character: the character there, or the one before.
        assert_eq!(widen(text, 12..12), 12..13);
        assert_eq!(widen(text, 14..14), 13..14);
        assert_eq!(widen(text, 18..18), 17..18);
        // An empty line has nothing to mark.
        assert_eq!(widen(text, 19..19), 19..19);
        assert_eq!(widen("", 0..0), 0..0);
        assert_eq!(widen(text, 0..3), 0..3);
    }

    #[test]
    fn only_languages_with_a_worker_that_reports_problems_have_them() {
        assert_eq!(problems("json", "a.json", "[1,]").len(), 1);
        assert_eq!(problems("typescript", "a.ts", "let x = ;").len(), 1);
        assert_eq!(problems("javascript", "a.js", "let x = ;").len(), 1);
        assert_eq!(problems("css", "a.css", "a {}").len(), 1);
        assert_eq!(problems("scss", "a.scss", "a {}"), []);
        assert_eq!(problems("plaintext", "a.jsonc", "[1,]"), []);
        assert_eq!(problems("rust", "a.rs", "fn ("), []);
    }
}
