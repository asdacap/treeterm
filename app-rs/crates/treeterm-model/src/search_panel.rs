//! What the Files viewer's and Review's search panels share (`SearchPanel.tsx`): the rows
//! the panel lists, its match counter and the debounced worktree search ("In repo").

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;
use gpui_kit::{Context, Task};
use treeterm_core::exec::ExecApi;
use treeterm_core::review::Side;
use treeterm_core::search::{self, Completion, RepoHit, RepoSearchEvent, RepoSearchTool};

/// Typing restarts the repo search; it waits for a pause first (`REPO_SEARCH_DEBOUNCE_MS`).
pub const REPO_SEARCH_DEBOUNCE: Duration = Duration::from_millis(250);

/// The worktree search (`RepoResults` in `SearchPanel.tsx`). Hits keep arriving while it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoResults {
    /// Nothing to search for.
    Idle,
    Searching { hits: Vec<RepoHit> },
    Done { hits: Vec<RepoHit>, tool: RepoSearchTool, completion: Completion },
    Failed { hits: Vec<RepoHit>, message: String },
}

impl RepoResults {
    /// Take in what the running search reported; a search that already ended ignores it
    /// (a replaced one is dropped with its task).
    pub fn apply(&mut self, event: RepoSearchEvent) {
        let RepoResults::Searching { hits } = self else {
            return;
        };
        match event {
            RepoSearchEvent::Hits(more) => hits.extend(more),
            RepoSearchEvent::Done { tool, completion } => {
                *self = RepoResults::Done { hits: std::mem::take(hits), tool, completion };
            }
            RepoSearchEvent::Failed(error) => {
                *self = RepoResults::Failed { hits: std::mem::take(hits), message: error.to_string() };
            }
        }
    }
}

/// Search the worktree at `root` for `query` once typing pauses; each event goes to `apply`
/// on the owner. Dropping the task kills the search.
pub fn spawn_repo_search<T: 'static>(
    exec: Arc<dyn ExecApi>,
    root: &str,
    query: &str,
    cx: &mut Context<T>,
    apply: fn(&mut T, RepoSearchEvent, &mut Context<T>),
) -> Task<()> {
    // The command starts on the first poll, after the pause.
    let events = search::repo_search(exec, root, query);
    let pause = cx.background_executor().timer(REPO_SEARCH_DEBOUNCE);
    cx.spawn(async move |this, cx| {
        pause.await;
        let mut events = std::pin::pin!(events);
        while let Some(event) = events.next().await {
            if this.update(cx, |owner, cx| apply(owner, event, cx)).is_err() {
                return; // tab closed
            }
        }
    })
}

/// A row of a search panel, in display order. Consecutive hits in one file share a
/// [`SearchRow::File`] heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchRow {
    Section(&'static str),
    File(String),
    /// A match in the Files viewer's open file; `index` is its place among its matches.
    InFile { index: usize, line_number: usize, preview: String, active: bool },
    /// A match in Review's diff; `index` is its place among the diff's matches. Lines of the
    /// old side are labelled `−N`.
    InDiff { index: usize, side: Side, line_number: u32, preview: String, active: bool },
    InRepo(RepoHit),
    Note(String),
    Error(String),
}

/// Chromium's tab stops (`tab-size: 8`).
const TAB_SIZE: usize = 8;

/// A hit's preview as `.search-panel-hit-preview` draws it (`white-space: pre`): each tab runs
/// to the next multiple of 8 columns from the start of the preview.
pub fn expand_tabs(preview: &str) -> String {
    let mut expanded = String::with_capacity(preview.len());
    let mut column = 0;
    for ch in preview.chars() {
        if ch == '\t' {
            let width = TAB_SIZE - column % TAB_SIZE;
            expanded.extend(std::iter::repeat_n(' ', width));
            column += width;
        } else {
            expanded.push(ch);
            column += 1;
        }
    }
    expanded
}

/// How the panel's list goes from showing one set of rows to another, keeping what it
/// measured of the rows that stay (and so the scroll position).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowsChange {
    Unchanged,
    /// As many rows as before; these changed in place (moving the active match changes two).
    Remeasure(Vec<usize>),
    /// The rows at `old` are replaced by `count` new ones; those before and after stay.
    Splice { old: std::ops::Range<usize>, count: usize },
}

/// What changed from `old` to `new`.
pub fn rows_change(old: &[SearchRow], new: &[SearchRow]) -> RowsChange {
    if old.len() == new.len() {
        let changed: Vec<usize> = old.iter().zip(new).enumerate().filter(|(_, (a, b))| a != b).map(|(index, _)| index).collect();
        return if changed.is_empty() { RowsChange::Unchanged } else { RowsChange::Remeasure(changed) };
    }
    let before = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let after = old[before..].iter().rev().zip(new[before..].iter().rev()).take_while(|(a, b)| a == b).count();
    RowsChange::Splice { old: before..old.len() - after, count: new.len() - before - after }
}

