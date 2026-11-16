//! The Review tab: what a worktree changed. Port of the data side of `ReviewBrowser.tsx`,
//! its comments (`createReviewCommentStore.ts`, `CommentsList.tsx`), viewed files
//! (`createReviewViewedFilesStore.ts`) and prompt buttons (`ReviewCommentsButton.tsx`,
//! `PromptCommitButton.tsx`).
//!
//! Views (Electron's `viewMode`): `committed` — the branch against its parent's branch
//! (merge-base, `parent...HEAD`); `uncommitted` — the working tree against `HEAD`,
//! including untracked files; `commits` — the branch's commits since the parent.
//! Each listed file's part of the patch (plain unified diff text, rendered by the view) is read
//! when it is first wanted (its section nears the diff's viewport, or find searches it), and
//! fails and is refreshed on its own ([`FileDiffs`]).
//!
//! Comments and viewed marks live in workspace metadata (`reviewComments`,
//! `reviewViewedFiles`), in Electron's format, so both apps share them.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui_kit::{App, BackgroundExecutor, Context, Entity, EventEmitter, Subscription, Task, WeakEntity};
use indexmap::{IndexMap, IndexSet};
use serde_json::{Value, json};
use treeterm_core::exec::{ExecClient, ExecRequest, ExecTimeout};
use treeterm_core::git::{
    ConflictInfo, DiffFile, DiffResult, FileChangeStats, FileChangeStatus, GitApi, GitLogCommit, LogScope, PathOrigin,
    Staging, UncommittedChanges, UncommittedFile,
};
use treeterm_core::review::{
    CommitAnchor, MetadataError, NewReviewComment, REVIEW_COMMENTS_KEY, REVIEW_VIEWED_FILES_KEY, Resolution,
    ReviewComment, ReviewCommentStore, ReviewComments, Side, ViewedFile, ViewedFileStats, ViewedFiles,
    ViewedFilesStore, build_prompt_for_comments, generate_review_prompt,
};

use treeterm_core::files::{FavouriteFile, is_image_path};
use treeterm_core::search::{Whitespace, ignore_whitespace_in_patch};

use crate::file_browser::FileBrowser;
use crate::review_find::{DiffLayout, DiffMatch, find_in_patch, in_file_first};
use crate::review_patch::{FileDiff, FileDiffs};
use crate::search_panel::{RepoResults, SearchRow, count_label, push_repo_section, spawn_repo_search};
use crate::review_metadata::{Blame, Later, Now};
use crate::session_store::AfterMerge;
use crate::workspace_store::{MergeState, WorkspaceStore};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewMode {
    Committed,
    Uncommitted,
    Commits,
}

impl ReviewMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ReviewMode::Committed => "committed",
            ReviewMode::Uncommitted => "uncommitted",
            ReviewMode::Commits => "commits",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "uncommitted" => ReviewMode::Uncommitted,
            "commits" => ReviewMode::Commits,
            _ => ReviewMode::Committed,
        }
    }
}

/// What the current view shows ([`ReviewModel::content`]). Each view keeps what it loaded
/// while another is shown, and a reload keeps it on screen until the new answer is complete,
/// so `Loading` shows only before a view's first list of files (each file's diff is then read
/// when wanted).
#[derive(Debug, Clone)]
pub enum ReviewContent {
    Loading,
    /// The branch since it left its parent: changed files and their diffs.
    Committed(Committed),
    /// The working tree: changed files (staged and unstaged) and their diffs.
    Uncommitted(Uncommitted),
    Commits(Vec<GitLogCommit>),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Committed {
    pub files: Vec<DiffFile>,
    pub diffs: FileDiffs,
    /// `diff-summary`: the compared branches and the totals.
    pub base_branch: String,
    pub head_branch: String,
    pub total_additions: u64,
    pub total_deletions: u64,
}

#[derive(Debug, Clone)]
pub struct Uncommitted {
    pub changes: UncommittedChanges,
    /// A file both staged and unstaged shows its staged part.
    pub diffs: FileDiffs,
}

impl Uncommitted {
    pub fn has_staged(&self) -> bool {
        self.changes.files.iter().any(|f| f.entry.staged == Staging::Staged)
    }
}

/// One view's data (Electron's `diff`, `uncommitted`, `commits`), kept while another view
/// is shown.
#[derive(Debug, Clone)]
enum Slot<T> {
    /// Not asked for: the Commits list loads when first shown, and without a base there is
    /// no committed diff.
    NotLoaded,
    Loading,
    Ready(T),
    Failed(String),
}

impl<T> Slot<T> {
    /// A load starts: what is shown stays until the answer arrives.
    fn start(&mut self) {
        match self {
            Slot::Ready(_) => {}
            Slot::NotLoaded | Slot::Loading | Slot::Failed(_) => *self = Slot::Loading,
        }
    }

    fn ready(&self) -> Option<&T> {
        match self {
            Slot::Ready(value) => Some(value),
            Slot::NotLoaded | Slot::Loading | Slot::Failed(_) => None,
        }
    }

    fn ready_mut(&mut self) -> Option<&mut T> {
        match self {
            Slot::Ready(value) => Some(value),
            Slot::NotLoaded | Slot::Loading | Slot::Failed(_) => None,
        }
    }

    /// A load's list of files arrived: it shows at once (its files' diffs loading), unless
    /// what is shown stays until the load is complete (a reload). Whether it shows.
    fn show_listed(&mut self, listed: impl FnOnce() -> T) -> bool {
        match self {
            Slot::Ready(_) => false,
            Slot::NotLoaded | Slot::Loading | Slot::Failed(_) => {
                *self = Slot::Ready(listed());
                true
            }
        }
    }

    fn content(&self, ready: impl FnOnce(&T) -> ReviewContent) -> ReviewContent {
        match self {
            Slot::NotLoaded | Slot::Loading => ReviewContent::Loading,
            Slot::Ready(value) => ready(value),
            Slot::Failed(error) => ReviewContent::Failed(error.clone()),
        }
    }
}

/// The commits listed so far (pages appended as the list scrolls), and whether more exist.
#[derive(Debug, Clone)]
struct CommitList {
    commits: Vec<GitLogCommit>,
    has_more: bool,
}

/// Whether the tab is on screen (`isVisible`), as last told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Visibility {
    /// Not told yet: no view has shown the tab since it was made (and loaded) when the
    /// session connected or the tab opened.
    Unknown,
    Hidden,
    Shown,
}

/// Whether the view shown is the one asked for, or Uncommitted standing in for Committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModeFallback {
    Asked,
    /// Committed was asked for (the saved `viewMode`, the default, or shown when the base
    /// went away) while there is no base to compare against: it comes back with one (a
    /// child's Review tabs are made before the session knows its parent's branch).
    NoBaseForCommitted,
}

/// A stage/unstage or commit in progress, or why the last one failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitOp {
    Idle,
    Running,
    Failed(String),
}

/// What a stage or unstage applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageTarget {
    File(String),
    All,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReviewEvent {
    StateChanged(Value),
    /// Drop a key from the tab's state (Electron persists `undefined`, never `null`).
    StateRemoved(&'static str),
    /// A commit succeeded (the view clears its message box; the workspace's git status is
    /// refreshed).
    Committed,
    /// Staging changed the index, or Review reloaded: refresh the workspace's git status
    /// (`refreshGit`).
    GitChanged,
    /// "Cancel" / "Close": close this Review tab.
    CloseRequested,
    /// The merge buttons: merge this workspace's branch into its parent.
    MergeRequested { squash: bool, after: AfterMerge },
    /// A merge that removes this workspace would commit its `files` uncommitted files: the
    /// view asks first, and a yes is [`ReviewModel::confirm_merge`].
    ConfirmMerge { squash: bool, files: usize },
    /// "Close and Clean": remove this (detached) workspace, keeping its branch.
    CloseAndCleanRequested,
    /// Electron's `alert()`: the view shows the message until it is dismissed.
    Alert(String),
}

impl EventEmitter<ReviewEvent> for ReviewModel {}

/// What the header's actions wait for (`isProcessing`): all of them are disabled meanwhile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Processing {
    Idle,
    /// The fresh conflict check a merge runs first.
    CheckingConflicts,
    /// This workspace is merging into its parent.
    Merging,
    /// This workspace is being removed ("Close and Clean", or from the tree).
    Removing,
}

/// The branch a worktree is reviewed against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewBase {
    /// A child workspace: its parent's branch (or the tab's override).
    Branch(String),
    /// A top-level workspace: only uncommitted changes make sense.
    None,
}

/// `HEAD` as the review last read it: what new comments are anchored to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    Loading,
    Known(String),
    Failed(String),
}

/// The comment being written, under a diff line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentDraft {
    pub file_path: String,
    pub side: Side,
    /// 1-based, in `side`'s file.
    pub line_number: u32,
}

/// `FavouriteSelection`: a favourite outside the diff, shown in a file viewer (the Files
/// tab's, with its review comments) in place of the diff.
#[derive(Clone)]
pub enum FavouriteSelection {
    None,
    Selected { file: FavouriteFile, viewer: Entity<FileBrowser> },
}

impl FavouriteSelection {
    /// The selected favourite's workspace-relative path.
    pub fn relative_path(&self) -> Option<&str> {
        match self {
            FavouriteSelection::None => None,
            FavouriteSelection::Selected { file, .. } => Some(&file.relative_path),
        }
    }
}

/// Find in the diff (`StackedDiffList`'s find bar).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffFind {
    /// The last query stays for the next Cmd/Ctrl+F.
    Closed { query: String },
    Open(FindState),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindState {
    pub query: String,
    /// The file whose matches are listed first, as "In this file".
    pub file: String,
    /// The selected match.
    pub active: Option<FindTarget>,
    pub repo: RepoResults,
    /// Bumped whenever the bar is (re)opened, so the view focuses its box (`focusNonce`).
    pub focus: u64,
}

/// A match the diff should scroll to and select.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindTarget {
    pub path: String,
    pub side: Side,
    /// 1-based, in `side`'s file.
    pub line: u32,
    /// Bumped on every selection, so choosing the same match again moves there again.
    pub seq: u64,
}

impl FindTarget {
    fn is(&self, found: &DiffMatch) -> bool {
        self.path == found.path && self.side == found.side && self.line == found.line
    }
}

/// A failure the tab shows until dismissed (a metadata write, a prompt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    Clear,
    Error(String),
}

/// Something loaded in the background (the base branch list, a commit's diff).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loaded<T> {
    Loading,
    Ready(T),
    Failed(String),
}

/// `checkConflicts`'s answers for a child: its branch merged into its parent's, dry-run, and
/// whether the parent moved on. The last answers stay while a check runs and after one
/// fails (`conflictInfo`, `parentHasNewCommits`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflicts {
    /// The last dry-run that ran (none yet: no conflicts).
    pub last: ConflictInfo,
    /// The parent has commits this branch does not contain yet.
    pub parent_has_new_commits: bool,
    pub check: ConflictCheck,
}

/// A conflict check running, or why the last one failed (`conflictError`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictCheck {
    /// Not a child workspace, not checked yet, or the last check ran.
    Idle,
    Checking,
    Failed(String),
}

impl Default for Conflicts {
    fn default() -> Self {
        Self {
            last: ConflictInfo { has_conflicts: false, conflicted_files: Vec::new(), messages: Vec::new() },
            parent_has_new_commits: false,
            check: ConflictCheck::Idle,
        }
    }
}

impl Conflicts {
    /// `hasConflicts`.
    pub fn has_conflicts(&self) -> bool {
        self.last.has_conflicts
    }

    /// A check's outcome. Whether the parent moved on is known whenever
    /// `merge-base --is-ancestor` ran, even when the dry-run failed; when it could not run,
    /// nothing is learnt (Electron's `Promise.all` rejects).
    fn apply(&mut self, conflicts: Result<ConflictInfo, String>, merged: Result<bool, String>) {
        match (conflicts, merged) {
            (_, Err(error)) => self.check = ConflictCheck::Failed(error),
            (Ok(conflicts), Ok(merged)) => {
                self.parent_has_new_commits = !merged;
                self.last = conflicts;
                self.check = ConflictCheck::Idle;
            }
            (Err(error), Ok(merged)) => {
                self.parent_has_new_commits = !merged;
                self.check = ConflictCheck::Failed(error);
            }
        }
    }
}

/// A commit picked in the Commits view, and its changes.
#[derive(Debug, Clone)]
pub struct SelectedCommit {
    pub commit: GitLogCommit,
    pub diff: Loaded<CommitDiff>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitDiff {
    pub files: Vec<DiffFile>,
    pub diffs: FileDiffs,
}

/// What the view tabs count (`hasUncommitted`, the tab badges), from what each view loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Overview {
    /// Files the branch changed since its base.
    pub committed: usize,
    /// Changed files in the working tree.
    pub uncommitted: usize,
    /// Commits listed so far, and whether more exist.
    pub commits: usize,
    pub more_commits: bool,
}

/// The workspace as the Review header shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewHeader {
    /// The workspace's `name` (`wsData.name`), not the Settings tab's `displayName`.
    pub name: String,
    pub branch: String,
    /// The tab names a parent workspace (`parentWorkspaceId`): the child's actions show
    /// (merge, or Close and Clean), even while the parent's branch is unknown.
    pub has_parent: bool,
    /// The parent's branch, the default base (`None`: a top-level workspace, or a parent
    /// not loaded or not on a branch).
    pub parent_branch: Option<String>,
    pub detached: bool,
    /// An AI harness tab is running (the prompt buttons show).
    pub harness: bool,
}

/// Commits listed per page (`getLog(…, 50)`).
pub const COMMITS_PAGE: usize = 50;

