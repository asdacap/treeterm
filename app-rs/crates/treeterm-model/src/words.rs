//! The Editor tab's word-based aids, as Monaco's: the suggestions its base editor worker
//! offers while typing, in every language (`WordBasedCompletionItemProvider` and the
//! worker's `textualSuggest`), filtered and ranked as its suggest widget does ([`fuzzy`]),
//! and Change All Occurrences (`editor.action.changeAll`); and the word under the cursor
//! the Files viewer's "Search" looks for ([`word_at`], `getWordAtPosition`).
//!
//! They read words differently, as in Monaco:
//! - Suggestions and [`word_at`] use the language's word pattern (its language
//!   configuration's `wordPattern`): CSS, SCSS and Less words hold `-` and may start with
//!   `.`, `#`, `@`, `!` or `:`; HTML words hold `-`, `#`, `%` and `?`; TypeScript,
//!   JavaScript and PHP words hold `$`; Java and Kotlin words `@` and `$`; C# words `@`.
//!   Other languages use Monaco's default pattern: a number with a decimal point (`3.14`,
//!   `.5`), or a run of characters that are neither whitespace nor one of the default word
//!   separators. [`word_at`] takes the pattern of the language at the cursor, which in an
//!   HTML, PHP or Markdown file can be another one ([`embedded`]): JavaScript in a
//!   `<script>` block, CSS in a `<style>` block, a code fence's language.
//! - Change All Occurrences uses the editor's word separators (`editor.wordSeparators`,
//!   through `getConfiguredWordAtPosition`) in every language: on `name` in PHP's `$name`
//!   it also changes `$this->name`, as Monaco does.

mod embedded;
pub mod fuzzy;

use std::collections::HashSet;
use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

/// Monaco's `USUAL_WORD_SEPARATORS`, the default `editor.wordSeparators`.
const WORD_SEPARATORS: &str = "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?";

/// The worker stops collecting words once it has more than this many (`_suggestionsLimit`).
const SUGGESTIONS_LIMIT: usize = 10_000;

/// A word pattern Monaco writes as a JavaScript regular expression. Its `\w` and `\d` are
/// ASCII, so they are spelled out here.
fn pattern(source: &str) -> Regex {
    Regex::new(source).unwrap_or_else(|error| panic!("word pattern {source}: {error}"))
}

/// Monaco's `createWordRegExp(allowed)`: a number with a decimal point, or a run of
/// characters that are neither whitespace nor a word separator other than `allowed`.
fn separated_words(allowed: &str) -> Regex {
    let separators: String =
        WORD_SEPARATORS.chars().filter(|c| !allowed.contains(*c)).map(|c| regex::escape(&c.to_string())).collect();
    pattern(&format!(r"(-?[0-9]*\.[0-9][0-9A-Za-z_]*)|([^{separators}\s]+)"))
}

