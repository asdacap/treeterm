//! Find in Review's diff (the find bar of `StackedDiffList.tsx`, over `findLinesInDiff`).
//!
//! The diff is searched the way it is shown: GPUI Kit's `Diff` over the review's patch,
//! which pairs the nth deleted line of a change with its nth added line, keeps
//! [`DIFF_CONTEXT`] unchanged lines around each change while unchanged regions are folded,
//! and still shows a fold shorter than [`MIN_COLLAPSED_LINES`]. Rows come in on-screen order
//! (side by side: each deletion before its addition; unified: a change's deletions before
//! its additions), and lines folded away are skipped as Electron skips them. An unchanged
//! line is found on the new side.

use treeterm_core::review::Side;
use treeterm_core::search::{DiffStyle, LineMatcher, UnchangedRegions};

/// Unchanged lines kept around each change before the rest folds (Pierre showed 4).
pub const DIFF_CONTEXT: usize = 4;

/// A fold hiding fewer lines than this is shown instead (GPUI Kit's default).
pub const MIN_COLLAPSED_LINES: usize = 2;

/// How the diff lays its rows out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffLayout {
    pub style: DiffStyle,
    pub unchanged: UnchangedRegions,
}

/// A line the diff shows that matches the query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffMatch {
    pub path: String,
    pub side: Side,
    /// 1-based, in `side`'s file.
    pub line: u32,
    /// The whole line.
    pub preview: String,
}

/// One source line of a side.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceLine {
    line: u32,
    text: String,
}

/// A row of GPUI Kit's diff: an unchanged line on both sides, or the nth line of a
/// change's deletions beside its nth addition.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pair {
    Unchanged { modified: SourceLine },
    Changed { original: Option<SourceLine>, modified: Option<SourceLine> },
}

impl Pair {
    fn is_changed(&self) -> bool {
        matches!(self, Pair::Changed { .. })
    }
}

/// A file of the patch: its pairs, and which pairs each hunk holds.
#[derive(Debug, Default)]
struct PatchFile {
    path: String,
    pairs: Vec<Pair>,
    hunks: Vec<std::ops::Range<usize>>,
}

impl PatchFile {
    fn flush(&mut self, deleted: &mut Vec<SourceLine>, added: &mut Vec<SourceLine>) {
        let (mut deleted, mut added) = (std::mem::take(deleted).into_iter(), std::mem::take(added).into_iter());
        loop {
            match (deleted.next(), added.next()) {
                (None, None) => break,
                (original, modified) => self.pairs.push(Pair::Changed { original, modified }),
            }
        }
    }

    fn end_hunk(&mut self, start: usize) {
        if self.pairs.len() > start {
            self.hunks.push(start..self.pairs.len());
        }
    }
}