/// Patches carry whole files, so the view folds unchanged lines itself and can reveal them
/// all (Pierre's "hide unchanged regions").
const FULL_CONTEXT: &str = "-U1000000";

pub struct ReviewModel {
    exec: ExecClient,
    git: GitApi,
    path: String,
    base: ReviewBase,
    /// The tab state's `parentWorkspaceId` is set (Electron's `hasParent`).
    has_parent: bool,
    mode: ReviewMode,
    fallback: ModeFallback,
    /// Each view's data: the branch against its base, the working tree, the commits. The
    /// first two load together whatever is shown; the commits when first shown.
    committed: Slot<Committed>,
    uncommitted: Slot<Uncommitted>,
    commits: Slot<CommitList>,
    committed_load: Option<Task<()>>,
    uncommitted_load: Option<Task<()>>,
    commits_load: Option<Task<()>>,
    /// Files read on their own (near the viewport, searched, or a header's refresh), by list.
    /// A load of the whole list drops those started before it (it reads them again), and keeps
    /// those started since.
    file_loads: HashMap<(DiffList, String), Task<()>>,
    staging: GitOp,
    commit: GitOp,
    /// The workspace whose metadata holds the comments and viewed marks.
    workspace: WeakEntity<WorkspaceStore>,
    head: Head,
    draft: Option<CommentDraft>,
    /// Adding the draft.
    adding: GitOp,
    notice: Notice,
    /// The file at the top of the diff (`activeFile`): the tree highlights it and "next
    /// unviewed" starts from it. It follows the scroll, a click and a jump.
    active_file: Option<String>,
    /// The files in the diff's top band at its last report (see [`Self::follow_scroll`]).
    scroll_band: Vec<String>,
    /// The file the diff scrolls to once a list shows (`scrollToFile`): the one clicked or
    /// jumped to, or the tab state's `selectedFilePath` when the tab opens.
    scroll_request: Option<String>,
    /// The workspace's favourite files (`FavouriteFileSection`), its ancestors' included, and
    /// the paths they were resolved from.
    favourites: crate::file_browser::Favourites,
    favourite_paths: Vec<String>,
    favourite: FavouriteSelection,
    find: DiffFind,
    /// Find selections and openings so far (each gets a new `seq` / `focus`).
    find_seq: u64,
    _find: Option<Task<()>>,
    _favourites: Option<Task<()>>,
    /// Both sides of each image file shown.
    images: HashMap<String, ImageSides>,
    _images: HashMap<String, Task<()>>,
    conflicts: Conflicts,
    /// Local and remote branches for the base picker, loaded when it opens.
    branches: Loaded<Vec<String>>,
    selected_commit: Option<SelectedCommit>,
    /// The whitespace toggle: with [`Whitespace::Ignore`], lines equal once trimmed show
    /// unchanged ([`PatchReader`]).
    whitespace: Whitespace,
    /// "Refresh changes" (or the tab shown again) runs until what it reloads has answered.
    refreshing: bool,
    visibility: Visibility,
    conflicts_load: Option<Task<()>>,
    /// The fresh conflict check before a merge, while it runs.
    pre_merge: Option<Task<()>>,
    _branches: Option<Task<()>>,
    _commit: Option<Task<()>>,
    _more: Option<Task<()>>,
    _op: Option<Task<()>>,
    head_load: Option<Task<()>>,
    _add: Option<Task<()>>,
    _workspace: Subscription,
}

/// Git commands for review must not hang the tab forever on a huge repo.
const GIT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub enum ImageSides {
    Loading,
    /// `None`: the file does not exist on that side (added / deleted).
    Loaded {
        before: Option<std::sync::Arc<Vec<u8>>>,
        after: Option<std::sync::Arc<Vec<u8>>>,
    },
    Failed(String),
}

/// Which way "next unviewed" goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Next,
    Previous,
}

/// `navigateUnviewed`: the first file after (or before) `current` in `order` that is not
/// viewed; from the start (or end) when `current` is not in the list. No wrapping.
pub fn unviewed_from<'a>(
    order: &'a [String],
    viewed: &IndexMap<String, ViewedFileStats>,
    current: Option<&str>,
    direction: Direction,
) -> Option<&'a str> {
    let at = current.and_then(|current| order.iter().position(|path| path == current));
    let unviewed = |path: &&String| !viewed.contains_key(path.as_str());
    match (direction, at) {
        (Direction::Next, Some(index)) => order[index + 1..].iter().find(unviewed),
        (Direction::Next, None) => order.iter().find(unviewed),
        (Direction::Previous, Some(index)) => order[..index].iter().rev().find(unviewed),
        (Direction::Previous, None) => order.iter().rev().find(unviewed),
    }
    .map(String::as_str)
}

/// The file Electron ends up making active when the diff's top band goes from `previous`
/// to `band` (both in list order): the band's last file. A file entering the band is
/// reported by `StackedDiffList`'s `IntersectionObserver`, but making it active renders
/// `ReviewBrowser` again with a new `files` array, which makes a new observer; a new
/// observer reports every section in the band, and the last one wins. An unchanged or empty
/// band changes nothing.
pub fn band_active<'a>(previous: &[String], band: &'a [String]) -> Option<&'a str> {
    if band == previous {
        return None;
    }
    band.last().map(String::as_str)
}

/// Which list of files the diff shows (`StackedDiffList`): each view mounts its own, and in
/// Commits it lists the picked commit's files (another commit's replace them).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DiffList {
    Committed,
    Uncommitted,
    /// The picked commit's, by its hash.
    Commit(String),
}

impl DiffList {
    /// The view showing it.
    pub fn mode(&self) -> ReviewMode {
        match self {
            DiffList::Committed => ReviewMode::Committed,
            DiffList::Uncommitted => ReviewMode::Uncommitted,
            DiffList::Commit(_) => ReviewMode::Commits,
        }
    }
}

/// The last "Collapse all" / "Expand all" of a list (`collapseAll`). It lasts as long as
/// the list: files listed later follow it. Another view's list starts untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollapseAll {
    Untouched,
    Collapsed,
    Expanded,
}

impl CollapseAll {
    /// The button's label.
    pub fn label(self) -> &'static str {
        match self {
            CollapseAll::Collapsed => "Expand all",
            CollapseAll::Untouched | CollapseAll::Expanded => "Collapse all",
        }
    }

    /// After a click on the button.
    pub fn toggled(self) -> Self {
        match self {
            CollapseAll::Collapsed => CollapseAll::Expanded,
            CollapseAll::Untouched | CollapseAll::Expanded => CollapseAll::Collapsed,
        }
    }

    /// Whether a file newly listed starts collapsed (`FileDiffSection`): a viewed one does,
    /// then the last collapse-all applies.
    pub fn initial_collapsed(self, viewed: bool) -> bool {
        match self {
            CollapseAll::Untouched => viewed,
            CollapseAll::Collapsed => true,
            CollapseAll::Expanded => false,
        }
    }
}

impl ReviewModel {
    pub fn new(
        exec: ExecClient,
        git: GitApi,
        path: String,
        base: ReviewBase,
        state: &Value,
        workspace: &Entity<WorkspaceStore>,
        cx: &mut Context<Self>,
    ) -> Self {
        let requested = ReviewMode::parse(state.get("viewMode").and_then(Value::as_str).unwrap_or(""));
        let has_parent = state.get("parentWorkspaceId").and_then(Value::as_str).is_some_and(|id| !id.is_empty());
        // No base: nothing to diff the branch against (Uncommitted shows until there is one);
        // Commits lists the whole history.
        let (mode, fallback) = match (&base, requested) {
            (ReviewBase::None, ReviewMode::Committed) => (ReviewMode::Uncommitted, ModeFallback::NoBaseForCommitted),
            (_, mode) => (mode, ModeFallback::Asked),
        };
        // Comments and viewed marks are read from the metadata whenever shown; favourites
        // are resolved again when the workspace's favourite paths (its ancestors' included)
        // change.
        let observe = cx.observe(workspace, |model: &mut Self, workspace, cx| {
            let paths = workspace.read(cx).all_favourite_paths();
            if paths != model.favourite_paths {
                // Other favourites: the pane goes back to the diff.
                model.favourite = FavouriteSelection::None;
                model.load_favourites(cx);
            }
            cx.notify();
        });
        let mut model = Self {
            exec,
            git,
            path,
            base,
            has_parent,
            mode,
            fallback,
            committed: Slot::NotLoaded,
            uncommitted: Slot::NotLoaded,
            commits: Slot::NotLoaded,
            committed_load: None,
            uncommitted_load: None,
            commits_load: None,
            file_loads: HashMap::new(),
            staging: GitOp::Idle,
            commit: GitOp::Idle,
            workspace: workspace.downgrade(),
            head: Head::Loading,
            draft: None,
            adding: GitOp::Idle,
            notice: Notice::Clear,
            active_file: None,
            scroll_band: Vec::new(),
            // A Review opened again comes back to the file last clicked in Committed.
            scroll_request: state.get("selectedFilePath").and_then(Value::as_str).filter(|p| !p.is_empty()).map(str::to_owned),
            favourites: crate::file_browser::Favourites::Empty,
            favourite_paths: Vec::new(),
            favourite: FavouriteSelection::None,
            find: DiffFind::Closed { query: String::new() },
            find_seq: 0,
            _find: None,
            _favourites: None,
            images: HashMap::new(),
            _images: HashMap::new(),
            conflicts: Conflicts::default(),
            branches: Loaded::Loading,
            selected_commit: None,
            whitespace: Whitespace::Compare,
            refreshing: false,
            visibility: Visibility::Unknown,
            conflicts_load: None,
            pre_merge: None,
            _branches: None,
            _commit: None,
            _more: None,
            _op: None,
            head_load: None,
            _add: None,
            _workspace: observe,
        };
        model.load_favourites(cx);
        model.reload_all(cx);
        model
    }

    pub fn mode(&self) -> ReviewMode {
        self.mode
    }

    pub fn base(&self) -> &ReviewBase {
        &self.base
    }

    /// What the current view shows.
    pub fn content(&self) -> ReviewContent {
        match (self.mode, &self.base) {
            (ReviewMode::Committed, ReviewBase::Branch(_)) => {
                self.committed.content(|committed| ReviewContent::Committed(committed.clone()))
            }
            (ReviewMode::Committed, ReviewBase::None) => {
                ReviewContent::Failed("This workspace has no parent to compare against".to_owned())
            }
            (ReviewMode::Uncommitted, _) => {
                self.uncommitted.content(|uncommitted| ReviewContent::Uncommitted(uncommitted.clone()))
            }
            (ReviewMode::Commits, _) => self.commits.content(|list| ReviewContent::Commits(list.commits.clone())),
        }
    }

    pub fn staging(&self) -> &GitOp {
        &self.staging
    }

    pub fn commit_op(&self) -> &GitOp {
        &self.commit
    }

    pub fn head(&self) -> &Head {
        &self.head
    }

    pub fn draft(&self) -> Option<&CommentDraft> {
        self.draft.as_ref()
    }

    pub fn adding(&self) -> &GitOp {
        &self.adding
    }

    pub fn notice(&self) -> &Notice {
        &self.notice
    }

    pub fn active_file(&self) -> Option<&str> {
        self.active_file.as_deref()
    }

    pub fn overview(&self) -> Overview {
        let (commits, more_commits) = match &self.commits {
            Slot::Ready(list) => (list.commits.len(), list.has_more),
            Slot::NotLoaded | Slot::Loading | Slot::Failed(_) => (0, false),
        };
        Overview {
            committed: self.committed.ready().map_or(0, |committed| committed.files.len()),
            uncommitted: self.uncommitted.ready().map_or(0, |uncommitted| uncommitted.changes.files.len()),
            commits,
            more_commits,
        }
    }

    pub fn conflicts(&self) -> &Conflicts {
        &self.conflicts
    }

    pub fn branches(&self) -> &Loaded<Vec<String>> {
        &self.branches
    }

    pub fn selected_commit(&self) -> Option<&SelectedCommit> {
        self.selected_commit.as_ref()
    }

    /// The list the diff shows in the current view; none in Commits until a commit is
    /// picked (`selectedCommit && <StackedDiffList>`). Committed and Uncommitted keep theirs
    /// whatever happens to the picked commit (a base change, a checkout, a refresh).
    pub fn diff_list(&self) -> Option<DiffList> {
        match self.mode {
            ReviewMode::Committed => Some(DiffList::Committed),
            ReviewMode::Uncommitted => Some(DiffList::Uncommitted),
            ReviewMode::Commits => self.selected_commit.as_ref().map(|s| DiffList::Commit(s.commit.hash.clone())),
        }
    }

    pub fn ignore_whitespace(&self) -> bool {
        self.whitespace == Whitespace::Ignore
    }

    /// The workspace's name and branch, its parent and its branch, and whether a harness runs.
    pub fn header(&self, cx: &App) -> ReviewHeader {
        let Some(workspace) = self.workspace.upgrade() else {
            return ReviewHeader {
                name: String::new(),
                branch: String::new(),
                has_parent: self.has_parent,
                parent_branch: None,
                detached: false,
                harness: false,
            };
        };
        let workspace = workspace.read(cx);
        let file = workspace.file();
        ReviewHeader {
            name: file.name.clone(),
            branch: file.git_branch.clone().unwrap_or_default(),
            has_parent: self.has_parent,
            parent_branch: workspace.parent_branch().map(str::to_owned),
            detached: file.is_detached == Some(true),
            harness: workspace.has_running_harness(),
        }
    }