/// The word pattern of Monaco's `language` (as `detect_monaco_language` names it, or a
/// language embedded in a file), from Monaco 0.55.1's language configurations; the default
/// one for a language with none.
fn word_pattern(language: &str) -> &'static Regex {
    static DEFAULT: LazyLock<Regex> = LazyLock::new(|| separated_words(""));
    // TypeScript, JavaScript (which takes TypeScript's configuration) and PHP.
    static DOLLAR: LazyLock<Regex> = LazyLock::new(|| separated_words("$"));
    // Java, Kotlin, Apex and Pascal.
    static AT_DOLLAR: LazyLock<Regex> = LazyLock::new(|| separated_words("@$"));
    static CSHARP: LazyLock<Regex> = LazyLock::new(|| separated_words("@"));
    // HTML, Handlebars, Liquid and Twig.
    static HTML: LazyLock<Regex> = LazyLock::new(|| separated_words("#%-?"));
    static RAZOR: LazyLock<Regex> = LazyLock::new(|| separated_words("#%?"));
    static POWERSHELL: LazyLock<Regex> = LazyLock::new(|| separated_words("$-"));
    // The JSON language service's configuration (`jsonMode`).
    static JSON: LazyLock<Regex> = LazyLock::new(|| pattern(r#"(-?[0-9]*\.[0-9][0-9A-Za-z_]*)|([^\[\{\]\}:",\s]+)"#));
    static CSS: LazyLock<Regex> = LazyLock::new(|| {
        pattern(r"(#?-?[0-9]*\.[0-9][0-9A-Za-z_]*%?)|((::|[@#.!:])?[0-9A-Za-z_\-?]+%?)|::|[@#.!:]")
    });
    static SCSS: LazyLock<Regex> = LazyLock::new(|| {
        pattern(r"(#?-?[0-9]*\.[0-9][0-9A-Za-z_]*%?)|([@$#!.:]?[0-9A-Za-z_\-?]+%?)|[@#!.]")
    });
    static LESS: LazyLock<Regex> = LazyLock::new(|| {
        pattern(r"(#?-?[0-9]*\.[0-9][0-9A-Za-z_]*%?)|([@#!.:]?[0-9A-Za-z_\-?]+%?)|[@#!.]")
    });
    static SCALA: LazyLock<Regex> = LazyLock::new(|| {
        pattern(r"(unary_[@\~!\#%\^\&*()\-=+\\|:<>/?]+)|([a-zA-Z_$][0-9A-Za-z_$]*?_=)|(`[^`]+`)|([a-zA-Z_$][0-9A-Za-z_$]*)")
    });
    match language {
        "typescript" | "javascript" | "php" => &DOLLAR,
        "java" | "kotlin" | "apex" | "pascal" => &AT_DOLLAR,
        "csharp" => &CSHARP,
        "html" | "handlebars" | "liquid" | "twig" => &HTML,
        "razor" => &RAZOR,
        "powershell" => &POWERSHELL,
        "json" => &JSON,
        "css" => &CSS,
        "scss" => &SCSS,
        "less" => &LESS,
        "scala" => &SCALA,
        _ => &DEFAULT,
    }
}

/// The lines of `text`, each with the byte it starts at, without their line break.
fn lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut next = 0;
    text.split('\n').map(move |line| {
        let start = next;
        next += line.len() + 1;
        (start, line.strip_suffix('\r').unwrap_or(line))
    })
}

/// The words `pattern` reads in `text`, line by line (a word never spans lines).
fn pattern_words<'a>(pattern: &'a Regex, text: &'a str) -> impl Iterator<Item = &'a str> {
    lines(text).flat_map(|(_, line)| pattern.find_iter(line).map(|found| found.as_str()))
}

/// The bytes of the first word `pattern` reads in `text` that touches byte `at` on either
/// side (`getWordAtText`).
fn word_touching(pattern: &Regex, text: &str, at: usize) -> Option<Range<usize>> {
    pattern.find_iter(text).map(|found| found.range()).find(|found| found.start <= at && at <= found.end)
}

/// The bytes of the word `pattern` reads at byte `offset` of `text`, touching it on either
/// side (the first such word: `getWordAtPosition`); empty when there is none.
fn pattern_word_around(pattern: &Regex, text: &str, offset: usize) -> Range<usize> {
    if !text.is_char_boundary(offset) {
        return offset..offset;
    }
    let start = text[..offset].rfind('\n').map_or(0, |index| index + 1);
    let (_, line) = lines(&text[start..]).next().unwrap_or((0, ""));
    word_touching(pattern, line, offset - start).map_or(offset..offset, |word| start + word.start..start + word.end)
}

