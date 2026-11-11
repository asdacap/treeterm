//! A workspace's git status for the tree and header: uncommitted changes, conflicts with the
//! parent, commits behind upstream, the GitHub PR. Drives `treeterm_core::git_status`
//! (port of `createGitControllerStore.ts`): `refresh_git` for the checks, `RefreshScheduler`
//! for the cooldown.

use std::sync::{Arc, RwLock};

use futures::FutureExt as _;
use futures::future::LocalBoxFuture;
use gpui_kit::{AsyncApp, Context, Task, WeakEntity};
use treeterm_core::git::{GitApi, GitInfo};
use treeterm_core::git_status::{
    DepError, GitControllerDeps, GitStatus, GitWorkspace, Next, RefreshScheduler, StatusGitApi, WorkingTree,
    refresh_git,
};
use treeterm_core::github::{GitHubApi, GitHubClient};
use treeterm_core::settings::GithubSettings;

use crate::session_store::SessionStore;

/// What the controller's checks reach: the workspace and its parent through the session,
/// git on the daemon's host, GitHub over HTTP.
struct Deps {
    id: String,
    session: WeakEntity<SessionStore>,
    git: GitApi,
    github: Arc<dyn GitHubApi>,
    cx: AsyncApp,
}

impl Deps {
    fn read(&self, id: &str) -> Option<GitWorkspace> {
        self.session
            .read_with(&self.cx, |session, cx| {
                session
                    .workspace_store(id)
                    .map(|store| {
                        let ws = store.read(cx).workspace();
                        GitWorkspace::from_file(&ws.id, &ws.path, &ws.file)
                    })
            })
            .ok()
            .flatten()
    }

    /// `refreshGitInfo(id)`: re-read the branch and record it in the workspace body.
    fn refresh_info(&self, id: &str) -> LocalBoxFuture<'static, Result<(), DepError>> {
        let Some(workspace) = self.read(id) else {
            return async { Ok(()) }.boxed_local();
        };
        let (git, session, id, cx) = (self.git.clone(), self.session.clone(), id.to_owned(), self.cx.clone());
        async move {
            let info = git.get_info(&workspace.path).await;
            let branch = match info {
                GitInfo::Repo { branch, .. } => branch,
                GitInfo::NotRepo => return Ok(()),
            };
            match session.update(&mut cx.clone(), |session, cx| {
                if let Some(store) = session.workspace_store(&id).cloned() {
                    store.update(cx, |w, cx| w.set_git_branch(&branch, cx));
                }
            }) {
                Ok(()) | Err(_) => Ok(()), // Err: the session went away
            }
        }
        .boxed_local()
    }
}

impl GitControllerDeps for Deps {
    fn git(&self) -> &dyn StatusGitApi {
        &self.git
    }

    fn github(&self) -> &dyn GitHubApi {
        self.github.as_ref()
    }

    fn workspace(&self) -> GitWorkspace {
        self.read(&self.id).unwrap_or_else(|| GitWorkspace {
            id: self.id.clone(),
            path: String::new(),
            is_git_repo: false,
            is_worktree: false,
            parent_id: None,
            git_branch: None,
            git_root_path: None,
        })
    }

    fn lookup_workspace(&self, id: &str) -> Option<GitWorkspace> {
        self.read(id)
    }

    fn refresh_git_info(&self) -> LocalBoxFuture<'static, Result<(), DepError>> {
        self.refresh_info(&self.id.clone())
    }

    fn refresh_workspace_git_info(&self, id: &str) -> LocalBoxFuture<'static, Result<(), DepError>> {
        self.refresh_info(id)
    }
}

/// GitHub settings shared with the HTTP client, kept current by the session.
pub type SharedGithubSettings = Arc<RwLock<GithubSettings>>;

/// The GitHub client a session's controllers share.
pub fn github_client(
    conn: &treeterm_core::daemon::DaemonConnection,
    settings: SharedGithubSettings,
) -> Result<Arc<dyn GitHubApi>, treeterm_core::http::HttpSetupError> {
    let http = treeterm_core::http::ReqwestHttp::new(conn.rt().clone())?;
    let source = move || settings.read().map(|s| s.clone()).unwrap_or_else(|poisoned| poisoned.into_inner().clone());
    Ok(Arc::new(GitHubClient::new(Arc::new(conn.exec_client()), Arc::new(http), Arc::new(source))))
}

pub struct GitController {
    id: String,
    session: WeakEntity<SessionStore>,
    git: GitApi,
    github: Arc<dyn GitHubApi>,
    status: GitStatus,
    scheduler: RefreshScheduler,
    /// Why the last refresh's failing steps failed (logged, and shown on hover).
    failures: Vec<String>,
    _run: Option<Task<()>>,
    _timer: Option<Task<()>>,
}

impl GitController {
    pub fn new(id: String, session: WeakEntity<SessionStore>, git: GitApi, github: Arc<dyn GitHubApi>) -> Self {
        Self {
            id,
            session,
            git,
            github,
            status: GitStatus::default(),
            scheduler: RefreshScheduler::default(),
            failures: Vec::new(),
            _run: None,
            _timer: None,
        }
    }

    pub fn status(&self) -> &GitStatus {
        &self.status
    }

    /// `gitRefreshing`.
    pub fn is_refreshing(&self) -> bool {
        self.scheduler.is_refreshing()
    }

    pub fn failures(&self) -> &[String] {
        &self.failures
    }

