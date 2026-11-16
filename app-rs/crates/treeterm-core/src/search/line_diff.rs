//! A two-file line diff grouped into hunks, as `@pierre/diffs`' `parseDiffFromFile` builds
//! it for the diff viewer: jsdiff's `structuredPatch` grouping (4 lines of context, hunks
//! closer than 8 lines merged) over a Myers line diff.
//!
//! Lines keep their terminators (`"a\n"`, `"b\r\n"`, a last line without one), as jsdiff
//! tokenizes them; with [`Whitespace::Ignore`] two lines are equal when equal trimmed.
//!
//! [`ignore_whitespace_in_patch`] diffs a git patch's hunks again the same way, for the
//! Review tab, which shows git's patches.

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
            Whitespace::Ignore => js_trim(line).to_owned(),
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

/// `String.prototype.trim`, which jsdiff's `ignoreWhitespace` compares lines with:
/// JavaScript's white space and line terminators, a byte order mark included (`str::trim`
/// leaves it, and trims U+0085, which JavaScript does not).
fn js_trim(line: &str) -> &str {
    line.trim_matches(|c: char| c == '\u{FEFF}' || (c.is_whitespace() && c != '\u{85}'))
}

/// Whether a line of a hunk ends with a newline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Newline<'a> {
    Kept,
    /// The `\ No newline at end of file` line that follows it.
    Missing(&'a str),
}

/// A hunk's line on one of its sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SideLine<'a> {
    /// Without its tag and `\n` (a CRLF file's `\r` stays).
    text: &'a str,
    newline: Newline<'a>,
}

/// A git patch with its hunks diffed again as jsdiff's `ignoreWhitespace` diffs two files for
/// the diff viewer (`parseDiffOptions: { ignoreWhitespace }`): lines equal once trimmed are
/// unchanged, so a change of indentation, trailing spaces or line ending drops out, while one
/// of spacing within a line stays (`git diff -w` would hide it too). A whole-file patch
/// (`-U1000000`) is diffed as jsdiff diffs the files. Everything outside hunks (file headers,
/// binary notices) and each hunk's header are kept as they are: each side keeps its lines.
///
/// An unchanged line shows the modified side's text, as jsdiff's equal lines are its tokens;
/// the Kit's diff has one text per context line, where Pierre's split view draws each side's.
pub fn ignore_whitespace_in_patch(patch: &str) -> String {
    let lines: Vec<&str> = patch.split_inclusive('\n').collect();
    let mut out = String::with_capacity(patch.len());
    let mut ix = 0;
    while ix < lines.len() {
        out.push_str(lines[ix]);
        ix += 1;
        // A hunk that does not read as git writes one stays as it is, line by line.
        if let Some((old, new, end)) =
            hunk_counts(lines[ix - 1]).and_then(|counts| hunk_sides(&lines, ix, counts))
        {
            push_rediffed(&mut out, &old, &new);
            ix = end;
        }
    }
    out
}

/// The line counts of a hunk header (`@@ -a,b +c,d @@`; a count left out is 1).
fn hunk_counts(line: &str) -> Option<(usize, usize)> {
    let (ranges, _) = line.strip_prefix("@@ -")?.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let count = |range: &str| -> Option<usize> {
        let (start, count) = range.split_once(',').unwrap_or((range, "1"));
        start.parse::<usize>().ok()?;
        count.parse().ok()
    };
    Some((count(old)?, count(new)?))
}

/// The lines of the hunk whose body starts at `lines[start]`, on each side, as many as its
/// header counts, and the index after them (and after a final `\ No newline at end of file`).
/// `None`: it ends early or holds a line that is not a hunk's.
fn hunk_sides<'a>(
    lines: &[&'a str],
    start: usize,
    (old_count, new_count): (usize, usize),
) -> Option<(Vec<SideLine<'a>>, Vec<SideLine<'a>>, usize)> {
    let (mut old, mut new): (Vec<SideLine<'a>>, Vec<SideLine<'a>>) = (Vec::new(), Vec::new());
    // The sides the last line went to, whose newline a `\` line says is missing.
    let (mut to_old, mut to_new) = (false, false);
    let mut ix = start;
    while let Some(line) = lines.get(ix) {
        let text = line.strip_suffix('\n').unwrap_or(line);
        if text.starts_with('\\') {
            for (to, side) in [(to_old, &mut old), (to_new, &mut new)] {
                if let (true, Some(last)) = (to, side.last_mut()) {
                    last.newline = Newline::Missing(text);
                }
            }
            (to_old, to_new) = (false, false);
            ix += 1;
            continue;
        }
        if old.len() == old_count && new.len() == new_count {
            break;
        }
        (to_old, to_new) = match text.as_bytes().first() {
            Some(b' ') => (true, true),
            Some(b'-') => (true, false),
            Some(b'+') => (false, true),
            Some(_) | None => return None,
        };
        let side_line = SideLine {
            text: &text[1..],
            newline: Newline::Kept,
        };
        if to_old {
            old.push(side_line);
        }
        if to_new {
            new.push(side_line);
        }
        if old.len() > old_count || new.len() > new_count {
            return None;
        }
        ix += 1;
    }
    (old.len() == old_count && new.len() == new_count).then_some((old, new, ix))
}

