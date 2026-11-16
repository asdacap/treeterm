//! What Monaco's language workers add to the code editors, without a language server: the
//! problems they mark (squiggles, with a message on hover), CSS colours, completions, and
//! links. Pure functions over a file's text; the code editors install them through GPUI
//! Kit's LSP hooks.
//!
//! - JSON ([`json`]): the JSON worker's problems, and its completions without a schema,
//!   ported.
//! - TypeScript and JavaScript ([`syntax`]): syntax errors, read from the tree-sitter
//!   grammars GPUI Kit highlights them with.
//! - CSS, SCSS and Less ([`css`]): colours, property completions, and (CSS) the lint
//!   warnings about rules and declarations.
//! - HTML ([`html`]): element and attribute name completions.
//! - Every language ([`links`]): the URLs the base editor worker links, and where Monaco's
//!   opener takes them.

pub mod css;
pub mod html;
pub mod json;
pub mod links;
pub mod syntax;
mod uri;
mod web_data;

use std::ops::Range;

use crate::words::Trigger;

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

/// A completion a language worker offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// What the list shows.
    pub label: String,
    /// What a pick puts in place of [`Completions::replace`].
    pub insert: Insert,
}

/// What a picked completion inserts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Insert {
    /// This text, the cursor after it.
    Text(String),
    /// A snippet, as Monaco's language workers write one: the cursor goes to its first tab
    /// stop (`$1`, else `$0`, else the end), and `\$`, `\}` and `\\` stand for those
    /// characters.
    Snippet(String),
}

impl Insert {
    /// The text or snippet as written.
    pub fn source(&self) -> &str {
        match self {
            Self::Text(source) | Self::Snippet(source) => source,
        }
    }
}

/// The completions at the cursor, best matches first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completions {
    /// The bytes a picked completion replaces: what was typed, and the rest of the name.
    pub replace: Range<usize>,
    pub items: Vec<Completion>,
}

/// The completions a language worker of Monaco's `language` offers at byte `offset` of
/// `text`: CSS, SCSS and Less property names, HTML element and attribute names, JSON keys
/// and values. `None` where it offers none, and in other languages: Monaco's word suggestions
/// are offered there ([`crate::words::suggestions`]). Where what was typed matches none of the
/// JSON worker's proposals the list is empty: Monaco asks no other provider then.
pub fn completions(language: &str, text: &str, offset: usize, trigger: Trigger) -> Option<Completions> {
    let offered = |found: &Completions| !found.items.is_empty();
    match language {
        "css" | "scss" | "less" => css::property_completions(language, text, offset, trigger).filter(offered),
        "html" => html::completions(text, offset, trigger).filter(offered),
        "json" => json::completions(text, offset, trigger),
        _ => None,
    }
}

/// The characters that ask a language worker for its completions as they are typed (its
/// `triggerCharacters`), in comments and strings too.
fn trigger_characters(language: &str) -> &'static [char] {
    match language {
        "json" => &json::TRIGGER_CHARACTERS,
        "css" | "scss" | "less" => &['/', '-', ':'],
        "html" => &['.', ':', '<', '"', '=', '/'],
        _ => &[],
    }
}

/// What asked the Editor tab for suggestions, as Monaco's suggest controller tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked {
    /// Ctrl+Space (or Cmd/Ctrl+I).
    Invoked,
    /// A character typed while suggestions show: they follow what is typed.
    Refined,
    /// A character typed while none show: Monaco's quick suggestions, which it keeps quiet
    /// while typing in a comment or a string (`quickSuggestions: { comments: 'off', strings:
    /// 'off' }`), except for a character the language's worker is triggered by.
    Typed { in_comment_or_string: bool },
}

/// What the Editor tab suggests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Suggested {
    /// The language worker's completions.
    Language(Completions),
    /// Words of the file and of the other open files of its language.
    Words(crate::words::Suggestions),
    Nothing,
}

