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
use treeterm_core::git::{DiffFile, GitApi, GitLogCommit, LogScope, Staging, UncommittedChanges};
use treeterm_core::review::{
    CommitAnchor, MetadataError, NewReviewComment, REVIEW_COMMENTS_KEY, REVIEW_VIEWED_FILES_KEY, Resolution,
    ReviewComment, ReviewCommentStore, ReviewComments, Side, ViewedFile, ViewedFileStats, ViewedFiles,
    ViewedFilesStore, build_prompt_for_comments, generate_review_prompt,
};

use crate::review_metadata::{Blame, Later, Now};
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
    /// A commit succeeded (the view clears its message box).
    Committed,
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
    _load: Option<Task<()>>,
    _op: Option<Task<()>>,
    _head: Option<Task<()>>,
    _add: Option<Task<()>>,
    _workspace: Subscription,
}

/// Git commands for review must not hang the tab forever on a huge repo.
const GIT_TIMEOUT: Duration = Duration::from_secs(60);

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
        let mode = match (&base, requested) {
            (ReviewBase::None, ReviewMode::Committed | ReviewMode::Commits) => ReviewMode::Uncommitted,
            (_, mode) => mode,
        };
        // Comments and viewed marks are read from the metadata whenever shown.
        let observe = cx.observe(workspace, |_, _, cx| cx.notify());
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

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.load(cx);
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
        cx.notify();
        self.refresh_head(cx);
        let exec = self.exec.clone();
        let git = self.git.clone();
        let path = self.path.clone();
        let base = self.base.clone();
        let mode = self.mode;
        let committed = match &base {
            ReviewBase::Branch(branch) => Some(self.git_request(&["diff", "--no-color", "-M", &format!("{branch}...HEAD")])),
            ReviewBase::None => None,
        };
        let staged = self.git_request(&["diff", "--no-color", "-M", "--cached"]);
        let unstaged = self.git_request(&["diff", "--no-color", "-M"]);
        let untracked = self.git_request(&["ls-files", "--others", "--exclude-standard", "-z"]);
        self._load = Some(cx.spawn(async move |this, cx| {
            let content = match (mode, base) {
                (ReviewMode::Commits, ReviewBase::Branch(branch)) => {
                    match git.get_log(&path, LogScope::Since(branch), 0, 200).await {
                        Ok(log) => ReviewContent::Commits(log.commits),
                        Err(error) => ReviewContent::Failed(error.to_string()),
                    }
                }
                (ReviewMode::Committed, ReviewBase::Branch(branch)) => match committed {
                    Some(request) => match git.get_diff(&path, &branch).await {
                        Ok(diff) => match patch_of(exec.run(request).await) {
                            Ok(patch) => ReviewContent::Committed(Committed { files: diff.files, patch }),
                            Err(error) => ReviewContent::Failed(error),
                        },
                        Err(error) => ReviewContent::Failed(format!("Failed to load diff: {error}")),
                    },
                    None => ReviewContent::Failed("No base branch".to_owned()),
                },
                (ReviewMode::Uncommitted, _) => match git.get_uncommitted_changes(&path).await {
                    Ok(changes) => match uncommitted_patch(&exec, &path, staged, unstaged, untracked).await {
                        Ok(patch) => ReviewContent::Uncommitted(Uncommitted { changes, patch }),
                        Err(error) => ReviewContent::Failed(error),
                    },
                    Err(error) => ReviewContent::Failed(format!("Failed to load uncommitted changes: {error}")),
                },
                (ReviewMode::Committed | ReviewMode::Commits, ReviewBase::None) => {
                    ReviewContent::Failed("This workspace has no parent to compare against".to_owned())
                }
            };
            match this.update(cx, |model, cx| {
                // A later mode change replaced this load.
                if model.mode == mode {
                    model.content = content;
                    model.reconcile_viewed(cx);
                    cx.notify();
                }
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
            ReviewContent::Loading | ReviewContent::Commits(_) | ReviewContent::Failed(_) => Vec::new(),
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
            ReviewContent::Loading | ReviewContent::Commits(_) | ReviewContent::Failed(_) => {}
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
        cx.notify();
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

fn patch_of(result: Result<treeterm_core::exec::CommandOutput, treeterm_core::exec::ExecError>) -> Result<String, String> {
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