/// The word at byte `offset` of `text`, a file of Monaco's `language`, as Monaco's
/// `getWordAtPosition` reads it: by the word pattern of the language at `offset` (in an
/// HTML, PHP or Markdown file, perhaps one embedded there), within the part of the line in
/// that language, touching `offset` on either side; or, at the start of that part, the word
/// ending there in the language before it. Empty when there is none, or when `offset` is not
/// a character boundary of `text`.
pub fn word_at<'a>(text: &'a str, offset: usize, language: &str) -> &'a str {
    if !text.is_char_boundary(offset) {
        return "";
    }
    let start = text[..offset].rfind('\n').map_or(0, |index| index + 1);
    let (_, line) = lines(&text[start..]).next().unwrap_or((0, ""));
    let at = offset - start;
    let parts = embedded::line_parts(text, start, line, language);
    let Some(index) = parts.iter().rposition(|(part, _)| part.start <= at) else {
        return "";
    };
    let word_in = |(part, language): &(Range<usize>, &str)| {
        word_touching(word_pattern(language), &line[part.clone()], at - part.start)
            .map(|word| part.start + word.start..part.start + word.end)
    };
    let word = word_in(&parts[index]).or_else(|| match index.checked_sub(1) {
        Some(before) if parts[index].0.start == at => word_in(&parts[before]),
        Some(_) | None => None,
    });
    word.map_or("", |word| &text[start + word.start..start + word.end])
}

/// Whether `c` can be part of a word, by the editor's word separators.
fn is_word_char(c: char) -> bool {
    !c.is_whitespace() && !WORD_SEPARATORS.contains(c)
}

/// The bytes of the word at byte `offset` of `text` by the editor's word separators
/// (touching it on either side); empty when there is none.
fn word_around(text: &str, offset: usize) -> Range<usize> {
    if !text.is_char_boundary(offset) {
        return offset..offset;
    }
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word_char(*c))
        .last()
        .map_or(offset, |(index, _)| index);
    let end =
        text[offset..].char_indices().find(|(_, c)| !is_word_char(*c)).map_or(text.len(), |(index, _)| offset + index);
    start..end
}

/// The words of `text`, each with the bytes it spans.
fn words(text: &str) -> impl Iterator<Item = (Range<usize>, &str)> {
    let mut start = None;
    text.char_indices().chain(std::iter::once((text.len(), ' '))).filter_map(move |(index, c)| {
        match (start, is_word_char(c)) {
            (None, true) => {
                start = Some(index);
                None
            }
            (Some(from), false) => {
                start = None;
                Some((from..index, &text[from..index]))
            }
            (None, false) | (Some(_), true) => None,
        }
    })
}

/// Whether JavaScript's `Number(word)` reads `word` as a number, as Monaco's worker asks
/// before suggesting it: a decimal number with an optional sign, decimal point and
/// exponent, `Infinity`, or a hexadecimal, octal or binary integer (`0x1f`).
fn is_number(word: &str) -> bool {
    let digits = |text: &str, radix: u32| !text.is_empty() && text.chars().all(|c| c.is_digit(radix));
    let radix = |lower: &str, upper: &str| word.strip_prefix(lower).or_else(|| word.strip_prefix(upper));
    if let Some(rest) = radix("0x", "0X") {
        return digits(rest, 16);
    }
    if let Some(rest) = radix("0o", "0O") {
        return digits(rest, 8);
    }
    if let Some(rest) = radix("0b", "0B") {
        return digits(rest, 2);
    }
    let unsigned = word.strip_prefix(['+', '-']).unwrap_or(word);
    if unsigned == "Infinity" {
        return true;
    }
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (unsigned, None),
    };
    let exponent_ok = exponent.is_none_or(|exponent| digits(exponent.strip_prefix(['+', '-']).unwrap_or(exponent), 10));
    let mantissa_ok = match mantissa.split_once('.') {
        Some((whole, fraction)) => {
            (whole.is_empty() || digits(whole, 10))
                && (fraction.is_empty() || digits(fraction, 10))
                && !(whole.is_empty() && fraction.is_empty())
        }
        None => digits(mantissa, 10),
    };
    mantissa_ok && exponent_ok
}

/// What asked for suggestions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Typing (Monaco's quick suggestions): there are suggestions only while a word is typed.
    Typing,
    /// Asked for (Ctrl+Space): with no word typed, every word is suggested.
    Invoked,
}

