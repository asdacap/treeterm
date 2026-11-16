//! The Editor tab's word-based aids, as Monaco's: Change All Occurrences
//! (`editor.action.changeAll`).
//!
//! A word is Monaco's default: a run of characters that are neither whitespace nor one of
//! the editor's word separators.

use std::ops::Range;

/// Monaco's `editor.wordSeparators`.
const WORD_SEPARATORS: &str = "`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?";

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