/// What the Editor tab suggests at byte `offset` of `text`, a file of Monaco's `language`,
/// given `others`, the texts of the other open files of that language: the language worker's
/// completions where it offers some, else words ([`crate::words::suggestions`]), as Monaco
/// asks its word-based provider only when the language's offered nothing.
pub fn suggest(language: &str, text: &str, others: &[String], offset: usize, asked: Asked) -> Suggested {
    let trigger = match asked {
        Asked::Invoked => Trigger::Invoked,
        Asked::Refined | Asked::Typed { .. } => Trigger::Typing,
    };
    if asked == (Asked::Typed { in_comment_or_string: true }) {
        let typed = text.get(..offset).and_then(|before| before.chars().next_back());
        if !typed.is_some_and(|typed| trigger_characters(language).contains(&typed)) {
            return Suggested::Nothing;
        }
        return completions(language, text, offset, trigger).map_or(Suggested::Nothing, Suggested::Language);
    }
    match completions(language, text, offset, trigger) {
        Some(found) => Suggested::Language(found),
        None => Suggested::Words(crate::words::suggestions(text, others, offset, trigger, language)),
    }
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

/// Run `work` on a thread with the stack of GPUI's background threads (Rust's default,
/// 2 MiB), where the code editors find problems and colours.
#[cfg(test)]
fn on_background_stack<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    let thread = std::thread::Builder::new().stack_size(2 << 20).spawn(work).expect("a thread");
    thread.join().expect("the work to finish")
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

    #[test]
    fn only_languages_with_a_worker_that_completes_have_completions() {
        let found = completions("css", "a { colo }", 8, Trigger::Typing).expect("properties");
        assert_eq!((found.replace, found.items[0].label.as_str()), (4..8, "color"));
        let found = completions("html", "<", 1, Trigger::Typing).expect("elements");
        assert_eq!(found.replace, 1..1);
        assert_eq!(completions("rust", "fn colo", 7, Trigger::Typing), None);
        // Where the worker has nothing, none: word suggestions take over.
        assert_eq!(completions("css", "a { color: r }", 12, Trigger::Typing), None);
    }

    fn words_of(suggested: Suggested) -> Vec<String> {
        match suggested {
            Suggested::Words(found) => found.words,
            other => panic!("words, not {other:?}"),
        }
    }

    #[test]
    fn typing_suggests_the_languages_completions_or_else_words_of_every_open_file() {
        let typed = Asked::Typed { in_comment_or_string: false };
        let others = ["forest".to_owned()];
        assert_eq!(words_of(suggest("rust", "food fo", &others, 7, typed)), ["food", "forest"]);
        assert_eq!(words_of(suggest("rust", "food fo", &others, 7, Asked::Refined)), ["food", "forest"]);
        assert_eq!(words_of(suggest("rust", "food ", &others, 5, Asked::Invoked)), ["food", "forest"]);
        match suggest("css", "a { colo }", &others, 8, typed) {
            Suggested::Language(found) => assert_eq!(found.items[0].label, "color"),
            other => panic!("properties, not {other:?}"),
        }
    }

    #[test]
    fn typing_in_a_comment_or_string_suggests_only_where_a_trigger_character_asks_the_worker() {
        let quiet = Asked::Typed { in_comment_or_string: true };
        assert_eq!(suggest("rust", "food // fo", &[], 10, quiet), Suggested::Nothing);
        // Once suggestions show, typing there goes on refining them.
        assert_eq!(words_of(suggest("rust", "food // fo", &[], 10, Asked::Refined)), ["food"]);
        // `<` asks the HTML worker there; a letter does not.
        assert!(matches!(suggest("html", "<", &[], 1, quiet), Suggested::Language(_)));
        assert_eq!(suggest("html", "<!-- <a t", &[], 9, quiet), Suggested::Nothing);
        // A trigger character where the worker has nothing: nothing, not words.
        assert_eq!(suggest("css", "/* a -", &[], 6, quiet), Suggested::Nothing);
        assert_eq!(suggest("rust", "", &[], 0, quiet), Suggested::Nothing);
    }
}