/// Words to suggest at the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestions {
    /// The part of the word typed before the cursor, which a picked word replaces (Monaco's
    /// default `insert` mode leaves the rest of the word).
    pub replace: Range<usize>,
    pub words: Vec<String>,
}

/// The words to suggest at byte `offset` of `text`, a file of Monaco's `language`, from it
/// and from `others`, the texts of the other open files of that language (Monaco's
/// `wordBasedSuggestions: 'matchingDocuments'`): those [`fuzzy`] matches what was typed of
/// the word at the cursor with (its letters in order, the first at a word start or a
/// camelCase hump), but never that word itself or a number. The best matches come first,
/// then in byte order (the label order Monaco breaks ties with). With nothing typed (asked
/// for), every word, in byte order.
pub fn suggestions(text: &str, others: &[String], offset: usize, trigger: Trigger, language: &str) -> Suggestions {
    let pattern = word_pattern(language);
    let word = pattern_word_around(pattern, text, offset);
    let replace = word.start..offset.max(word.start);
    let typed = &text[replace.clone()];
    if typed.is_empty() && trigger == Trigger::Typing {
        return Suggestions { replace, words: Vec::new() };
    }
    let current = &text[word];
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    let documents = std::iter::once(text).chain(others.iter().map(String::as_str));
    for candidate in documents.flat_map(|document| pattern_words(pattern, document)) {
        if candidate == current || is_number(candidate) || !seen.insert(candidate) {
            continue;
        }
        found.push(candidate);
        if seen.len() > SUGGESTIONS_LIMIT {
            break;
        }
    }
    let mut ranked: Vec<(i32, &str)> = if typed.is_empty() {
        found.into_iter().map(|word| (0, word)).collect()
    } else {
        found.into_iter().filter_map(|word| Some((fuzzy::score(typed, word)?.score, word))).collect()
    };
    ranked.sort_by(|(score_a, a), (score_b, b)| score_b.cmp(score_a).then_with(|| a.cmp(b)));
    Suggestions { replace, words: ranked.into_iter().map(|(_, word)| word.to_owned()).collect() }
}

/// The bytes of a suggested `word` that match what was `typed`, as the suggest widget
/// highlights them; none when they do not match.
pub fn highlights(typed: &str, word: &str) -> Vec<Range<usize>> {
    let Some(found) = fuzzy::score(typed, word) else {
        return Vec::new();
    };
    let starts: Vec<(usize, char)> = word.char_indices().collect();
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for position in found.positions {
        let (start, c) = starts[position];
        match ranges.last_mut() {
            Some(last) if last.end == start => last.end = start + c.len_utf8(),
            _ => ranges.push(start..start + c.len_utf8()),
        }
    }
    ranges
}

