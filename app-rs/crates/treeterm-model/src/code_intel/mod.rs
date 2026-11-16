//! What Monaco's language workers add to the code editors, without a language server: the
//! problems they mark (squiggles, with a message on hover), CSS colours, completions, and
//! links. Pure functions over a file's text; the code editors install them through GPUI
//! Kit's LSP hooks.
//!
//! - JSON ([`json`]): the JSON worker's problems, and its completions without a schema,
//!   ported.
//! - TypeScript and JavaScript ([`syntax`]): syntax errors, read from the tree-sitter
//!   grammars GPUI Kit highlights them with.
//! - CSS, SCSS and Less ([`css`]): colours, property completions, a declaration's property
//!   documented on hover, and (CSS) the lint warnings about rules and declarations.
//! - HTML ([`html`]): completions, hover documentation and matching tag highlights, over the
//!   worker's scanner and parser, ported.
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

use crate::indentation::Indentation;
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
    /// The bytes a pick replaces: what was typed, and perhaps more (the rest of the name).
    pub replace: Range<usize>,
    /// What a pick puts in their place.
    pub insert: Insert,
    pub then: Then,
}

/// What follows picking a completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Then {
    Done,
    /// Suggestions are asked for again where the cursor is left (the worker's
    /// `editor.action.triggerSuggest` command): an attribute's values between its quotes.
    Suggest,
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

/// The completions a language worker of Monaco's `language` offers at byte `offset` of
/// `text`, best matches first: CSS, SCSS and Less property names; HTML elements, end tags,
/// attributes, attribute values and character entities; JSON keys and values. `None` where it
/// offers none, and in other languages: Monaco's word suggestions are offered there
/// ([`crate::words::suggestions`]). Where what was typed matches none of the worker's
/// proposals the list is empty: Monaco asks no other provider then.
pub fn completions(language: &str, text: &str, offset: usize, trigger: Trigger) -> Option<Vec<Completion>> {
    match language {
        "css" | "scss" | "less" => css::property_completions(language, text, offset, trigger),
        "html" => html::completions(text, offset, trigger),
        "json" => json::completions(text, offset, trigger),
        _ => None,
    }
}

/// What a language worker shows on hover: markdown, over the bytes `range`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hover {
    pub contents: String,
    pub range: Range<usize>,
}

/// What the language worker of Monaco's `language` shows on hover at byte `offset` of `text`:
/// HTML's documentation of elements, attributes, values and character entities; CSS's of a
/// declaration's property. `None` elsewhere, and in other languages.
pub fn hover(language: &str, text: &str, offset: usize) -> Option<Hover> {
    match language {
        "html" => html::hover(text, offset),
        "css" | "scss" | "less" => css::hover(language, text, offset),
        _ => None,
    }
}

/// Whether the language worker of Monaco's `language` highlights at the cursor (its document
/// highlights, which Monaco's word highlighter paints): HTML's matching tag names.
pub fn highlights_at_cursor(language: &str) -> bool {
    language == "html"
}

/// What the language worker of Monaco's `language` highlights with the cursor at byte `offset`
/// of `text` ([`highlights_at_cursor`]); nothing in other languages.
pub fn highlights(language: &str, text: &str, offset: usize) -> Vec<Range<usize>> {
    match language {
        "html" => html::matching_tags(text, offset),
        _ => Vec::new(),
    }
}

/// A change to a text: the bytes `range` replaced with `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub range: Range<usize>,
    pub text: String,
}

/// `text` with `edits` made (in order, none overlapping), and where `selection` goes: its
/// ends move with the text around them, and an end inside a replaced range stays within it.
pub fn apply_edits(text: &str, edits: &[TextEdit], selection: Range<usize>) -> (String, Range<usize>) {
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    for edit in edits {
        out.push_str(&text[copied..edit.range.start]);
        out.push_str(&edit.text);
        copied = edit.range.end;
    }
    out.push_str(&text[copied..]);
    let map = |at: usize| {
        let mut moved = at;
        for edit in edits.iter().rev() {
            if edit.range.end <= at {
                moved = moved + edit.text.len() - edit.range.len();
            } else if edit.range.start < at {
                moved = moved - (at - edit.range.start) + (at - edit.range.start).min(edit.text.len());
            }
        }
        moved
    };
    (out, map(selection.start)..map(selection.end))
}

/// What Format Document or Format Selection formats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatScope {
    Document,
    /// The lines a selection touches; with nothing selected, the cursor's line.
    Selection(Range<usize>),
}

/// Whether Monaco formats a file of `language` (its worker has a formatter): the editable
/// editor's right-click menu offers Format Document, and Format Selection, then.
pub fn formats(language: &str) -> bool {
    language == "json"
}