/// `countLabel`: the panel's match counter (`3/12`, `12 found`, `Searching…`).
pub fn count_label(query: &str, active: Option<usize>, total: usize, pending: bool) -> String {
    if query.is_empty() {
        return String::new();
    }
    if pending && total == 0 {
        return "Searching…".to_owned();
    }
    let count = match active {
        Some(index) => format!("{}/{total}", index + 1),
        None => format!("{total} found"),
    };
    if pending { format!("{count}…") } else { count }
}

/// Consecutive hits of one file under one heading (`groupByFile`).
fn push_repo_rows(rows: &mut Vec<SearchRow>, hits: impl IntoIterator<Item = RepoHit>) -> usize {
    let mut shown = 0;
    let mut last_file: Option<String> = None;
    for hit in hits {
        if last_file.as_deref() != Some(hit.file_path.as_str()) {
            rows.push(SearchRow::File(hit.file_path.clone()));
            last_file = Some(hit.file_path.clone());
        }
        rows.push(SearchRow::InRepo(hit));
        shown += 1;
    }
    shown
}

/// "In repo": the worktree's hits `shown` keeps (those in view are listed above), then how
/// the search is going.
pub fn push_repo_section(rows: &mut Vec<SearchRow>, repo: &RepoResults, shown: impl Fn(&RepoHit) -> bool) {
    rows.push(SearchRow::Section("In repo"));
    let kept = |hits: &[RepoHit]| -> Vec<RepoHit> { hits.iter().filter(|hit| shown(hit)).cloned().collect() };
    match repo {
        RepoResults::Idle => {}
        RepoResults::Searching { hits } => {
            push_repo_rows(rows, kept(hits));
            rows.push(SearchRow::Note("Searching repo…".to_owned()));
        }
        RepoResults::Done { hits, tool, completion } => {
            let count = push_repo_rows(rows, kept(hits));
            let found = if count == 0 { "No matches".to_owned() } else { format!("{count} found") };
            let truncated = match completion {
                Completion::Complete => "",
                Completion::Truncated => " (stopped at the result limit)",
            };
            rows.push(SearchRow::Note(format!("{found}{truncated} · {}", tool.as_str())));
        }
        RepoResults::Failed { hits, message } => {
            push_repo_rows(rows, kept(hits));
            rows.push(SearchRow::Error(format!("Search failed: {message}")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(file: &str, line: u64) -> RepoHit {
        RepoHit { file_path: file.to_owned(), line_number: line, column: 1, preview: format!("{file}:{line}") }
    }

    #[test]
    fn the_counter_reads_like_electrons() {
        assert_eq!(count_label("", Some(0), 3, false), "");
        assert_eq!(count_label("x", None, 0, true), "Searching…");
        assert_eq!(count_label("x", None, 3, false), "3 found");
        assert_eq!(count_label("x", Some(1), 3, false), "2/3");
        assert_eq!(count_label("x", Some(1), 3, true), "2/3…");
    }

    #[test]
    fn repo_hits_are_grouped_by_consecutive_file() {
        let mut rows = Vec::new();
        let shown = push_repo_rows(&mut rows, vec![hit("a.rs", 1), hit("a.rs", 4), hit("b.rs", 2), hit("a.rs", 9)]);
        assert_eq!(shown, 4);
        assert_eq!(
            rows,
            vec![
                SearchRow::File("a.rs".into()),
                SearchRow::InRepo(hit("a.rs", 1)),
                SearchRow::InRepo(hit("a.rs", 4)),
                SearchRow::File("b.rs".into()),
                SearchRow::InRepo(hit("b.rs", 2)),
                SearchRow::File("a.rs".into()),
                SearchRow::InRepo(hit("a.rs", 9)),
            ]
        );
    }

    #[test]
    fn repo_hits_are_grouped_by_file_and_filtered() {
        let mut rows = Vec::new();
        let repo = RepoResults::Done {
            hits: vec![hit("a", 1), hit("a", 2), hit("b", 1), hit("c", 3)],
            tool: RepoSearchTool::GitGrep,
            completion: Completion::Truncated,
        };
        push_repo_section(&mut rows, &repo, |hit| hit.file_path != "b");
        assert_eq!(rows[0], SearchRow::Section("In repo"));
        assert_eq!(rows[1], SearchRow::File("a".to_owned()));
        assert!(matches!(&rows[2], SearchRow::InRepo(h) if h.line_number == 1));
        assert!(matches!(&rows[3], SearchRow::InRepo(h) if h.line_number == 2));
        assert_eq!(rows[4], SearchRow::File("c".to_owned()));
        assert_eq!(rows[6], SearchRow::Note("3 found (stopped at the result limit) · git grep".to_owned()));

        let mut rows = Vec::new();
        push_repo_section(&mut rows, &RepoResults::Failed { hits: vec![], message: "boom".to_owned() }, |_| true);
        assert_eq!(rows, vec![SearchRow::Section("In repo"), SearchRow::Error("Search failed: boom".to_owned())]);
        let mut rows = Vec::new();
        push_repo_section(&mut rows, &RepoResults::Searching { hits: vec![] }, |_| true);
        assert_eq!(rows[1], SearchRow::Note("Searching repo…".to_owned()));
    }

    #[test]
    fn tabs_run_to_the_next_eighth_column_as_in_chromium() {
        assert_eq!(expand_tabs("\tx"), format!("{}x", " ".repeat(8)));
        assert_eq!(expand_tabs("ab\tc"), format!("ab{}c", " ".repeat(6)));
        assert_eq!(expand_tabs("12345678\ty"), format!("12345678{}y", " ".repeat(8)));
        assert_eq!(expand_tabs("1234567\t\tz"), format!("1234567{}z", " ".repeat(9)));
        assert_eq!(expand_tabs("é\tx"), format!("é{}x", " ".repeat(7)), "a character is one column, whatever its bytes");
        assert_eq!(expand_tabs("no tabs  here"), "no tabs  here");
        assert_eq!(expand_tabs(""), "");
    }

    #[test]
    fn only_the_rows_that_changed_are_measured_again() {
        let row = |index: usize, active: bool| SearchRow::InFile { index, line_number: index + 1, preview: "x".into(), active };
        let note = |text: &str| SearchRow::Note(text.to_owned());
        let rows = vec![SearchRow::Section("In this file"), row(0, true), row(1, false), row(2, false)];
        assert_eq!(rows_change(&rows, &rows), RowsChange::Unchanged);
        assert_eq!(rows_change(&[], &[]), RowsChange::Unchanged);

        // Moving to the next match changes the old and the new active row only.
        let stepped = vec![SearchRow::Section("In this file"), row(0, false), row(1, true), row(2, false)];
        assert_eq!(rows_change(&rows, &stepped), RowsChange::Remeasure(vec![1, 2]));

        // Repo hits arriving go in before the trailing note.
        let searching = vec![SearchRow::Section("In repo"), note("Searching repo…")];
        let found = vec![
            SearchRow::Section("In repo"),
            SearchRow::File("a.rs".into()),
            SearchRow::InRepo(hit("a.rs", 1)),
            note("Searching repo…"),
        ];
        assert_eq!(rows_change(&searching, &found), RowsChange::Splice { old: 1..1, count: 2 });
        assert_eq!(rows_change(&found, &searching), RowsChange::Splice { old: 1..3, count: 0 });

        // A change at the end, from and to nothing.
        let done = vec![SearchRow::Section("In repo"), SearchRow::File("a.rs".into()), SearchRow::InRepo(hit("a.rs", 1))];
        assert_eq!(rows_change(&found, &done), RowsChange::Splice { old: 3..4, count: 0 });
        assert_eq!(rows_change(&[], &rows), RowsChange::Splice { old: 0..0, count: 4 });
        assert_eq!(rows_change(&rows, &[]), RowsChange::Splice { old: 0..4, count: 0 });

        // Repeated rows are not counted twice: what stays before and after never overlaps.
        let same = vec![note("a"), note("a")];
        assert_eq!(rows_change(&same, &same[..1]), RowsChange::Splice { old: 1..2, count: 0 });
        assert_eq!(rows_change(&same[..1], &same), RowsChange::Splice { old: 1..1, count: 1 });
    }

    #[test]
    fn events_fill_a_running_search_only() {
        let mut repo = RepoResults::Searching { hits: vec![] };
        repo.apply(RepoSearchEvent::Hits(vec![hit("a", 1)]));
        repo.apply(RepoSearchEvent::Done { tool: RepoSearchTool::Ripgrep, completion: Completion::Complete });
        assert!(matches!(&repo, RepoResults::Done { hits, .. } if hits.len() == 1));
        repo.apply(RepoSearchEvent::Hits(vec![hit("b", 1)]));
        assert!(matches!(&repo, RepoResults::Done { hits, .. } if hits.len() == 1));
    }
}