/// Whether `a` and `b` are the same character in any case.
fn same_ignoring_case(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// Where `needle` (not empty) occurs in `text` in any case, without overlaps.
fn find_ignoring_case(text: &str, needle: &str) -> Vec<Range<usize>> {
    let matches_at = |start: usize| -> Option<usize> {
        let mut rest = text[start..].char_indices();
        for wanted in needle.chars() {
            let (_, c) = rest.next()?;
            if !same_ignoring_case(c, wanted) {
                return None;
            }
        }
        Some(rest.next().map_or(text.len(), |(index, _)| start + index))
    };
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(c) = text[from..].chars().next() {
        match matches_at(from) {
            Some(end) => {
                found.push(from..end);
                from = end;
            }
            None => from += c.len_utf8(),
        }
    }
    found
}

/// Change All Occurrences: with nothing selected, every occurrence of the word at the
/// cursor as a whole word in the same case, words read by the editor's word separators in
/// every language; with a selection, every occurrence of its text in any case (the find
/// widget's defaults). The one at the selection comes first, to stay the primary cursor.
/// Empty when the cursor is not on a word.
pub fn all_occurrences(text: &str, selection: Range<usize>) -> Vec<Range<usize>> {
    let mut found: Vec<Range<usize>> = if selection.is_empty() {
        let word = word_around(text, selection.start);
        let current = &text[word];
        if current.is_empty() {
            return Vec::new();
        }
        words(text).filter(|(_, candidate)| *candidate == current).map(|(range, _)| range).collect()
    } else {
        find_ignoring_case(text, &text[selection.clone()])
    };
    if let Some(primary) = found.iter().position(|range| range.start <= selection.end && selection.start <= range.end) {
        found.swap(0, primary);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The suggestions at `cursor` in `text`, a plain-text file.
    fn words_at(text: &str, cursor: &str, trigger: Trigger) -> Vec<String> {
        words_in("plaintext", text, cursor, trigger)
    }

    /// The suggestions at `cursor` in `text`, a file of Monaco's `language`.
    fn words_in(language: &str, text: &str, cursor: &str, trigger: Trigger) -> Vec<String> {
        suggestions(&text.replace(cursor, ""), &[], text.find(cursor).unwrap(), trigger, language).words
    }

    #[test]
    fn suggests_the_words_starting_with_what_was_typed_in_any_case() {
        let text = "fooBar foobar Football bar food\nfo|";
        assert_eq!(words_at(text, "|", Trigger::Typing), ["fooBar", "foobar", "food", "Football"]);
        // What was typed replaces only the part before the cursor.
        let at = text.find('|').unwrap();
        assert_eq!(suggestions(&text.replace('|', ""), &[], at, Trigger::Typing, "plaintext").replace, at - 2..at);
    }

    #[test]
    fn suggests_words_with_the_typed_letters_in_order_from_a_word_start_or_hump() {
        // `ba` starts the hump of `fooBar` and the part after `_`; `foobar` and `sidebar`
        // have no `b` at a word start.
        assert_eq!(words_at("foobar fooBar sidebar foo_bar\nba|", "|", Trigger::Typing), ["fooBar", "foo_bar"]);
        assert_eq!(words_at("foobar fooBar foo_bar\noo|", "|", Trigger::Typing), Vec::<String>::new());
        // The best matches first: a prefix, then the closest humps; ties in byte order.
        assert_eq!(
            words_at("fooBar foo_bar fileBuffer fbx Fb oops\nfb|", "|", Trigger::Typing),
            ["Fb", "fbx", "fooBar", "fileBuffer", "foo_bar"]
        );
    }

    #[test]
    fn highlights_the_matched_letters() {
        assert_eq!(highlights("fb", "fooBar"), [0..1, 3..4]);
        assert_eq!(highlights("fo", "foobar"), vec![0..2]);
        assert_eq!(highlights("éb", "éBar"), vec![0..3]);
        assert_eq!(highlights("oo", "foobar"), Vec::<Range<usize>>::new());
    }

    #[test]
    fn never_suggests_the_word_being_typed_numbers_or_a_word_twice() {
        assert_eq!(words_at("food food fo 12 0x1f 1e3 1x Infinity\nf|o", "|", Trigger::Typing), ["food"]);
        assert_eq!(words_at("x1 x2 x1\nx|", "|", Trigger::Typing), ["x1", "x2"]);
        assert_eq!(words_at("1x 12 1e 0b2\n1|", "|", Trigger::Typing), ["1e", "1x"]);
    }

    #[test]
    fn words_end_at_separators_and_whitespace() {
        assert_eq!(words_at("a.alpha(beta) a-ant\ta_b\na|", "|", Trigger::Typing), ["a_b", "alpha", "ant"]);
    }

    #[test]
    fn words_are_read_with_the_languages_word_pattern() {
        let typed = |language: &str, text: &str| words_in(language, text, "|", Trigger::Typing);
        // CSS, SCSS and Less: `-` joins words, which may start with `.`, `#`, `@`, `!` or `:`.
        assert_eq!(typed("css", ".btn-primary { font-size: 1.5em }\n.btn|"), [".btn-primary"]);
        assert_eq!(typed("css", ".btn-primary { font-size: 1.5em }\nfo|"), ["font-size"]);
        assert_eq!(typed("scss", "$base-color: red;\n$b|"), ["$base-color"]);
        assert_eq!(typed("less", "@base-color: red;\n@b|"), ["@base-color"]);
        // HTML: `-`, `#`, `%` and `?` join words.
        assert_eq!(typed("html", "<li class=\"nav-item\">\nnav|"), ["nav-item"]);
        // `$` joins TypeScript, JavaScript and PHP words; `@` Java, Kotlin and C# ones.
        assert_eq!(typed("php", "$name = $this->name;\n$n|"), ["$name"]);
        assert_eq!(typed("javascript", "const $el = jQuery;\n$|"), ["$el"]);
        assert_eq!(typed("kotlin", "@JvmStatic fun f() {}\n@J|"), ["@JvmStatic"]);
        assert_eq!(typed("csharp", "var @class = 1;\n@c|"), ["@class"]);
        // JSON: only brackets, `:`, `"`, `,` and whitespace end words.
        assert_eq!(typed("json", "{\"key-name\": -1.5, \"a+b\": true}\nk|"), ["key-name"]);
        // Scala: setters and backquoted names are words.
        assert_eq!(typed("scala", "def value_=(v: Int) = `my val`\nva|"), ["value_=", "`my val`"]);
        // Elsewhere `-`, `$` and `@` end words; a picked word replaces the typed one from its start.
        assert_eq!(typed("rust", "nav-item $name @attr\nna|"), ["name", "nav"]);
        let text = "$name\n$n";
        assert_eq!(suggestions(text, &[], text.len(), Trigger::Typing, "php").replace, 6..8);
        assert_eq!(suggestions(text, &[], text.len(), Trigger::Typing, "rust").replace, 7..8);
    }

    #[test]
    fn numbers_are_what_javascript_reads_as_one() {
        for number in ["12", "-1.5", "+3", ".5", "1.", "1e3", "1.5E-3", "0x1F", "0o17", "0b101", "Infinity", "-Infinity"] {
            assert!(is_number(number), "{number}");
        }
        for word in [".", "-", "1e", "e3", "1.2.3", "1.5em", "0x", "0b2", "-0x1f", "1_000", "#1.5", "10%"] {
            assert!(!is_number(word), "{word}");
        }
        // A CSS word that is a number (`.5`) is not suggested; one with a unit is.
        assert_eq!(words_in("css", "a { margin: 1.5px .5 }\n.|", "|", Trigger::Typing), ["1.5px"]);
    }

    /// The other open files of the language give their words too, once each, but never the
    /// word being typed or a number.
    #[test]
    fn suggests_the_words_of_the_other_open_files_too() {
        let others = ["fond food 12 fo".to_owned(), "fox fond".to_owned()];
        let text = "food\nfo";
        assert_eq!(suggestions(text, &others, text.len(), Trigger::Typing, "plaintext").words, ["fond", "food", "fox"]);
        assert_eq!(suggestions("", &others, 0, Trigger::Invoked, "plaintext").words, ["fo", "fond", "food", "fox"]);
    }

    #[test]
    fn suggests_nothing_while_typing_outside_a_word_but_everything_when_asked() {
        assert_eq!(words_at("alpha beta\n|", "|", Trigger::Typing), Vec::<String>::new());
        assert_eq!(words_at("beta alpha 7\n|", "|", Trigger::Invoked), ["alpha", "beta"]);
    }

    /// The word at `|` in `text`, a file of Monaco's `language`.
    fn word_at_bar(language: &str, text: &str) -> String {
        word_at(&text.replace('|', ""), text.find('|').unwrap(), language).to_owned()
    }

    #[test]
    fn the_word_at_the_cursor_is_read_with_the_languages_word_pattern() {
        assert_eq!(word_at_bar("css", "a { back|ground-color: red }"), "background-color");
        assert_eq!(word_at_bar("typescript", "const $e|l = 1"), "$el");
        assert_eq!(word_at_bar("plaintext", "cost $pr|ice"), "price");
        assert_eq!(word_at_bar("json", "{\"a-|b\": 1}"), "a-b");
        assert_eq!(word_at_bar("html", "<p data-|id=\"1\">"), "data-id");
        assert_eq!(word_at_bar("rust", "let n = -1.|5e3;"), "-1.5e3");
        assert_eq!(word_at_bar("markdown", "naï|ve café"), "naïve");
    }

    #[test]
    fn the_word_at_the_cursor_touches_it_on_either_side_of_its_line() {
        // Right after a word, the first word touching the cursor.
        assert_eq!(word_at_bar("rust", "foo_bar|(baz)"), "foo_bar");
        assert_eq!(word_at_bar("rust", "foo_bar(|baz)"), "baz");
        assert_eq!(word_at_bar("rust", "a\r\nfoo|\r\nb"), "foo");
        assert_eq!(word_at_bar("rust", "a  |  b"), "");
        // Inside a character, or past the end.
        assert_eq!(word_at("é", 1, "rust"), "");
        assert_eq!(word_at("ab", 3, "rust"), "");
    }

    #[test]
    fn html_reads_a_script_block_as_javascript_and_a_style_block_as_css() {
        // HTML (Vue and Svelte files too) keeps `?` and `-` in words and ends them at `$`.
        let vue = "<template>\n  <p>{{ fo|o?.bar }}</p>\n</template>\n<script setup lang=\"ts\">\n";
        assert_eq!(word_at_bar("html", vue), "foo?");
        assert_eq!(word_at_bar("html", "<p>this.$em|it</p>"), "emit");
        assert_eq!(word_at_bar("html", "<p>cou|nt-1</p>"), "count-1");
        let script = "<template><p/></template>\n<script>\nexport default {\n  m() { return {} },\n}\n";
        assert_eq!(word_at_bar("html", &format!("{script}const n = fo|o?.bar;\n</script>")), "foo");
        assert_eq!(word_at_bar("html", &format!("{script}this.$em|it('done');\n</script>")), "$emit");
        assert_eq!(word_at_bar("html", &format!("{script}let n = cou|nt-1;\n</script>")), "count");
        assert_eq!(word_at_bar("html", "<style scoped>\n.b|tn { color: red }\n</style>"), ".btn");
        // Tags in any case; not in a comment; and after the block, HTML again.
        assert_eq!(word_at_bar("html", "<SCRIPT>\n$|x\n</SCRIPT>"), "$x");
        assert_eq!(word_at_bar("html", "<!-- <script> -->\n$|x"), "x");
        assert_eq!(word_at_bar("html", "<script>\n</script>\n$|x"), "x");
    }

    #[test]
    fn a_script_or_style_type_names_the_language_of_its_block() {
        assert_eq!(word_at_bar("html", "<script type=\"module\">$|x</script>"), "$x");
        assert_eq!(word_at_bar("html", "<script type='text/typescript'>$|x</script>"), "$x");
        // A language Monaco does not know reads words with its default pattern.
        assert_eq!(word_at_bar("html", "<script type=\"text/x-template\">$|x</script>"), "x");
        assert_eq!(word_at_bar("html", "<script type=\"text/x-handlebars-template\">{{fo|o-bar}}</script>"), "foo-bar");
        assert_eq!(word_at_bar("html", "<style type=\"text/scss\">$base-co|lor: red;</style>"), "$base-color");
        // HTML in a script block hands its own blocks on.
        assert_eq!(word_at_bar("html", "<script type=\"text/html\">\n<style>.b|tn{}</style>\n</script>"), ".btn");
    }

    #[test]
    fn a_word_is_read_within_the_part_of_the_line_in_its_language() {
        assert_eq!(word_at_bar("html", "<style>.b|tn</style>"), ".btn");
        assert_eq!(word_at_bar("html", "<script>|$x</script>"), "$x");
        // At the start of a part, the word ending there in the part before.
        assert_eq!(word_at_bar("html", "<script>$x|</script>"), "$x");
        assert_eq!(word_at_bar("html", "<script>$x<|/script>"), "");
    }

    #[test]
    fn php_hands_script_and_style_blocks_outside_its_code_on() {
        assert_eq!(word_at_bar("php", "<style>\n.b|tn { }\n</style>"), ".btn");
        assert_eq!(word_at_bar("php", "<style>\n<?php echo $co|lor ?>\n</style>"), "$color");
        assert_eq!(word_at_bar("php", "<style>\n<?php echo $color ?> .b|tn\n</style>"), ".btn");
        // In PHP's code, and in its strings, tags are text.
        assert_eq!(word_at_bar("php", "<?php\necho \"<style>\";\n$na|me = 1;\n"), "$name");
        assert_eq!(word_at_bar("php", "<?php $s = '?><style>';\n$na|me = 1;"), "$name");
        assert_eq!(word_at_bar("php", "<?php $s = 1; ?>\n<style>\n.b|tn\n</style>"), ".btn");
        // PHP's tags are in lower case only.
        assert_eq!(word_at_bar("php", "<STYLE>\n.b|tn\n</STYLE>"), "btn");
    }

    #[test]
    fn a_markdown_code_fence_is_read_in_its_language() {
        assert_eq!(word_at_bar("markdown", "Text $x\n\n```ts\nconst $|x = 1;\n```\n"), "$x");
        assert_eq!(word_at_bar("markdown", "```ts\nconst $x = 1;\n```\n\n$|x"), "x");
        assert_eq!(word_at_bar("markdown", "  ```css title=\"a\"\n.b|tn {}\n  ```"), ".btn");
        assert_eq!(word_at_bar("markdown", "```mermaid\n$|x\n```"), "x");
        assert_eq!(word_at_bar("markdown", "```\n$|x\n```"), "x");
        assert_eq!(word_at_bar("markdown", "```html\n<style>\n.b|tn {}\n</style>\n```"), ".btn");
        // Raw HTML's script blocks too, but not one in inline code.
        assert_eq!(word_at_bar("markdown", "<script>\n$|x\n</script>"), "$x");
        assert_eq!(word_at_bar("markdown", "Add a `<script>` tag.\n\n$|x"), "x");
    }

    #[test]
    fn a_line_too_long_to_tokenize_is_read_in_the_files_language() {
        let long = format!("<script>\nlet $|x = 1; {}\n</script>", "a".repeat(20_000));
        assert_eq!(word_at_bar("html", &long), "x");
        let short = format!("<script>\nlet $|x = 1; {}\n</script>", "a".repeat(19_000));
        assert_eq!(word_at_bar("html", &short), "$x");
    }

    #[test]
    fn a_collapsed_cursor_changes_the_word_wherever_it_stands_alone_in_the_same_case() {
        let text = "foo foobar Foo xfoo foo.bar (foo)";
        let ranges = all_occurrences(text, 21..21);
        assert_eq!(ranges.iter().map(|range| &text[range.clone()]).collect::<Vec<_>>(), ["foo"; 3]);
        assert_eq!(ranges, [20..23, 0..3, 29..32]);
        // At the end of a word.
        assert_eq!(all_occurrences(text, 3..3)[0], 0..3);
        assert_eq!(all_occurrences("a  b", 2..2), Vec::<Range<usize>>::new());
    }

    #[test]
    fn a_collapsed_cursor_reads_words_by_the_editors_separators_in_every_language() {
        // On `name` of PHP's `$name`, `$this->name` changes too, as in Monaco.
        let text = "$name = $this->name;";
        assert_eq!(all_occurrences(text, 2..2), [1..5, 15..19]);
    }

    #[test]
    fn a_selection_changes_its_text_anywhere_in_any_case() {
        let text = "Foo foobar FOO.x fo";
        assert_eq!(all_occurrences(text, 4..7), [4..7, 0..3, 11..14]);
        assert_eq!(all_occurrences("aaa", 1..3), vec![0..2]);
        assert_eq!(all_occurrences("Straße STRASSE straße", 0..7), [0..7, 16..23]);
    }
}
