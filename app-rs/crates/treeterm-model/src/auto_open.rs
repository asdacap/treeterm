//! "Auto Open All Recent Worktrees" (`AutoOpenWorktreesDialog.tsx`): the root workspace's
//! worktrees that the worktree registry remembers (optionally all of them), placed in a
//! hierarchy by git ancestry, to open as child workspaces in one batch.
//!
//! Detection runs again whenever what it reads changes (the deps of Electron's detection
//! effect): the include-unknown toggle, the root's path or branch, or the open workspaces.
//! Ok waits for the whole batch (`isProcessing`): the rows, the toggle and Cancel are
//! disabled meanwhile, and the dialog closes once every worktree is open, or shows why the
//! batch stopped.

use std::collections::{BTreeSet, HashMap, HashSet};

use gpui_kit::{App, Context, Entity, EventEmitter, Subscription, Task};
use treeterm_core::worktree_hierarchy::{WorktreeRef, detect_worktree_hierarchy};
use treeterm_core::worktree_registry::{WorktreeRegistry, WorktreeRegistryEntry};

use crate::child_dialog::Listing;
use crate::session_store::{AutoOpenItem, SessionStore};

/// A detected worktree, with what the registry remembers of it (`DetectedRow`).
#[derive(Debug, Clone, PartialEq)]
pub struct AutoOpenRow {
    pub path: String,
    pub branch: String,
    /// The detected parent worktree; `None` for directly under the root.
    pub parent_path: Option<String>,
    /// `None`: the registry does not know it (the "unknown" badge).
    pub entry: Option<WorktreeRegistryEntry>,
    /// Levels below the root: its children are 0.
    pub depth: usize,
}

impl AutoOpenRow {
    /// The remembered display name, else the branch.
    pub fn name(&self) -> String {
        self.entry
            .as_ref()
            .and_then(|e| e.display_name.clone())
            .unwrap_or_else(|| self.branch.clone())
    }
}

/// What the dialog tells its view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoOpenEvent {
    /// Every checked worktree is open: the dialog closes.
    Opened,
    /// The root is no longer loaded: the dialog closes (Electron renders it only while its
    /// root is).
    RootGone,
}

/// Ok on the checked rows (`isProcessing`).
enum Opening {
    Idle,
    /// Waits for the batch, which runs on in the session if the dialog goes away.
    Waiting {
        _wait: Task<()>,
    },
}

/// What detection reads; a change detects again.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Inputs {
    root: WorktreeRef,
    /// Where the worktrees are listed: the root's repository.
    repo: String,
    /// Every loaded workspace's path; none of them is offered.
    open: BTreeSet<String>,
    include_unknown: bool,
}

pub struct AutoOpenModel {
    session: Entity<SessionStore>,
    root_id: String,
    include_unknown: bool,
    /// In detection order.
    rows: Listing<AutoOpenRow>,
    /// Kept while detecting again, and after a detection fails (`checkedPaths`).
    checked: HashSet<String>,
    /// Why detection or the batch failed.
    error: Option<String>,
    opening: Opening,
    /// What the latest detection read.
    inputs: Option<Inputs>,
    /// The latest detection; replacing it drops the one before, so a stale result never
    /// lands (Electron's `cancelled`).
    _load: Option<Task<()>>,
    _session: Subscription,
}

impl EventEmitter<AutoOpenEvent> for AutoOpenModel {}

