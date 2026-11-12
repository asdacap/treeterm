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
    fn events_fill_a_running_search_only() {
        let mut repo = RepoResults::Searching { hits: vec![] };
        repo.apply(RepoSearchEvent::Hits(vec![hit("a", 1)]));
        repo.apply(RepoSearchEvent::Done { tool: RepoSearchTool::Ripgrep, completion: Completion::Complete });
        assert!(matches!(&repo, RepoResults::Done { hits, .. } if hits.len() == 1));
        repo.apply(RepoSearchEvent::Hits(vec![hit("b", 1)]));
        assert!(matches!(&repo, RepoResults::Done { hits, .. } if hits.len() == 1));
    }
}
