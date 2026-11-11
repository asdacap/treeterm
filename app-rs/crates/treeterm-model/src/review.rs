//! The Review tab: what a worktree changed. Port of the data side of `ReviewBrowser.tsx`,
//! its comments (`createReviewCommentStore.ts`, `CommentsList.tsx`), viewed files
//! (`createReviewViewedFilesStore.ts`) and prompt buttons (`ReviewCommentsButton.tsx`,
//! `PromptCommitButton.tsx`).
//!
//! Views (Electron's `viewMode`): `committed` — the branch against its parent's branch
//! (merge-base, `parent...HEAD`); `uncommitted` — the working tree against `HEAD`,
//! including untracked files; `commits` — the branch's commits since the parent.
//! The patch is plain unified diff text, rendered by the view.
//!
//! Comments and viewed marks live in workspace metadata (`reviewComments`,
//! `reviewViewedFiles`), in Electron's format, so both apps share them.

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::{App, Context, Entity, EventEmitter, Subscription, Task, WeakEntity};
use indexmap::{IndexMap, IndexSet};
use serde_json::{Value, json};
use treeterm_core::exec::{ExecClient, ExecRequest, ExecTimeout};
use treeterm_core::git::{ConflictInfo, DiffFile, GitApi, GitLogCommit, LogScope, Staging, UncommittedChanges};
use treeterm_core::review::{
    CommitAnchor, MetadataError, NewReviewComment, REVIEW_COMMENTS_KEY, REVIEW_VIEWED_FILES_KEY, Resolution,
    ReviewComment, ReviewCommentStore, ReviewComments, Side, ViewedFile, ViewedFileStats, ViewedFiles,
    ViewedFilesStore, build_prompt_for_comments, generate_review_prompt,
};

use crate::review_metadata::{Blame, Later, Now};
use crate::session_store::{AfterMerge, RemoveMode};
use crate::workspace_store::WorkspaceStore;

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

#[derive(Debug, Clone)]
pub enum ReviewContent {
    Loading,
    /// The branch since it left its parent: changed files and their patch.
    Committed(Committed),
    /// The working tree: changed files (staged and unstaged) and their patch, staged first.
    Uncommitted(Uncommitted),
    Commits(Vec<GitLogCommit>),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Committed {
    pub files: Vec<DiffFile>,
    pub patch: String,
    /// `diff-summary`: the compared branches and the totals.
    pub base_branch: String,
    pub head_branch: String,
    pub total_additions: u64,
    pub total_deletions: u64,
}

#[derive(Debug, Clone)]
pub struct Uncommitted {
    pub changes: UncommittedChanges,
    pub patch: String,
}

impl Uncommitted {
    pub fn has_staged(&self) -> bool {
        self.changes.files.iter().any(|f| f.entry.staged == Staging::Staged)
    }
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
    /// A commit succeeded (the view clears its message box).
    Committed,
    /// "Cancel" / "Close": close this Review tab.
    CloseRequested,
    /// The merge buttons: merge this workspace's branch into its parent.
    MergeRequested { squash: bool, after: AfterMerge },
    /// "Close and Clean": remove this (detached) workspace.
    RemoveRequested(RemoveMode),
}

impl EventEmitter<ReviewEvent> for ReviewModel {}

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

/// `checkConflicts`: a child's branch merged into its parent's, dry-run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictCheck {
    /// Not a child workspace, or not checked yet.
    Idle,
    Checking,
    Checked {
        conflicts: ConflictInfo,
        /// The parent has commits this branch does not contain yet.
        parent_has_new_commits: bool,
    },
    Failed(String),
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
    pub patch: String,
}