/// The edits that format `scope` of `text`, a file of Monaco's `language` the editor
/// indents with `indentation`; none in a language without a formatter.
pub fn format(language: &str, text: &str, scope: FormatScope, indentation: Indentation) -> Vec<TextEdit> {
    if !formats(language) {
        return Vec::new();
    }
    let range = match scope {
        FormatScope::Document => None,
        FormatScope::Selection(range) if range.is_empty() => {
            let at = range.start.min(text.len());
            let start = text[..at].rfind(['\n', '\r']).map_or(0, |index| index + 1);
            let end = text[at..].find(['\n', '\r']).map_or(text.len(), |index| at + index);
            Some(start..end)
        }
        FormatScope::Selection(range) => Some(range),
    };
    json::format(text, range, indentation)
}

/// The characters that ask a language worker for its completions as they are typed (its
/// `triggerCharacters`), in comments and strings too.
fn trigger_characters(language: &str) -> &'static [char] {
    match language {
        "json" => &json::TRIGGER_CHARACTERS,
        "css" | "scss" | "less" => &['/', '-', ':'],
        "html" => &html::TRIGGER_CHARACTERS,
        _ => &[],
    }
}

/// What asked the Editor tab for suggestions, as Monaco's suggest controller tells them apart
/// ([`Session::ask`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked {
    /// Ctrl+Space (or Cmd/Ctrl+I).
    Invoked,
    /// A character typed where the suggestions showing, or on their way, were asked for: they
    /// follow what is typed.
    Refined,
    /// A character typed with no suggestions showing or on their way: Monaco's quick
    /// suggestions, which it keeps quiet while typing in a comment or a string
    /// (`quickSuggestions: { comments: 'off', strings: 'off' }`), except for a character the
    /// language's worker is triggered by.
    Typed,
}

/// The Editor tab's suggestions from one keystroke to the next, as Monaco's suggest model keeps
/// them: a character typed where they were last asked for goes on with them ([`Asked::Refined`]),
/// in a comment or a string too. GPUI Kit closes them at every edit and asks again, dropping the
/// request still on its way, so it is the requests that start a session and carry it on (as
/// Monaco's starts when it asks), not their answers, which arrive only when no key comes first.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    /// No suggestions show or are on their way.
    #[default]
    Idle,
    /// Suggestions show, or are on their way, asked for with the cursor at this byte offset.
    Active { at: usize },
}

/// What asks the Editor tab for suggestions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request<'a> {
    /// Ctrl+Space (or Cmd/Ctrl+I).
    Invoked,
    /// A character typed: `line` is the text of its line up to the cursor, which it ends.
    Typed { line: &'a str },
}

impl Session {
    /// `request` asks for suggestions with the cursor at byte `offset` of a file of Monaco's
    /// `language`: how it asks ([`suggest`] answers it), and the session from then on. Asking
    /// starts one, and so does a character the language's worker is triggered by. A character
    /// typed where the session's last request was made goes on with it while the cursor ends a
    /// word being typed, which the suggestions are filtered by (Monaco's filtering leaves
    /// nothing to show once it does not). Any other character ends it: in code, quick
    /// suggestions ask again at the next one, which is the same as going on.
    pub fn ask(self, language: &str, offset: usize, request: Request<'_>) -> (Asked, Session) {
        let Request::Typed { line } = request else {
            return (Asked::Invoked, Session::Active { at: offset });
        };
        let Some(typed) = line.chars().next_back() else {
            return (Asked::Typed, Session::Idle);
        };
        let refined = self == Session::Active { at: offset.saturating_sub(typed.len_utf8()) };
        let goes_on = trigger_characters(language).contains(&typed)
            || refined && crate::words::typing_word(line, line.len(), language);
        let asked = if refined { Asked::Refined } else { Asked::Typed };
        (asked, if goes_on { Session::Active { at: offset } } else { Session::Idle })
    }

    /// The answer to the request made with the cursor at byte `offset` came back, `shown`
    /// whether it has suggestions to show: none end the session that request carried on, as
    /// Monaco's ends once nothing is left to show. Any other answer leaves the session as it is.
    pub fn answered(self, offset: usize, shown: bool) -> Session {
        if !shown && self == (Session::Active { at: offset }) { Session::Idle } else { self }
    }
}

/// What the Editor tab suggests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Suggested {
    /// The language worker's completions.
    Language(Vec<Completion>),
    /// Words of the file and of the other open files of its language.
    Words(crate::words::Suggestions),
    Nothing,
}

