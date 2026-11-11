//! A workspace's git status for the tree and toolbar: uncommitted changes, whether its
//! branch differs from or conflicts with its parent's, commits behind upstream, and its
//! GitHub PR (port of the logic of `store/createGitControllerStore.ts`).
//!
//! The steps are async functions over [`GitControllerDeps`]; they report each result as a
//! [`GitStatusUpdate`] as soon as it is known, so a view updates step by step as Electron's
//! did. When refreshes run is decided by the sans-IO [`RefreshScheduler`].
//!
//! Electron ignored a failing step; [`refresh_git`] still goes on to the next one, and
//! returns every failure so the caller can log or show it.

mod scheduler;

#[cfg(test)]
mod tests;

use futures::FutureExt;
use futures::future::{BoxFuture, LocalBoxFuture};

use crate::git::{ConflictInfo, DiffResult, GitApi, GitError};
use crate::github::{GitHubApi, GitHubError, PostCommentsOutcome, PrInfo, PrLookup};
use crate::review::ReviewComment;
use crate::wire::workspace_file::WorkspaceFile;

pub use scheduler::{Next, REFRESH_COOLDOWN, RefreshScheduler, Ticket};

/// What the controller reads of a workspace (the fields of Electron's `Workspace` it uses).
/// The optional fields are optional in the workspace file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitWorkspace {
    pub id: String,
    pub path: String,
    pub is_git_repo: bool,
    pub is_worktree: bool,
    pub parent_id: Option<String>,
    pub git_branch: Option<String>,
    pub git_root_path: Option<String>,
}

impl GitWorkspace {
    pub fn from_file(id: &str, path: &str, file: &WorkspaceFile) -> Self {
        Self {
            id: id.to_owned(),
            path: path.to_owned(),
            is_git_repo: file.is_git_repo,
            is_worktree: file.is_worktree,
            parent_id: file.parent_id.clone(),
            git_branch: file.git_branch.clone(),
            git_root_path: file.git_root_path.clone(),
        }
    }
}

/// The git operations the controller runs, as a trait so it can be driven by a fake.
pub trait StatusGitApi {
    fn has_uncommitted_changes(
        &self,
        repo_path: &str,
    ) -> BoxFuture<'static, Result<bool, GitError>>;
    fn get_diff(
        &self,
        worktree_path: &str,
        parent_branch: &str,
    ) -> BoxFuture<'static, Result<DiffResult, GitError>>;
    fn check_merge_conflicts(
        &self,
        repo_path: &str,
        source_branch: &str,
        target_branch: &str,
    ) -> BoxFuture<'static, Result<ConflictInfo, GitError>>;
    fn fetch(&self, repo_path: &str) -> BoxFuture<'static, Result<(), GitError>>;
    fn get_behind_count(&self, repo_path: &str) -> BoxFuture<'static, Result<u64, GitError>>;
    fn pull(&self, repo_path: &str) -> BoxFuture<'static, Result<(), GitError>>;
}

impl StatusGitApi for GitApi {
    fn has_uncommitted_changes(
        &self,
        repo_path: &str,
    ) -> BoxFuture<'static, Result<bool, GitError>> {
        let (git, repo_path) = (self.clone(), repo_path.to_owned());
        async move { git.has_uncommitted_changes(&repo_path).await }.boxed()
    }

    fn get_diff(
        &self,
        worktree_path: &str,
        parent_branch: &str,
    ) -> BoxFuture<'static, Result<DiffResult, GitError>> {
        let (git, worktree_path, parent_branch) = (
            self.clone(),
            worktree_path.to_owned(),
            parent_branch.to_owned(),
        );
        async move { git.get_diff(&worktree_path, &parent_branch).await }.boxed()
    }

    fn check_merge_conflicts(
        &self,
        repo_path: &str,
        source_branch: &str,
        target_branch: &str,
    ) -> BoxFuture<'static, Result<ConflictInfo, GitError>> {
        let (git, repo_path, source, target) = (
            self.clone(),
            repo_path.to_owned(),
            source_branch.to_owned(),
            target_branch.to_owned(),
        );
        async move {
            git.check_merge_conflicts(&repo_path, &source, &target)
                .await
        }
        .boxed()
    }

    fn fetch(&self, repo_path: &str) -> BoxFuture<'static, Result<(), GitError>> {
        let (git, repo_path) = (self.clone(), repo_path.to_owned());
        async move { git.fetch(&repo_path).await }.boxed()
    }

    fn get_behind_count(&self, repo_path: &str) -> BoxFuture<'static, Result<u64, GitError>> {
        let (git, repo_path) = (self.clone(), repo_path.to_owned());
        async move { git.get_behind_count(&repo_path).await }.boxed()
    }

    fn pull(&self, repo_path: &str) -> BoxFuture<'static, Result<(), GitError>> {
        let (git, repo_path) = (self.clone(), repo_path.to_owned());
        async move { git.pull(&repo_path).await }.boxed()
    }
}