/// What the view tabs count besides the current view (`hasUncommitted`, the tab badges).
/// Loaded with every view.
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
    pub name: String,
    pub branch: String,
    /// The parent's branch, the default base (`None`: a top-level workspace).
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
    mode: ReviewMode,
    content: ReviewContent,
    staging: GitOp,
    commit: GitOp,
    /// The workspace whose metadata holds the comments and viewed marks.
    workspace: WeakEntity<WorkspaceStore>,
    head: Head,
    draft: Option<CommentDraft>,
    /// Adding the draft.
    adding: GitOp,
    notice: Notice,
    /// The file last selected or jumped to, where "next unviewed" starts from.
    focused_file: Option<String>,
    /// Both sides of an image file (`ImageDiffViewer`), when one is selected.
    image_diff: Option<ImageDiff>,
    /// The workspace's favourite files (`FavouriteFileSection`), and the paths they were
    /// resolved from.
    favourites: crate::file_browser::Favourites,
    favourite_paths: Vec<String>,
    _favourites: Option<Task<()>>,
    /// Both sides of each image file shown.
    images: HashMap<String, ImageSides>,
    _images: Vec<Task<()>>,
    overview: Overview,
    conflicts: ConflictCheck,
    /// Local and remote branches for the base picker, loaded when it opens.
    branches: Loaded<Vec<String>>,
    selected_commit: Option<SelectedCommit>,
    /// `git diff -w`.
    ignore_whitespace: bool,
    refreshing: bool,
    _overview: Option<Task<()>>,
    _conflicts: Option<Task<()>>,
    _branches: Option<Task<()>>,
    _commit: Option<Task<()>>,
    _more: Option<Task<()>>,
    _image: Option<Task<()>>,
    _load: Option<Task<()>>,
    _op: Option<Task<()>>,
    _head: Option<Task<()>>,
    _add: Option<Task<()>>,
    _workspace: Subscription,
}

/// Git commands for review must not hang the tab forever on a huge repo.
const GIT_TIMEOUT: Duration = Duration::from_secs(60);

/// An image file's two sides in Review.
#[derive(Debug, Clone)]
pub struct ImageDiff {
    pub path: String,
    pub sides: ImageSides,
}

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