/// What the Editor tab suggests at byte `offset` of `text`, a file of Monaco's `language`,
/// given `others`, the texts of the other open files of that language: the language worker's
/// completions where it offers some, else words ([`crate::words::suggestions`]), as Monaco
/// asks its word-based provider only when the language's offered nothing. Where it was
/// [`Asked::Typed`], `in_comment_or_string` tells whether the character typed is in a comment
/// or a string, where only a trigger character asks, and only the language's worker.
pub fn suggest(
    language: &str,
    text: &str,
    others: &[String],
    offset: usize,
    asked: Asked,
    in_comment_or_string: impl FnOnce() -> bool,
) -> Suggested {
    let trigger = match asked {
        Asked::Invoked => Trigger::Invoked,
        Asked::Refined | Asked::Typed => Trigger::Typing,
    };
    if asked == Asked::Typed && in_comment_or_string() {
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
        assert_eq!((found[0].replace.clone(), found[0].label.as_str()), (4..8, "color"));
        let found = completions("html", "<", 1, Trigger::Typing).expect("elements");
        assert_eq!(found[0].replace, 1..1);
        assert_eq!(completions("rust", "fn colo", 7, Trigger::Typing), None);
        // Where the worker has nothing, none: word suggestions take over.
        assert_eq!(completions("css", "a { color: r }", 12, Trigger::Typing), None);
        // Where it has proposals but none matches what was typed, an empty list: no words.
        assert_eq!(completions("css", "a { qqq }", 7, Trigger::Typing), Some(Vec::new()));
    }

    #[test]
    fn edits_are_made_and_the_selection_moves_with_the_text() {
        let edits = [
            TextEdit { range: 1..1, text: "\n  ".to_owned() },
            TextEdit { range: 5..8, text: " ".to_owned() },
        ];
        let (text, selection) = apply_edits("{\"a\":   1}", &edits, 4..10);
        assert_eq!(text, "{\n  \"a\": 1}");
        // After an edit: moved by what it added or took; inside one: kept within it.
        assert_eq!(selection, 7..11);
        assert_eq!(apply_edits("abc", &[], 1..2), ("abc".to_owned(), 1..2));
        let spaces = [TextEdit { range: 1..4, text: " ".to_owned() }];
        assert_eq!(apply_edits("a   b", &spaces, 3..3), ("a b".to_owned(), 2..2));
    }

    #[test]
    fn only_json_is_formatted_and_an_empty_selection_formats_its_line() {
        let spaces = Indentation::MONACO_DEFAULT;
        assert!(formats("json") && !formats("css") && !formats("rust"));
        assert_eq!(format("rust", "fn  x", FormatScope::Document, spaces), []);
        let text = "[1,\n[2,3]]";
        let applied = |scope| apply_edits(text, &format("json", text, scope, spaces), 0..0).0;
        assert_eq!(applied(FormatScope::Document), "[\n    1,\n    [\n        2,\n        3\n    ]\n]");
        // The cursor on the second line: only its edits, from its indent.
        assert_eq!(applied(FormatScope::Selection(5..5)), "[1,\n[\n    2,\n    3\n]\n]");
    }

    fn words_of(suggested: Suggested) -> Vec<String> {
        match suggested {
            Suggested::Words(found) => found.words,
            other => panic!("words, not {other:?}"),
        }
    }

    #[test]
    fn typing_suggests_the_languages_completions_or_else_words_of_every_open_file() {
        let code = || false;
        let others = ["forest".to_owned()];
        assert_eq!(words_of(suggest("rust", "food fo", &others, 7, Asked::Typed, code)), ["food", "forest"]);
        assert_eq!(words_of(suggest("rust", "food fo", &others, 7, Asked::Refined, code)), ["food", "forest"]);
        assert_eq!(words_of(suggest("rust", "food ", &others, 5, Asked::Invoked, code)), ["food", "forest"]);
        match suggest("css", "a { colo }", &others, 8, Asked::Typed, code) {
            Suggested::Language(found) => assert_eq!(found[0].label, "color"),
            other => panic!("properties, not {other:?}"),
        }
    }

    #[test]
    fn typing_in_a_comment_or_string_suggests_only_where_a_trigger_character_asks_the_worker() {
        let quiet = || true;
        assert_eq!(suggest("rust", "food // fo", &[], 10, Asked::Typed, quiet), Suggested::Nothing);
        // Once suggestions show, typing there goes on refining them, and asking is not quiet:
        // neither needs to know where the cursor is.
        let unasked = || panic!("only quick suggestions ask whether the cursor is in a comment or a string");
        assert_eq!(words_of(suggest("rust", "food // fo", &[], 10, Asked::Refined, unasked)), ["food"]);
        assert_eq!(words_of(suggest("rust", "food // ", &[], 8, Asked::Invoked, unasked)), ["food"]);
        // `<` asks the HTML worker there; a letter does not.
        assert!(matches!(suggest("html", "<", &[], 1, Asked::Typed, quiet), Suggested::Language(_)));
        assert_eq!(suggest("html", "<!-- <a t", &[], 9, Asked::Typed, quiet), Suggested::Nothing);
        // A trigger character where the worker has nothing: nothing, not words.
        assert_eq!(suggest("css", "/* a -", &[], 6, Asked::Typed, quiet), Suggested::Nothing);
        assert_eq!(suggest("rust", "", &[], 0, Asked::Typed, quiet), Suggested::Nothing);
    }

    /// A character typed with the cursor at the end of `line` (which starts the text).
    fn typed(session: Session, language: &str, line: &str) -> (Asked, Session) {
        session.ask(language, line.len(), Request::Typed { line })
    }

    #[test]
    fn asking_or_a_trigger_character_starts_a_session_as_it_asks() {
        let idle = Session::Idle;
        assert_eq!(idle.ask("rust", 8, Request::Invoked), (Asked::Invoked, Session::Active { at: 8 }));
        let active = Session::Active { at: 8 };
        assert_eq!(active.ask("rust", 8, Request::Invoked), (Asked::Invoked, active));
        // A JSON key's quote, HTML's `<`: their workers' trigger characters.
        assert_eq!(typed(idle, "json", "{\""), (Asked::Typed, Session::Active { at: 2 }));
        assert_eq!(typed(idle, "html", "<"), (Asked::Typed, Session::Active { at: 1 }));
        // Another character with none showing: quick suggestions, and no session (in code the
        // next character asks the same again; in a comment or a string, nothing is suggested).
        assert_eq!(typed(idle, "rust", "// f"), (Asked::Typed, Session::Idle));
        assert_eq!(typed(idle, "json", "{\"a"), (Asked::Typed, Session::Idle));
        // Typed elsewhere than the last request: the cursor moved, the session does not go on.
        assert_eq!(typed(Session::Active { at: 1 }, "rust", "// f"), (Asked::Typed, Session::Idle));
        // Nothing typed (the Kit never asks so).
        assert_eq!(typed(Session::Active { at: 0 }, "rust", ""), (Asked::Typed, Session::Idle));
    }

    #[test]
    fn letters_typed_one_after_another_go_on_with_the_session_before_any_answer() {
        // Ctrl+Space in a comment, then two letters before either answer comes back.
        let (_, session) = Session::Idle.ask("rust", 3, Request::Invoked);
        let (asked, session) = typed(session, "rust", "// f");
        assert_eq!((asked, session), (Asked::Refined, Session::Active { at: 4 }));
        assert_eq!(typed(session, "rust", "// fo"), (Asked::Refined, Session::Active { at: 5 }));
        // A JSON key, after the quote that asked for the keys.
        let (_, session) = typed(Session::Idle, "json", "{\"");
        let (asked, session) = typed(session, "json", "{\"b");
        assert_eq!((asked, session), (Asked::Refined, Session::Active { at: 3 }));
        assert_eq!(typed(session, "json", "{\"br"), (Asked::Refined, Session::Active { at: 4 }));
        // `-` is part of a word in HTML and JSON (their word patterns), not in Rust.
        assert_eq!(typed(Session::Active { at: 3 }, "html", "<my-"), (Asked::Refined, Session::Active { at: 4 }));
        assert_eq!(typed(Session::Active { at: 3 }, "json", "{\"a-"), (Asked::Refined, Session::Active { at: 4 }));
        assert_eq!(typed(Session::Active { at: 4 }, "rust", "// a-"), (Asked::Refined, Session::Idle));
    }

    #[test]
    fn a_character_that_leaves_no_word_ends_the_session_unless_it_asks_the_worker() {
        // A space or a dot after a word: what shows is refined by it once more, and then ends.
        assert_eq!(typed(Session::Active { at: 5 }, "rust", "// fo "), (Asked::Refined, Session::Idle));
        assert_eq!(typed(Session::Active { at: 2 }, "rust", "fo."), (Asked::Refined, Session::Idle));
        // A line break: no word on the new line.
        assert_eq!(typed(Session::Active { at: 2 }, "rust", "fo\n"), (Asked::Refined, Session::Idle));
        // The JSON worker's space, HTML's `/`: it goes on, asking the worker.
        assert_eq!(typed(Session::Active { at: 5 }, "json", "{\"a\": "), (Asked::Refined, Session::Active { at: 6 }));
        assert_eq!(typed(Session::Active { at: 1 }, "html", "</"), (Asked::Refined, Session::Active { at: 2 }));
    }

    #[test]
    fn only_an_answer_with_nothing_to_show_to_the_latest_request_ends_the_session() {
        let active = Session::Active { at: 4 };
        assert_eq!(active.answered(4, false), Session::Idle);
        assert_eq!(active.answered(4, true), active);
        // The answer to an earlier request (a later one went on with the session), and one after
        // the session ended (Escape): the session is left as it is.
        assert_eq!(active.answered(3, false), active);
        assert_eq!(Session::Idle.answered(4, true), Session::Idle);
    }
}