/// A failure of the owner's own operations (re-reading a workspace's branch, ...).
pub type DepError = Box<dyn std::error::Error + Send + Sync>;

/// What the controller works with. The workspace accessors return the current state, so
/// each step sees what the previous ones refreshed. Futures need not be `Send`: the owner
/// may run them on its UI executor.
pub trait GitControllerDeps {
    fn git(&self) -> &dyn StatusGitApi;
    fn github(&self) -> &dyn GitHubApi;
    /// The workspace this controller is for.
    fn workspace(&self) -> GitWorkspace;
    fn lookup_workspace(&self, id: &str) -> Option<GitWorkspace>;
    /// Re-read this workspace's branch: an agent (or the user) may have switched or renamed
    /// it from a terminal.
    fn refresh_git_info(&self) -> LocalBoxFuture<'static, Result<(), DepError>>;
    fn refresh_workspace_git_info(&self, id: &str)
    -> LocalBoxFuture<'static, Result<(), DepError>>;
}

// === state ==================================================================================

/// `hasUncommittedChanges`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkingTree {
    #[default]
    Clean,
    Dirty,
}

/// `isDiffCleanFromParent`: whether the branch has nothing to merge into its parent (no
/// commits of its own, nothing uncommitted). Unknown counts as not clean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParentDiff {
    #[default]
    NotClean,
    Clean,
}

/// `hasConflictsWithParent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParentConflicts {
    #[default]
    None,
    Conflicting,
}

/// `prInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum PullRequest {
    #[default]
    None,
    Found(PrInfo),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GitStatus {
    pub working_tree: WorkingTree,
    pub parent_diff: ParentDiff,
    pub parent_conflicts: ParentConflicts,
    pub behind_count: u64,
    pub pull_request: PullRequest,
}

/// One result of a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitStatusUpdate {
    WorkingTree(WorkingTree),
    ParentDiff(ParentDiff),
    ParentConflicts(ParentConflicts),
    BehindCount(u64),
    PullRequest(PullRequest),
}

impl GitStatus {
    pub fn apply(&mut self, update: GitStatusUpdate) {
        match update {
            GitStatusUpdate::WorkingTree(working_tree) => self.working_tree = working_tree,
            GitStatusUpdate::ParentDiff(parent_diff) => self.parent_diff = parent_diff,
            GitStatusUpdate::ParentConflicts(conflicts) => self.parent_conflicts = conflicts,
            GitStatusUpdate::BehindCount(count) => self.behind_count = count,
            GitStatusUpdate::PullRequest(pull_request) => self.pull_request = pull_request,
        }
    }
}

/// A step of [`refresh_git`] that failed; the others still ran.
#[derive(Debug, thiserror::Error)]
pub enum RefreshFailure {
    #[error("refreshing the workspace's branch failed: {0}")]
    OwnBranch(DepError),
    #[error("checking for uncommitted changes failed: {0}")]
    Uncommitted(GitError),
    #[error("refreshing the parent's branch failed: {0}")]
    ParentBranch(DepError),
    #[error("diffing against the parent failed: {0}")]
    ParentDiff(GitError),
    #[error("checking for conflicts with the parent failed: {0}")]
    ParentConflicts(GitError),
    #[error("{0}")]
    Fetch(GitError),
    #[error("counting commits behind upstream failed: {0}")]
    BehindCount(GitError),
    #[error("loading the pull request failed: {0}")]
    PullRequest(GitHubError),
}