/// Image files Review shows side by side (`isImagePath`).
pub fn is_image_path(path: &str) -> bool {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico")
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
        // No base: nothing to diff the branch against; Commits lists the whole history.
        let mode = match (&base, requested) {
            (ReviewBase::None, ReviewMode::Committed) => ReviewMode::Uncommitted,
            (_, mode) => mode,
        };
        // Comments and viewed marks are read from the metadata whenever shown; favourites
        // are resolved again when the workspace's favourite paths change.
        let observe = cx.observe(workspace, |model: &mut Self, workspace, cx| {
            let paths = workspace.read(cx).favourite_paths().to_vec();
            if paths != model.favourite_paths {
                model.load_favourites(cx);
            }
            cx.notify();
        });
        let mut model = Self {
            exec,
            git,
            path,
            base,
            mode,
            content: ReviewContent::Loading,
            staging: GitOp::Idle,
            commit: GitOp::Idle,
            workspace: workspace.downgrade(),
            head: Head::Loading,
            draft: None,
            adding: GitOp::Idle,
            notice: Notice::Clear,
            focused_file: None,
            image_diff: None,
            favourites: crate::file_browser::Favourites::Empty,
            favourite_paths: Vec::new(),
            _favourites: None,
            images: HashMap::new(),
            _images: Vec::new(),
            overview: Overview::default(),
            conflicts: ConflictCheck::Idle,
            branches: Loaded::Loading,
            selected_commit: None,
            ignore_whitespace: false,
            refreshing: false,
            _overview: None,
            _conflicts: None,
            _branches: None,
            _commit: None,
            _more: None,
            _image: None,
            _load: None,
            _op: None,
            _head: None,
            _add: None,
            _workspace: observe,
        };
        model.load(cx);
        model
    }

    pub fn mode(&self) -> ReviewMode {
        self.mode
    }

    pub fn base(&self) -> &ReviewBase {
        &self.base
    }

    pub fn content(&self) -> &ReviewContent {
        &self.content
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

    pub fn focused_file(&self) -> Option<&str> {
        self.focused_file.as_deref()
    }

    pub fn overview(&self) -> Overview {
        self.overview
    }

    pub fn conflicts(&self) -> &ConflictCheck {
        &self.conflicts
    }

    pub fn branches(&self) -> &Loaded<Vec<String>> {
        &self.branches
    }

    pub fn selected_commit(&self) -> Option<&SelectedCommit> {
        self.selected_commit.as_ref()
    }

    pub fn ignore_whitespace(&self) -> bool {
        self.ignore_whitespace
    }

    /// The workspace's name and branch, its parent's branch and whether a harness runs.
    pub fn header(&self, cx: &App) -> ReviewHeader {
        let Some(workspace) = self.workspace.upgrade() else {
            return ReviewHeader { name: String::new(), branch: String::new(), parent_branch: None, detached: false, harness: false };
        };
        let workspace = workspace.read(cx);
        let file = workspace.file();
        ReviewHeader {
            name: file.display_name().to_owned(),
            branch: file.git_branch.clone().unwrap_or_default(),
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
        if base == self.base {
            return;
        }
        self.base = base;
        self.selected_commit = None;
        self._commit = None;
        self.overview.commits = 0;
        self.overview.more_commits = false;
        if self.base == ReviewBase::None && self.mode == ReviewMode::Committed {
            self.mode = ReviewMode::Uncommitted;
        }
        self.load(cx);
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

    /// The whitespace toggle: reload the patches with or without `-w`.
    pub fn set_ignore_whitespace(&mut self, ignore: bool, cx: &mut Context<Self>) {
        if self.ignore_whitespace == ignore {
            return;
        }
        self.ignore_whitespace = ignore;
        self.reload_quietly(cx);
        if let Some(hash) = self.selected_commit.as_ref().map(|s| s.commit.hash.clone()) {
            self.select_commit(&hash, cx);
        }
    }

    /// Show a listed commit's changes below the list (`loadCommitDiff`).
    pub fn select_commit(&mut self, hash: &str, cx: &mut Context<Self>) {
        let ReviewContent::Commits(commits) = &self.content else {
            return;
        };
        let Some(commit) = commits.iter().find(|c| c.hash == hash).cloned() else {
            return;
        };
        let (git, path, hash) = (self.git.clone(), self.path.clone(), commit.hash.clone());
        let mut args = vec!["show", "--no-color", "-M", FULL_CONTEXT, "--format="];
        if self.ignore_whitespace {
            args.push("-w");
        }
        args.push(&hash);
        let request = self.git_request(&args);
        let exec = self.exec.clone();
        self.selected_commit = Some(SelectedCommit { commit, diff: Loaded::Loading });
        self.focused_file = None;
        if self.image_diff.is_some() {
            self.close_image_diff(cx);
        }
        cx.notify();
        self._commit = Some(cx.spawn(async move |this, cx| {
            let files = git.get_commit_diff(&path, &hash).await;
            let diff = match files {
                Ok(files) => match patch_of(exec.run(request).await) {
                    Ok(patch) => Loaded::Ready(CommitDiff { files, patch }),
                    Err(error) => Loaded::Failed(error),
                },
                Err(error) => Loaded::Failed(format!("Failed to load commit diff: {error}")),
            };
            match this.update(cx, |model, cx| {
                if let Some(selected) = &mut model.selected_commit
                    && selected.commit.hash == hash
                {
                    selected.diff = diff;
                    model.reconcile_viewed(cx);
                    model.load_images(cx);
                    cx.notify();
                }
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
    }

    /// The next page of commits, appended (the list's infinite scroll).
    pub fn load_more_commits(&mut self, cx: &mut Context<Self>) {
        let ReviewContent::Commits(commits) = &self.content else {
            return;
        };
        if !self.overview.more_commits || self._more.is_some() {
            return;
        }
        let (git, path, scope, skip) = (self.git.clone(), self.path.clone(), log_scope(&self.base), commits.len());
        self._more = Some(cx.spawn(async move |this, cx| {
            let log = git.get_log(&path, scope, skip, COMMITS_PAGE).await;
            match this.update(cx, |model, cx| {
                model._more = None;
                match (log, &mut model.content) {
                    (Ok(log), ReviewContent::Commits(commits)) if commits.len() == skip => {
                        commits.extend(log.commits);
                        model.overview.commits = commits.len();
                        model.overview.more_commits = log.has_more;
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

    /// The merge buttons (`mergeAndRemove` / `mergeAndKeep`). The session runs the merge and
    /// shows its output.
    pub fn merge(&mut self, squash: bool, after: AfterMerge, cx: &mut Context<Self>) {
        cx.emit(ReviewEvent::MergeRequested { squash, after });
    }

    /// "Close and Clean": remove the worktree, keep the branch.
    pub fn close_and_clean(&mut self, cx: &mut Context<Self>) {
        cx.emit(ReviewEvent::RemoveRequested(RemoveMode::KeepBranch));
    }

    /// "Prompt Rebase": ask the harness to rebase onto the parent's branch.
    pub fn prompt_rebase(&mut self, cx: &mut Context<Self>) {
        let header = self.header(cx);
        if let Some(parent) = header.parent_branch {
            self.prompt_harness(&format!("rebase local branch {} onto {parent}", header.branch), &[], cx);
        }
    }

    /// "Prompt Update from Parent": ask the harness to merge the parent's branch in.
    pub fn prompt_update_from_parent(&mut self, cx: &mut Context<Self>) {
        let header = self.header(cx);
        if let Some(parent) = header.parent_branch {
            self.prompt_harness(&format!("merge {parent} into local branch {}", header.branch), &[], cx);
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
                    model.reload_quietly(cx);
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
                        model.reload_quietly(cx);
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
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        self.draft = None;
        self.focused_file = None;
        cx.emit(ReviewEvent::StateChanged(json!({ "viewMode": mode.as_str() })));
        self.load(cx);
    }

    /// "Refresh changes": reload everything, keeping what is shown until the new content
    /// arrives (`handleRefresh`).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.reload_quietly(cx);
    }

    /// A load is running (the refresh button spins).
    pub fn is_refreshing(&self) -> bool {
        self.refreshing
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

    fn load(&mut self, cx: &mut Context<Self>) {
        self.content = ReviewContent::Loading;
        self.reload_quietly(cx);
    }

    /// Re-read the content, keeping the current one on screen until the new one arrives
    /// (after staging, so the list does not flash "Loading…"). `HEAD` is re-read too and the
    /// comments' outdated flags follow it (`loadReviews`).
    fn reload_quietly(&mut self, cx: &mut Context<Self>) {
        self.refreshing = true;
        cx.notify();
        self.refresh_head(cx);
        self.load_favourites(cx);
        self.load_overview(cx);
        self.check_conflicts(cx);
        let exec = self.exec.clone();
        let git = self.git.clone();
        let path = self.path.clone();
        let base = self.base.clone();
        let mode = self.mode;
        let committed = match &base {
            ReviewBase::Branch(branch) => Some(self.diff_request(&[&format!("{branch}...HEAD")])),
            ReviewBase::None => None,
        };
        let staged = self.diff_request(&["--cached"]);
        let unstaged = self.diff_request(&[]);
        let untracked = self.git_request(&["ls-files", "--others", "--exclude-standard", "-z"]);
        self._load = Some(cx.spawn(async move |this, cx| {
            let (content, more) = match (mode, base) {
                (ReviewMode::Commits, base) => match git.get_log(&path, log_scope(&base), 0, COMMITS_PAGE).await {
                    Ok(log) => (ReviewContent::Commits(log.commits), log.has_more),
                    Err(error) => (ReviewContent::Failed(format!("Failed to load commits: {error}")), false),
                },
                (ReviewMode::Committed, ReviewBase::Branch(branch)) => match committed {
                    Some(request) => match git.get_diff(&path, &branch).await {
                        Ok(diff) => match patch_of(exec.run(request).await) {
                            Ok(patch) => (
                                ReviewContent::Committed(Committed {
                                    files: diff.files,
                                    patch,
                                    base_branch: diff.base_branch,
                                    head_branch: diff.head_branch,
                                    total_additions: diff.total_additions,
                                    total_deletions: diff.total_deletions,
                                }),
                                false,
                            ),
                            Err(error) => (ReviewContent::Failed(error), false),
                        },
                        Err(error) => (ReviewContent::Failed(format!("Failed to load diff: {error}")), false),
                    },
                    None => (ReviewContent::Failed("No base branch".to_owned()), false),
                },
                (ReviewMode::Uncommitted, _) => match git.get_uncommitted_changes(&path).await {
                    Ok(changes) => match uncommitted_patch(&exec, &path, staged, unstaged, untracked).await {
                        Ok(patch) => (ReviewContent::Uncommitted(Uncommitted { changes, patch }), false),
                        Err(error) => (ReviewContent::Failed(error), false),
                    },
                    Err(error) => {
                        (ReviewContent::Failed(format!("Failed to load uncommitted changes: {error}")), false)
                    }
                },
                (ReviewMode::Committed, ReviewBase::None) => {
                    (ReviewContent::Failed("This workspace has no parent to compare against".to_owned()), false)
                }
            };
            match this.update(cx, |model, cx| {
                // A later mode change replaced this load.
                if model.mode == mode {
                    if let ReviewContent::Commits(commits) = &content {
                        model.overview.commits = commits.len();
                        model.overview.more_commits = more;
                        // The selection survives a reload while its commit is still listed.
                        if model.selected_commit.as_ref().is_some_and(|s| !commits.iter().any(|c| c.hash == s.commit.hash)) {
                            model.selected_commit = None;
                            model._commit = None;
                        }
                    }
                    model.content = content;
                    model.refreshing = false;
                    model.reconcile_viewed(cx);
                    model.load_images(cx);
                    cx.notify();
                }
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
    }

    /// `git diff` with the view's options: renames, whole files, `-w`.
    fn diff_request(&self, args: &[&str]) -> ExecRequest {
        let mut all = vec!["diff", "--no-color", "-M", FULL_CONTEXT];
        if self.ignore_whitespace {
            all.push("-w");
        }
        all.extend_from_slice(args);
        self.git_request(&all)
    }

    /// Open a file in an editor tab of the workspace (a favourite the diff does not show).
    pub fn open_file(&mut self, path: &str, cx: &mut Context<Self>) {
        if let Some(workspace) = self.workspace.upgrade()
            && workspace.update(cx, |w, cx| w.open_in_editor(path, cx)).is_none()
        {
            self.notice = Notice::Error("The editor application is not registered".to_owned());
            cx.notify();
        }
    }

    /// The workspace's favourite files, shown above the changed files.
    pub fn favourites(&self) -> &crate::file_browser::Favourites {
        &self.favourites
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
                    (workspace.fs_client(), workspace.favourite_paths().to_vec())
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
                model.favourites = favourites;
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
    }

    /// The tab counts and `hasUncommitted`, whatever the view.
    fn load_overview(&mut self, cx: &mut Context<Self>) {
        let (git, path, base) = (self.git.clone(), self.path.clone(), self.base.clone());
        self._overview = Some(cx.spawn(async move |this, cx| {
            let committed = match &base {
                ReviewBase::Branch(branch) => git.get_diff(&path, branch).await.map(|diff| diff.files.len()),
                ReviewBase::None => Ok(0),
            };
            let uncommitted = git.get_uncommitted_changes(&path).await.map(|changes| changes.files.len());
            match this.update(cx, |model, cx| {
                match committed {
                    Ok(count) => model.overview.committed = count,
                    // The Committed view shows this failure itself.
                    Err(_) => model.overview.committed = 0,
                }
                match uncommitted {
                    Ok(count) => model.overview.uncommitted = count,
                    Err(error) => model.notice = Notice::Error(format!("Failed to load uncommitted changes: {error}")),
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
    }

    /// `checkConflicts` for a child workspace: a dry-run merge into the parent's branch and
    /// whether the parent moved on.
    fn check_conflicts(&mut self, cx: &mut Context<Self>) {
        let (git, path) = (self.git.clone(), self.path.clone());
        // The workspace is read once this update is over: the first load runs while the
        // workspace store is making this tab.
        self._conflicts = Some(cx.spawn(async move |this, cx| {
            let Ok(header) = this.update(cx, |model, cx| model.header(cx)) else {
                return; // the tab closed
            };
            let (Some(parent), false) = (header.parent_branch, header.branch.is_empty()) else {
                match this.update(cx, |model, cx| {
                    model.conflicts = ConflictCheck::Idle;
                    cx.notify();
                }) {
                    Ok(()) | Err(_) => {} // Err: tab closed
                }
                return;
            };
            let branch = header.branch;
            match this.update(cx, |model, cx| {
                model.conflicts = ConflictCheck::Checking;
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
            let (conflicts, merged) = futures::join!(
                git.check_merge_conflicts(&path, &branch, &parent),
                git.is_ancestor(&path, &parent, &branch)
            );
            let check = match (conflicts, merged) {
                (Ok(conflicts), Ok(merged)) => ConflictCheck::Checked { conflicts, parent_has_new_commits: !merged },
                (Err(error), _) | (_, Err(error)) => ConflictCheck::Failed(error.to_string()),
            };
            match this.update(cx, |model, cx| {
                model.conflicts = check;
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
    }

    /// Re-read `HEAD` and mark comments outdated whose line moved since.
    fn refresh_head(&mut self, cx: &mut Context<Self>) {
        let (git, workspace, blame) = (self.git.clone(), self.workspace.clone(), self.blame());
        let path = self.path.clone();
        self._head = Some(cx.spawn(async move |this, cx| {
            let head = git.get_head_commit_hash(&path).await;
            let outdated = match &head {
                Ok(hash) => ReviewCommentStore::new(Later::new(workspace, cx.clone(), blame))
                    .update_outdated_review_comments(hash)
                    .await
                    .map_err(|error| format!("Failed to update review comments: {error}")),
                Err(_) => Ok(()),
            };
            match this.update(cx, |model, cx| {
                model.head = match head {
                    Ok(hash) => Head::Known(hash),
                    Err(error) => Head::Failed(error.to_string()),
                };
                if let Err(error) = outdated {
                    model.notice = Notice::Error(error);
                }
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
        match &self.content {
            ReviewContent::Committed(committed) => committed.files.iter().map(ViewedFile::from).collect(),
            ReviewContent::Uncommitted(uncommitted) => uncommitted.changes.files.iter().map(ViewedFile::from).collect(),
            ReviewContent::Commits(_) => match self.selected_commit.as_ref().map(|s| &s.diff) {
                Some(Loaded::Ready(diff)) => diff.files.iter().map(ViewedFile::from).collect(),
                Some(Loaded::Loading | Loaded::Failed(_)) | None => Vec::new(),
            },
            ReviewContent::Loading | ReviewContent::Failed(_) => Vec::new(),
        }
    }

    /// The shown files in list order (staged before unstaged), each once.
    pub fn file_order(&self) -> Vec<String> {
        let mut order: IndexSet<String> = IndexSet::new();
        match &self.content {
            ReviewContent::Committed(committed) => order.extend(committed.files.iter().map(|f| f.path.clone())),
            ReviewContent::Uncommitted(uncommitted) => {
                for side in [Staging::Staged, Staging::Unstaged] {
                    order.extend(
                        uncommitted.changes.files.iter().filter(|f| f.entry.staged == side).map(|f| f.entry.path.clone()),
                    );
                }
            }
            ReviewContent::Commits(_) => order.extend(self.shown_files().iter().map(|f| f.path.to_owned())),
            ReviewContent::Loading | ReviewContent::Failed(_) => {}
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

    /// Drop marks whose file changed since it was marked (`reconcileViewedFiles`).
    fn reconcile_viewed(&mut self, cx: &mut Context<Self>) {
        let files: Vec<(String, treeterm_core::git::FileChangeStats)> =
            self.shown_files().iter().map(|file| (file.path.to_owned(), file.stats)).collect();
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

    /// The file a click in the list (or a jump) went to.
    pub fn focus_file(&mut self, path: &str, cx: &mut Context<Self>) {
        self.focused_file = Some(path.to_owned());
        if is_image_path(path) {
            self.load_image_diff(path, cx);
        } else if self.image_diff.is_some() {
            self.close_image_diff(cx);
        }
        cx.notify();
    }

    pub fn image_diff(&self) -> Option<&ImageDiff> {
        self.image_diff.as_ref()
    }

    pub fn close_image_diff(&mut self, cx: &mut Context<Self>) {
        self.image_diff = None;
        self._image = None;
        cx.notify();
    }

    /// Load both sides of an image change: the parent branch against the branch in
    /// Committed, the index / HEAD against the working tree in Uncommitted.
    fn load_image_diff(&mut self, path: &str, cx: &mut Context<Self>) {
        let sides = self.image_sides(path);
        let file = path.to_owned();
        self.image_diff = Some(ImageDiff { path: path.to_owned(), sides: ImageSides::Loading });
        self._image = Some(cx.spawn(async move |this, cx| {
            let sides = sides.await;
            match this.update(cx, |model, cx| {
                if let Some(diff) = &mut model.image_diff
                    && diff.path == file
                {
                    diff.sides = sides;
                    cx.notify();
                }
            }) {
                Ok(()) | Err(_) => {}
            }
        }));
    }

    /// The shown diff's image files, each shown inline under its header
    /// (`ImageDiffViewer` in `FileDiffSection`). Loaded with every view.
    pub fn image(&self, path: &str) -> Option<&ImageSides> {
        self.images.get(path)
    }

    /// Load both sides of every image file shown, once per load.
    fn load_images(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<String> =
            self.shown_files().iter().map(|file| file.path.to_owned()).filter(|path| is_image_path(path)).collect();
        self.images.clear();
        self._images.clear();
        for path in paths {
            let sides = self.image_sides(&path);
            self.images.insert(path.clone(), ImageSides::Loading);
            self._images.push(cx.spawn(async move |this, cx| {
                let sides = sides.await;
                match this.update(cx, |model, cx| {
                    if let Some(shown) = model.images.get_mut(&path) {
                        *shown = sides;
                        cx.notify();
                    }
                }) {
                    Ok(()) | Err(_) => {} // Err: tab closed
                }
            }));
        }
    }

    /// Both sides of an image change: the base against the branch in Committed, the
    /// index / HEAD against the working tree in Uncommitted, the parent against the commit.
    fn image_sides(&self, path: &str) -> impl std::future::Future<Output = ImageSides> + use<> {
        use treeterm_core::git::FileDiffContents;
        let staged = match &self.content {
            ReviewContent::Uncommitted(u) => u
                .changes
                .files
                .iter()
                .find(|f| f.entry.path == path)
                .map(|f| f.entry.staged),
            ReviewContent::Loading | ReviewContent::Committed(_) | ReviewContent::Commits(_) | ReviewContent::Failed(_) => None,
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

    /// The next (or previous) file not viewed yet, from the focused file; it becomes the
    /// focused file. The view scrolls to it.
    pub fn jump_unviewed(&mut self, direction: Direction, cx: &mut Context<Self>) -> Option<String> {
        let viewed = match self.viewed_files(cx) {
            Ok(viewed) => viewed,
            Err(error) => {
                self.notice = Notice::Error(error.to_string());
                cx.notify();
                return None;
            }
        };
        let order = self.file_order();
        let target = unviewed_from(&order, &viewed, self.focused_file.as_deref(), direction)?.to_owned();
        self.focus_file(&target, cx);
        Some(target)
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

    /// Paste `prompt` into the workspace's AI harness; on success mark `addressed` addressed.
    fn prompt_harness(&mut self, prompt: &str, addressed: &[String], cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        if let Err(error) = workspace.update(cx, |workspace, cx| workspace.prompt_harness(prompt, cx)) {
            self.notice = Notice::Error(error);
            cx.notify();
            return;
        }
        let result = self.with_metadata(cx, |metadata| {
            ReviewCommentStore::new(metadata)
                .mark_review_comments_addressed(addressed)
                .map_err(|e| e.to_string())
        });
        match result {
            Some(()) | None => {}
        }
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
        self.prompt_harness(&prompt, &open, cx);
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
        self.prompt_harness(&prompt, std::slice::from_ref(&comment.id), cx);
    }

    /// "Prompt Commit": ask the harness to commit.
    pub fn prompt_commit(&mut self, cx: &mut Context<Self>) {
        self.prompt_harness("commit", &[], cx);
    }
}

/// The commits the Commits view lists: the branch's since its base, or all of `HEAD`'s.
fn log_scope(base: &ReviewBase) -> LogScope {
    match base {
        ReviewBase::Branch(branch) => LogScope::Since(branch.clone()),
        ReviewBase::None => LogScope::All,
    }
}

fn patch_of(result:Result<treeterm_core::exec::CommandOutput, treeterm_core::exec::ExecError>) -> Result<String, String> {
    match result {
        Ok(output) if output.exit_code == 0 => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
        Ok(output) => Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// Staged changes, unstaged changes, then each untracked file as an added file.
async fn uncommitted_patch(
    exec: &ExecClient,
    path: &str,
    staged: ExecRequest,
    unstaged: ExecRequest,
    untracked: ExecRequest,
) -> Result<String, String> {
    let mut patch = String::new();
    for request in [staged, unstaged] {
        patch.push_str(&patch_of(exec.run(request).await)?);
    }
    let listed = match exec.run(untracked).await {
        Ok(output) if output.exit_code == 0 => output.stdout,
        Ok(output) => return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
        Err(error) => return Err(error.to_string()),
    };
    for file in listed.split(|b| *b == 0).filter(|f| !f.is_empty()) {
        let file = String::from_utf8_lossy(file).into_owned();
        let request = ExecRequest {
            cwd: path.to_owned(),
            command: "git".to_owned(),
            args: vec!["diff".into(), "--no-color".into(), "--no-index".into(), "/dev/null".into(), file],
            env: HashMap::new(),
            timeout: ExecTimeout::After(GIT_TIMEOUT),
        };
        // `--no-index` exits 1 when the files differ, which they always do here.
        match exec.run(request).await {
            Ok(output) if output.exit_code <= 1 => patch.push_str(&String::from_utf8_lossy(&output.stdout)),
            Ok(output) => return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(patch)
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
}
