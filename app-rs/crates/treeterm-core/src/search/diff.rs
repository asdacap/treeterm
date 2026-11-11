//! Search within a diff, a file and around a cursor (port of `utils/diffSearch.ts`).
//!
//! A diff is searched the way the viewer shows it: the same hunks, in on-screen row order.
//! Lines folded away inside a collapsed unchanged region are skipped, since there is no
//! rendered row to scroll to.
//!
//! Positions are 1-based lines and 1-based byte columns (Monaco counted UTF-16 units); an
//! offset into a line is a byte offset.

use std::sync::LazyLock;

use regex::Regex;

use super::line_diff::{HunkBlock, Whitespace, line_diff};
use crate::git::FileDiffContents;

/// An unchanged gap between hunks of at most this many lines is shown even when unchanged
/// regions are collapsed (`DEFAULT_COLLAPSED_CONTEXT_THRESHOLD`).
pub const COLLAPSED_CONTEXT_THRESHOLD: usize = 1;

/// Which file a row's line number counts in (`@pierre/diffs`' `SelectionSide`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffSearchSide {
    Additions,
    Deletions,
}

impl DiffSearchSide {
    pub fn as_str(self) -> &'static str {
        match self {
            DiffSearchSide::Additions => "additions",
            DiffSearchSide::Deletions => "deletions",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DiffSearchHit {
    pub side: DiffSearchSide,
    /// 1-based, in the side's file.
    pub line_number: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffStyle {
    /// Side by side: a row pairs the nth deletion with the nth addition.
    Split,
    Unified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnchangedRegions {
    Collapsed,
    Expanded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DiffSearchOptions {
    pub diff_style: DiffStyle,
    pub unchanged: UnchangedRegions,
    pub whitespace: Whitespace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Case {
    Insensitive,
    Sensitive,
}

/// Smart case: an all-lowercase query matches case-insensitively; any capital letter makes
/// it exact.
fn case_of(query: &str) -> Case {
    if query.to_lowercase() == query {
        Case::Insensitive
    } else {
        Case::Sensitive
    }
}

/// `createLineMatcher`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMatcher {
    query: String,
    case: Case,
}

impl LineMatcher {
    pub fn new(query: &str) -> Self {
        Self {
            query: query.to_owned(),
            case: case_of(query),
        }
    }

    pub fn matches(&self, text: &str) -> bool {
        match self.case {
            Case::Sensitive => text.contains(&self.query),
            Case::Insensitive => text.to_lowercase().contains(&self.query),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextSearchHit {
    /// 1-based.
    pub line_number: usize,
    /// 1-based byte column in the line.
    pub column: usize,
}

/// Every occurrence of `query` in `text`, non-overlapping, with the smart-case rule of
/// [`LineMatcher`].
pub fn find_in_text(text: &str, query: &str) -> Vec<TextSearchHit> {
    if query.is_empty() {
        return Vec::new();
    }
    let case = case_of(query);
    let mut hits = Vec::new();
    for (index, line) in text.split('\n').enumerate() {
        // The searched text, and for each of its bytes the offset in `line` it came from
        // (lowercasing can change a character's length).
        let (haystack, origin): (String, Vec<usize>) = match case {
            Case::Sensitive => (line.to_owned(), (0..line.len()).collect()),
            Case::Insensitive => {
                let mut lowered = String::with_capacity(line.len());
                let mut origin = Vec::with_capacity(line.len());
                for (offset, c) in line.char_indices() {
                    for lower in c.to_lowercase() {
                        lowered.push(lower);
                        origin.extend(std::iter::repeat_n(offset, lower.len_utf8()));
                    }
                }
                (lowered, origin)
            }
        };
        let mut from = 0;
        while let Some(at) = haystack[from..].find(query).map(|at| at + from) {
            hits.push(TextSearchHit {
                line_number: index + 1,
                column: origin[at] + 1,
            });
            from = at + query.len();
        }
    }
    hits
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    hit: DiffSearchHit,
    text: String,
}

/// A text diff laid out as rows for searching. Building it parses the diff, the expensive
/// part, so keep it while the contents and options stay the same (TS cached it per contents
/// object) and search it on every keystroke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchableDiff {
    rows: Vec<Row>,
}

impl SearchableDiff {
    pub fn new(original: &str, modified: &str, options: DiffSearchOptions) -> Self {
        let diff = line_diff(original, modified, options.whitespace);
        if diff.hunks.is_empty() {
            return Self { rows: Vec::new() };
        }

        let mut rows = Vec::new();
        let mut push = |side: DiffSearchSide, index: usize| {
            let lines = match side {
                DiffSearchSide::Additions => &diff.addition_lines,
                DiffSearchSide::Deletions => &diff.deletion_lines,
            };
            if let Some(text) = lines.get(index) {
                rows.push(Row {
                    hit: DiffSearchHit {
                        side,
                        line_number: index + 1,
                    },
                    text: text.clone(),
                });
            }
        };
        // Unchanged lines between hunks: shown when expanded, or when the gap is too small
        // to be worth collapsing.
        let push_gap = |push: &mut dyn FnMut(DiffSearchSide, usize), from: usize, to: usize| {
            let collapsed = options.unchanged == UnchangedRegions::Collapsed
                && to.saturating_sub(from) > COLLAPSED_CONTEXT_THRESHOLD;
            if !collapsed {
                for index in from..to {
                    push(DiffSearchSide::Additions, index);
                }
            }
        };

        let mut next_addition = 0;
        for hunk in &diff.hunks {
            push_gap(&mut push, next_addition, hunk.addition_line_index);
            for block in &hunk.blocks {
                match *block {
                    HunkBlock::Context {
                        lines,
                        addition_line_index,
                        ..
                    } => {
                        for i in 0..lines {
                            push(DiffSearchSide::Additions, addition_line_index + i);
                        }
                    }
                    HunkBlock::Change {
                        deletions,
                        additions,
                        addition_line_index,
                        deletion_line_index,
                    } => match options.diff_style {
                        DiffStyle::Split => {
                            for i in 0..deletions.max(additions) {
                                if i < deletions {
                                    push(DiffSearchSide::Deletions, deletion_line_index + i);
                                }
                                if i < additions {
                                    push(DiffSearchSide::Additions, addition_line_index + i);
                                }
                            }
                        }
                        DiffStyle::Unified => {
                            for i in 0..deletions {
                                push(DiffSearchSide::Deletions, deletion_line_index + i);
                            }
                            for i in 0..additions {
                                push(DiffSearchSide::Additions, addition_line_index + i);
                            }
                        }
                    },
                }
            }
            next_addition = hunk.addition_line_index + hunk.addition_count;
        }
        push_gap(&mut push, next_addition, diff.addition_lines.len());
        Self { rows }
    }

    /// The rows matching `query`, in on-screen order; none for an empty query.
    pub fn find(&self, query: &str) -> Vec<DiffSearchHit> {
        if query.is_empty() {
            return Vec::new();
        }
        let matcher = LineMatcher::new(query);
        self.rows
            .iter()
            .filter(|row| matcher.matches(&row.text))
            .map(|row| row.hit)
            .collect()
    }
}

/// `findLinesInDiff`. Image and binary diffs have no text lines to search.
pub fn find_lines_in_diff(
    contents: &FileDiffContents,
    query: &str,
    options: DiffSearchOptions,
) -> Vec<DiffSearchHit> {
    match contents {
        FileDiffContents::Text {
            original_content,
            modified_content,
            ..
        } if !query.is_empty() => {
            SearchableDiff::new(original_content, modified_content, options).find(query)
        }
        FileDiffContents::Text { .. }
        | FileDiffContents::Image { .. }
        | FileDiffContents::Binary => Vec::new(),
    }
}

static WORD_CHAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}_$]$").expect("valid regex"));

fn is_word_char(c: char) -> bool {
    WORD_CHAR.is_match(c.encode_utf8(&mut [0; 4]))
}

/// The word around byte `offset` in `text`; empty when `offset` sits outside a word (or
/// inside a character, or past the end).
pub fn word_at(text: &str, offset: usize) -> &str {
    if offset > text.len() || !text.is_char_boundary(offset) {
        return "";
    }
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word_char(*c))
        .last()
        .map_or(offset, |(index, _)| index);
    let end = text[offset..]
        .char_indices()
        .find(|(_, c)| !is_word_char(*c))
        .map_or(text.len(), |(index, _)| offset + index);
    &text[start..end]
}

#[cfg(test)]
mod tests {
    //! Port of `utils/diffSearch.test.ts`, case by case.

    use super::*;

    fn lines(count: usize) -> String {
        (1..=count).map(|i| format!("line {i}\n")).collect()
    }

    fn contents(original: &str, modified: &str) -> FileDiffContents {
        FileDiffContents::Text {
            original_content: original.to_owned(),
            modified_content: modified.to_owned(),
            language: "plaintext".to_owned(),
        }
    }

    const UNIFIED: DiffSearchOptions = DiffSearchOptions {
        diff_style: DiffStyle::Unified,
        unchanged: UnchangedRegions::Collapsed,
        whitespace: Whitespace::Compare,
    };

    fn hit(side: DiffSearchSide, line_number: usize) -> DiffSearchHit {
        DiffSearchHit { side, line_number }
    }

    use DiffSearchSide::{Additions, Deletions};

    mod create_line_matcher {
        use super::*;

        #[test]
        fn matches_case_insensitively_for_a_lowercase_query() {
            assert!(LineMatcher::new("foo").matches("const FOO = 1"));
        }

        #[test]
        fn matches_exactly_once_the_query_has_a_capital_letter() {
            let matcher = LineMatcher::new("Foo");
            assert!(matcher.matches("const Foo = 1"));
            assert!(!matcher.matches("const foo = 1"));
        }
    }

    mod find_lines_in_diff {
        use super::*;

        #[test]
        fn returns_no_hits_for_an_empty_query() {
            assert_eq!(
                find_lines_in_diff(&contents("a\n", "b\n"), "", UNIFIED),
                vec![]
            );
        }

        #[test]
        fn finds_added_and_deleted_lines_on_their_own_side() {
            let before = lines(10);
            let after = before.replace("line 5\n", "line five\n");
            assert_eq!(
                find_lines_in_diff(&contents(&before, &after), "line 5", UNIFIED),
                vec![hit(Deletions, 5)]
            );
            assert_eq!(
                find_lines_in_diff(&contents(&before, &after), "five", UNIFIED),
                vec![hit(Additions, 5)]
            );
        }

        #[test]
        fn skips_unchanged_lines_folded_away_between_hunks() {
            let before = lines(40);
            let after = before
                .replace("line 2\n", "changed\n")
                .replace("line 38\n", "changed\n");
            assert_eq!(
                find_lines_in_diff(&contents(&before, &after), "line 20", UNIFIED),
                vec![]
            );
        }

        #[test]
        fn includes_every_unchanged_line_when_unchanged_regions_are_expanded() {
            let before = lines(40);
            let after = before
                .replace("line 2\n", "changed\n")
                .replace("line 38\n", "changed\n");
            let expanded = DiffSearchOptions {
                unchanged: UnchangedRegions::Expanded,
                ..UNIFIED
            };
            assert_eq!(
                find_lines_in_diff(&contents(&before, &after), "line 20", expanded),
                vec![hit(Additions, 20)]
            );
        }

        #[test]
        fn orders_unified_changes_as_all_deletions_then_all_additions() {
            let before = "keep\nold a\nold b\nkeep\n";
            let after = "keep\nnew a\nnew b\nkeep\n";
            assert_eq!(
                find_lines_in_diff(&contents(before, after), " a", UNIFIED),
                vec![hit(Deletions, 2), hit(Additions, 2)]
            );
            let sides: Vec<_> = find_lines_in_diff(&contents(before, after), " ", UNIFIED)
                .into_iter()
                .map(|hit| hit.side)
                .collect();
            assert_eq!(sides, vec![Deletions, Deletions, Additions, Additions]);
        }

        #[test]
        fn orders_split_changes_row_by_row() {
            let before = "keep\nold a\nold b\nkeep\n";
            let after = "keep\nnew a\nnew b\nkeep\n";
            let split = DiffSearchOptions {
                diff_style: DiffStyle::Split,
                ..UNIFIED
            };
            assert_eq!(
                find_lines_in_diff(&contents(before, after), " ", split),
                vec![
                    hit(Deletions, 2),
                    hit(Additions, 2),
                    hit(Deletions, 3),
                    hit(Additions, 3)
                ]
            );
        }

        #[test]
        fn returns_no_hits_when_both_sides_are_identical() {
            assert_eq!(
                find_lines_in_diff(&contents("same\n", "same\n"), "same", UNIFIED),
                vec![]
            );
        }

        #[test]
        fn shows_a_one_line_gap_between_hunks_even_when_collapsed() {
            let before = lines(20);
            // Changes at lines 5 and 15: the hunks cover lines 1-9 and 11-19, leaving line 10
            // alone between them.
            let after = before
                .replace("line 5\n", "x\n")
                .replace("line 15\n", "y\n");
            assert_eq!(
                find_lines_in_diff(&contents(&before, &after), "line 10", UNIFIED),
                vec![hit(Additions, 10)]
            );
        }
    }

    mod find_lines_in_diff_on_non_text_diffs {
        use super::*;

        #[test]
        fn finds_nothing_in_image_or_binary_diffs() {
            let image = FileDiffContents::Image {
                original_base64: "aGk=".to_owned(),
                modified_base64: String::new(),
            };
            assert_eq!(find_lines_in_diff(&image, "a", UNIFIED), vec![]);
            assert_eq!(
                find_lines_in_diff(&FileDiffContents::Binary, "a", UNIFIED),
                vec![]
            );
        }
    }

    mod find_in_text {
        use super::*;

        fn at(line_number: usize, column: usize) -> TextSearchHit {
            TextSearchHit {
                line_number,
                column,
            }
        }

        #[test]
        fn finds_every_occurrence_with_1_based_line_and_column() {
            assert_eq!(
                find_in_text("foo bar foo\nbaz\n  foo", "foo"),
                vec![at(1, 1), at(1, 9), at(3, 3)]
            );
        }

        #[test]
        fn is_case_insensitive_for_a_lowercase_query_and_case_sensitive_otherwise() {
            assert_eq!(find_in_text("Foo foo", "foo").len(), 2);
            assert_eq!(find_in_text("Foo foo", "Foo"), vec![at(1, 1)]);
        }

        #[test]
        fn finds_nothing_for_an_empty_query() {
            assert_eq!(find_in_text("foo", ""), vec![]);
        }

        #[test]
        fn columns_are_byte_offsets_into_the_original_line() {
            // 'İ' lowercases to two characters; the hit still points into the original.
            assert_eq!(find_in_text("İx café", "café"), vec![at(1, 5)]);
            assert_eq!(find_in_text("aaaa", "aa"), vec![at(1, 1), at(1, 3)]);
        }
    }

    mod word_at {
        use super::*;

        #[test]
        fn finds_the_word() {
            let cases = [
                ("inside a word", "const needle = 1", 8, "needle"),
                ("at the start of a word", "const needle = 1", 6, "needle"),
                ("at the end of a word", "const needle = 1", 12, "needle"),
                ("at the start of the text", "foo.bar", 0, "foo"),
                ("at the end of the text", "foo.bar", 7, "bar"),
                ("with $ and _", "x = $el_name;", 7, "$el_name"),
                ("with non-ASCII letters", "let café = 1", 6, "café"),
                ("on whitespace", "a   b", 2, ""),
            ];
            for (label, text, offset, word) in cases {
                assert_eq!(word_at(text, offset), word, "{label}");
            }
        }

        #[test]
        fn is_empty_inside_a_character_or_past_the_end() {
            assert_eq!(word_at("é", 1), "");
            assert_eq!(word_at("ab", 3), "");
        }
    }
}