    /// `refreshGit`: run the checks now, or once the cooldown since the last run ends.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let (_ticket, next) = self.scheduler.request(now);
        self.follow(next, cx);
    }

    fn follow(&mut self, next: Next, cx: &mut Context<Self>) {
        match next {
            Next::Nothing => {}
            Next::StartRun => self.start_run(cx),
            Next::WakeAt(at) => {
                let executor = cx.background_executor().clone();
                let wait = at.saturating_duration_since(executor.now());
                self._timer = Some(cx.spawn(async move |this, cx| {
                    executor.timer(wait).await;
                    match this.update(cx, |controller, cx| {
                        let now = cx.background_executor().now();
                        let next = controller.scheduler.timer_fired(now);
                        controller.follow(next, cx);
                    }) {
                        Ok(()) | Err(_) => {} // Err: workspace gone
                    }
                }));
            }
        }
        cx.notify();
    }

    fn start_run(&mut self, cx: &mut Context<Self>) {
        let (id, session, git, github) = (self.id.clone(), self.session.clone(), self.git.clone(), self.github.clone());
        let working_tree: WorkingTree = self.status.working_tree;
        self._run = Some(cx.spawn(async move |this, cx| {
            let deps = Deps { id, session, git, github, cx: cx.clone() };
            let mut sink = cx.clone();
            let apply_to = this.clone();
            let mut apply = move |update| {
                match apply_to.update(&mut sink, |controller, cx| {
                    controller.status.apply(update);
                    cx.notify();
                }) {
                    Ok(()) | Err(_) => {}
                }
            };
            let failures = refresh_git(&deps, working_tree, &mut apply).await;
            match this.update(cx, |controller, cx| {
                controller.failures = failures.iter().map(ToString::to_string).collect();
                for failure in &controller.failures {
                    tracing::debug!(workspace = %controller.id, %failure, "git status step failed");
                }
                let now = cx.background_executor().now();
                let (_done, next) = controller.scheduler.run_finished(now);
                controller.follow(next, cx);
            }) {
                Ok(()) | Err(_) => {}
            }
        }));
    }
}

/// What a PR needs, at a glance (`getPrSignal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrSignal {
    None,
    MergeConflict,
    CiFailure,
    CiRunning,
    ReadyToMerge,
}

pub fn pr_signal(status: &GitStatus) -> PrSignal {
    use treeterm_core::git_status::{ParentConflicts, PullRequest};
    use treeterm_core::github::{CheckConclusion, CheckStatus, PrDetails, PrState, ReviewState};
    if status.parent_conflicts == ParentConflicts::Conflicting {
        return PrSignal::MergeConflict;
    }
    let PullRequest::Found(pr) = &status.pull_request else {
        return PrSignal::None;
    };
    let PrDetails::Loaded(details) = &pr.details else {
        return PrSignal::None;
    };
    let completed = |c: &&treeterm_core::github::CheckRun| c.status == CheckStatus::Completed;
    if details.check_runs.iter().filter(completed).any(|c| c.conclusion == CheckConclusion::Failure) {
        return PrSignal::CiFailure;
    }
    if details.check_runs.iter().any(|c| c.status != CheckStatus::Completed) {
        return PrSignal::CiRunning;
    }
    if details.state != PrState::Open {
        return PrSignal::None;
    }
    let approved = details.reviews.iter().any(|r| r.state == ReviewState::Approved);
    let changes_requested = details.reviews.iter().any(|r| r.state == ReviewState::ChangesRequested);
    if approved && !changes_requested && details.unresolved_threads.is_empty() {
        PrSignal::ReadyToMerge
    } else {
        PrSignal::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use treeterm_core::git_status::{ParentConflicts, PullRequest};
    use treeterm_core::github::{
        CheckConclusion, CheckRun, CheckStatus, PrDetails, PrInfo, PrState, PrStatus, Review, ReviewState,
    };

    fn with_pr(state: PrState, reviews: Vec<ReviewState>, checks: Vec<(CheckStatus, CheckConclusion)>) -> GitStatus {
        GitStatus {
            pull_request: PullRequest::Found(PrInfo {
                number: 7,
                url: "https://github.com/o/r/pull/7".into(),
                title: "t".into(),
                details: PrDetails::Loaded(PrStatus {
                    state,
                    reviews: reviews.into_iter().map(|state| Review { author: "a".into(), state }).collect(),
                    check_runs: checks
                        .into_iter()
                        .map(|(status, conclusion)| CheckRun { name: "ci".into(), status, conclusion })
                        .collect(),
                    unresolved_threads: Vec::new(),
                }),
            }),
            ..GitStatus::default()
        }
    }

    #[test]
    fn conflicts_win_then_ci_then_approval() {
        let conflicting = GitStatus { parent_conflicts: ParentConflicts::Conflicting, ..GitStatus::default() };
        assert_eq!(pr_signal(&conflicting), PrSignal::MergeConflict);
        assert_eq!(pr_signal(&GitStatus::default()), PrSignal::None);
        let failing = with_pr(PrState::Open, vec![ReviewState::Approved], vec![(CheckStatus::Completed, CheckConclusion::Failure)]);
        assert_eq!(pr_signal(&failing), PrSignal::CiFailure);
        let running = with_pr(PrState::Open, vec![], vec![(CheckStatus::InProgress, CheckConclusion::NotYet)]);
        assert_eq!(pr_signal(&running), PrSignal::CiRunning);
        let ready = with_pr(PrState::Open, vec![ReviewState::Approved], vec![(CheckStatus::Completed, CheckConclusion::Success)]);
        assert_eq!(pr_signal(&ready), PrSignal::ReadyToMerge);
        let contested = with_pr(PrState::Open, vec![ReviewState::Approved, ReviewState::ChangesRequested], vec![]);
        assert_eq!(pr_signal(&contested), PrSignal::None);
        let merged = with_pr(PrState::Merged, vec![ReviewState::Approved], vec![]);
        assert_eq!(pr_signal(&merged), PrSignal::None);
    }
}
