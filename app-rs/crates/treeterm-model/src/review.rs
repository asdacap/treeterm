//! The Review tab: what a worktree changed. Port of the data side of `ReviewBrowser.tsx`.
//!
//! Views (Electron's `viewMode`): `committed` — the branch against its parent's branch
//! (merge-base, `parent...HEAD`); `uncommitted` — the working tree against `HEAD`,
//! including untracked files; `commits` — the branch's commits since the parent.
//! The patch is plain unified diff text, rendered by the view.

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::{Context, EventEmitter, Task};
use serde_json::{Value, json};
use treeterm_core::exec::{ExecClient, ExecRequest, ExecTimeout};
use treeterm_core::git::{GitApi, GitLogCommit, LogScope, Staging, UncommittedChanges};

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
    Patch(String),
    /// The working tree: changed files (staged and unstaged) and their patch, staged first.
    Uncommitted(Uncommitted),
    Commits(Vec<GitLogCommit>),
    Failed(String),
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

pub struct ReviewModel {
    exec: ExecClient,
    git: GitApi,
    path: String,
    base: ReviewBase,
    mode: ReviewMode,
    content: ReviewContent,
    staging: GitOp,
    commit: GitOp,
    _load: Option<Task<()>>,
    _op: Option<Task<()>>,
}

/// Git commands for review must not hang the tab forever on a huge repo.
const GIT_TIMEOUT: Duration = Duration::from_secs(60);

impl ReviewModel {
    pub fn new(exec: ExecClient, git: GitApi, path: String, base: ReviewBase, state: &Value, cx: &mut Context<Self>) -> Self {
        let requested = ReviewMode::parse(state.get("viewMode").and_then(Value::as_str).unwrap_or(""));
        let mode = match (&base, requested) {
            (ReviewBase::None, ReviewMode::Committed | ReviewMode::Commits) => ReviewMode::Uncommitted,
            (_, mode) => mode,
        };
        let mut model = Self {
            exec,
            git,
            path,
            base,
            mode,
            content: ReviewContent::Loading,
            staging: GitOp::Idle,
            commit: GitOp::Idle,
            _load: None,
            _op: None,
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
    /// (after staging, so the list does not flash "Loading…").
    fn reload_quietly(&mut self, cx: &mut Context<Self>) {
        cx.notify();
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
                (ReviewMode::Committed, ReviewBase::Branch(_)) => match committed {
                    Some(request) => patch_of(exec.run(request).await),
                    None => ReviewContent::Failed("No base branch".to_owned()),
                },
                (ReviewMode::Uncommitted, _) => match git.get_uncommitted_changes(&path).await {
                    Ok(changes) => match uncommitted_patch(&exec, &path, staged, unstaged, untracked).await {
                        ReviewContent::Patch(patch) => ReviewContent::Uncommitted(Uncommitted { changes, patch }),
                        other => other,
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
                    cx.notify();
                }
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
    }
}

fn patch_of(result: Result<treeterm_core::exec::CommandOutput, treeterm_core::exec::ExecError>) -> ReviewContent {
    match result {
        Ok(output) if output.exit_code == 0 => ReviewContent::Patch(String::from_utf8_lossy(&output.stdout).into_owned()),
        Ok(output) => ReviewContent::Failed(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
        Err(error) => ReviewContent::Failed(error.to_string()),
    }
}

/// Staged changes, unstaged changes, then each untracked file as an added file.
async fn uncommitted_patch(
    exec: &ExecClient,
    path: &str,
    staged: ExecRequest,
    unstaged: ExecRequest,
    untracked: ExecRequest,
) -> ReviewContent {
    let mut patch = String::new();
    for request in [staged, unstaged] {
        match patch_of(exec.run(request).await) {
            ReviewContent::Patch(part) => patch.push_str(&part),
            other => return other,
        }
    }
    let listed = match exec.run(untracked).await {
        Ok(output) if output.exit_code == 0 => output.stdout,
        Ok(output) => return ReviewContent::Failed(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
        Err(error) => return ReviewContent::Failed(error.to_string()),
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
            Ok(output) => return ReviewContent::Failed(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
            Err(error) => return ReviewContent::Failed(error.to_string()),
        }
    }
    ReviewContent::Patch(patch)
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
}