impl AutoOpenModel {
    pub fn new(session: Entity<SessionStore>, root_id: String, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&session, |model, _, cx| {
            model.follow(cx);
            cx.notify();
        });
        let mut model = Self {
            session,
            root_id,
            include_unknown: false,
            rows: Listing::Loading,
            checked: HashSet::new(),
            error: None,
            opening: Opening::Idle,
            inputs: None,
            _load: None,
            _session: observe,
        };
        model.follow(cx);
        model
    }

    /// "Under:" (`rootWs.name`), read live.
    pub fn root_name(&self, cx: &App) -> String {
        self.session
            .read(cx)
            .workspace_store(&self.root_id)
            .map(|w| w.read(cx).file().name.clone())
            .unwrap_or_default()
    }

    pub fn include_unknown(&self) -> bool {
        self.include_unknown
    }

    /// "Include unknown worktrees (not in recent history)": detects again. Disabled while the
    /// batch runs.
    pub fn set_include_unknown(&mut self, include: bool, cx: &mut Context<Self>) {
        if self.processing() {
            return;
        }
        self.include_unknown = include;
        self.follow(cx);
    }

    /// The detected rows in tree order: each followed by its children.
    pub fn rows(&self) -> Listing<AutoOpenRow> {
        match &self.rows {
            Listing::Loading => Listing::Loading,
            Listing::Loaded(rows) => {
                Listing::Loaded(tree_order(rows).into_iter().cloned().collect())
            }
        }
    }

    pub fn is_checked(&self, path: &str) -> bool {
        self.checked.contains(path)
    }

    /// A row click (`toggleChecked`), ignored while the batch runs.
    pub fn toggle(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.processing() {
            return;
        }
        if !self.checked.remove(path) {
            self.checked.insert(path.to_owned());
        }
        cx.notify();
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The batch is running: the controls are disabled.
    pub fn processing(&self) -> bool {
        match self.opening {
            Opening::Idle => false,
            Opening::Waiting { .. } => true,
        }
    }

    /// Ok's count (`checkedPaths.size`).
    pub fn count(&self) -> usize {
        self.checked.len()
    }

    pub fn can_confirm(&self) -> bool {
        self.batch().is_some()
    }

    pub fn confirm_label(&self) -> String {
        if self.processing() {
            "Opening... Please wait".to_owned()
        } else {
            format!("Ok ({})", self.count())
        }
    }

    /// Ok (`handleConfirm`): open the checked rows in one batch and wait for it. Reports
    /// `Opened` once every one is open; a failure shows, and the controls come back.
    pub fn confirm(&mut self, cx: &mut Context<Self>) {
        let Some(items) = self.batch() else {
            return;
        };
        let root = self.root_id.clone();
        let opened = self
            .session
            .update(cx, |s, cx| s.auto_open_worktrees(&root, items, cx));
        let wait = cx.spawn(async move |this, cx| {
            // Canceled: the batch went away with the session (its errors are never empty).
            let outcome = opened
                .await
                .unwrap_or_else(|_| Err("Failed to open worktrees".to_owned()));
            // Err: the dialog is gone (its task would have been dropped with it).
            match this.update(cx, |model, cx| {
                match outcome {
                    // Still processing until the dialog closes.
                    Ok(()) => cx.emit(AutoOpenEvent::Opened),
                    Err(error) => {
                        model.opening = Opening::Idle;
                        model.error = Some(error);
                    }
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {}
            }
        });
        self.error = None;
        self.opening = Opening::Waiting { _wait: wait };
        cx.notify();
    }

    /// The batch Ok opens, or `None` while Ok is disabled: during the batch, while detecting
    /// or with nothing checked. After a failed detection the count is kept and the batch is
    /// empty, as in Electron.
    fn batch(&self) -> Option<Vec<AutoOpenItem>> {
        match (&self.opening, &self.rows) {
            (Opening::Idle, Listing::Loaded(rows)) if !self.checked.is_empty() => {
                Some(batch_items(rows, &self.checked))
            }
            (Opening::Idle, Listing::Loaded(_) | Listing::Loading)
            | (Opening::Waiting { .. }, _) => None,
        }
    }

    /// What detection reads now, or `None` when the root is not loaded.
    fn inputs(&self, cx: &App) -> Option<Inputs> {
        let session = self.session.read(cx);
        let ws = session.workspace_store(&self.root_id)?.read(cx).workspace();
        let open = session
            .workspace_ids()
            .filter_map(|id| session.workspace_store(id))
            .map(|w| w.read(cx).workspace().path.clone())
            .collect();
        Some(Inputs {
            root: WorktreeRef {
                path: ws.path.clone(),
                branch: ws.file.git_branch.clone().unwrap_or_default(),
            },
            repo: ws
                .file
                .git_root_path
                .clone()
                .unwrap_or_else(|| ws.path.clone()),
            open,
            include_unknown: self.include_unknown,
        })
    }

    /// Detect again when what detection reads has changed; report a root that has gone.
    fn follow(&mut self, cx: &mut Context<Self>) {
        match self.inputs(cx) {
            Some(inputs) if self.inputs.as_ref() == Some(&inputs) => {}
            Some(inputs) => self.detect(inputs, cx),
            None => {
                self._load = None;
                self.inputs = None;
                self.rows = Listing::Loaded(Vec::new());
                self.error = Some("Root workspace not found".to_owned());
                cx.emit(AutoOpenEvent::RootGone);
            }
        }
    }

    /// `load`: clears the error and shows "Detecting worktrees..."; the checked rows stay
    /// until it succeeds, when every detected row is checked.
    fn detect(&mut self, inputs: Inputs, cx: &mut Context<Self>) {
        self.error = None;
        self.rows = Listing::Loading;
        let (git, registry) = {
            let session = self.session.read(cx);
            (session.connection().git(), session.worktree_registry())
        };
        let read = inputs.clone();
        self._load = Some(cx.spawn(async move |this, cx| {
            let detected = detect_rows(git, registry, read).await;
            // Err: the dialog is gone (its task would have been dropped with it).
            match this.update(cx, |model, cx| {
                match detected {
                    Ok(rows) => {
                        model.checked = rows.iter().map(|r| r.path.clone()).collect();
                        model.rows = Listing::Loaded(rows);
                    }
                    Err(error) => {
                        model.error = Some(format!("Failed to detect worktrees: {error}"));
                        model.rows = Listing::Loaded(Vec::new());
                    }
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {}
            }
        }));
        self.inputs = Some(inputs);
        cx.notify();
    }
}

/// The repository's worktrees that are neither the root nor open, known to the registry
/// (or all with "Include unknown"), each under its nearest ancestor among them.
async fn detect_rows(
    git: treeterm_core::git::GitApi,
    registry: Option<WorktreeRegistry>,
    inputs: Inputs,
) -> Result<Vec<AutoOpenRow>, String> {
    let Inputs {
        root,
        repo,
        open,
        include_unknown,
    } = inputs;
    let worktrees = git.list_worktrees(&repo).await.map_err(|e| e.to_string())?;
    let entries = match registry {
        Some(registry) => registry.list().await.map_err(|e| e.to_string())?,
        None => Vec::new(),
    };
    let by_path: HashMap<String, WorktreeRegistryEntry> =
        entries.into_iter().map(|e| (e.path.clone(), e)).collect();
    let candidates: Vec<WorktreeRef> = worktrees
        .into_iter()
        .filter(|w| w.path != root.path && !open.contains(&w.path))
        .filter(|w| include_unknown || by_path.contains_key(&w.path))
        .map(|w| WorktreeRef {
            path: w.path,
            branch: w.branch,
        })
        .collect();
    let is_ancestor = |a: String, b: String| {
        let (git, repo) = (git.clone(), repo.clone());
        async move {
            git.is_ancestor(&repo, &a, &b)
                .await
                .map_err(|e| e.to_string())
        }
    };
    let nodes = detect_worktree_hierarchy(&candidates, &root, is_ancestor).await?;
    let depth = depths(
        &nodes
            .iter()
            .map(|n| (n.path.clone(), n.parent_path.clone()))
            .collect::<Vec<_>>(),
    );
    Ok(nodes
        .into_iter()
        .map(|n| AutoOpenRow {
            depth: depth.get(&n.path).copied().unwrap_or(0),
            entry: by_path.get(&n.path).cloned(),
            path: n.path,
            branch: n.branch,
            parent_path: n.parent_path,
        })
        .collect())
}

/// The checked rows as a batch. A row whose detected parent is unchecked hangs from its
/// nearest checked ancestor (or the root), so the opened tree stays connected
/// (`resolveParent`).
fn batch_items(rows: &[AutoOpenRow], checked: &HashSet<String>) -> Vec<AutoOpenItem> {
    let parents: HashMap<&str, Option<&str>> = rows
        .iter()
        .map(|r| (r.path.as_str(), r.parent_path.as_deref()))
        .collect();
    let resolve = |path: &str| -> Option<String> {
        let mut parent = parents.get(path).copied().flatten();
        let mut hops = 0;
        while let Some(candidate) = parent {
            hops += 1;
            if checked.contains(candidate) {
                return Some(candidate.to_owned());
            }
            if hops > rows.len() {
                return None;
            }
            parent = parents.get(candidate).copied().flatten();
        }
        None
    };
    rows.iter()
        .filter(|r| checked.contains(&r.path))
        .map(|r| AutoOpenItem {
            path: r.path.clone(),
            branch: r.branch.clone(),
            parent_path: resolve(&r.path),
            display_name: r.entry.as_ref().and_then(|e| e.display_name.clone()),
            description: r.entry.as_ref().and_then(|e| e.description.clone()),
        })
        .collect()
}

/// Depth of each node from its parent chain (root children are 0).
fn depths(rows: &[(String, Option<String>)]) -> HashMap<String, usize> {
    let parents: HashMap<&str, Option<&str>> = rows
        .iter()
        .map(|(p, parent)| (p.as_str(), parent.as_deref()))
        .collect();
    rows.iter()
        .map(|(path, _)| {
            let mut depth = 0;
            let mut current = parents.get(path.as_str()).copied().flatten();
            while let Some(parent) = current {
                depth += 1;
                if depth > rows.len() {
                    break; // a cycle cannot happen, but never loop forever
                }
                current = parents.get(parent).copied().flatten();
            }
            (path.clone(), depth)
        })
        .collect()
}

/// The rows in tree order (each followed by its children).
fn tree_order(rows: &[AutoOpenRow]) -> Vec<&AutoOpenRow> {
    fn walk<'a>(parent: Option<&str>, rows: &'a [AutoOpenRow], out: &mut Vec<&'a AutoOpenRow>) {
        for row in rows.iter().filter(|r| r.parent_path.as_deref() == parent) {
            if out.iter().any(|seen| seen.path == row.path) {
                continue;
            }
            out.push(row);
            walk(Some(&row.path), rows, out);
        }
    }
    let mut out = Vec::with_capacity(rows.len());
    walk(None, rows, &mut out);
    // Rows whose parent is not listed still show, at the end.
    for row in rows {
        if !out.iter().any(|seen| seen.path == row.path) {
            out.push(row);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{AutoOpenRow, batch_items, depths, tree_order};

    fn row(path: &str, parent: Option<&str>) -> AutoOpenRow {
        AutoOpenRow {
            path: path.to_owned(),
            branch: path.trim_start_matches('/').to_owned(),
            parent_path: parent.map(str::to_owned),
            entry: None,
            depth: 0,
        }
    }

    #[test]
    fn depth_follows_the_parent_chain() {
        let rows = vec![
            ("/a".to_owned(), None),
            ("/b".to_owned(), Some("/a".to_owned())),
            ("/c".to_owned(), Some("/b".to_owned())),
        ];
        let depth = depths(&rows);
        assert_eq!((depth["/a"], depth["/b"], depth["/c"]), (0, 1, 2));
    }

    #[test]
    fn rows_show_each_followed_by_its_children_and_orphans_last() {
        let rows = vec![
            row("/c", Some("/a")),
            row("/b", None),
            row("/lost", Some("/gone")),
            row("/a", None),
        ];
        let order: Vec<&str> = tree_order(&rows)
            .into_iter()
            .map(|r| r.path.as_str())
            .collect();
        assert_eq!(order, ["/b", "/a", "/c", "/lost"]);
    }

    #[test]
    fn a_row_under_an_unchecked_parent_hangs_from_its_nearest_checked_ancestor() {
        let rows = vec![
            row("/a", None),
            row("/b", Some("/a")),
            row("/c", Some("/b")),
        ];
        let checked: HashSet<String> = ["/a", "/c"].into_iter().map(str::to_owned).collect();
        let items = batch_items(&rows, &checked);
        let parents: Vec<(&str, Option<&str>)> = items
            .iter()
            .map(|i| (i.path.as_str(), i.parent_path.as_deref()))
            .collect();
        assert_eq!(parents, [("/a", None), ("/c", Some("/a"))]);
        // Nothing checked above it: the root.
        let checked: HashSet<String> = ["/c"].into_iter().map(str::to_owned).collect();
        assert_eq!(batch_items(&rows, &checked)[0].parent_path, None);
    }
}
