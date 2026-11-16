//! The Editor tab's word-based aids, as Monaco's: the suggestions its base editor worker
//! offers while typing, in every language (`WordBasedCompletionItemProvider` and the
//! worker's `textualSuggest`), and Change All Occurrences (`editor.action.changeAll`).
//!
//! A word is Monaco's default: a run of characters that are neither whitespace nor one of
//! the editor's word separators. (Monaco's default word pattern also reads `3.14` or `.5`
//! as one word; such words are numbers, which are never suggested.)

use std::collections::HashSet;
use std::ops::Range;

/// Monaco's `editor.wordSeparators`.
const WORD_SEPARATORS: &str = "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?";

/// The worker stops collecting words once it has more than this many (`_suggestionsLimit`).
const SUGGESTIONS_LIMIT: usize = 10_000;

/// Whether `c` can be part of a word.
fn is_word_char(c: char) -> bool {
    !c.is_whitespace() && !WORD_SEPARATORS.contains(c)
}

/// The bytes of the word at byte `offset` of `text` (touching it on either side); empty
/// when there is none.
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
/// before suggesting it. (A word holds no sign or decimal point: they are separators.)
fn is_number(word: &str) -> bool {
    let digits = |text: &str, radix: u32| !text.is_empty() && text.chars().all(|c| c.is_digit(radix));
    let radix = |lower: &str, upper: &str| word.strip_prefix(lower).or_else(|| word.strip_prefix(upper));
    if word == "Infinity" {
        return true;
    }
    if let Some(rest) = radix("0x", "0X") {
        return digits(rest, 16);
    }
    if let Some(rest) = radix("0o", "0O") {
        return digits(rest, 8);
    }
    if let Some(rest) = radix("0b", "0B") {
        return digits(rest, 2);
    }
    match word.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => digits(mantissa, 10) && digits(exponent, 10),
        None => digits(word, 10),
    }
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

/// The words of `text` to suggest at byte `offset`: those that start with what was typed of
/// the word at the cursor, in any case, but never that word itself or a number. Words
/// typed in the same case come first, then in byte order (the label order Monaco breaks
/// ties with).
pub fn suggestions(text: &str, offset: usize, trigger: Trigger) -> Suggestions {
    let word = word_around(text, offset);
    let replace = word.start..offset.max(word.start);
    let typed = &text[replace.clone()];
    if typed.is_empty() && trigger == Trigger::Typing {
        return Suggestions { replace, words: Vec::new() };
    }
    let current = &text[word];
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for (_, candidate) in words(text) {
        if candidate == current || is_number(candidate) || !seen.insert(candidate) {
            continue;
        }
        found.push(candidate);
        if seen.len() > SUGGESTIONS_LIMIT {
            break;
        }
    }
    let typed_lower = typed.to_lowercase();
    let mut words: Vec<String> =
        found.into_iter().filter(|word| word.to_lowercase().starts_with(&typed_lower)).map(str::to_owned).collect();
    words.sort_by(|a, b| (!a.starts_with(typed), a).cmp(&(!b.starts_with(typed), b)));
    Suggestions { replace, words }
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
/// cursor as a whole word in the same case; with a selection, every occurrence of its text
/// in any case (the find widget's defaults). The one at the selection comes first, to stay
/// the primary cursor. Empty when the cursor is not on a word.
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

    fn words_at(text: &str, cursor: &str, trigger: Trigger) -> Vec<String> {
        suggestions(&text.replace(cursor, ""), text.find(cursor).unwrap(), trigger).words
    }

    #[test]
    fn suggests_the_words_starting_with_what_was_typed_in_any_case() {
        let text = "fooBar foobar Football bar food\nfo|";
        assert_eq!(words_at(text, "|", Trigger::Typing), ["fooBar", "foobar", "food", "Football"]);
        // What was typed replaces only the part before the cursor.
        let at = text.find('|').unwrap();
        assert_eq!(suggestions(&text.replace('|', ""), at, Trigger::Typing).replace, at - 2..at);
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
    fn suggests_nothing_while_typing_outside_a_word_but_everything_when_asked() {
        assert_eq!(words_at("alpha beta\n|", "|", Trigger::Typing), Vec::<String>::new());
        assert_eq!(words_at("beta alpha 7\n|", "|", Trigger::Invoked), ["alpha", "beta"]);
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
    fn a_selection_changes_its_text_anywhere_in_any_case() {
        let text = "Foo foobar FOO.x fo";
        assert_eq!(all_occurrences(text, 4..7), [4..7, 0..3, 11..14]);
        assert_eq!(all_occurrences("aaa", 1..3), vec![0..2]);
        assert_eq!(all_occurrences("Straße STRASSE straße", 0..7), [0..7, 16..23]);
    }
}
