//! A two-file line diff grouped into hunks, as `@pierre/diffs`' `parseDiffFromFile` builds
//! it for the diff viewer: jsdiff's `structuredPatch` grouping (4 lines of context, hunks
//! closer than 8 lines merged) over a Myers line diff.
//!
//! Lines keep their terminators (`"a\n"`, `"b\r\n"`, a last line without one), as jsdiff
//! tokenizes them; with [`Whitespace::Ignore`] two lines are equal when equal trimmed.

use similar::{Algorithm, DiffOp, capture_diff_slices};

/// Context lines around each change (jsdiff's default).
pub const CONTEXT_LINES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Whitespace {
    Compare,
    Ignore,
}

/// A run of lines within a hunk. Indices are 0-based into [`LineDiff`]'s line lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkBlock {
    /// Lines on both sides.
    Context {
        lines: usize,
        addition_line_index: usize,
        deletion_line_index: usize,
    },
    /// Deleted and added lines, in any interleaving.
    Change {
        deletions: usize,
        additions: usize,
        addition_line_index: usize,
        deletion_line_index: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// First modified-side line the hunk covers (0-based).
    pub addition_line_index: usize,
    /// Modified-side lines it covers, context included.
    pub addition_count: usize,
    pub deletion_line_index: usize,
    pub deletion_count: usize,
    pub blocks: Vec<HunkBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineDiff {
    /// The modified file's lines.
    pub addition_lines: Vec<String>,
    /// The original file's lines.
    pub deletion_lines: Vec<String>,
    /// Empty when the files are equal.
    pub hunks: Vec<Hunk>,
}

/// Split after each `\n`, keeping it (`split(/(?<=\n)/)`); `""` has no lines.
fn split_lines(content: &str) -> Vec<String> {
    content.split_inclusive('\n').map(str::to_owned).collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tag {
    Equal,
    Removed,
    Added,
}

#[derive(Clone, Copy)]
struct Component {
    tag: Tag,
    len: usize,
}

/// One line of a hunk being built.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HunkLine {
    Context,
    Deletion,
    Addition,
}

pub fn line_diff(original: &str, modified: &str, whitespace: Whitespace) -> LineDiff {
    let deletion_lines = split_lines(original);
    let addition_lines = split_lines(modified);
    let key = |line: &String| -> String {
        match whitespace {
            Whitespace::Compare => line.clone(),
            Whitespace::Ignore => line.trim().to_owned(),
        }
    };
    let old_keys: Vec<String> = deletion_lines.iter().map(key).collect();
    let new_keys: Vec<String> = addition_lines.iter().map(key).collect();
    let ops = capture_diff_slices(Algorithm::Myers, &old_keys, &new_keys);

    let mut components: Vec<Component> = Vec::new();
    let mut push = |tag: Tag, len: usize| {
        if len == 0 {
            return;
        }
        match components.last_mut() {
            Some(last) if last.tag == tag => last.len += len,
            _ => components.push(Component { tag, len }),
        }
    };
    for op in ops {
        match op {
            DiffOp::Equal { len, .. } => push(Tag::Equal, len),
            DiffOp::Delete { old_len, .. } => push(Tag::Removed, old_len),
            DiffOp::Insert { new_len, .. } => push(Tag::Added, new_len),
            DiffOp::Replace {
                old_len, new_len, ..
            } => {
                push(Tag::Removed, old_len);
                push(Tag::Added, new_len);
            }
        }
    }

    LineDiff {
        hunks: group_hunks(&components),
        addition_lines,
        deletion_lines,
    }
}

/// jsdiff's `structuredPatch` grouping, then `@pierre/diffs`' blocks.
fn group_hunks(components: &[Component]) -> Vec<Hunk> {
    struct Open {
        old_start: usize,
        new_start: usize,
        lines: Vec<HunkLine>,
    }

    let mut hunks = Vec::new();
    let mut open: Option<Open> = None;
    // 0-based positions of the next line on each side.
    let (mut old_line, mut new_line) = (0usize, 0usize);
    // A trailing empty component closes the last hunk.
    let sentinel = Component {
        tag: Tag::Equal,
        len: 0,
    };
    let all: Vec<Component> = components
        .iter()
        .copied()
        .chain(std::iter::once(sentinel))
        .collect();

    for (i, current) in all.iter().enumerate() {
        match current.tag {
            Tag::Added | Tag::Removed => {
                let hunk = open.get_or_insert_with(|| {
                    let context = match i.checked_sub(1).map(|prev| all[prev]) {
                        Some(prev) => prev.len.min(CONTEXT_LINES),
                        None => 0,
                    };
                    Open {
                        old_start: old_line - context,
                        new_start: new_line - context,
                        lines: vec![HunkLine::Context; context],
                    }
                });
                if current.tag == Tag::Added {
                    hunk.lines
                        .extend(std::iter::repeat_n(HunkLine::Addition, current.len));
                    new_line += current.len;
                } else {
                    hunk.lines
                        .extend(std::iter::repeat_n(HunkLine::Deletion, current.len));
                    old_line += current.len;
                }
            }
            Tag::Equal => {
                if let Some(mut hunk) = open.take() {
                    if current.len <= CONTEXT_LINES * 2 && i + 2 < all.len() {
                        hunk.lines
                            .extend(std::iter::repeat_n(HunkLine::Context, current.len));
                        open = Some(hunk);
                    } else {
                        let context = current.len.min(CONTEXT_LINES);
                        hunk.lines
                            .extend(std::iter::repeat_n(HunkLine::Context, context));
                        hunks.push(build_hunk(hunk.old_start, hunk.new_start, &hunk.lines));
                    }
                }
                old_line += current.len;
                new_line += current.len;
            }
        }
    }
    hunks
}

fn build_hunk(old_start: usize, new_start: usize, lines: &[HunkLine]) -> Hunk {
    let (mut deletion_index, mut addition_index) = (old_start, new_start);
    let mut blocks: Vec<HunkBlock> = Vec::new();
    for line in lines {
        match (line, blocks.last_mut()) {
            (HunkLine::Context, Some(HunkBlock::Context { lines, .. })) => *lines += 1,
            (HunkLine::Context, _) => blocks.push(HunkBlock::Context {
                lines: 1,
                addition_line_index: addition_index,
                deletion_line_index: deletion_index,
            }),
            (HunkLine::Deletion | HunkLine::Addition, Some(HunkBlock::Change { .. })) => {}
            (HunkLine::Deletion | HunkLine::Addition, _) => blocks.push(HunkBlock::Change {
                deletions: 0,
                additions: 0,
                addition_line_index: addition_index,
                deletion_line_index: deletion_index,
            }),
        }
        match (line, blocks.last_mut()) {
            (HunkLine::Deletion, Some(HunkBlock::Change { deletions, .. })) => *deletions += 1,
            (HunkLine::Addition, Some(HunkBlock::Change { additions, .. })) => *additions += 1,
            _ => {}
        }
        match line {
            HunkLine::Context => {
                deletion_index += 1;
                addition_index += 1;
            }
            HunkLine::Deletion => deletion_index += 1,
            HunkLine::Addition => addition_index += 1,
        }
    }
    Hunk {
        addition_line_index: new_start,
        addition_count: addition_index - new_start,
        deletion_line_index: old_start,
        deletion_count: deletion_index - old_start,
        blocks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(count: usize) -> String {
        (1..=count).map(|i| format!("line {i}\n")).collect()
    }

    #[test]
    fn equal_files_have_no_hunks() {
        assert!(
            line_diff("a\nb\n", "a\nb\n", Whitespace::Compare)
                .hunks
                .is_empty()
        );
        assert!(line_diff("", "", Whitespace::Compare).hunks.is_empty());
    }

    #[test]
    fn a_change_gets_four_lines_of_context_each_side() {
        let before = numbered(20);
        let after = before.replace("line 10\n", "ten\n");
        let diff = line_diff(&before, &after, Whitespace::Compare);
        assert_eq!(
            diff.hunks,
            vec![Hunk {
                addition_line_index: 5,
                addition_count: 9,
                deletion_line_index: 5,
                deletion_count: 9,
                blocks: vec![
                    HunkBlock::Context {
                        lines: 4,
                        addition_line_index: 5,
                        deletion_line_index: 5
                    },
                    HunkBlock::Change {
                        deletions: 1,
                        additions: 1,
                        addition_line_index: 9,
                        deletion_line_index: 9
                    },
                    HunkBlock::Context {
                        lines: 4,
                        addition_line_index: 10,
                        deletion_line_index: 10
                    },
                ],
            }]
        );
    }

    #[test]
    fn changes_up_to_eight_lines_apart_share_a_hunk() {
        let before = numbered(40);
        let close = before
            .replace("line 10\n", "x\n")
            .replace("line 19\n", "y\n");
        assert_eq!(
            line_diff(&before, &close, Whitespace::Compare).hunks.len(),
            1
        );
        let apart = before
            .replace("line 10\n", "x\n")
            .replace("line 20\n", "y\n");
        assert_eq!(
            line_diff(&before, &apart, Whitespace::Compare).hunks.len(),
            2
        );
    }

    #[test]
    fn a_new_file_is_one_hunk_of_additions() {
        let diff = line_diff("", "a\nb", Whitespace::Compare);
        assert_eq!(diff.addition_lines, vec!["a\n", "b"]);
        assert_eq!(
            diff.hunks,
            vec![Hunk {
                addition_line_index: 0,
                addition_count: 2,
                deletion_line_index: 0,
                deletion_count: 0,
                blocks: vec![HunkBlock::Change {
                    deletions: 0,
                    additions: 2,
                    addition_line_index: 0,
                    deletion_line_index: 0
                }],
            }]
        );
    }

    #[test]
    fn ignoring_whitespace_compares_trimmed_lines() {
        assert!(
            line_diff("a\n  b\n", "a\nb  \n", Whitespace::Ignore)
                .hunks
                .is_empty()
        );
        assert_eq!(
            line_diff("a\n  b\n", "a\nb  \n", Whitespace::Compare)
                .hunks
                .len(),
            1
        );
    }

    #[test]
    fn a_missing_final_newline_is_a_change() {
        assert_eq!(line_diff("a\n", "a", Whitespace::Compare).hunks.len(), 1);
    }
}