/// A hunk's body diffed again over its sides' trimmed lines.
fn push_rediffed(out: &mut String, old: &[SideLine<'_>], new: &[SideLine<'_>]) {
    let old_keys: Vec<&str> = old.iter().map(|line| js_trim(line.text)).collect();
    let new_keys: Vec<&str> = new.iter().map(|line| js_trim(line.text)).collect();
    for op in capture_diff_slices(Algorithm::Myers, &old_keys, &new_keys) {
        match op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => {
                for k in 0..len {
                    let line = new[new_index + k];
                    // A context line's `\ No newline` ends both sides: the original side must
                    // end there too.
                    let newline = match line.newline {
                        Newline::Missing(_) if old_index + k + 1 < old.len() => Newline::Kept,
                        newline => newline,
                    };
                    push_line(out, ' ', SideLine { newline, ..line });
                }
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => old[old_index..old_index + old_len]
                .iter()
                .for_each(|line| push_line(out, '-', *line)),
            DiffOp::Insert {
                new_index, new_len, ..
            } => new[new_index..new_index + new_len]
                .iter()
                .for_each(|line| push_line(out, '+', *line)),
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                old[old_index..old_index + old_len]
                    .iter()
                    .for_each(|line| push_line(out, '-', *line));
                new[new_index..new_index + new_len]
                    .iter()
                    .for_each(|line| push_line(out, '+', *line));
            }
        }
    }
}