#[derive(Debug, thiserror::Error)]
pub enum ControllerError {
    #[error("Not a git repo")]
    NotGitRepo,
    #[error("Missing workspace info")]
    MissingWorkspaceInfo,
    #[error("Parent branch not found")]
    ParentBranchNotFound,
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    GitHub(#[from] GitHubError),
    #[error(transparent)]
    Refresh(DepError),
}

// === steps ==================================================================================

/// The parent's branch, when the workspace is a worktree whose parent has one.
fn parent_branch<D: GitControllerDeps + ?Sized>(
    deps: &D,
    workspace: &GitWorkspace,
) -> Option<String> {
    match (workspace.is_worktree, &workspace.parent_id) {
        (true, Some(parent_id)) => deps.lookup_workspace(parent_id)?.git_branch,
        (true, None) | (false, _) => None,
    }
}

/// `refreshGit`: run every status check once, reporting each result through `apply` as it
/// arrives. `working_tree` is the current value (the parent-diff step needs it when the
/// uncommitted check fails).
pub async fn refresh_git<D: GitControllerDeps + ?Sized>(
    deps: &D,
    mut working_tree: WorkingTree,
    apply: &mut dyn FnMut(GitStatusUpdate),
) -> Vec<RefreshFailure> {
    let mut failures = Vec::new();

    // First, so the checks below use a branch switched or renamed out of band.
    if let Err(error) = deps.refresh_git_info().await {
        failures.push(RefreshFailure::OwnBranch(error));
    }

    match deps
        .git()
        .has_uncommitted_changes(&deps.workspace().path)
        .await
    {
        Ok(dirty) => {
            working_tree = if dirty {
                WorkingTree::Dirty
            } else {
                WorkingTree::Clean
            };
            apply(GitStatusUpdate::WorkingTree(working_tree));
        }
        Err(error) => failures.push(RefreshFailure::Uncommitted(error)),
    }

    let workspace = deps.workspace();
    if let (true, Some(parent_id)) = (workspace.is_worktree, &workspace.parent_id) {
        match deps.refresh_workspace_git_info(parent_id).await {
            Err(error) => failures.push(RefreshFailure::ParentBranch(error)),
            Ok(()) => {
                if let Some(parent_branch) = parent_branch(deps, &workspace) {
                    let clean = match deps.git().get_diff(&workspace.path, &parent_branch).await {
                        Ok(diff) => diff.files.is_empty(),
                        Err(error) => {
                            failures.push(RefreshFailure::ParentDiff(error));
                            false
                        }
                    };
                    apply(GitStatusUpdate::ParentDiff(match (clean, working_tree) {
                        (true, WorkingTree::Clean) => ParentDiff::Clean,
                        (true, WorkingTree::Dirty) | (false, _) => ParentDiff::NotClean,
                    }));
                }
            }
        }
    }

    let workspace = deps.workspace();
    if let (Some(branch), Some(parent_branch)) =
        (&workspace.git_branch, parent_branch(deps, &workspace))
    {
        let conflicts = match deps
            .git()
            .check_merge_conflicts(&workspace.path, branch, &parent_branch)
            .await
        {
            Ok(conflicts) if conflicts.has_conflicts => ParentConflicts::Conflicting,
            Ok(_) => ParentConflicts::None,
            Err(error) => {
                failures.push(RefreshFailure::ParentConflicts(error));
                ParentConflicts::None
            }
        };
        apply(GitStatusUpdate::ParentConflicts(conflicts));
    }

    let workspace = deps.workspace();
    if workspace.is_git_repo {
        // A failed fetch (offline, no remote) still leaves the last fetched upstream to count.
        if let Err(error) = deps.git().fetch(&workspace.path).await {
            failures.push(RefreshFailure::Fetch(error));
        }
        match deps.git().get_behind_count(&workspace.path).await {
            Ok(count) => apply(GitStatusUpdate::BehindCount(count)),
            Err(error) => failures.push(RefreshFailure::BehindCount(error)),
        }
    }

    let workspace = deps.workspace();
    if let (Some(branch), Some(root), Some(parent_branch)) = (
        &workspace.git_branch,
        &workspace.git_root_path,
        parent_branch(deps, &workspace),
    ) {
        match deps
            .github()
            .get_pr_info(root, branch, &parent_branch)
            .await
        {
            Ok(PrLookup::Found(info)) => {
                apply(GitStatusUpdate::PullRequest(PullRequest::Found(info)))
            }
            Ok(PrLookup::NoPr { .. }) => apply(GitStatusUpdate::PullRequest(PullRequest::None)),
            Err(error) => failures.push(RefreshFailure::PullRequest(error)),
        }
    }

    failures
}

/// `pullFromRemote`: pull, then re-read the branch. A successful pull leaves nothing behind.
pub async fn pull_from_remote<D: GitControllerDeps + ?Sized>(
    deps: &D,
    apply: &mut dyn FnMut(GitStatusUpdate),
) -> Result<(), ControllerError> {
    let workspace = deps.workspace();
    if !workspace.is_git_repo {
        return Err(ControllerError::NotGitRepo);
    }
    deps.git().pull(&workspace.path).await?;
    apply(GitStatusUpdate::BehindCount(0));
    deps.refresh_git_info()
        .await
        .map_err(ControllerError::Refresh)
}

/// Where "open on GitHub" goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHubPage {
    PullRequest {
        url: String,
    },
    /// No PR yet: the page that opens one.
    CreatePullRequest {
        url: String,
    },
}

