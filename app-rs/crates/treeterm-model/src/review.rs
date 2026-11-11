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
use treeterm_core::git::{GitApi, GitLogCommit, LogScope};

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
    Commits(Vec<GitLogCommit>),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReviewEvent {
    StateChanged(Value),
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
    _load: Option<Task<()>>,
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
            _load: None,
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
        let tracked = self.git_request(&["diff", "--no-color", "-M", "HEAD"]);
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
                (ReviewMode::Uncommitted, _) => uncommitted_patch(&exec, &path, tracked, untracked).await,
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

/// Tracked changes against `HEAD`, then each untracked file as an added file.
async fn uncommitted_patch(exec: &ExecClient, path: &str, tracked: ExecRequest, untracked: ExecRequest) -> ReviewContent {
    let mut patch = match patch_of(exec.run(tracked).await) {
        ReviewContent::Patch(patch) => patch,
        other => return other,
    };
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