/// The patch's files, in patch order, keyed by the path the diff shows them under (the new
/// path; the old one for a deleted file).
fn parse(patch: &str) -> Vec<PatchFile> {
    let mut files: Vec<PatchFile> = Vec::new();
    let mut file: Option<PatchFile> = None;
    let mut old_path: Option<String> = None;
    let (mut deleted, mut added) = (Vec::new(), Vec::new());
    let (mut old, mut new, mut hunk_start) = (0u32, 0u32, 0usize);
    let mut in_hunk = false;
    let finish = |file: &mut Option<PatchFile>, files: &mut Vec<PatchFile>, deleted: &mut Vec<SourceLine>, added: &mut Vec<SourceLine>, hunk_start: usize| {
        if let Some(mut done) = file.take() {
            done.flush(deleted, added);
            done.end_hunk(hunk_start);
            files.push(done);
        }
    };
    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            finish(&mut file, &mut files, &mut deleted, &mut added, hunk_start);
            (old_path, in_hunk) = (None, false);
        } else if !in_hunk && let Some(rest) = line.strip_prefix("--- ") {
            old_path = rest.strip_prefix("a/").map(str::to_owned);
        } else if !in_hunk && let Some(rest) = line.strip_prefix("+++ ") {
            let path = rest.strip_prefix("b/").map(str::to_owned).or_else(|| old_path.clone());
            file = path.map(|path| PatchFile { path, ..PatchFile::default() });
        } else if let Some(rest) = line.strip_prefix("@@ -") {
            let Some(current) = file.as_mut() else {
                continue;
            };
            current.flush(&mut deleted, &mut added);
            current.end_hunk(hunk_start);
            hunk_start = current.pairs.len();
            // `@@ -a[,b] +c[,d] @@`
            let mut parts = rest.split(' ');
            let start = |part: Option<&str>| {
                part.and_then(|p| p.trim_start_matches('+').split(',').next()?.parse::<u32>().ok()).unwrap_or(0)
            };
            (old, new) = (start(parts.next()), start(parts.next()));
            in_hunk = true;
        } else if in_hunk && let Some(current) = file.as_mut() {
            let text = line.get(1..).unwrap_or("").to_owned();
            match line.as_bytes().first() {
                Some(b'-') => {
                    deleted.push(SourceLine { line: old, text });
                    old += 1;
                }
                Some(b'+') => {
                    added.push(SourceLine { line: new, text });
                    new += 1;
                }
                Some(b'\\') => {} // "\ No newline at end of file"
                Some(_) | None => {
                    current.flush(&mut deleted, &mut added);
                    current.pairs.push(Pair::Unchanged { modified: SourceLine { line: new, text } });
                    old += 1;
                    new += 1;
                }
            }
        }
    }
    finish(&mut file, &mut files, &mut deleted, &mut added, hunk_start);
    files
}

/// Which pairs show as code: changes with their context (all of them when unchanged
/// regions are expanded), and folds too short to be worth folding. GPUI Kit's
/// `visible_pairs`.
fn visible_pairs(file: &PatchFile, unchanged: UnchangedRegions) -> Vec<bool> {
    let pairs = &file.pairs;
    let (mut hunk_start, mut hunk_end) = (vec![0; pairs.len()], vec![pairs.len(); pairs.len()]);
    for hunk in &file.hunks {
        hunk_start[hunk.clone()].fill(hunk.start);
        hunk_end[hunk.clone()].fill(hunk.end);
    }
    let mut visible = match unchanged {
        UnchangedRegions::Expanded => vec![true; pairs.len()],
        UnchangedRegions::Collapsed => {
            let mut visible = vec![false; pairs.len()];
            let mut ix = 0;
            while ix < pairs.len() {
                if !pairs[ix].is_changed() {
                    ix += 1;
                    continue;
                }
                let start = ix;
                while ix < pairs.len() && ix < hunk_end[start] && pairs[ix].is_changed() {
                    ix += 1;
                }
                let from = start.saturating_sub(DIFF_CONTEXT).max(hunk_start[start]);
                let to = ix.saturating_add(DIFF_CONTEXT).min(hunk_end[start]);
                visible[from..to].fill(true);
            }
            visible
        }
    };
    let mut ix = 0;
    while ix < pairs.len() {
        if visible[ix] {
            ix += 1;
            continue;
        }
        let start = ix;
        while ix < pairs.len() && ix < hunk_end[start] && !visible[ix] {
            ix += 1;
        }
        if ix - start < MIN_COLLAPSED_LINES {
            visible[start..ix].fill(true);
        }
    }
    visible
}

/// The lines a file shows, in on-screen order.
fn rows(file: &PatchFile, layout: DiffLayout) -> Vec<(Side, &SourceLine)> {
    let visible = visible_pairs(file, layout.unchanged);
    let pairs = &file.pairs;
    let mut rows = Vec::new();
    let mut ix = 0;
    while ix < pairs.len() {
        if !visible[ix] {
            ix += 1;
            continue;
        }
        match &pairs[ix] {
            Pair::Unchanged { modified } => {
                rows.push((Side::Modified, modified));
                ix += 1;
            }
            Pair::Changed { .. } => {
                let start = ix;
                while ix < pairs.len() && pairs[ix].is_changed() {
                    ix += 1;
                }
                let run = &pairs[start..ix];
                match layout.style {
                    DiffStyle::Split => {
                        for pair in run {
                            if let Pair::Changed { original, modified } = pair {
                                rows.extend(original.iter().map(|line| (Side::Original, line)));
                                rows.extend(modified.iter().map(|line| (Side::Modified, line)));
                            }
                        }
                    }
                    DiffStyle::Unified => {
                        for pair in run {
                            if let Pair::Changed { original: Some(line), .. } = pair {
                                rows.push((Side::Original, line));
                            }
                        }
                        for pair in run {
                            if let Pair::Changed { modified: Some(line), .. } = pair {
                                rows.push((Side::Modified, line));
                            }
                        }
                    }
                }
            }
        }
    }
    rows
}