fn push_line(out: &mut String, tag: char, line: SideLine<'_>) {
    out.push(tag);
    out.push_str(line.text);
    out.push('\n');
    if let Newline::Missing(marker) = line.newline {
        out.push_str(marker);
        out.push('\n');
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

    #[test]
    fn whitespace_is_trimmed_as_javascript_trims_it() {
        // A byte order mark is white space to JavaScript, U+0085 is not.
        assert!(
            line_diff("\u{FEFF}a\n", "a\n", Whitespace::Ignore)
                .hunks
                .is_empty()
        );
        assert_eq!(
            line_diff("\u{85}a\n", "a\n", Whitespace::Ignore)
                .hunks
                .len(),
            1
        );
        assert_eq!(js_trim("\t a\u{3000}\r\n"), "a");
    }

    const HEADER: &str =
        "diff --git a/f.txt b/f.txt\nindex 1111111..2222222 100644\n--- a/f.txt\n+++ b/f.txt\n";

    fn file(hunks: &str) -> String {
        format!("{HEADER}{hunks}")
    }

    #[test]
    fn a_change_of_indentation_or_trailing_spaces_is_unchanged() {
        let patch = file("@@ -1,4 +1,4 @@\n a\n-  b\n-c\n+    b\n+c  \n d\n");
        assert_eq!(
            ignore_whitespace_in_patch(&patch),
            file("@@ -1,4 +1,4 @@\n a\n     b\n c  \n d\n")
        );
    }

    #[test]
    fn a_change_of_spacing_within_a_line_stays() {
        let patch = file("@@ -1,3 +1,3 @@\n x\n-a b\n+a  b\n y\n");
        assert_eq!(ignore_whitespace_in_patch(&patch), patch);
    }

    #[test]
    fn changes_are_paired_again_around_lines_equal_once_trimmed() {
        // Git pairs every line; once trimmed, only `two` changed.
        let patch = file("@@ -1,3 +1,3 @@\n-  one\n-two\n-three\n+one\n+TWO\n+three\t\n");
        assert_eq!(
            ignore_whitespace_in_patch(&patch),
            file("@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\t\n")
        );
    }

    #[test]
    fn a_change_of_line_ending_is_unchanged() {
        let patch = file("@@ -1,2 +1,2 @@\n-a\r\n-b\r\n+a\n+b\n");
        assert_eq!(
            ignore_whitespace_in_patch(&patch),
            file("@@ -1,2 +1,2 @@\n a\n b\n")
        );
    }

    const NO_NEWLINE: &str = "\\ No newline at end of file\n";

    #[test]
    fn a_final_newline_added_or_dropped_is_unchanged() {
        // Dropped: the line keeps the modified side's missing newline.
        let dropped = file(&format!("@@ -1,2 +1,2 @@\n a\n-b\n+b\n{NO_NEWLINE}"));
        assert_eq!(
            ignore_whitespace_in_patch(&dropped),
            file(&format!("@@ -1,2 +1,2 @@\n a\n b\n{NO_NEWLINE}"))
        );
        // Added.
        let added = file(&format!("@@ -1,2 +1,2 @@\n a\n-b\n{NO_NEWLINE}+b\n"));
        assert_eq!(
            ignore_whitespace_in_patch(&added),
            file("@@ -1,2 +1,2 @@\n a\n b\n")
        );
        // Missing on both sides, and the line indented: it stays missing.
        let both = file(&format!(
            "@@ -1,2 +1,2 @@\n a\n-b\n{NO_NEWLINE}+  b\n{NO_NEWLINE}"
        ));
        assert_eq!(
            ignore_whitespace_in_patch(&both),
            file(&format!("@@ -1,2 +1,2 @@\n a\n   b\n{NO_NEWLINE}"))
        );
        // Untouched context keeps its marker.
        let context = file(&format!("@@ -1,2 +1,2 @@\n-a\n+A\n b\n{NO_NEWLINE}"));
        assert_eq!(ignore_whitespace_in_patch(&context), context);
    }

    #[test]
    fn a_line_without_newline_the_original_side_goes_on_after_has_none_marked() {
        // jsdiff writes ` b`, `\ No newline…`, `-c`: a marker after a context line ends both
        // sides, and the Kit refuses a line after it.
        let patch = file(&format!("@@ -1,3 +1,2 @@\n a\n-b\n-c\n+b\n{NO_NEWLINE}"));
        assert_eq!(
            ignore_whitespace_in_patch(&patch),
            file("@@ -1,3 +1,2 @@\n a\n b\n-c\n")
        );
        // A deleted or added final line keeps its marker.
        let deleted = file(&format!("@@ -1,2 +1,1 @@\n a\n-b\n{NO_NEWLINE}"));
        assert_eq!(ignore_whitespace_in_patch(&deleted), deleted);
    }

    #[test]
    fn file_headers_and_binary_notices_are_kept_as_they_are() {
        let patch = [
            "diff --git a/old.txt b/new.txt\nold mode 100644\nnew mode 100755\nsimilarity index 90%\nrename from old.txt\nrename to new.txt\nindex 1111111..2222222\n--- a/old.txt\n+++ b/new.txt\n",
            "@@ -1,2 +1,2 @@ fn heading()\n-  x\n+x\n y\n",
            "diff --git a/img.png b/img.png\nindex 3333333..4444444 100644\nBinary files a/img.png and b/img.png differ\n",
            "diff --git a/run.sh b/run.sh\nold mode 100644\nnew mode 100755\n",
            "diff --git a/gone.txt b/gone.txt\ndeleted file mode 100644\nindex 5555555..0000000\n--- a/gone.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-gone\n",
            "diff --git a/new.txt b/new.txt\nnew file mode 100644\nindex 0000000..6666666\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1 @@\n+  new\n",
        ]
        .concat();
        assert_eq!(
            ignore_whitespace_in_patch(&patch),
            patch.replace("-  x\n+x\n y\n", " x\n y\n")
        );
    }

    #[test]
    fn each_hunk_of_each_file_is_diffed_on_its_own() {
        let first =
            file("@@ -1,2 +1,2 @@\n-a\n+ a\n b\n@@ -10,2 +10,3 @@ heading\n x\n-y\n+ y\n+z\n");
        let second = "diff --git a/g.txt b/g.txt\nindex 7777777..8888888 100644\n--- a/g.txt\n+++ b/g.txt\n@@ -1 +1 @@\n-p q\n+p  q\n";
        assert_eq!(
            ignore_whitespace_in_patch(&format!("{first}{second}")),
            format!(
                "{}{second}",
                file("@@ -1,2 +1,2 @@\n  a\n b\n@@ -10,2 +10,3 @@ heading\n x\n  y\n+z\n")
            )
        );
    }

    #[test]
    fn a_hunk_that_does_not_read_as_git_writes_one_is_kept() {
        for hunks in [
            // Ends before its counts.
            "@@ -1,3 +1,3 @@\n-  a\n+a\n",
            // Holds a line that is not a hunk's.
            "@@ -1,2 +1,2 @@\n-  a\n+a\n?b\n b\n",
            // More lines than counted.
            "@@ -1,1 +1,1 @@\n-  a\n-b\n+a\n",
        ] {
            let patch = file(hunks);
            assert_eq!(ignore_whitespace_in_patch(&patch), patch);
        }
        assert_eq!(hunk_counts("@@ -1 +1 @@"), Some((1, 1)));
        assert_eq!(hunk_counts("@@ -0,0 +1,3 @@ x\n"), Some((0, 3)));
        assert_eq!(hunk_counts("@@@ -1,2 -1,2 +1,3 @@@\n"), None);
        assert_eq!(hunk_counts("@@ -a,1 +1 @@\n"), None);
    }
}