    /// The base differs from the parent's branch (`isBaseOverridden`).
    pub fn is_base_overridden(&self, cx: &App) -> bool {
        match (&self.base, self.header(cx).parent_branch) {
            (ReviewBase::Branch(base), Some(parent)) => *base != parent,
            (ReviewBase::Branch(_), None) => true,
            (ReviewBase::None, _) => false,
        }
    }

    /// `BaseBranchSelector`'s choice: compare against `branch`, or the parent's branch
    /// (`None`, "Reset to default"). Kept in the tab state as `baseBranchOverride`.
    pub fn set_base(&mut self, branch: Option<String>, cx: &mut Context<Self>) {
        let parent = self.header(cx).parent_branch;
        let base = match (&branch, &parent) {
            (Some(branch), _) => ReviewBase::Branch(branch.clone()),
            (None, Some(parent)) => ReviewBase::Branch(parent.clone()),
            (None, None) => ReviewBase::None,
        };
        // Choosing the default clears the override, so the tab follows the parent again.
        match branch.filter(|branch| parent.as_ref() != Some(branch)) {
            Some(branch) => cx.emit(ReviewEvent::StateChanged(json!({ "baseBranchOverride": branch }))),
            None => cx.emit(ReviewEvent::StateRemoved("baseBranchOverride")),
        }
        self.compare_against(base, cx);
    }

    /// Compare against `base` from now on (the picker's choice, or the parent's new branch
    /// for a tab without an override), keeping the view. Without a base, Committed has
    /// nothing to show: Uncommitted is shown, until a base brings Committed back (the saved
    /// `viewMode` still says committed).
    pub(crate) fn compare_against(&mut self, base: ReviewBase, cx: &mut Context<Self>) {
        if base == self.base {
            return;
        }
        self.base = base;
        match (&self.base, self.mode, self.fallback) {
            (ReviewBase::None, ReviewMode::Committed, _) => {
                self.mode = ReviewMode::Uncommitted;
                self.fallback = ModeFallback::NoBaseForCommitted;
            }
            (ReviewBase::Branch(_), _, ModeFallback::NoBaseForCommitted) => {
                self.mode = ReviewMode::Committed;
                self.fallback = ModeFallback::Asked;
            }
            (ReviewBase::None, ReviewMode::Uncommitted | ReviewMode::Commits, _)
            | (ReviewBase::Branch(_), _, ModeFallback::Asked) => {}
        }
        self.reload_all(cx);
    }

    /// The parent's branch changed or became known, making `base` this tab's base. A tab
    /// following the parent compares against it; one keeping its base (an override) checks
    /// its conflicts again against the parent's branch: made before that branch was known,
    /// at start-up, its check found no parent.
    pub(crate) fn parent_branch_changed(&mut self, base: ReviewBase, cx: &mut Context<Self>) {
        if base == self.base {
            self.check_conflicts(cx);
            cx.notify();
        } else {
            self.compare_against(base, cx);
        }
    }

    /// The workspace is on another branch (a checkout, a rename): everything reloads.
    pub(crate) fn branch_changed(&mut self, cx: &mut Context<Self>) {
        self.reload_all(cx);
    }