/// The repository, head and base a PR of this workspace is looked up by.
struct PrContext {
    root: String,
    head: String,
    base: String,
}

fn pr_context<D: GitControllerDeps + ?Sized>(deps: &D) -> Result<PrContext, ControllerError> {
    let workspace = deps.workspace();
    let (Some(parent_id), Some(head), Some(root)) = (
        workspace.parent_id,
        workspace.git_branch,
        workspace.git_root_path,
    ) else {
        return Err(ControllerError::MissingWorkspaceInfo);
    };
    let base = deps
        .lookup_workspace(&parent_id)
        .and_then(|parent| parent.git_branch)
        .ok_or(ControllerError::ParentBranchNotFound)?;
    Ok(PrContext { root, head, base })
}

/// `openGitHub`: the PR's page (refreshing the PR status on the way), or the page to open
/// one.
pub async fn open_github<D: GitControllerDeps + ?Sized>(
    deps: &D,
    apply: &mut dyn FnMut(GitStatusUpdate),
) -> Result<GitHubPage, ControllerError> {
    let PrContext { root, head, base } = pr_context(deps)?;
    match deps.github().get_pr_info(&root, &head, &base).await? {
        PrLookup::Found(info) => {
            let url = info.url.clone();
            apply(GitStatusUpdate::PullRequest(PullRequest::Found(info)));
            Ok(GitHubPage::PullRequest { url })
        }
        PrLookup::NoPr { create_url } => {
            apply(GitStatusUpdate::PullRequest(PullRequest::None));
            Ok(GitHubPage::CreatePullRequest { url: create_url })
        }
    }
}

/// `pushReviewCommentsToGitHub`: post `comments` on this workspace's PR.
pub async fn push_review_comments_to_github<D: GitControllerDeps + ?Sized>(
    deps: &D,
    comments: Vec<ReviewComment>,
) -> Result<PostCommentsOutcome, ControllerError> {
    if comments.is_empty() {
        return Ok(PostCommentsOutcome {
            posted: 0,
            failed: Vec::new(),
        });
    }
    let PrContext { root, head, base } = pr_context(deps)?;
    Ok(deps
        .github()
        .post_review_comments(&root, &head, &base, comments)
        .await?)
}