/// Every line of `files` (in that order; files the patch lacks have none) the diff shows
/// that matches `query` (smart case); none for an empty query.
pub fn find_in_patch(patch: &str, files: &[String], query: &str, layout: DiffLayout) -> Vec<DiffMatch> {
    if query.is_empty() {
        return Vec::new();
    }
    let matcher = LineMatcher::new(query);
    let parsed = parse(patch);
    files
        .iter()
        .filter_map(|path| parsed.iter().find(|file| &file.path == path))
        .flat_map(|file| {
            rows(file, layout)
                .into_iter()
                .filter(|(_, line)| matcher.matches(&line.text))
                .map(|(side, line)| DiffMatch { path: file.path.clone(), side, line: line.line, preview: line.text.clone() })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The matches in `file` first ("In this file"), then the rest of the diff in order.
pub fn in_file_first(matches: Vec<DiffMatch>, file: &str) -> Vec<DiffMatch> {
    let (mut first, rest): (Vec<DiffMatch>, Vec<DiffMatch>) = matches.into_iter().partition(|m| m.path == file);
    first.extend(rest);
    first
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPLIT_FOLDED: DiffLayout = DiffLayout { style: DiffStyle::Split, unchanged: UnchangedRegions::Collapsed };
    const UNIFIED_FOLDED: DiffLayout = DiffLayout { style: DiffStyle::Unified, unchanged: UnchangedRegions::Collapsed };
    const SPLIT_ALL: DiffLayout = DiffLayout { style: DiffStyle::Split, unchanged: UnchangedRegions::Expanded };

    /// A whole-file patch (as Review asks git for) of a file of `count` lines `line N`,
    /// each line of `changed` replaced by `CHANGED N` and `added N`.
    fn patch_changing(path: &str, count: u32, changed: &[u32]) -> String {
        let mut body = String::new();
        for n in 1..=count {
            if changed.contains(&n) {
                body.push_str(&format!("-line {n}\n+CHANGED {n}\n+added {n}\n"));
            } else {
                body.push_str(&format!(" line {n}\n"));
            }
        }
        let new_count = count as usize + changed.len();
        format!("diff --git a/{path} b/{path}\nindex 1..2 100644\n--- a/{path}\n+++ b/{path}\n@@ -1,{count} +1,{new_count} @@\n{body}")
    }

    fn whole_file_patch(path: &str, count: u32, changed: u32) -> String {
        patch_changing(path, count, &[changed])
    }

    fn found(matches: &[DiffMatch]) -> Vec<(Side, u32)> {
        matches.iter().map(|m| (m.side, m.line)).collect()
    }

    #[test]
    fn folded_lines_are_skipped_and_context_is_searched() {
        let patch = whole_file_patch("a.txt", 30, 15);
        let files = vec!["a.txt".to_owned()];
        // Context is 4 lines each side of the change: lines 11–14 and 16–19 (new 17–20).
        let hits = find_in_patch(&patch, &files, "line", SPLIT_FOLDED);
        assert_eq!(
            found(&hits),
            vec![
                (Side::Modified, 11),
                (Side::Modified, 12),
                (Side::Modified, 13),
                (Side::Modified, 14),
                (Side::Original, 15),
                (Side::Modified, 17),
                (Side::Modified, 18),
                (Side::Modified, 19),
                (Side::Modified, 20),
            ]
        );
        assert_eq!(hits[4].preview, "line 15");
        // Expanded, every line is there.
        assert_eq!(find_in_patch(&patch, &files, "line", SPLIT_ALL).len(), 30);
    }

    #[test]
    fn a_change_reads_deletion_beside_addition_or_deletions_first() {
        let patch = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,3 @@\n-x one\n-x two\n+x ONE\n+x TWO\n+x three\n";
        let files = vec!["a.txt".to_owned()];
        let split = find_in_patch(patch, &files, "x", SPLIT_FOLDED);
        assert_eq!(
            found(&split),
            vec![(Side::Original, 1), (Side::Modified, 1), (Side::Original, 2), (Side::Modified, 2), (Side::Modified, 3)]
        );
        let unified = find_in_patch(patch, &files, "x", UNIFIED_FOLDED);
        assert_eq!(
            found(&unified),
            vec![(Side::Original, 1), (Side::Original, 2), (Side::Modified, 1), (Side::Modified, 2), (Side::Modified, 3)]
        );
    }

    #[test]
    fn a_one_line_fold_is_shown_a_two_line_one_folds() {
        let files = vec!["a".to_owned()];
        // Changes at 5 and 15: their context covers 1–9 and 11–19, leaving line 10 alone.
        let one = patch_changing("a", 19, &[5, 15]);
        assert_eq!(find_in_patch(&one, &files, "line 10", SPLIT_FOLDED).len(), 1);
        // Changes at 5 and 16: lines 10 and 11 fold.
        let two = patch_changing("a", 20, &[5, 16]);
        assert!(find_in_patch(&two, &files, "line 10", SPLIT_FOLDED).is_empty());
        assert!(find_in_patch(&two, &files, "line 11", SPLIT_FOLDED).is_empty());
        assert_eq!(find_in_patch(&two, &files, "line 9", SPLIT_FOLDED).len(), 1);
    }

    #[test]
    fn smart_case_files_order_and_this_file_first() {
        let patch = format!("{}{}", whole_file_patch("a.txt", 3, 2), whole_file_patch("b.txt", 3, 2));
        let files = vec!["b.txt".to_owned(), "a.txt".to_owned(), "missing.txt".to_owned()];
        let hits = find_in_patch(&patch, &files, "changed", SPLIT_FOLDED);
        assert_eq!(hits.iter().map(|m| m.path.as_str()).collect::<Vec<_>>(), vec!["b.txt", "a.txt"]);
        assert!(find_in_patch(&patch, &files, "Changed", SPLIT_FOLDED).is_empty(), "a capital makes it exact");
        assert!(find_in_patch(&patch, &files, "", SPLIT_FOLDED).is_empty());
        let ordered = in_file_first(hits, "a.txt");
        assert_eq!(ordered.iter().map(|m| m.path.as_str()).collect::<Vec<_>>(), vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn deleted_and_added_files_and_no_newline_markers() {
        let patch = "diff --git a/gone.txt b/gone.txt\ndeleted file mode 100644\n--- a/gone.txt\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-bye one\n-bye two\n\\ No newline at end of file\ndiff --git a/new.txt b/new.txt\nnew file mode 100644\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1 @@\n+bye new\n";
        let files = vec!["gone.txt".to_owned(), "new.txt".to_owned()];
        let hits = find_in_patch(patch, &files, "bye", SPLIT_FOLDED);
        assert_eq!(
            hits.iter().map(|m| (m.path.as_str(), m.side, m.line)).collect::<Vec<_>>(),
            vec![("gone.txt", Side::Original, 1), ("gone.txt", Side::Original, 2), ("new.txt", Side::Modified, 1)]
        );
    }

    #[test]
    fn hunks_bound_context_and_folds() {
        // Two hunks of a normal (3-context) patch: lines between them are not in the patch.
        let patch = "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1,3 +1,3 @@\n k1\n-k2\n+K2\n k3\n@@ -20,3 +20,3 @@\n k20\n-k21\n+K21\n k22\n";
        let hits = find_in_patch(patch, &["a".to_owned()], "k", SPLIT_FOLDED);
        assert_eq!(
            found(&hits),
            vec![
                (Side::Modified, 1),
                (Side::Original, 2),
                (Side::Modified, 2),
                (Side::Modified, 3),
                (Side::Modified, 20),
                (Side::Original, 21),
                (Side::Modified, 21),
                (Side::Modified, 22),
            ]
        );
    }
}