    /// Local then remote branches, each once, for the base picker.
    pub fn load_branches(&mut self, cx: &mut Context<Self>) {
        let (git, path) = (self.git.clone(), self.path.clone());
        self.branches = Loaded::Loading;
        cx.notify();
        self._branches = Some(cx.spawn(async move |this, cx| {
            let (local, remote) = futures::join!(git.list_local_branches(&path), git.list_remote_branches(&path));
            let branches = match (local, remote) {
                (Ok(local), Ok(remote)) => {
                    let all: IndexSet<String> = local.into_iter().chain(remote).collect();
                    Loaded::Ready(all.into_iter().collect())
                }
                (Err(error), _) | (_, Err(error)) => Loaded::Failed(error.to_string()),
            };
            match this.update(cx, |model, cx| {
                model.branches = branches;
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
    }

    /// The whitespace toggle: the patches are read again, lines equal once trimmed
    /// unchanged or not.
    pub fn set_ignore_whitespace(&mut self, ignore: bool, cx: &mut Context<Self>) {
        let whitespace = if ignore { Whitespace::Ignore } else { Whitespace::Compare };
        if self.whitespace == whitespace {
            return;
        }
        self.whitespace = whitespace;
        self.load_committed(cx);
        self.load_uncommitted(cx);
        // The commit's changes stay shown until they are read again (Electron's sections
        // re-read their contents, keeping the list).
        self.load_commit_diff(cx);
    }

    /// Show a listed commit's changes below the list (`loadCommitDiff`).
    pub fn select_commit(&mut self, hash: &str, cx: &mut Context<Self>) {
        let Slot::Ready(list) = &self.commits else {
            return;
        };
        let Some(commit) = list.commits.iter().find(|c| c.hash == hash).cloned() else {
            return;
        };
        self.selected_commit = Some(SelectedCommit { commit, diff: Loaded::Loading });
        // Another commit's files: the band is watched afresh.
        self.active_file = None;
        self.scroll_band.clear();
        cx.notify();
        self.load_commit_diff(cx);
    }

    /// Read the selected commit's changes (none selected: nothing to read): its files, which
    /// show at once the first time (each file's diff read when wanted); read again, what is
    /// shown stays until the diffs asked for in it are read again.
    fn load_commit_diff(&mut self, cx: &mut Context<Self>) {
        self.drop_file_loads(|list| matches!(list, DiffList::Commit(_)));
        let Some(selected) = &self.selected_commit else {
            return;
        };
        let (git, path, hash) = (self.git.clone(), self.path.clone(), selected.commit.hash.clone());
        let patches = self.patch_reader(cx);
        self._commit = Some(cx.spawn(async move |this, cx| {
            let files = match git.get_commit_diff(&path, &hash).await {
                Ok(files) => files,
                Err(error) => {
                    let failed = Loaded::Failed(format!("Failed to load commit diff: {error}"));
                    match this.update(cx, |model, cx| model.commit_diff_read(&hash, failed, true, cx)) {
                        Ok(()) | Err(_) => {} // Err: tab closed
                    }
                    return;
                }
            };
            let list = DiffList::Commit(hash.clone());
            let listed = FileDiffs::not_loaded(files.iter().map(|file| file.path.as_str()));
            let reads = match this.update(cx, |model, cx| {
                if model.selected_commit.as_ref().is_some_and(|s| !matches!(s.diff, Loaded::Ready(_))) {
                    let diff = Loaded::Ready(CommitDiff { files: files.clone(), diffs: listed.clone() });
                    model.commit_diff_read(&hash, diff, true, cx);
                    return None;
                }
                let again = model.asked_again(&list, &listed);
                Some(model.commit_reads(&hash, &files, &again))
            }) {
                Ok(Some(reads)) => reads,
                Ok(None) | Err(_) => return, // None: shown at once; Err: tab closed
            };
            let diffs = read_files(&patches, listed, reads).await;
            match this.update(cx, |model, cx| {
                let show = |model: &mut Self, diffs, cx: &mut Context<Self>| {
                    model.commit_diff_read(&hash, Loaded::Ready(CommitDiff { files, diffs }), true, cx);
                };
                model.reloaded(&list, diffs, show, cx);
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
    }

    /// The picked commit `hash`'s changes as read (another commit picked since: dropped);
    /// `listed`: other files than shown, whose images load.
    fn commit_diff_read(&mut self, hash: &str, diff: Loaded<CommitDiff>, listed: bool, cx: &mut Context<Self>) {
        if let Some(selected) = &mut self.selected_commit
            && selected.commit.hash == hash
        {
            selected.diff = diff;
            if listed {
                self.load_images(cx);
            }
            self.load_for_find(cx);
            cx.notify();
        }
    }

    /// `git show` of a commit's changes (whole files), limited to `paths`. Without renames, as
    /// the commit's list (`diff-tree`) has them: a renamed file is listed as its old path
    /// deleted and its new one added (Electron read each listed path's two sides), and with
    /// renames, git would pair the two halves into one rename whenever one command reads both
    /// (a reload reads every file asked for at once), the deleted file then showing no section
    /// and the added one no lines.
    fn show_request(&self, hash: &str, paths: &[&str]) -> ExecRequest {
        let mut args = vec!["show", "--no-color", "--no-renames", FULL_CONTEXT, "--format=", hash];
        args.extend_from_slice(paths);
        self.git_request(&args)
    }

    /// The next page of commits, appended (the list's infinite scroll).
    pub fn load_more_commits(&mut self, cx: &mut Context<Self>) {
        let Slot::Ready(list) = &self.commits else {
            return;
        };
        if !list.has_more || self._more.is_some() || self.commits_load.is_some() {
            return;
        }
        let (git, path, scope, skip) = (self.git.clone(), self.path.clone(), log_scope(&self.base), list.commits.len());
        self._more = Some(cx.spawn(async move |this, cx| {
            let log = git.get_log(&path, scope, skip, COMMITS_PAGE).await;
            match this.update(cx, |model, cx| {
                model._more = None;
                match (log, &mut model.commits) {
                    (Ok(log), Slot::Ready(list)) if list.commits.len() == skip => {
                        list.commits.extend(log.commits);
                        list.has_more = log.has_more;
                    }
                    (Ok(_), _) => {} // the list was reloaded meanwhile
                    (Err(error), _) => model.notice = Notice::Error(format!("Failed to load commits: {error}")),
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
        cx.notify();
    }

    /// A page of commits is loading.
    pub fn loading_more_commits(&self) -> bool {
        self._more.is_some()
    }

    /// Mark (or unmark) several shown files at once: "check all", "mark all above".
    pub fn set_files_viewed(&mut self, paths: &[String], viewed: bool, cx: &mut Context<Self>) {
        let files: Vec<(String, treeterm_core::git::FileChangeStats)> = self
            .shown_files()
            .iter()
            .filter(|file| paths.iter().any(|path| path == file.path))
            .map(|file| (file.path.to_owned(), file.stats))
            .collect();
        let result = self.with_metadata(cx, |metadata| {
            let store = ViewedFilesStore::new(metadata);
            if viewed {
                let files: Vec<ViewedFile<'_>> = files.iter().map(|(path, stats)| ViewedFile { path, stats: *stats }).collect();
                store.mark_files_viewed(&files).map_err(|e| e.to_string())
            } else {
                let paths: Vec<&str> = files.iter().map(|(path, _)| path.as_str()).collect();
                store.unmark_files_viewed(&paths).map_err(|e| e.to_string())
            }
        });
        match result {
            Some(()) | None => {} // None: the failure is the notice
        }
    }

    /// "Cancel" / "Close".
    pub fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(ReviewEvent::CloseRequested);
    }

    /// The merge buttons (`handleMerge` / `handleMergeAndKeep`). When both branches are
    /// known, a fresh dry-run comes first: conflicts that appeared since the last check
    /// refuse the merge ("Cannot merge: …") and show, so `git merge` never stops half-way in
    /// the parent's worktree. A merge that removes this workspace then asks before committing
    /// its uncommitted files ([`ReviewEvent::ConfirmMerge`]). The session runs the merge; its
    /// output is the workspace's [`Self::merge_output`].
    pub fn merge(&mut self, squash: bool, after: AfterMerge, cx: &mut Context<Self>) {
        if self.processing(cx) != Processing::Idle {
            return;
        }
        let header = self.header(cx);
        // An unknown parent: no dry-run; the session refuses the merge itself.
        let (Some(parent), false) = (header.parent_branch, header.branch.is_empty()) else {
            self.merge_checked(squash, after, cx);
            return;
        };
        let (git, path, branch) = (self.git.clone(), self.path.clone(), header.branch);
        self.pre_merge = Some(cx.spawn(async move |this, cx| {
            let fresh = git.check_merge_conflicts(&path, &branch, &parent).await;
            match this.update(cx, |model, cx| {
                model.pre_merge = None;
                match fresh {
                    Ok(conflicts) if conflicts.has_conflicts => {
                        let message = format!(
                            "Cannot merge: {} conflict(s) detected. Resolve conflicts first.",
                            conflicts.conflicted_files.len()
                        );
                        model.conflicts.last = conflicts;
                        cx.emit(ReviewEvent::Alert(message));
                    }
                    Ok(_) => model.merge_checked(squash, after, cx),
                    // As Electron: a dry-run that cannot run does not hold the merge back.
                    Err(error) => {
                        tracing::warn!(%error, "conflict check before merging failed");
                        model.merge_checked(squash, after, cx);
                    }
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
        cx.notify();
    }

    /// The merge passed its conflict check.
    fn merge_checked(&mut self, squash: bool, after: AfterMerge, cx: &mut Context<Self>) {
        match (after, self.overview().uncommitted) {
            (AfterMerge::Remove, files @ 1..) => cx.emit(ReviewEvent::ConfirmMerge { squash, files }),
            (AfterMerge::Remove, 0) | (AfterMerge::Keep, _) => cx.emit(ReviewEvent::MergeRequested { squash, after }),
        }
    }

    /// Yes to [`ReviewEvent::ConfirmMerge`]: merge, committing the uncommitted files first.
    pub fn confirm_merge(&mut self, squash: bool, cx: &mut Context<Self>) {
        cx.emit(ReviewEvent::MergeRequested { squash, after: AfterMerge::Remove });
    }

    /// This workspace's merge into its parent, running or finished (`mergeOutput`).
    pub fn merge_output(&self, cx: &App) -> Option<crate::workspace_store::MergeOutput> {
        self.workspace.upgrade()?.read(cx).merge_output().cloned()
    }

    /// What the header's actions wait for (`isProcessing`).
    pub fn processing(&self, cx: &App) -> Processing {
        let Some(workspace) = self.workspace.upgrade() else {
            return Processing::Idle;
        };
        let workspace = workspace.read(cx);
        if workspace.merge_output().is_some_and(|o| o.state == MergeState::Running) {
            Processing::Merging
        } else if workspace.is_removing() {
            Processing::Removing
        } else if self.pre_merge.is_some() {
            Processing::CheckingConflicts
        } else {
            Processing::Idle
        }
    }

    /// The merge output's close button.
    pub fn dismiss_merge_output(&mut self, cx: &mut Context<Self>) {
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |w, cx| w.dismiss_merge_output(cx));
        }
    }

    /// "Close and Clean": remove the worktree, keep the branch. The session refuses it
    /// unless this is a worktree of a loaded git repo ([`Self::close_failed`]).
    pub fn close_and_clean(&mut self, cx: &mut Context<Self>) {
        cx.emit(ReviewEvent::CloseAndCleanRequested);
    }

    /// "Close and Clean" was refused before anything ran.
    pub(crate) fn close_failed(&mut self, error: &str, cx: &mut Context<Self>) {
        cx.emit(ReviewEvent::Alert(format!("Close failed: {error}")));
    }

    /// "Prompt Rebase": ask the harness to rebase onto the parent's branch.
    pub fn prompt_rebase(&mut self, cx: &mut Context<Self>) {
        let header = self.header(cx);
        if let Some(parent) = header.parent_branch {
            self.prompt_harness(&format!("rebase local branch {} onto {parent}", header.branch), Vec::new(), cx);
        }
    }

    /// "Prompt Update from Parent": ask the harness to merge the parent's branch in.
    pub fn prompt_update_from_parent(&mut self, cx: &mut Context<Self>) {
        let header = self.header(cx);
        if let Some(parent) = header.parent_branch {
            self.prompt_harness(&format!("merge {parent} into local branch {}", header.branch), Vec::new(), cx);
        }
    }

    fn busy(&self) -> bool {
        self.staging == GitOp::Running || self.commit == GitOp::Running
    }

    /// `git add` a file or everything.
    pub fn stage(&mut self, target: StageTarget, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let (git, path) = (self.git.clone(), self.path.clone());
        self.staging = GitOp::Running;
        self._op = Some(cx.spawn(async move |this, cx| {
            let result = match &target {
                StageTarget::File(file) => git
                    .stage_file(&path, file)
                    .await
                    .map_err(|e| format!("Failed to stage {file}: {e}")),
                StageTarget::All => git.stage_all(&path).await.map_err(|e| format!("Failed to stage all files: {e}")),
            };
            Self::finish_staging(this, result, cx).await;
        }));
        cx.notify();
    }

    /// `git reset HEAD` a file or everything.
    pub fn unstage(&mut self, target: StageTarget, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        let (git, path) = (self.git.clone(), self.path.clone());
        self.staging = GitOp::Running;
        self._op = Some(cx.spawn(async move |this, cx| {
            let result = match &target {
                StageTarget::File(file) => git
                    .unstage_file(&path, file)
                    .await
                    .map_err(|e| format!("Failed to unstage {file}: {e}")),
                StageTarget::All => {
                    git.unstage_all(&path).await.map_err(|e| format!("Failed to unstage all files: {e}"))
                }
            };
            Self::finish_staging(this, result, cx).await;
        }));
        cx.notify();
    }

    async fn finish_staging(this: gpui_kit::WeakEntity<Self>, result: Result<(), String>, cx: &mut gpui_kit::AsyncApp) {
        match this.update(cx, |model, cx| {
            match result {
                Ok(()) => {
                    model.staging = GitOp::Idle;
                    model.load_uncommitted(cx);
                    cx.emit(ReviewEvent::GitChanged);
                }
                Err(error) => model.staging = GitOp::Failed(error),
            }
            cx.notify();
        }) {
            Ok(()) | Err(_) => {} // Err: tab closed
        }
    }

    /// Commit what is staged. An empty message is refused, as in Electron.
    pub fn commit(&mut self, message: &str, cx: &mut Context<Self>) {
        let message = message.trim().to_owned();
        if message.is_empty() {
            self.commit = GitOp::Failed("Commit message is required".to_owned());
            cx.notify();
            return;
        }
        if self.busy() {
            return;
        }
        let (git, path) = (self.git.clone(), self.path.clone());
        self.commit = GitOp::Running;
        self._op = Some(cx.spawn(async move |this, cx| {
            let result = git.commit_staged(&path, &message).await;
            match this.update(cx, |model, cx| {
                match result {
                    Ok(()) => {
                        model.commit = GitOp::Idle;
                        cx.emit(ReviewEvent::Committed);
                        // `HEAD` moved: new comments anchor to the new commit.
                        model.refresh_head(cx);
                        model.load_uncommitted(cx);
                        model.load_committed(cx);
                    }
                    Err(error) => model.commit = GitOp::Failed(format!("Failed to commit: {error}")),
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {}
            }
        }));
        cx.notify();
    }

    pub fn set_mode(&mut self, mode: ReviewMode, cx: &mut Context<Self>) {
        // A view picked stays once a base arrives; picking the Uncommitted shown in
        // Committed's place saves it (Electron persists every click).
        let stood_in = std::mem::replace(&mut self.fallback, ModeFallback::Asked) == ModeFallback::NoBaseForCommitted;
        if self.mode == mode {
            if stood_in {
                cx.emit(ReviewEvent::StateChanged(json!({ "viewMode": mode.as_str() })));
            }
            return;
        }
        self.mode = mode;
        // Another view, another diff (Electron's find bar goes with its list, and the new
        // list's band is watched afresh). The comment being written stays, as Electron's
        // `commentInput`, and so does a file still to scroll to (`scrollToFile`).
        self.find = DiffFind::Closed { query: String::new() };
        self._find = None;
        self.active_file = None;
        self.scroll_band.clear();
        cx.emit(ReviewEvent::StateChanged(json!({ "viewMode": mode.as_str() })));
        // The views keep what they loaded; the commits load when first shown (and again
        // while none are listed: `commits.length === 0`).
        let listed = matches!(&self.commits, Slot::Ready(list) if !list.commits.is_empty());
        if mode == ReviewMode::Commits && !listed && self.commits_load.is_none() {
            self.load_commits(COMMITS_PAGE, cx);
        }
        self.load_images(cx);
        cx.notify();
    }

    /// "Refresh changes" (`handleRefresh`): reload everything, keeping what is shown until
    /// the new content arrives; the favourites are resolved again and the workspace's git
    /// status is refreshed. Commits already listed reload as many (Electron never reloaded
    /// them).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if let FavouriteSelection::Selected { file, viewer } = &self.favourite {
            let path = file.path.clone();
            viewer.update(cx, |viewer, cx| viewer.open(&path, false, cx));
        }
        self.refreshing = true;
        self.load_favourites(cx);
        self.refresh_head(cx);
        self.load_committed(cx);
        self.load_uncommitted(cx);
        self.check_conflicts(cx);
        match &self.commits {
            Slot::NotLoaded => {}
            Slot::Ready(list) => self.load_commits(list.commits.len().max(COMMITS_PAGE), cx),
            Slot::Loading | Slot::Failed(_) => self.load_commits(COMMITS_PAGE, cx),
        }
        cx.emit(ReviewEvent::GitChanged);
        cx.notify();
    }

    /// The tab is on screen (the active tab of the workspace shown) or not. Shown again after
    /// being hidden (a tab switch, back to the workspace), it refreshes (`ReviewBrowser`'s
    /// visibility effect). Shown for the first time, it refreshes too unless what it loaded
    /// when made is still loading: every Review tab of a session is made and loaded when the
    /// session connects, and a view first shows it later (Electron mounts it, and so loads
    /// it, when its workspace is shown).
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        let next = if visible { Visibility::Shown } else { Visibility::Hidden };
        let stale = match std::mem::replace(&mut self.visibility, next) {
            Visibility::Hidden => true,
            Visibility::Unknown => !self.is_loading(),
            Visibility::Shown => false,
        };
        if visible && stale {
            self.refresh(cx);
        }
    }

    /// "Refresh changes" runs (its button spins, disabled).
    pub fn is_refreshing(&self) -> bool {
        self.refreshing
    }

    /// A file's diff is being read, in the list shown: its section shows "Loading diff..." and
    /// its header's refresh button spins, disabled.
    pub fn is_file_loading(&self, path: &str) -> bool {
        self.diff_list().and_then(|list| self.diffs(&list)).is_some_and(|diffs| diffs.is_loading(path))
    }

    /// A file header's refresh (`onRefresh`): that file's diff alone (and an image's sides) is
    /// read again in the list shown, or for the first time, its section "Loading diff..."
    /// meanwhile. Not while it is being read (the button is disabled).
    pub fn refresh_file(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(list) = self.diff_list() else {
            return;
        };
        if self.diffs(&list).is_none_or(|diffs| diffs.is_loading(path)) || !self.read_file(list, path, cx) {
            return;
        }
        // An image's sides are read again too.
        if self.images.contains_key(path) {
            self.load_image(path.to_owned(), cx);
        }
        cx.notify();
    }

    /// The files whose sections came near the diff's viewport (`FileDiffSection`'s
    /// `IntersectionObserver`, 1500px around it): those of the list shown not asked for yet are
    /// read, each on its own (`onRequestLoad`); a file being read, read or failed stays so.
    pub fn request_files(&mut self, paths: &[String], cx: &mut Context<Self>) {
        let Some(list) = self.diff_list() else {
            return;
        };
        let Some(diffs) = self.diffs(&list) else {
            return;
        };
        let wanted: Vec<String> =
            paths.iter().filter(|path| matches!(diffs.get(path), Some(FileDiff::NotLoaded))).cloned().collect();
        for path in &wanted {
            self.read_file(list.clone(), path, cx);
        }
        if !wanted.is_empty() {
            cx.notify();
        }
    }

    /// Read `path` of `list` on its own (`handleRequestLoad`), its section "Loading diff..."
    /// meanwhile. Whether it is read (not listed: it is not). A whole load of the list started
    /// later drops it and reads the file itself.
    fn read_file(&mut self, list: DiffList, path: &str, cx: &mut Context<Self>) -> bool {
        let Some(FileRead { request, read, .. }) = self.file_reads(&list, &[path.to_owned()]).pop() else {
            return false;
        };
        if let Some(diffs) = self.diffs_mut(&list) {
            diffs.set(path, FileDiff::Loading);
        }
        let patches = self.patch_reader(cx);
        let key = (list, path.to_owned());
        let (list, path) = key.clone();
        let load = cx.spawn(async move |this, cx| {
            let diff = FileDiff::of(&path, &patches.read(request, read).await);
            match this.update(cx, |model, cx| {
                model.file_loads.remove(&(list.clone(), path.clone()));
                if let Some(diffs) = model.diffs_mut(&list) {
                    diffs.set(&path, diff);
                }
                model.load_for_find(cx);
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        });
        self.file_loads.insert(key, load);
        true
    }

    /// The commands reading `paths` of `list` as shown (a path it does not list is not read).
    fn file_reads(&self, list: &DiffList, paths: &[String]) -> Vec<FileRead> {
        match list {
            DiffList::Committed => match (&self.base, self.committed.ready()) {
                (ReviewBase::Branch(branch), Some(committed)) => self.range_reads(branch, &committed.files, paths),
                (ReviewBase::Branch(_), None) | (ReviewBase::None, _) => Vec::new(),
            },
            DiffList::Uncommitted => match self.uncommitted.ready() {
                Some(uncommitted) => self.uncommitted_reads(&uncommitted.changes, paths),
                None => Vec::new(),
            },
            DiffList::Commit(hash) => match &self.selected_commit {
                Some(SelectedCommit { commit, diff: Loaded::Ready(diff) }) if commit.hash == *hash => {
                    self.commit_reads(hash, &diff.files, paths)
                }
                Some(_) | None => Vec::new(),
            },
        }
    }

    /// The `git diff` of the branch against `branch` reading `paths` of its `files`.
    fn range_reads(&self, branch: &str, files: &[DiffFile], paths: &[String]) -> Vec<FileRead> {
        let range = format!("{branch}...HEAD");
        listed_reads(files, paths, |spec| self.diff_request(&[&[range.as_str()][..], spec].concat()))
    }

    /// The `git show` of commit `hash` reading `paths` of its `files`.
    fn commit_reads(&self, hash: &str, files: &[DiffFile], paths: &[String]) -> Vec<FileRead> {
        listed_reads(files, paths, |spec| self.show_request(hash, spec))
    }

    /// The commands reading `paths` of the working tree's `changes`, each file's first
    /// listing's diff (staged, else unstaged): one `git diff --cached` for the staged files,
    /// one `git diff` for the unstaged ones, and one per untracked file (as added).
    fn uncommitted_reads(&self, changes: &UncommittedChanges, paths: &[String]) -> Vec<FileRead> {
        let mut listed: HashMap<(&str, Staging), &UncommittedFile> = HashMap::new();
        for file in &changes.files {
            listed.entry((file.entry.path.as_str(), file.entry.staged)).or_insert(file);
        }
        let (mut staged, mut unstaged, mut reads) = (Vec::new(), Vec::new(), Vec::new());
        for path in paths {
            match (listed.get(&(path.as_str(), Staging::Staged)), listed.get(&(path.as_str(), Staging::Unstaged))) {
                (Some(file), _) => staged.push(*file),
                (None, Some(file)) if file.entry.status == FileChangeStatus::Untracked => reads.push(FileRead {
                    paths: vec![path.clone()],
                    request: untracked_request(&self.path, path),
                    read: untracked_patch,
                }),
                (None, Some(file)) => unstaged.push(*file),
                (None, None) => {}
            }
        }
        for (files, options) in [(staged, &["--cached"][..]), (unstaged, &[][..])] {
            if files.is_empty() {
                continue;
            }
            let spec = pathspec(files.iter().map(|file| (file.entry.path.as_str(), &file.entry.origin)));
            reads.push(FileRead {
                paths: files.iter().map(|file| file.entry.path.clone()).collect(),
                request: self.diff_request(&[options, &spec].concat()),
                read: patch_of,
            });
        }
        reads
    }

    /// The files of `list` a reload listing `listed` reads again: those asked for in what is
    /// shown (being read, read or failed), but not those being read on their own since the
    /// reload started (that read is the later one).
    fn asked_again(&self, list: &DiffList, listed: &FileDiffs) -> Vec<String> {
        let Some(diffs) = self.diffs(list) else {
            return Vec::new();
        };
        let asked: HashSet<&str> = diffs.asked().collect();
        listed
            .iter()
            .map(|(path, _)| path)
            .filter(|path| asked.contains(path) && !self.file_loads.contains_key(&(list.clone(), (*path).to_owned())))
            .map(str::to_owned)
            .collect()
    }

    /// The diffs of `list`'s files, as loaded.
    fn diffs(&self, list: &DiffList) -> Option<&FileDiffs> {
        match list {
            DiffList::Committed => self.committed.ready().map(|committed| &committed.diffs),
            DiffList::Uncommitted => self.uncommitted.ready().map(|uncommitted| &uncommitted.diffs),
            DiffList::Commit(hash) => match &self.selected_commit {
                Some(SelectedCommit { commit, diff: Loaded::Ready(diff) }) if commit.hash == *hash => Some(&diff.diffs),
                Some(_) | None => None,
            },
        }
    }

    fn diffs_mut(&mut self, list: &DiffList) -> Option<&mut FileDiffs> {
        match list {
            DiffList::Committed => self.committed.ready_mut().map(|committed| &mut committed.diffs),
            DiffList::Uncommitted => self.uncommitted.ready_mut().map(|uncommitted| &mut uncommitted.diffs),
            DiffList::Commit(hash) => match &mut self.selected_commit {
                Some(SelectedCommit { commit, diff: Loaded::Ready(diff) }) if commit.hash == *hash => Some(&mut diff.diffs),
                Some(_) | None => None,
            },
        }
    }

    /// A whole list loads again: the files read on their own before it are read by it.
    fn drop_file_loads(&mut self, of: impl Fn(&DiffList) -> bool) {
        self.file_loads.retain(|(list, _), _| !of(list));
    }

    /// A reload of `list` read again the files asked for when it listed them (`diffs`): `show`
    /// puts its list, with them, in place of the one shown. A file asked for on its own since
    /// the reload listed its files (or started) keeps that read, the later one, whether it
    /// finished first (read or failed: [`FileDiffs::keep_asked_since`]) or still runs (loading,
    /// also a file the reload read again). Unless the new list reads that file another way than
    /// the list it was read from (staged since, say): it is then read again, on its own.
    fn reloaded(
        &mut self,
        list: &DiffList,
        mut diffs: FileDiffs,
        show: impl FnOnce(&mut Self, FileDiffs, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) {
        let mut own: IndexSet<String> = match self.diffs(list) {
            Some(shown) => diffs.keep_asked_since(shown).into_iter().collect(),
            None => IndexSet::new(),
        };
        for (_, path) in self.file_loads.keys().filter(|(loading, _)| loading == list) {
            diffs.set(path, FileDiff::Loading);
            own.insert(path.clone());
        }
        let read_with: Vec<(String, Option<ExecRequest>)> = own
            .into_iter()
            .map(|path| {
                let request = self.file_request(list, &path);
                (path, request)
            })
            .collect();
        show(self, diffs, cx);
        for (path, before) in read_with {
            match self.file_request(list, &path) {
                Some(now) if Some(&now) != before.as_ref() => {
                    self.read_file(list.clone(), &path, cx);
                }
                Some(_) | None => {}
            }
        }
    }

    /// The command reading `path` of `list` on its own, as shown (none: not listed).
    fn file_request(&self, list: &DiffList, path: &str) -> Option<ExecRequest> {
        self.file_reads(list, &[path.to_owned()]).pop().map(|read| read.request)
    }

    /// Something is being read from git for the views: a view's data, `HEAD` or the
    /// conflict check.
    pub fn is_loading(&self) -> bool {
        self.committed_load.is_some()
            || self.uncommitted_load.is_some()
            || self.commits_load.is_some()
            || self.head_load.is_some()
            || self.conflicts_load.is_some()
    }

    /// A load finished: a refresh is over once nothing it started still runs.
    fn settle_refresh(&mut self) {
        if !self.is_loading() {
            self.refreshing = false;
        }
    }

    fn git_request(&self, args: &[&str]) -> ExecRequest {
        ExecRequest {
            cwd: self.path.clone(),
            command: "git".to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            env: HashMap::new(),
            timeout: ExecTimeout::After(GIT_TIMEOUT),
        }
    }

    /// What the review compares changed (the tab opened, the base or the workspace's own
    /// branch moved): every view reloads against it, keeping what is shown until the answers
    /// arrive (`ReviewBrowser`'s branch and base effect). The commits listed belong to the
    /// old range: they are dropped, and load again only if shown.
    fn reload_all(&mut self, cx: &mut Context<Self>) {
        self.selected_commit = None;
        self._commit = None;
        self.drop_file_loads(|list| matches!(list, DiffList::Commit(_)));
        self._more = None;
        self.commits = Slot::NotLoaded;
        self.commits_load = None;
        self.refresh_head(cx);
        self.load_committed(cx);
        self.load_uncommitted(cx);
        self.check_conflicts(cx);
        if self.mode == ReviewMode::Commits {
            self.load_commits(COMMITS_PAGE, cx);
        }
        cx.notify();
    }

    /// A merge that kept this workspace changed what it compares: the branch's and the
    /// working tree's changes reload (`handleMergeAndKeep`).
    pub(crate) fn reload_changes(&mut self, cx: &mut Context<Self>) {
        self.load_committed(cx);
        self.load_uncommitted(cx);
        cx.notify();
    }

    /// `loadDiff`: the branch against its base (none without a base). Its files show at once
    /// the first time (each file's diff read when wanted); reloaded, what is shown stays until
    /// the diffs asked for in it are read again. Its viewed marks are reconciled with it,
    /// whatever view is shown.
    fn load_committed(&mut self, cx: &mut Context<Self>) {
        self.drop_file_loads(|list| *list == DiffList::Committed);
        let ReviewBase::Branch(branch) = self.base.clone() else {
            self.committed = Slot::NotLoaded;
            self.committed_load = None;
            return;
        };
        self.committed.start();
        let (patches, git, path) = (self.patch_reader(cx), self.git.clone(), self.path.clone());
        self.committed_load = Some(cx.spawn(async move |this, cx| {
            let diff = match git.get_diff(&path, &branch).await {
                Ok(diff) => diff,
                Err(error) => {
                    match this.update(cx, |model, cx| {
                        model.committed_load = None;
                        model.committed = Slot::Failed(format!("Failed to load diff: {error}"));
                        model.view_loaded(ReviewMode::Committed, cx);
                    }) {
                        Ok(()) | Err(_) => {} // Err: tab closed
                    }
                    return;
                }
            };
            let files = viewed_entries(diff.files.iter().map(ViewedFile::from));
            let listed = FileDiffs::not_loaded(diff.files.iter().map(|file| file.path.as_str()));
            let reads = match this.update(cx, |model, cx| {
                if model.committed.show_listed(|| committed(&diff, listed.clone())) {
                    model.committed_load = None;
                    model.listed(ReviewMode::Committed, &files, cx);
                    return None;
                }
                let again = model.asked_again(&DiffList::Committed, &listed);
                Some(model.range_reads(&branch, &diff.files, &again))
            }) {
                Ok(Some(reads)) => reads,
                Ok(None) | Err(_) => return, // None: shown at once; Err: tab closed
            };
            let diffs = read_files(&patches, listed, reads).await;
            match this.update(cx, |model, cx| {
                model.committed_load = None;
                let show = |model: &mut Self, diffs, _: &mut Context<Self>| {
                    model.committed = Slot::Ready(committed(&diff, diffs));
                };
                model.reloaded(&DiffList::Committed, diffs, show, cx);
                model.listed(ReviewMode::Committed, &files, cx);
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
    }

    /// `loadUncommittedChanges`: the working tree against `HEAD` (staged, unstaged, untracked
    /// as added). Its files show at once the first time (each file's diff read when wanted);
    /// reloaded, what is shown stays until the diffs asked for in it are read again. Its
    /// viewed marks are reconciled with it, whatever view is shown.
    fn load_uncommitted(&mut self, cx: &mut Context<Self>) {
        self.drop_file_loads(|list| *list == DiffList::Uncommitted);
        self.uncommitted.start();
        let (patches, git, path) = (self.patch_reader(cx), self.git.clone(), self.path.clone());
        self.uncommitted_load = Some(cx.spawn(async move |this, cx| {
            let changes = match git.get_uncommitted_changes(&path).await {
                Ok(changes) => changes,
                Err(error) => {
                    match this.update(cx, |model, cx| {
                        model.uncommitted_load = None;
                        model.uncommitted = Slot::Failed(format!("Failed to load uncommitted changes: {error}"));
                        model.view_loaded(ReviewMode::Uncommitted, cx);
                    }) {
                        Ok(()) | Err(_) => {} // Err: tab closed
                    }
                    return;
                }
            };
            let files = viewed_entries(changes.files.iter().map(ViewedFile::from));
            let listed = FileDiffs::not_loaded(changes.files.iter().map(|file| file.entry.path.as_str()));
            let reads = match this.update(cx, |model, cx| {
                if model.uncommitted.show_listed(|| Uncommitted { changes: changes.clone(), diffs: listed.clone() }) {
                    model.uncommitted_load = None;
                    model.listed(ReviewMode::Uncommitted, &files, cx);
                    return None;
                }
                let again = model.asked_again(&DiffList::Uncommitted, &listed);
                Some(model.uncommitted_reads(&changes, &again))
            }) {
                Ok(Some(reads)) => reads,
                Ok(None) | Err(_) => return, // None: shown at once; Err: tab closed
            };
            let diffs = read_files(&patches, listed, reads).await;
            match this.update(cx, |model, cx| {
                model.uncommitted_load = None;
                let show = |model: &mut Self, diffs, _: &mut Context<Self>| {
                    model.uncommitted = Slot::Ready(Uncommitted { changes, diffs });
                };
                model.reloaded(&DiffList::Uncommitted, diffs, show, cx);
                model.listed(ReviewMode::Uncommitted, &files, cx);
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
    }

    /// `mode`'s list of files (`files`, as the viewed marks key them) shows: its viewed marks
    /// are reconciled with it.
    fn listed(&mut self, mode: ReviewMode, files: &[(String, FileChangeStats)], cx: &mut Context<Self>) {
        self.reconcile_viewed(files, cx);
        self.view_loaded(mode, cx);
    }

    /// `loadCommits`: the first `count` commits since the base. A picked commit stays while
    /// it is still listed.
    fn load_commits(&mut self, count: usize, cx: &mut Context<Self>) {
        // "Loading commits..." shows while none are listed (`commitsLoading && commits.length
        // === 0`); a list keeps showing until the new one arrives.
        match &self.commits {
            Slot::Ready(list) if list.commits.is_empty() => self.commits = Slot::Loading,
            Slot::NotLoaded | Slot::Loading | Slot::Ready(_) | Slot::Failed(_) => self.commits.start(),
        }
        self._more = None;
        let (git, path, scope) = (self.git.clone(), self.path.clone(), log_scope(&self.base));
        self.commits_load = Some(cx.spawn(async move |this, cx| {
            let log = git.get_log(&path, scope, 0, count).await;
            match this.update(cx, |model, cx| {
                model.commits_load = None;
                model.commits = match log {
                    Ok(log) => {
                        if model.selected_commit.as_ref().is_some_and(|s| !log.commits.iter().any(|c| c.hash == s.commit.hash)) {
                            model.selected_commit = None;
                            model._commit = None;
                        }
                        Slot::Ready(CommitList { commits: log.commits, has_more: log.has_more })
                    }
                    Err(error) => Slot::Failed(format!("Failed to load commits: {error}")),
                };
                model.settle_refresh();
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
    }

    /// `mode`'s data arrived: shown, its image files load (and find reads its files).
    fn view_loaded(&mut self, mode: ReviewMode, cx: &mut Context<Self>) {
        if self.mode == mode {
            self.load_images(cx);
        }
        self.load_for_find(cx);
        self.settle_refresh();
        cx.notify();
    }

    /// `git diff` with the view's options: renames, whole files.
    fn diff_request(&self, args: &[&str]) -> ExecRequest {
        let mut all = vec!["diff", "--no-color", "-M", FULL_CONTEXT];
        all.extend_from_slice(args);
        self.git_request(&all)
    }

    /// How the views' patches are read now.
    fn patch_reader(&self, cx: &App) -> PatchReader {
        PatchReader { exec: self.exec.clone(), whitespace: self.whitespace, executor: cx.background_executor().clone() }
    }

    /// The workspace's favourite files, shown above the changed files.
    pub fn favourites(&self) -> &crate::file_browser::Favourites {
        &self.favourites
    }

    pub fn favourite_selection(&self) -> &FavouriteSelection {
        &self.favourite
    }

    /// Show a favourite that is not in the diff in a file viewer, in the diff's place (one in
    /// the diff is scrolled to instead: [`Self::show_diff`]).
    pub fn select_favourite(&mut self, file: FavouriteFile, cx: &mut Context<Self>) {
        if self.favourite.relative_path() == Some(file.relative_path.as_str()) {
            return;
        }
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let viewer = workspace.update(cx, |w, cx| w.file_viewer(&file.path, cx));
        self.favourite = FavouriteSelection::Selected { file, viewer };
        cx.notify();
    }

    /// Selecting anything in the diff gives the pane back to it.
    pub fn show_diff(&mut self, cx: &mut Context<Self>) {
        if matches!(self.favourite, FavouriteSelection::Selected { .. }) {
            self.favourite = FavouriteSelection::None;
            cx.notify();
        }
    }

    /// Resolve the favourite paths to files (a favourite directory names every file under
    /// it); none: the section is hidden.
    fn load_favourites(&mut self, cx: &mut Context<Self>) {
        let path = self.path.clone();
        // The workspace is read once this update is over: the first load runs while the
        // workspace store is making this tab.
        self._favourites = Some(cx.spawn(async move |this, cx| {
            let Ok(Some((fs, paths))) = this.update(cx, |model, cx| {
                let workspace = model.workspace.upgrade()?;
                let (fs, paths) = {
                    let workspace = workspace.read(cx);
                    (workspace.fs_client(), workspace.all_favourite_paths())
                };
                model.favourite_paths = paths.clone();
                model.favourites = if paths.is_empty() {
                    crate::file_browser::Favourites::Empty
                } else {
                    crate::file_browser::Favourites::Loading
                };
                cx.notify();
                Some((fs, paths))
            }) else {
                return; // the tab or the workspace closed
            };
            if paths.is_empty() {
                return;
            }
            let favourites = match treeterm_core::files::resolve_favourite_files(&path, &paths, &fs).await {
                Ok(files) => crate::file_browser::Favourites::Ready(files),
                Err(error) => crate::file_browser::Favourites::Failed(error.to_string()),
            };
            match this.update(cx, |model, cx| {
                // Never select one here; only drop a selection whose file no longer resolves.
                if let (crate::file_browser::Favourites::Ready(files), Some(selected)) =
                    (&favourites, model.favourite.relative_path())
                    && !files.iter().any(|file| file.relative_path == selected)
                {
                    model.favourite = FavouriteSelection::None;
                }
                model.favourites = favourites;
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
    }

    /// `checkConflicts` for a child workspace: a dry-run merge into the parent's branch and
    /// whether the parent moved on. The last answers stay on screen while it runs.
    fn check_conflicts(&mut self, cx: &mut Context<Self>) {
        let (git, path) = (self.git.clone(), self.path.clone());
        // The workspace is read once this update is over: the first load runs while the
        // workspace store is making this tab.
        self.conflicts_load = Some(cx.spawn(async move |this, cx| {
            let Ok(header) = this.update(cx, |model, cx| model.header(cx)) else {
                return; // the tab closed
            };
            let (Some(parent), false) = (header.parent_branch, header.branch.is_empty()) else {
                // A check this one replaced may have been running.
                match this.update(cx, |model, cx| {
                    model.conflicts_load = None;
                    model.conflicts.check = ConflictCheck::Idle;
                    model.settle_refresh();
                    cx.notify();
                }) {
                    Ok(()) | Err(_) => {} // Err: tab closed
                }
                return;
            };
            let branch = header.branch;
            match this.update(cx, |model, cx| {
                model.conflicts.check = ConflictCheck::Checking;
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
            let (conflicts, merged) = futures::join!(
                git.check_merge_conflicts(&path, &branch, &parent),
                git.is_ancestor(&path, &parent, &branch)
            );
            match this.update(cx, |model, cx| {
                model.conflicts_load = None;
                model.conflicts.apply(conflicts.map_err(|e| e.to_string()), merged.map_err(|e| e.to_string()));
                model.settle_refresh();
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
    }

    /// Re-read `HEAD` and mark comments outdated whose line moved since (`loadReviews`). A
    /// failed update is the tab's notice; an unreadable `HEAD` (no commit yet) is not, as
    /// Electron's `getHeadCommitHash` answers it without throwing (commenting says why).
    fn refresh_head(&mut self, cx: &mut Context<Self>) {
        let (git, workspace, blame) = (self.git.clone(), self.workspace.clone(), self.blame());
        let path = self.path.clone();
        self.head_load = Some(cx.spawn(async move |this, cx| {
            let head = git.get_head_commit_hash(&path).await;
            let outdated = match &head {
                Ok(hash) => ReviewCommentStore::new(Later::new(workspace, cx.clone(), blame))
                    .update_outdated_review_comments(hash)
                    .await
                    .map_err(|error| format!("Failed to load review state: {error}")),
                Err(_) => Ok(()),
            };
            match this.update(cx, |model, cx| {
                model.head_load = None;
                model.head = match head {
                    Ok(hash) => Head::Known(hash),
                    Err(error) => Head::Failed(error.to_string()),
                };
                if let Err(error) = outdated {
                    model.notice = Notice::Error(error);
                }
                model.settle_refresh();
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
    }

    fn blame(&self) -> Blame {
        Blame { git: self.git.clone(), path: self.path.clone() }
    }

    /// Run a store operation that finishes now; a failure becomes the tab's notice.
    fn with_metadata<R>(
        &mut self,
        cx: &mut Context<Self>,
        op: impl FnOnce(Now<'_>) -> Result<R, String>,
    ) -> Option<R> {
        let result = match self.workspace.upgrade() {
            Some(workspace) => op(Now::new(workspace, cx, self.blame())),
            None => Err("The workspace is closed".to_owned()),
        };
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                self.notice = Notice::Error(error);
                cx.notify();
                None
            }
        }
    }

    fn metadata_value(&self, key: &str, cx: &App) -> Option<String> {
        let workspace = self.workspace.upgrade()?;
        workspace.read(cx).file().metadata.get(key).cloned()
    }

    pub fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        self.notice = Notice::Clear;
        cx.notify();
    }

    // ---- viewed files ------------------------------------------------------------------

    /// The files of what is shown, as the viewed marks key them.
    fn shown_files(&self) -> Vec<ViewedFile<'_>> {
        match self.mode {
            ReviewMode::Committed => match &self.committed {
                Slot::Ready(committed) => committed.files.iter().map(ViewedFile::from).collect(),
                Slot::NotLoaded | Slot::Loading | Slot::Failed(_) => Vec::new(),
            },
            ReviewMode::Uncommitted => match &self.uncommitted {
                Slot::Ready(uncommitted) => uncommitted.changes.files.iter().map(ViewedFile::from).collect(),
                Slot::NotLoaded | Slot::Loading | Slot::Failed(_) => Vec::new(),
            },
            ReviewMode::Commits => match self.selected_commit.as_ref().map(|s| &s.diff) {
                Some(Loaded::Ready(diff)) => diff.files.iter().map(ViewedFile::from).collect(),
                Some(Loaded::Loading | Loaded::Failed(_)) | None => Vec::new(),
            },
        }
    }

    /// The shown files in list order (staged before unstaged), each once.
    pub fn file_order(&self) -> Vec<String> {
        let mut order: IndexSet<String> = IndexSet::new();
        match (self.mode, &self.uncommitted) {
            (ReviewMode::Uncommitted, Slot::Ready(uncommitted)) => {
                for side in [Staging::Staged, Staging::Unstaged] {
                    order.extend(
                        uncommitted.changes.files.iter().filter(|f| f.entry.staged == side).map(|f| f.entry.path.clone()),
                    );
                }
            }
            (ReviewMode::Uncommitted, Slot::NotLoaded | Slot::Loading | Slot::Failed(_))
            | (ReviewMode::Committed | ReviewMode::Commits, _) => {
                order.extend(self.shown_files().iter().map(|f| f.path.to_owned()));
            }
        }
        order.into_iter().collect()
    }

    /// Every viewed mark, from the workspace metadata.
    pub fn viewed_files(&self, cx: &App) -> Result<IndexMap<String, ViewedFileStats>, MetadataError> {
        Ok(ViewedFiles::parse(self.metadata_value(REVIEW_VIEWED_FILES_KEY, cx).as_deref())?.stats())
    }

    /// Viewed and total files of what is shown.
    pub fn viewed_count(&self, cx: &App) -> Result<(usize, usize), MetadataError> {
        let viewed = self.viewed_files(cx)?;
        let order = self.file_order();
        Ok((order.iter().filter(|path| viewed.contains_key(path.as_str())).count(), order.len()))
    }

    /// Drop marks whose file changed since it was marked (`reconcileViewedFiles`): `files`
    /// are the branch's or the working tree's, as each loads. Never a commit's: one commit can
    /// change a file differently from the whole branch (`loadCommitDiff` does not reconcile).
    fn reconcile_viewed(&mut self, files: &[(String, FileChangeStats)], cx: &mut Context<Self>) {
        if files.is_empty() {
            return;
        }
        let result = self.with_metadata(cx, |metadata| {
            let files: Vec<ViewedFile<'_>> = files.iter().map(|(path, stats)| ViewedFile { path, stats: *stats }).collect();
            ViewedFilesStore::new(metadata).reconcile_viewed_files(&files).map_err(|e| e.to_string())
        });
        match result {
            Some(()) | None => {} // None: the failure is the notice
        }
    }

    /// Mark a shown file viewed, or unmark it.
    pub fn toggle_viewed(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(stats) = self.shown_files().iter().find(|file| file.path == path).map(|file| file.stats) else {
            return;
        };
        let result = self.with_metadata(cx, |metadata| {
            ViewedFilesStore::new(metadata)
                .toggle_viewed_file(ViewedFile { path, stats })
                .map_err(|e| e.to_string())
        });
        match result {
            Some(()) | None => {}
        }
    }

    /// A click in the file list, or a jump (`handleSelectDiffFile`): the pane goes back to
    /// the diff, which scrolls to the file once a list shows it; the file is the active one.
    pub fn select_file(&mut self, path: &str, cx: &mut Context<Self>) {
        self.show_diff(cx);
        self.active_file = Some(path.to_owned());
        self.scroll_request = Some(path.to_owned());
        cx.notify();
    }

    /// A click in the Committed tree is kept in the tab state (`selectedFilePath`): the
    /// Review opened from it again (after a restart, or for a new parent) comes back there.
    pub fn remember_selected_file(&mut self, path: &str, cx: &mut Context<Self>) {
        cx.emit(ReviewEvent::StateChanged(json!({ "selectedFilePath": path })));
    }

    /// The file to scroll to, for the list being shown: it scrolls there when it lists the
    /// file, and drops it otherwise (`onScrollToFileHandled`).
    pub fn take_scroll_request(&mut self) -> Option<String> {
        self.scroll_request.take()
    }

    /// The files with a row in the top tenth of the diff, in list order, after a layout
    /// (`IntersectionObserver` with `rootMargin: '0px 0px -90% 0px'` over the file sections):
    /// when the band changes, its last file becomes the active one ([`band_active`]).
    pub fn follow_scroll(&mut self, band: &[String], cx: &mut Context<Self>) {
        if let Some(last) = band_active(&self.scroll_band, band)
            && self.active_file.as_deref() != Some(last)
        {
            self.active_file = Some(last.to_owned());
            cx.notify();
        }
        self.scroll_band = band.to_vec();
    }

    /// The diff lists other files: the observer watches them afresh, so the next band
    /// reported makes its last file the active one.
    pub fn reobserve_scroll(&mut self) {
        self.scroll_band.clear();
    }

    /// The shown diff's image files, each shown inline under its header
    /// (`ImageDiffViewer` in `FileDiffSection`). Loaded with every view.
    pub fn image(&self, path: &str) -> Option<&ImageSides> {
        self.images.get(path)
    }

    /// Load both sides of every image file shown (`isImagePath`), once per load.
    fn load_images(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<String> =
            self.shown_files().iter().map(|file| file.path.to_owned()).filter(|path| is_image_path(path)).collect();
        self.images.clear();
        self._images.clear();
        for path in paths {
            self.load_image(path, cx);
        }
    }

    /// Load both sides of image file `path` (again).
    fn load_image(&mut self, path: String, cx: &mut Context<Self>) {
        let sides = self.image_sides(&path);
        self.images.insert(path.clone(), ImageSides::Loading);
        let load = cx.spawn({
            let path = path.clone();
            async move |this, cx| {
                let sides = sides.await;
                match this.update(cx, |model, cx| {
                    if let Some(shown) = model.images.get_mut(&path) {
                        *shown = sides;
                        cx.notify();
                    }
                }) {
                    Ok(()) | Err(_) => {} // Err: tab closed
                }
            }
        });
        self._images.insert(path, load);
    }

    /// Both sides of an image change: the base against the branch in Committed, the
    /// index / HEAD against the working tree in Uncommitted, the parent against the commit.
    fn image_sides(&self, path: &str) -> impl std::future::Future<Output = ImageSides> + use<> {
        use treeterm_core::git::FileDiffContents;
        let staged = match (self.mode, &self.uncommitted) {
            (ReviewMode::Uncommitted, Slot::Ready(u)) => {
                u.changes.files.iter().find(|f| f.entry.path == path).map(|f| f.entry.staged)
            }
            (ReviewMode::Uncommitted, Slot::NotLoaded | Slot::Loading | Slot::Failed(_))
            | (ReviewMode::Committed | ReviewMode::Commits, _) => None,
        };
        let (git, repo, base, mode, file) = (self.git.clone(), self.path.clone(), self.base.clone(), self.mode, path.to_owned());
        let commit = self.selected_commit.as_ref().map(|s| s.commit.hash.clone());
        async move {
            let contents = match (mode, base, staged, commit) {
                (ReviewMode::Uncommitted, _, staging, _) => {
                    git.get_uncommitted_file_contents_for_diff(&repo, &file, staging.unwrap_or(Staging::Unstaged)).await
                }
                (ReviewMode::Commits, _, _, Some(hash)) => git.get_commit_file_diff(&repo, &hash, &file).await,
                (ReviewMode::Committed | ReviewMode::Commits, ReviewBase::Branch(branch), _, _) => {
                    git.get_file_contents_for_diff(&repo, &branch, &file).await
                }
                (ReviewMode::Committed | ReviewMode::Commits, ReviewBase::None, _, _) => {
                    git.get_uncommitted_file_contents_for_diff(&repo, &file, Staging::Unstaged).await
                }
            };
            match contents {
                Ok(FileDiffContents::Image { original_base64, modified_base64 }) => {
                    let decode = |b64: &str| -> Result<Option<std::sync::Arc<Vec<u8>>>, String> {
                        use base64::Engine as _;
                        if b64.is_empty() {
                            return Ok(None);
                        }
                        base64::engine::general_purpose::STANDARD
                            .decode(b64)
                            .map(|bytes| Some(std::sync::Arc::new(bytes)))
                            .map_err(|e| e.to_string())
                    };
                    match (decode(&original_base64), decode(&modified_base64)) {
                        (Ok(before), Ok(after)) => ImageSides::Loaded { before, after },
                        (Err(e), _) | (_, Err(e)) => ImageSides::Failed(e),
                    }
                }
                Ok(FileDiffContents::Text { .. } | FileDiffContents::Binary) => {
                    ImageSides::Failed("Not shown as an image".to_owned())
                }
                Err(error) => ImageSides::Failed(error.to_string()),
            }
        }
    }

    /// "Next / previous unviewed" (`navigateUnviewed`): the next (or previous) file not
    /// viewed yet in `order`, the files as listed (tree order, filtered), from the active
    /// file. It is selected.
    pub fn jump_unviewed(&mut self, order: &[String], direction: Direction, cx: &mut Context<Self>) -> Option<String> {
        let viewed = match self.viewed_files(cx) {
            Ok(viewed) => viewed,
            Err(error) => {
                self.notice = Notice::Error(error.to_string());
                cx.notify();
                return None;
            }
        };
        let target = unviewed_from(order, &viewed, self.active_file.as_deref(), direction)?.to_owned();
        self.select_file(&target, cx);
        Some(target)
    }

    // ---- find --------------------------------------------------------------------------

    pub fn find(&self) -> &DiffFind {
        &self.find
    }

    /// Whether the diff list is on screen (`StackedDiffList` is mounted and shown): not while a
    /// favourite's viewer takes its place, nor while its view has no changes, is loading or
    /// failed; in Commits, once a commit is picked.
    pub fn diff_on_screen(&self) -> bool {
        if matches!(self.favourite, FavouriteSelection::Selected { .. }) {
            return false;
        }
        match self.content() {
            ReviewContent::Committed(committed) => !committed.files.is_empty(),
            ReviewContent::Uncommitted(uncommitted) => !uncommitted.changes.files.is_empty(),
            ReviewContent::Commits(_) => self.selected_commit.is_some(),
            ReviewContent::Loading | ReviewContent::Failed(_) => false,
        }
    }

    /// Cmd/Ctrl+F: open the bar (keeping its query) on `file`, the file being read, and
    /// focus it; nothing while the diff is not on screen ([`Self::diff_on_screen`]).
    pub fn open_find(&mut self, file: &str, cx: &mut Context<Self>) {
        if !self.diff_on_screen() {
            return;
        }
        self.find_seq += 1;
        match &mut self.find {
            DiffFind::Open(state) => {
                state.file = file.to_owned();
                state.focus = self.find_seq;
            }
            DiffFind::Closed { query } => {
                let query = std::mem::take(query);
                self.find = DiffFind::Open(FindState {
                    query,
                    file: file.to_owned(),
                    active: None,
                    repo: RepoResults::Idle,
                    focus: self.find_seq,
                });
                self.start_find_repo_search(cx);
                self.load_for_find(cx);
            }
        }
        cx.notify();
    }

    /// "Search …" from the diff's menu: open the bar on `query`, with `file`'s matches first.
    pub fn search_in_diff(&mut self, query: &str, file: &str, cx: &mut Context<Self>) {
        self.find_seq += 1;
        self.find = DiffFind::Open(FindState {
            query: query.to_owned(),
            file: file.to_owned(),
            active: None,
            repo: RepoResults::Idle,
            focus: self.find_seq,
        });
        self.start_find_repo_search(cx);
        self.load_for_find(cx);
        cx.notify();
    }

    /// Escape / the close button: close the bar; its repo search stops.
    pub fn close_find(&mut self, cx: &mut Context<Self>) {
        if let DiffFind::Open(state) = &mut self.find {
            self.find = DiffFind::Closed { query: std::mem::take(&mut state.query) };
            self._find = None;
            cx.notify();
        }
    }

    /// The query changed: no match is selected and the repo search restarts after a pause.
    pub fn set_find_query(&mut self, query: &str, cx: &mut Context<Self>) {
        let DiffFind::Open(state) = &mut self.find else {
            return;
        };
        if state.query == query {
            return;
        }
        state.query = query.to_owned();
        state.active = None;
        self.start_find_repo_search(cx);
        self.load_for_find(cx);
        cx.notify();
    }

    /// Find has a query (`searching`): its matches are looked for in every file of the diff.
    fn searching(&self) -> bool {
        matches!(&self.find, DiffFind::Open(state) if !state.query.is_empty())
    }

    /// While find has a query, the files of the list shown not asked for yet are read, so that
    /// their matches are found (`StackedDiffList`'s search loads): in list order, until
    /// [`MAX_SEARCH_LOADS`] are being read (counting those read for another reason). Each
    /// file read reads the next.
    fn load_for_find(&mut self, cx: &mut Context<Self>) {
        if !self.searching() {
            return;
        }
        let Some(list) = self.diff_list() else {
            return;
        };
        let Some(diffs) = self.diffs(&list) else {
            return;
        };
        let reading = diffs.iter().filter(|(_, diff)| matches!(diff, FileDiff::Loading)).count();
        let next: Vec<String> = diffs
            .iter()
            .filter(|(_, diff)| matches!(diff, FileDiff::NotLoaded))
            .map(|(path, _)| path.to_owned())
            .take(MAX_SEARCH_LOADS.saturating_sub(reading))
            .collect();
        for path in &next {
            self.read_file(list.clone(), path, cx);
        }
        if !next.is_empty() {
            cx.notify();
        }
    }

    /// Find still reads files of the diff (`pendingSearch`): more matches may come.
    pub fn find_pending(&self) -> bool {
        self.searching() && self.diff_list().and_then(|list| self.diffs(&list)).is_some_and(|diffs| !diffs.is_complete())
    }

    fn start_find_repo_search(&mut self, cx: &mut Context<Self>) {
        let DiffFind::Open(state) = &mut self.find else {
            return;
        };
        if state.query.is_empty() {
            state.repo = RepoResults::Idle;
            self._find = None;
            return;
        }
        state.repo = RepoResults::Searching { hits: Vec::new() };
        let query = state.query.clone();
        let exec: std::sync::Arc<dyn treeterm_core::exec::ExecApi> = std::sync::Arc::new(self.exec.clone());
        self._find = Some(spawn_repo_search(exec, &self.path, &query, cx, Self::apply_find_repo_event));
    }

    fn apply_find_repo_event(&mut self, event: treeterm_core::search::RepoSearchEvent, cx: &mut Context<Self>) {
        if let DiffFind::Open(state) = &mut self.find {
            state.repo.apply(event);
            cx.notify();
        }
    }

    /// The lines of `patch` the diff shows for `files` (in list order, as laid out) that
    /// match the query: the bar's file first, then the rest of the diff.
    pub fn find_matches(&self, patch: &str, files: &[String], layout: DiffLayout) -> Vec<DiffMatch> {
        let DiffFind::Open(state) = &self.find else {
            return Vec::new();
        };
        in_file_first(find_in_patch(patch, files, &state.query, layout), &state.file)
    }

    fn active_index(&self, matches: &[DiffMatch]) -> Option<usize> {
        let DiffFind::Open(FindState { active: Some(target), .. }) = &self.find else {
            return None;
        };
        matches.iter().position(|found| target.is(found))
    }

    /// The bar's counter for `matches` (`3/12`, `12 found`; while files are still read,
    /// `Searching…` until a match is found, then `12 found…`).
    pub fn find_count_label(&self, matches: &[DiffMatch]) -> String {
        match &self.find {
            DiffFind::Open(state) => {
                count_label(&state.query, self.active_index(matches), matches.len(), self.find_pending())
            }
            DiffFind::Closed { .. } => String::new(),
        }
    }

    /// The bar's results: "In this file", "In diff" (each saying "No matches" when none was
    /// found once every file is read), then the rest of the repo (the diff's own `files` left
    /// out).
    pub fn find_rows(&self, matches: &[DiffMatch], files: &[String]) -> Vec<SearchRow> {
        let DiffFind::Open(state) = &self.find else {
            return Vec::new();
        };
        if state.query.is_empty() {
            return Vec::new();
        }
        let (active, pending) = (self.active_index(matches), self.find_pending());
        let mut rows = Vec::new();
        for (title, in_file) in [("In this file", true), ("In diff", false)] {
            rows.push(SearchRow::Section(title));
            let section: Vec<(usize, &DiffMatch)> =
                matches.iter().enumerate().filter(|(_, found)| (found.path == state.file) == in_file).collect();
            if section.is_empty() && !pending {
                rows.push(SearchRow::Note("No matches".to_owned()));
            }
            let mut last: Option<&str> = None;
            for (index, found) in section {
                if last != Some(found.path.as_str()) {
                    rows.push(SearchRow::File(found.path.clone()));
                    last = Some(found.path.as_str());
                }
                rows.push(SearchRow::InDiff {
                    index,
                    side: found.side,
                    line_number: found.line,
                    preview: found.preview.clone(),
                    active: active == Some(index),
                });
            }
        }
        push_repo_section(&mut rows, &state.repo, |hit| !files.contains(&hit.file_path));
        rows
    }

    /// Select match `index` of `matches`: the view scrolls the diff there and selects it.
    pub fn select_find_match(&mut self, matches: &[DiffMatch], index: usize, cx: &mut Context<Self>) {
        let Some(found) = matches.get(index) else {
            return;
        };
        let DiffFind::Open(state) = &mut self.find else {
            return;
        };
        self.find_seq += 1;
        state.active = Some(FindTarget { path: found.path.clone(), side: found.side, line: found.line, seq: self.find_seq });
        cx.notify();
    }

    /// Enter / Shift+Enter: the next or previous match, wrapping around.
    pub fn step_find(&mut self, matches: &[DiffMatch], forward: bool, cx: &mut Context<Self>) {
        let total = matches.len();
        if total == 0 {
            return;
        }
        let next = match (self.active_index(matches), forward) {
            (None, true) => 0,
            (None, false) => total - 1,
            (Some(index), true) => (index + 1) % total,
            (Some(index), false) => (index + total - 1) % total,
        };
        self.select_find_match(matches, next, cx);
    }

    /// A repo hit outside the diff: a Files tab at its line (`openInFileBrowser`).
    pub fn open_find_hit(&mut self, hit: &treeterm_core::search::RepoHit, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let state = json!({
            "selectedPath": treeterm_core::search::to_workspace_absolute(&self.path, &hit.file_path),
            "scrollToLine": hit.line_number,
        });
        if workspace.update(cx, |w, cx| w.add_tab_with("filesystem", state, cx)).is_none() {
            self.notice = Notice::Error("The file browser is not registered".to_owned());
            cx.notify();
        }
    }

    // ---- comments ----------------------------------------------------------------------

    /// Every review comment of the workspace, from its metadata.
    pub fn comments(&self, cx: &App) -> Result<Vec<ReviewComment>, MetadataError> {
        Ok(ReviewComments::parse(self.metadata_value(REVIEW_COMMENTS_KEY, cx).as_deref())?.comments())
    }

    /// Start a comment under a diff line (only one is written at a time).
    pub fn start_comment(&mut self, file_path: &str, side: Side, line_number: u32, cx: &mut Context<Self>) {
        self.draft = Some(CommentDraft { file_path: file_path.to_owned(), side, line_number });
        self.adding = GitOp::Idle;
        cx.notify();
    }

    pub fn cancel_comment(&mut self, cx: &mut Context<Self>) {
        self.draft = None;
        self.adding = GitOp::Idle;
        self._add = None;
        cx.notify();
    }

    /// Save the draft with `text`, anchored to the current `HEAD` (and, on the modified
    /// side, to the line's blame).
    pub fn submit_comment(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = text.trim();
        let Some(draft) = self.draft.clone() else {
            return;
        };
        if text.is_empty() || self.adding == GitOp::Running {
            return;
        }
        let hash = match &self.head {
            Head::Known(hash) => hash.clone(),
            Head::Loading => {
                self.adding = GitOp::Failed("Still reading HEAD; try again".to_owned());
                cx.notify();
                return;
            }
            Head::Failed(error) => {
                self.adding = GitOp::Failed(format!("Cannot comment without a commit: {error}"));
                cx.notify();
                return;
            }
        };
        let comment = NewReviewComment {
            file_path: draft.file_path,
            line_number: draft.line_number,
            text: text.to_owned(),
            commit: CommitAnchor::Head(hash),
            side: draft.side,
        };
        let (workspace, blame) = (self.workspace.clone(), self.blame());
        self.adding = GitOp::Running;
        self._add = Some(cx.spawn(async move |this, cx| {
            let result = ReviewCommentStore::new(Later::new(workspace, cx.clone(), blame))
                .add_review_comment(comment)
                .await;
            match this.update(cx, |model, cx| {
                match result {
                    Ok(()) => {
                        model.draft = None;
                        model.adding = GitOp::Idle;
                    }
                    Err(error) => model.adding = GitOp::Failed(format!("Failed to add comment: {error}")),
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
        cx.notify();
    }

    pub fn delete_comment(&mut self, comment_id: &str, cx: &mut Context<Self>) {
        let result = self.with_metadata(cx, |metadata| {
            ReviewCommentStore::new(metadata).delete_review_comment(comment_id).map_err(|e| e.to_string())
        });
        match result {
            Some(()) | None => {}
        }
    }

    pub fn toggle_addressed(&mut self, comment_id: &str, cx: &mut Context<Self>) {
        let result = self.with_metadata(cx, |metadata| {
            ReviewCommentStore::new(metadata)
                .toggle_review_comment_addressed(comment_id)
                .map_err(|e| e.to_string())
        });
        match result {
            Some(()) | None => {}
        }
    }

    /// Type `prompt` into the workspace's AI harness; once it landed, mark `addressed`
    /// addressed.
    fn prompt_harness(&mut self, prompt: &str, addressed: Vec<String>, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let write = workspace.update(cx, |workspace, cx| workspace.prompt_harness(prompt, cx));
        cx.spawn(async move |this, cx| {
            let result = write.await;
            let shown = this.update(cx, |review, cx| match result {
                Ok(()) => {
                    let marked = review.with_metadata(cx, |metadata| {
                        ReviewCommentStore::new(metadata)
                            .mark_review_comments_addressed(&addressed)
                            .map_err(|e| e.to_string())
                    });
                    match marked {
                        Some(()) | None => {} // None: shown as the notice
                    }
                }
                Err(error) => {
                    review.notice = Notice::Error(error);
                    cx.notify();
                }
            });
            match shown {
                Ok(()) | Err(_) => {} // Err: the tab closed
            }
        })
        .detach();
    }

    /// "Address Comments": prompt the harness with every comment not yet addressed, then
    /// mark them addressed.
    pub fn prompt_open_comments(&mut self, cx: &mut Context<Self>) {
        let comments = match self.comments(cx) {
            Ok(comments) => comments,
            Err(error) => {
                self.notice = Notice::Error(error.to_string());
                cx.notify();
                return;
            }
        };
        let prompt = generate_review_prompt(&comments);
        if prompt.is_empty() {
            return;
        }
        let open: Vec<String> = comments
            .iter()
            .filter(|comment| comment.resolution == Resolution::Open)
            .map(|comment| comment.id.clone())
            .collect();
        self.prompt_harness(&prompt, open, cx);
    }

    /// "Prompt" / "Re-prompt" one comment, whatever its state.
    pub fn prompt_comment(&mut self, comment_id: &str, cx: &mut Context<Self>) {
        let comments = match self.comments(cx) {
            Ok(comments) => comments,
            Err(error) => {
                self.notice = Notice::Error(error.to_string());
                cx.notify();
                return;
            }
        };
        let Some(comment) = comments.iter().find(|comment| comment.id == comment_id) else {
            return;
        };
        let prompt = build_prompt_for_comments(&[comment]);
        self.prompt_harness(&prompt, vec![comment.id.clone()], cx);
    }

    /// "Prompt Commit": ask the harness to commit.
    pub fn prompt_commit(&mut self, cx: &mut Context<Self>) {
        self.prompt_harness("commit", Vec::new(), cx);
    }
}

/// A diff's files as the viewed marks key them, owned (reconciled once the model is free).
fn viewed_entries<'a>(files: impl Iterator<Item = ViewedFile<'a>>) -> Vec<(String, FileChangeStats)> {
    files.map(|file| (file.path.to_owned(), file.stats)).collect()
}

/// The commits the Commits view lists: the branch's since its base, or all of `HEAD`'s.
fn log_scope(base: &ReviewBase) -> LogScope {
    match base {
        ReviewBase::Branch(branch) => LogScope::Since(branch.clone()),
        ReviewBase::None => LogScope::All,
    }
}

/// The branch's changes as listed, with its files' diffs.
fn committed(diff: &DiffResult, diffs: FileDiffs) -> Committed {
    Committed {
        files: diff.files.clone(),
        diffs,
        base_branch: diff.base_branch.clone(),
        head_branch: diff.head_branch.clone(),
        total_additions: diff.total_additions,
        total_deletions: diff.total_deletions,
    }
}

type ExecResult = Result<treeterm_core::exec::CommandOutput, treeterm_core::exec::ExecError>;

/// How a command's output is read as a patch (git's message when it failed).
type ReadPatch = fn(ExecResult) -> Result<String, String>;

/// Reads the views' patches with the whitespace toggle as it was when they were asked for.
/// Ignoring whitespace, each hunk is diffed again over trimmed lines, as jsdiff's
/// `ignoreWhitespace` diffs the files for Pierre (`git diff -w` would also hide changes of
/// spacing within a line, which Electron shows), off the main thread.
struct PatchReader {
    exec: ExecClient,
    whitespace: Whitespace,
    executor: BackgroundExecutor,
}

impl PatchReader {
    /// `request`'s output read as a patch by `read`, then shown with the toggle.
    async fn read(&self, request: ExecRequest, read: ReadPatch) -> Result<String, String> {
        let patch = read(self.exec.run(request).await)?;
        Ok(match self.whitespace {
            Whitespace::Compare => patch,
            Whitespace::Ignore => self.executor.spawn(async move { ignore_whitespace_in_patch(&patch) }).await,
        })
    }
}

fn patch_of(result: ExecResult) -> Result<String, String> {
    match result {
        Ok(output) if output.exit_code == 0 => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
        Ok(output) => Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// `--no-index` exits 1 when the files differ, which they always do for an untracked file.
fn untracked_patch(result: ExecResult) -> Result<String, String> {
    match result {
        Ok(output) if output.exit_code <= 1 => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
        other => patch_of(other),
    }
}

/// An untracked file as added.
fn untracked_request(cwd: &str, file: &str) -> ExecRequest {
    ExecRequest {
        cwd: cwd.to_owned(),
        command: "git".to_owned(),
        args: vec!["diff".into(), "--no-color".into(), "--no-index".into(), "/dev/null".into(), file.to_owned()],
        env: HashMap::new(),
        timeout: ExecTimeout::After(GIT_TIMEOUT),
    }
}

/// Files searched at once while find reads every file's diff (`MAX_SEARCH_LOADS`).
pub const MAX_SEARCH_LOADS: usize = 4;

/// A command reading some of a list's files (`paths`), and how its output is read.
struct FileRead {
    paths: Vec<String>,
    request: ExecRequest,
    read: ReadPatch,
}

/// The command (`request`, given the pathspec) reading those of `paths` listed in `files`.
fn listed_reads(files: &[DiffFile], paths: &[String], request: impl FnOnce(&[&str]) -> ExecRequest) -> Vec<FileRead> {
    let by_path: HashMap<&str, &DiffFile> = files.iter().map(|file| (file.path.as_str(), file)).collect();
    let listed: Vec<&DiffFile> = paths.iter().filter_map(|path| by_path.get(path.as_str()).copied()).collect();
    if listed.is_empty() {
        return Vec::new();
    }
    let spec = pathspec(listed.iter().map(|file| (file.path.as_str(), &file.origin)));
    vec![FileRead {
        paths: listed.iter().map(|file| file.path.clone()).collect(),
        request: request(&spec),
        read: patch_of,
    }]
}

/// `--` and each path, with the path a renamed file came from (without it, the file would read
/// as added).
fn pathspec<'a>(files: impl IntoIterator<Item = (&'a str, &'a PathOrigin)>) -> Vec<&'a str> {
    let mut spec = vec!["--"];
    for (path, origin) in files {
        spec.push(path);
        match origin {
            PathOrigin::RenamedFrom(from) => spec.push(from),
            PathOrigin::Same => {}
        }
    }
    spec
}

/// `diffs` with the files of each of `reads` set to what it read (one command after another).
async fn read_files(patches: &PatchReader, mut diffs: FileDiffs, reads: Vec<FileRead>) -> FileDiffs {
    for FileRead { paths, request, read } in reads {
        diffs.set_read(paths.iter().map(String::as_str), &patches.read(request, read).await);
    }
    diffs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_modes_round_trip_and_default_to_committed() {
        for mode in [ReviewMode::Committed, ReviewMode::Uncommitted, ReviewMode::Commits] {
            assert_eq!(ReviewMode::parse(mode.as_str()), mode);
        }
        assert_eq!(ReviewMode::parse("favourites"), ReviewMode::Committed);
    }

    #[test]
    fn unviewed_navigation_follows_the_list_without_wrapping() {
        let order: Vec<String> = ["a", "b", "c", "d"].map(str::to_owned).to_vec();
        let viewed: IndexMap<String, ViewedFileStats> =
            [("b".to_owned(), ViewedFileStats::Unknown), ("d".to_owned(), ViewedFileStats::Unknown)].into();
        let next = |current| unviewed_from(&order, &viewed, current, Direction::Next);
        let previous = |current| unviewed_from(&order, &viewed, current, Direction::Previous);
        assert_eq!(next(None), Some("a"));
        assert_eq!(next(Some("a")), Some("c"));
        assert_eq!(next(Some("c")), None);
        assert_eq!(next(Some("gone")), Some("a"));
        assert_eq!(previous(None), Some("c"));
        assert_eq!(previous(Some("c")), Some("a"));
        assert_eq!(previous(Some("a")), None);
    }

    #[test]
    fn the_last_file_of_a_changed_top_band_becomes_active() {
        let band = |paths: &[&str]| paths.iter().map(|p| (*p).to_owned()).collect::<Vec<_>>();
        // A list shown (or listing other files): the last file in the band.
        assert_eq!(band_active(&[], &band(&["a"])), Some("a"));
        assert_eq!(band_active(&[], &band(&["a", "b"])), Some("b"));
        // Scrolling down, the file entering from below; scrolling up, still the lower file
        // (Electron's observer, made again once the upper one is active, settles there).
        assert_eq!(band_active(&band(&["a"]), &band(&["a", "b"])), Some("b"));
        assert_eq!(band_active(&band(&["b"]), &band(&["a", "b"])), Some("b"));
        assert_eq!(band_active(&band(&["b", "c"]), &band(&["a", "b", "c"])), Some("c"));
        // A file leaving: the last one left.
        assert_eq!(band_active(&band(&["a", "b"]), &band(&["b"])), Some("b"));
        assert_eq!(band_active(&band(&["b", "c"]), &band(&["b"])), Some("b"));
        // The same band, or none, changes nothing.
        assert_eq!(band_active(&band(&["a", "b"]), &band(&["a", "b"])), None);
        assert_eq!(band_active(&band(&["b"]), &[]), None);
    }

    #[test]
    fn a_diff_list_belongs_to_its_view() {
        assert_eq!(DiffList::Committed.mode(), ReviewMode::Committed);
        assert_eq!(DiffList::Uncommitted.mode(), ReviewMode::Uncommitted);
        assert_eq!(DiffList::Commit("abc".to_owned()).mode(), ReviewMode::Commits);
    }

    #[test]
    fn collapse_all_lasts_for_files_listed_later() {
        use CollapseAll::*;
        assert_eq!([Untouched, Collapsed, Expanded].map(CollapseAll::label), ["Collapse all", "Expand all", "Collapse all"]);
        assert_eq!([Untouched, Collapsed, Expanded].map(CollapseAll::toggled), [Collapsed, Expanded, Collapsed]);
        // Untouched: viewed files start collapsed; after a collapse-all or expand-all, every
        // file follows it.
        let starts = |command: CollapseAll| [false, true].map(|viewed| command.initial_collapsed(viewed));
        assert_eq!(starts(Untouched), [false, true]);
        assert_eq!(starts(Collapsed), [true, true]);
        assert_eq!(starts(Expanded), [false, false]);
    }

    fn conflicted(files: &[&str]) -> ConflictInfo {
        ConflictInfo {
            has_conflicts: !files.is_empty(),
            conflicted_files: files.iter().map(|f| (*f).to_owned()).collect(),
            messages: Vec::new(),
        }
    }

    #[test]
    fn a_conflict_check_keeps_what_it_could_not_learn() {
        let mut conflicts = Conflicts::default();
        assert!(!conflicts.has_conflicts());
        conflicts.apply(Ok(conflicted(&["a.txt"])), Ok(false));
        assert_eq!((conflicts.has_conflicts(), conflicts.parent_has_new_commits), (true, true));
        assert_eq!(conflicts.check, ConflictCheck::Idle);

        // The dry-run failed but the ancestry ran: the parent's state is new, the conflicts
        // are the last known ones.
        conflicts.apply(Err("merge-tree failed".into()), Ok(true));
        assert_eq!((conflicts.has_conflicts(), conflicts.parent_has_new_commits), (true, false));
        assert_eq!(conflicts.check, ConflictCheck::Failed("merge-tree failed".into()));

        // The ancestry could not run: nothing changes but the error.
        conflicts.apply(Ok(conflicted(&[])), Err("exec lost".into()));
        assert_eq!((conflicts.has_conflicts(), conflicts.parent_has_new_commits), (true, false));
        assert_eq!(conflicts.check, ConflictCheck::Failed("exec lost".into()));

        conflicts.apply(Ok(conflicted(&[])), Ok(true));
        assert_eq!((conflicts.has_conflicts(), conflicts.parent_has_new_commits), (false, false));
        assert_eq!(conflicts.check, ConflictCheck::Idle);
    }
}
