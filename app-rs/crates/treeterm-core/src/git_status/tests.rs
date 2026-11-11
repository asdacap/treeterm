//! Port of `store/createGitControllerStore.test.ts`, case by case. The refresh cases call
//! [`refresh_git`] directly; the debounce cases (and `gitRefreshing`) drive the
//! [`RefreshScheduler`] with explicit instants instead of fake timers.

use std::cell::RefCell;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::*;
use crate::fakes::block_on;
use crate::git::{DiffFile, FileChangeStats, FileChangeStatus};
use crate::github::{PrDetails, PrListItem, PrState, PrStatus};
use crate::review::{CommitAnchor, Freshness, LineAnchor, Resolution, Side};

fn failing(message: &str) -> GitError {
    GitError::PullFailed(message.to_owned())
}

/// Scripted git: every call is recorded as `name arg...`.
struct FakeGit {
    uncommitted: Result<bool, String>,
    diff_files: Result<usize, String>,
    conflicts: Result<bool, String>,
    fetch: Result<(), String>,
    behind: Result<u64, String>,
    pull: Result<(), String>,
    calls: Mutex<Vec<String>>,
}

impl Default for FakeGit {
    fn default() -> Self {
        Self {
            uncommitted: Ok(false),
            diff_files: Ok(0),
            conflicts: Ok(false),
            fetch: Ok(()),
            behind: Ok(0),
            pull: Ok(()),
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl FakeGit {
    fn record(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }

    fn called(&self, name: &str) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.split(' ').next() == Some(name))
            .cloned()
            .collect()
    }
}

fn ready<T: Send + 'static>(result: Result<T, String>) -> BoxFuture<'static, Result<T, GitError>> {
    async move { result.map_err(|message| failing(&message)) }.boxed()
}

impl StatusGitApi for FakeGit {
    fn has_uncommitted_changes(
        &self,
        repo_path: &str,
    ) -> BoxFuture<'static, Result<bool, GitError>> {
        self.record(format!("hasUncommittedChanges {repo_path}"));
        ready(self.uncommitted.clone())
    }

    fn get_diff(
        &self,
        worktree_path: &str,
        parent_branch: &str,
    ) -> BoxFuture<'static, Result<DiffResult, GitError>> {
        self.record(format!("getDiff {worktree_path} {parent_branch}"));
        let diff = self.diff_files.clone().map(|count| DiffResult {
            files: (0..count)
                .map(|i| DiffFile {
                    path: format!("f{i}.ts"),
                    status: FileChangeStatus::Modified,
                    stats: FileChangeStats::Text {
                        additions: 1,
                        deletions: 0,
                    },
                })
                .collect(),
            total_additions: count as u64,
            total_deletions: 0,
            base_branch: parent_branch.to_owned(),
            head_branch: "feat".to_owned(),
        });
        ready(diff)
    }

    fn check_merge_conflicts(
        &self,
        repo_path: &str,
        source_branch: &str,
        target_branch: &str,
    ) -> BoxFuture<'static, Result<ConflictInfo, GitError>> {
        self.record(format!(
            "checkMergeConflicts {repo_path} {source_branch} {target_branch}"
        ));
        ready(self.conflicts.clone().map(|has_conflicts| ConflictInfo {
            has_conflicts,
            conflicted_files: Vec::new(),
            messages: Vec::new(),
        }))
    }

    fn fetch(&self, repo_path: &str) -> BoxFuture<'static, Result<(), GitError>> {
        self.record(format!("fetch {repo_path}"));
        ready(self.fetch.clone())
    }

    fn get_behind_count(&self, repo_path: &str) -> BoxFuture<'static, Result<u64, GitError>> {
        self.record(format!("getBehindCount {repo_path}"));
        ready(self.behind.clone())
    }

    fn pull(&self, repo_path: &str) -> BoxFuture<'static, Result<(), GitError>> {
        self.record(format!("pull {repo_path}"));
        ready(self.pull.clone())
    }
}

struct FakeGitHub {
    pr_info: Result<PrLookup, GitHubError>,
    post: Result<PostCommentsOutcome, GitHubError>,
    calls: Mutex<Vec<String>>,
    posted: Mutex<Vec<Vec<ReviewComment>>>,
}

impl Default for FakeGitHub {
    fn default() -> Self {
        Self {
            pr_info: Ok(PrLookup::NoPr {
                create_url: "https://github.com/test/repo/compare/main...feat?expand=1".to_owned(),
            }),
            post: Ok(PostCommentsOutcome {
                posted: 0,
                failed: Vec::new(),
            }),
            calls: Mutex::new(Vec::new()),
            posted: Mutex::new(Vec::new()),
        }
    }
}

impl GitHubApi for FakeGitHub {
    fn get_pr_info(
        &self,
        repo_path: &str,
        head: &str,
        base: &str,
    ) -> BoxFuture<'static, Result<PrLookup, GitHubError>> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("getPrInfo {repo_path} {head} {base}"));
        let result = self.pr_info.clone();
        async move { result }.boxed()
    }

    fn list_open_prs(
        &self,
        _repo_path: &str,
    ) -> BoxFuture<'static, Result<Vec<PrListItem>, GitHubError>> {
        async { Ok(Vec::new()) }.boxed()
    }

    fn post_review_comments(
        &self,
        repo_path: &str,
        head: &str,
        base: &str,
        comments: Vec<ReviewComment>,
    ) -> BoxFuture<'static, Result<PostCommentsOutcome, GitHubError>> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("postReviewComments {repo_path} {head} {base}"));
        self.posted.lock().unwrap().push(comments);
        let result = self.post.clone();
        async move { result }.boxed()
    }
}

#[derive(Debug, Clone)]
struct DepFailure(&'static str);

impl std::fmt::Display for DepFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for DepFailure {}

#[derive(Default)]
struct Deps {
    git: FakeGit,
    github: FakeGitHub,
    workspace: Option<GitWorkspace>,
    parent: Option<GitWorkspace>,
    refresh_info_fails: Option<&'static str>,
    refresh_calls: RefCell<Vec<String>>,
}

impl GitControllerDeps for Deps {
    fn git(&self) -> &dyn StatusGitApi {
        &self.git
    }

    fn github(&self) -> &dyn GitHubApi {
        &self.github
    }

    fn workspace(&self) -> GitWorkspace {
        self.workspace
            .clone()
            .unwrap_or_else(|| make_workspace(|_| {}))
    }

    fn lookup_workspace(&self, id: &str) -> Option<GitWorkspace> {
        self.parent.clone().filter(|parent| parent.id == id)
    }

    fn refresh_git_info(&self) -> LocalBoxFuture<'static, Result<(), DepError>> {
        self.refresh_calls
            .borrow_mut()
            .push("refreshGitInfo".to_owned());
        let result = match self.refresh_info_fails {
            Some(message) => Err(Box::new(DepFailure(message)) as DepError),
            None => Ok(()),
        };
        async move { result }.boxed_local()
    }

    fn refresh_workspace_git_info(
        &self,
        id: &str,
    ) -> LocalBoxFuture<'static, Result<(), DepError>> {
        self.refresh_calls
            .borrow_mut()
            .push(format!("refreshWorkspaceGitInfo {id}"));
        async { Ok(()) }.boxed_local()
    }
}

/// The TS `makeWorkspace`: a git repo on branch `feat` at `/test`, with `overrides`.
fn make_workspace(overrides: impl FnOnce(&mut GitWorkspace)) -> GitWorkspace {
    let mut workspace = GitWorkspace {
        id: "ws-1".to_owned(),
        path: "/test".to_owned(),
        is_git_repo: true,
        is_worktree: false,
        parent_id: None,
        git_branch: Some("feat".to_owned()),
        git_root_path: Some("/test".to_owned()),
    };
    overrides(&mut workspace);
    workspace
}

fn parent(branch: Option<&str>) -> GitWorkspace {
    make_workspace(|ws| {
        ws.id = "parent".to_owned();
        ws.git_branch = branch.map(str::to_owned);
    })
}

/// A worktree child of `parent`.
fn child(overrides: impl FnOnce(&mut GitWorkspace)) -> GitWorkspace {
    make_workspace(|ws| {
        ws.is_worktree = true;
        ws.parent_id = Some("parent".to_owned());
        overrides(ws);
    })
}

fn with_parent(workspace: GitWorkspace, parent_branch: Option<&str>) -> Deps {
    Deps {
        workspace: Some(workspace),
        parent: Some(parent(parent_branch)),
        ..Deps::default()
    }
}

/// Run one refresh from the default state; returns the resulting state and the failures.
fn refresh(deps: &Deps) -> (GitStatus, Vec<RefreshFailure>) {
    refresh_from(deps, GitStatus::default())
}

fn refresh_from(deps: &Deps, mut status: GitStatus) -> (GitStatus, Vec<RefreshFailure>) {
    let mut updates = Vec::new();
    let failures = block_on(refresh_git(deps, status.working_tree, &mut |update| {
        updates.push(update)
    }));
    for update in updates {
        status.apply(update);
    }
    (status, failures)
}

fn pr_info() -> PrInfo {
    PrInfo {
        number: 1,
        url: "https://github.com/pr/1".to_owned(),
        title: "PR".to_owned(),
        details: PrDetails::Loaded(PrStatus {
            state: PrState::Open,
            reviews: Vec::new(),
            check_runs: Vec::new(),
            unresolved_threads: Vec::new(),
        }),
    }
}

mod create_git_controller_store {
    use super::*;

    mod refresh_git {
        use super::*;

        #[test]
        fn sets_git_refreshing_true_then_false() {
            let mut scheduler = RefreshScheduler::default();
            let now = Instant::now();
            let (_, next) = scheduler.request(now);
            assert_eq!(next, Next::StartRun);
            assert!(scheduler.is_refreshing());
            let (_, next) = scheduler.run_finished(now);
            assert_eq!(next, Next::Nothing);
            assert!(!scheduler.is_refreshing());
        }

        #[test]
        fn refreshes_the_current_workspace_branch_agent_may_change_it_out_of_band() {
            let deps = Deps::default();
            refresh(&deps);
            assert_eq!(*deps.refresh_calls.borrow(), vec!["refreshGitInfo"]);
        }

        #[test]
        fn catches_refresh_git_info_error_gracefully_and_still_completes() {
            let deps = Deps {
                refresh_info_fails: Some("gone"),
                ..Deps::default()
            };
            let (_, failures) = refresh(&deps);
            assert!(
                matches!(&failures[..], [RefreshFailure::OwnBranch(error)] if error.to_string() == "gone")
            );
            assert_eq!(deps.git.called("hasUncommittedChanges").len(), 1);
            assert_eq!(deps.git.called("getBehindCount").len(), 1);
        }

        #[test]
        fn sets_has_uncommitted_changes_from_git_api() {
            let deps = Deps {
                git: FakeGit {
                    uncommitted: Ok(true),
                    ..FakeGit::default()
                },
                ..Deps::default()
            };
            assert_eq!(refresh(&deps).0.working_tree, WorkingTree::Dirty);
            assert_eq!(
                deps.git.called("hasUncommittedChanges"),
                vec!["hasUncommittedChanges /test"]
            );
        }

        #[test]
        fn catches_has_uncommitted_changes_error_gracefully() {
            let deps = Deps {
                git: FakeGit {
                    uncommitted: Err("gone".to_owned()),
                    ..FakeGit::default()
                },
                ..Deps::default()
            };
            let (status, failures) = refresh(&deps);
            assert_eq!(status.working_tree, WorkingTree::Clean);
            assert!(matches!(&failures[..], [RefreshFailure::Uncommitted(_)]));
        }

        #[test]
        fn sets_is_diff_clean_from_parent_true_when_diff_is_empty_and_no_uncommitted() {
            let deps = with_parent(child(|_| {}), Some("main"));
            assert_eq!(refresh(&deps).0.parent_diff, ParentDiff::Clean);
            assert_eq!(deps.git.called("getDiff"), vec!["getDiff /test main"]);
        }

        #[test]
        fn sets_is_diff_clean_from_parent_false_when_diff_has_files() {
            let mut deps = with_parent(child(|_| {}), Some("main"));
            deps.git.diff_files = Ok(1);
            let (status, _) = refresh_from(
                &deps,
                GitStatus {
                    parent_diff: ParentDiff::Clean,
                    ..GitStatus::default()
                },
            );
            assert_eq!(status.parent_diff, ParentDiff::NotClean);
        }

        #[test]
        fn sets_is_diff_clean_from_parent_false_when_uncommitted_changes_exist_despite_clean_diff()
        {
            let mut deps = with_parent(child(|_| {}), Some("main"));
            deps.git.uncommitted = Ok(true);
            assert_eq!(refresh(&deps).0.parent_diff, ParentDiff::NotClean);
        }

        #[test]
        fn uses_the_previous_working_tree_when_the_uncommitted_check_fails() {
            let mut deps = with_parent(child(|_| {}), Some("main"));
            deps.git.uncommitted = Err("gone".to_owned());
            let previous = GitStatus {
                working_tree: WorkingTree::Dirty,
                ..GitStatus::default()
            };
            assert_eq!(
                refresh_from(&deps, previous).0.parent_diff,
                ParentDiff::NotClean
            );
        }

        #[test]
        fn refreshes_parent_git_info_before_reading_parent_branch() {
            let deps = with_parent(child(|_| {}), Some("main"));
            refresh(&deps);
            assert_eq!(
                *deps.refresh_calls.borrow(),
                vec!["refreshGitInfo", "refreshWorkspaceGitInfo parent"]
            );
        }

        #[test]
        fn skips_diff_check_for_non_worktree_workspace() {
            let deps = Deps::default();
            refresh(&deps);
            assert!(deps.git.called("getDiff").is_empty());
        }

        #[test]
        fn skips_diff_check_when_parent_has_no_git_branch() {
            let deps = with_parent(child(|_| {}), None);
            refresh(&deps);
            assert!(deps.git.called("getDiff").is_empty());
        }

        #[test]
        fn sets_is_diff_clean_from_parent_false_when_get_diff_returns_success_false() {
            let mut deps = with_parent(child(|_| {}), Some("main"));
            deps.git.diff_files = Err("diff failed".to_owned());
            let (status, failures) = refresh_from(
                &deps,
                GitStatus {
                    parent_diff: ParentDiff::Clean,
                    ..GitStatus::default()
                },
            );
            assert_eq!(status.parent_diff, ParentDiff::NotClean);
            assert!(matches!(&failures[..], [RefreshFailure::ParentDiff(_)]));
        }

        #[test]
        fn sets_has_conflicts_with_parent_true_when_conflicts_detected() {
            let mut deps = with_parent(child(|_| {}), Some("main"));
            deps.git.conflicts = Ok(true);
            assert_eq!(
                refresh(&deps).0.parent_conflicts,
                ParentConflicts::Conflicting
            );
            assert_eq!(
                deps.git.called("checkMergeConflicts"),
                vec!["checkMergeConflicts /test feat main"]
            );
        }

        #[test]
        fn sets_has_conflicts_with_parent_false_when_check_merge_conflicts_fails() {
            let mut deps = with_parent(child(|_| {}), Some("main"));
            deps.git.conflicts = Err("check failed".to_owned());
            let previous = GitStatus {
                parent_conflicts: ParentConflicts::Conflicting,
                ..GitStatus::default()
            };
            let (status, failures) = refresh_from(&deps, previous);
            assert_eq!(status.parent_conflicts, ParentConflicts::None);
            assert!(matches!(
                &failures[..],
                [RefreshFailure::ParentConflicts(_)]
            ));
        }

        #[test]
        fn skips_merge_conflict_check_when_workspace_has_no_git_branch() {
            let deps = with_parent(child(|ws| ws.git_branch = None), Some("main"));
            refresh(&deps);
            assert!(deps.git.called("checkMergeConflicts").is_empty());
        }

        #[test]
        fn calls_fetch_then_get_behind_count_and_sets_behind_count() {
            let deps = Deps {
                git: FakeGit {
                    behind: Ok(3),
                    ..FakeGit::default()
                },
                ..Deps::default()
            };
            assert_eq!(refresh(&deps).0.behind_count, 3);
            let calls = deps.git.calls.lock().unwrap().clone();
            let fetch = calls.iter().position(|call| call == "fetch /test").unwrap();
            let behind = calls
                .iter()
                .position(|call| call == "getBehindCount /test")
                .unwrap();
            assert!(fetch < behind);
        }

        #[test]
        fn skips_fetch_for_non_git_repo_workspace() {
            let deps = Deps {
                workspace: Some(make_workspace(|ws| ws.is_git_repo = false)),
                ..Deps::default()
            };
            refresh(&deps);
            assert!(deps.git.called("fetch").is_empty());
        }

        /// A failed fetch is reported; the behind count still reads the last fetched upstream.
        #[test]
        fn catches_fetch_errors_gracefully() {
            let deps = Deps {
                git: FakeGit {
                    fetch: Err("network".to_owned()),
                    ..FakeGit::default()
                },
                ..Deps::default()
            };
            let (status, failures) = refresh(&deps);
            assert_eq!(status.behind_count, 0);
            assert!(matches!(&failures[..], [RefreshFailure::Fetch(_)]));
        }

        #[test]
        fn sets_pr_info_when_pr_exists() {
            let mut deps = with_parent(
                child(|ws| ws.git_root_path = Some("/root".to_owned())),
                Some("main"),
            );
            deps.github.pr_info = Ok(PrLookup::Found(pr_info()));
            assert_eq!(refresh(&deps).0.pull_request, PullRequest::Found(pr_info()));
            assert_eq!(
                *deps.github.calls.lock().unwrap(),
                vec!["getPrInfo /root feat main"]
            );
        }

        #[test]
        fn sets_pr_info_to_null_when_no_pr() {
            let deps = with_parent(
                child(|ws| ws.git_root_path = Some("/root".to_owned())),
                Some("main"),
            );
            let previous = GitStatus {
                pull_request: PullRequest::Found(pr_info()),
                ..GitStatus::default()
            };
            assert_eq!(
                refresh_from(&deps, previous).0.pull_request,
                PullRequest::None
            );
        }

        #[test]
        fn skips_get_pr_info_for_non_worktree() {
            let deps = Deps::default();
            refresh(&deps);
            assert!(deps.github.calls.lock().unwrap().is_empty());
        }

        #[test]
        fn skips_get_pr_info_when_missing_git_branch() {
            let deps = with_parent(child(|ws| ws.git_branch = None), Some("main"));
            refresh(&deps);
            assert!(deps.github.calls.lock().unwrap().is_empty());
        }

        #[test]
        fn skips_get_pr_info_when_parent_has_no_git_branch() {
            let deps = with_parent(child(|_| {}), None);
            refresh(&deps);
            assert!(deps.github.calls.lock().unwrap().is_empty());
        }

        #[test]
        fn catches_get_pr_info_errors_gracefully() {
            let mut deps = with_parent(
                child(|ws| ws.git_root_path = Some("/root".to_owned())),
                Some("main"),
            );
            deps.github.pr_info = Err(GitHubError::NoPat);
            let (status, failures) = refresh(&deps);
            assert_eq!(status.pull_request, PullRequest::None);
            assert!(matches!(
                &failures[..],
                [RefreshFailure::PullRequest(GitHubError::NoPat)]
            ));
        }

        #[test]
        fn fires_all_three_side_effects_in_a_single_call() {
            let deps = with_parent(
                child(|ws| ws.git_root_path = Some("/root".to_owned())),
                Some("main"),
            );
            let (_, failures) = refresh(&deps);
            assert!(failures.is_empty());
            assert_eq!(deps.git.called("hasUncommittedChanges").len(), 1);
            assert_eq!(deps.git.called("fetch").len(), 1);
            assert_eq!(deps.github.calls.lock().unwrap().len(), 1);
        }
    }

    mod debounce {
        use super::*;

        const COOLDOWN: Duration = REFRESH_COOLDOWN;

        #[test]
        fn first_call_fires_immediately_without_waiting_for_any_timer() {
            let mut scheduler = RefreshScheduler::default();
            let (_, next) = scheduler.request(Instant::now());
            assert_eq!(next, Next::StartRun);
            assert!(scheduler.is_refreshing());
        }

        #[test]
        fn rapid_calls_coalesce_to_one_immediate_run_plus_one_queued_run_after_cooldown() {
            let mut scheduler = RefreshScheduler::default();
            let start = Instant::now();
            let (t1, first) = scheduler.request(start);
            let (t2, second) = scheduler.request(start);
            let (t3, third) = scheduler.request(start);
            assert_eq!(
                (first, second, third),
                (Next::StartRun, Next::Nothing, Next::Nothing)
            );

            let (completed, next) = scheduler.run_finished(start + Duration::from_millis(10));
            assert_eq!(completed, vec![t1]);
            assert_eq!(next, Next::WakeAt(start + COOLDOWN));

            assert_eq!(scheduler.timer_fired(start + COOLDOWN), Next::StartRun);
            let (completed, next) = scheduler.run_finished(start + COOLDOWN);
            assert_eq!(completed, vec![t2, t3]);
            assert_eq!(next, Next::Nothing);
        }

        #[test]
        fn queued_call_fires_once_in_flight_finishes_and_cooldown_elapses() {
            let mut scheduler = RefreshScheduler::default();
            let start = Instant::now();
            let (t1, _) = scheduler.request(start);
            let (t2, next) = scheduler.request(start);
            assert_eq!(next, Next::Nothing);

            // The first run outlasts the cooldown: the queued run waits for it, then goes at once.
            let finished = start + COOLDOWN + Duration::from_millis(500);
            let (completed, next) = scheduler.run_finished(finished);
            assert_eq!(completed, vec![t1]);
            assert_eq!(next, Next::WakeAt(finished));
            assert_eq!(scheduler.timer_fired(finished), Next::StartRun);
            assert_eq!(scheduler.run_finished(finished).0, vec![t2]);
        }

        #[test]
        fn calls_separated_by_more_than_the_cooldown_each_fire_immediately() {
            let mut scheduler = RefreshScheduler::default();
            let start = Instant::now();
            assert_eq!(scheduler.request(start).1, Next::StartRun);
            assert_eq!(scheduler.run_finished(start).1, Next::Nothing);
            assert_eq!(scheduler.request(start + COOLDOWN).1, Next::StartRun);
        }

        #[test]
        fn dispose_cancels_a_queued_cooldown_call() {
            let mut scheduler = RefreshScheduler::default();
            let start = Instant::now();
            assert_eq!(scheduler.request(start).1, Next::StartRun);
            assert_eq!(scheduler.run_finished(start).1, Next::Nothing);

            let (queued, next) = scheduler.request(start + Duration::from_millis(100));
            assert_eq!(next, Next::WakeAt(start + COOLDOWN));
            assert_eq!(scheduler.dispose(), vec![queued]);
            assert_eq!(scheduler.timer_fired(start + COOLDOWN), Next::Nothing);
            assert!(!scheduler.is_refreshing());
        }

        #[test]
        fn a_stale_or_early_wake_up_is_ignored() {
            let mut scheduler = RefreshScheduler::default();
            let start = Instant::now();
            assert_eq!(scheduler.request(start).1, Next::StartRun);
            assert_eq!(scheduler.run_finished(start).1, Next::Nothing);
            assert_eq!(scheduler.request(start).1, Next::WakeAt(start + COOLDOWN));
            assert_eq!(
                scheduler.request(start).1,
                Next::Nothing,
                "one wake-up is enough"
            );
            assert_eq!(
                scheduler.timer_fired(start + Duration::from_secs(1)),
                Next::Nothing
            );
            assert_eq!(scheduler.timer_fired(start + COOLDOWN), Next::StartRun);
            assert_eq!(scheduler.timer_fired(start + COOLDOWN), Next::Nothing);
        }

        #[test]
        #[should_panic(expected = "run_finished without a run")]
        fn finishing_a_run_that_never_started_is_a_bug() {
            let (_completed, _next) = RefreshScheduler::default().run_finished(Instant::now());
        }
    }

    mod pull_from_remote {
        use super::*;

        fn pull(deps: &Deps) -> (Result<(), ControllerError>, Vec<GitStatusUpdate>) {
            let mut updates = Vec::new();
            let result = block_on(super::super::pull_from_remote(deps, &mut |update| {
                updates.push(update)
            }));
            (result, updates)
        }

        #[test]
        fn success_resets_behind_count_calls_refresh_git_info() {
            let deps = Deps::default();
            let (result, updates) = pull(&deps);
            result.unwrap();
            assert_eq!(updates, vec![GitStatusUpdate::BehindCount(0)]);
            assert_eq!(*deps.refresh_calls.borrow(), vec!["refreshGitInfo"]);
            assert_eq!(deps.git.called("pull"), vec!["pull /test"]);
        }

        #[test]
        fn returns_error_for_non_git_repo() {
            let deps = Deps {
                workspace: Some(make_workspace(|ws| ws.is_git_repo = false)),
                ..Deps::default()
            };
            let (result, _) = pull(&deps);
            assert_eq!(result.unwrap_err().to_string(), "Not a git repo");
        }

        #[test]
        fn returns_pull_failure_result_without_resetting_behind_count() {
            let deps = Deps {
                git: FakeGit {
                    pull: Err("conflicts".to_owned()),
                    ..FakeGit::default()
                },
                ..Deps::default()
            };
            let (result, updates) = pull(&deps);
            assert_eq!(result.unwrap_err().to_string(), "conflicts");
            assert_eq!(updates, vec![]);
        }

        #[test]
        fn catches_thrown_error() {
            let deps = Deps {
                git: FakeGit {
                    pull: Err("fatal".to_owned()),
                    ..FakeGit::default()
                },
                ..Deps::default()
            };
            let (result, _) = pull(&deps);
            assert!(matches!(result, Err(ControllerError::Git(_))));
        }

        #[test]
        fn reports_a_failed_branch_refresh_after_the_pull() {
            let deps = Deps {
                refresh_info_fails: Some("gone"),
                ..Deps::default()
            };
            let (result, updates) = pull(&deps);
            assert_eq!(result.unwrap_err().to_string(), "gone");
            assert_eq!(updates, vec![GitStatusUpdate::BehindCount(0)]);
        }
    }

    mod open_github {
        use super::*;

        fn open(deps: &Deps) -> (Result<GitHubPage, ControllerError>, Vec<GitStatusUpdate>) {
            let mut updates = Vec::new();
            let result = block_on(super::super::open_github(deps, &mut |update| {
                updates.push(update)
            }));
            (result, updates)
        }

        #[test]
        fn returns_error_when_missing_parent_id() {
            let (result, _) = open(&Deps::default());
            assert_eq!(result.unwrap_err().to_string(), "Missing workspace info");
        }

        #[test]
        fn returns_error_when_parent_not_found() {
            let deps = Deps {
                workspace: Some(make_workspace(|ws| {
                    ws.parent_id = Some("parent".to_owned())
                })),
                ..Deps::default()
            };
            let (result, _) = open(&deps);
            assert_eq!(result.unwrap_err().to_string(), "Parent branch not found");
        }

        #[test]
        fn returns_url_with_has_pr_true_when_pr_exists() {
            let mut deps = with_parent(
                make_workspace(|ws| ws.parent_id = Some("parent".to_owned())),
                Some("main"),
            );
            deps.github.pr_info = Ok(PrLookup::Found(pr_info()));
            let (result, updates) = open(&deps);
            assert_eq!(
                result.unwrap(),
                GitHubPage::PullRequest {
                    url: "https://github.com/pr/1".to_owned()
                }
            );
            assert_eq!(
                updates,
                vec![GitStatusUpdate::PullRequest(PullRequest::Found(pr_info()))]
            );
        }

        #[test]
        fn returns_create_url_with_has_pr_false_when_no_pr() {
            let mut deps = with_parent(
                make_workspace(|ws| ws.parent_id = Some("parent".to_owned())),
                Some("main"),
            );
            deps.github.pr_info = Ok(PrLookup::NoPr {
                create_url: "https://github.com/create".to_owned(),
            });
            let (result, updates) = open(&deps);
            assert_eq!(
                result.unwrap(),
                GitHubPage::CreatePullRequest {
                    url: "https://github.com/create".to_owned()
                }
            );
            assert_eq!(
                updates,
                vec![GitStatusUpdate::PullRequest(PullRequest::None)]
            );
        }
    }

    mod push_review_comments_to_github {
        use super::*;

        fn comment() -> ReviewComment {
            ReviewComment {
                id: "c1".to_owned(),
                file_path: "a.ts".to_owned(),
                line_number: 1,
                text: "fix".to_owned(),
                commit: CommitAnchor::NotInRepo,
                line: LineAnchor::Untracked,
                created_at: 1,
                freshness: Freshness::Current,
                resolution: Resolution::Open,
                side: Side::Modified,
            }
        }

        fn push(
            deps: &Deps,
            comments: Vec<ReviewComment>,
        ) -> Result<PostCommentsOutcome, ControllerError> {
            block_on(super::super::push_review_comments_to_github(deps, comments))
        }

        #[test]
        fn returns_posted_0_for_empty_list_without_calling_the_api() {
            let deps = Deps::default();
            assert_eq!(
                push(&deps, vec![]).unwrap(),
                PostCommentsOutcome {
                    posted: 0,
                    failed: vec![]
                }
            );
            assert!(deps.github.calls.lock().unwrap().is_empty());
        }

        #[test]
        fn returns_error_when_workspace_is_missing_pr_context() {
            let deps = Deps {
                workspace: Some(make_workspace(|ws| {
                    ws.git_root_path = Some("/root".to_owned())
                })),
                ..Deps::default()
            };
            assert_eq!(
                push(&deps, vec![comment()]).unwrap_err().to_string(),
                "Missing workspace info"
            );
            assert!(deps.github.calls.lock().unwrap().is_empty());
        }

        #[test]
        fn returns_error_when_parent_branch_is_not_found() {
            let deps = Deps {
                workspace: Some(make_workspace(|ws| {
                    ws.parent_id = Some("parent".to_owned())
                })),
                ..Deps::default()
            };
            assert_eq!(
                push(&deps, vec![comment()]).unwrap_err().to_string(),
                "Parent branch not found"
            );
        }

        #[test]
        fn delegates_to_github_post_review_comments_with_resolved_branches() {
            let mut deps = with_parent(
                make_workspace(|ws| {
                    ws.parent_id = Some("parent".to_owned());
                    ws.git_root_path = Some("/root".to_owned());
                }),
                Some("main"),
            );
            deps.github.post = Ok(PostCommentsOutcome {
                posted: 1,
                failed: vec![],
            });
            assert_eq!(
                push(&deps, vec![comment()]).unwrap(),
                PostCommentsOutcome {
                    posted: 1,
                    failed: vec![]
                }
            );
            assert_eq!(
                *deps.github.calls.lock().unwrap(),
                vec!["postReviewComments /root feat main"]
            );
            assert_eq!(*deps.github.posted.lock().unwrap(), vec![vec![comment()]]);
        }
    }

    mod dispose {
        use super::*;

        #[test]
        fn dispose_cancels_a_queued_cooldown_refresh_git() {
            let mut scheduler = RefreshScheduler::default();
            let start = Instant::now();
            assert_eq!(scheduler.request(start).1, Next::StartRun);
            let (completed, _) = scheduler.run_finished(start);
            assert_eq!(completed.len(), 1);

            let (queued, _) = scheduler.request(start);
            assert_eq!(scheduler.dispose(), vec![queued]);
            assert_eq!(
                scheduler.timer_fired(start + REFRESH_COOLDOWN),
                Next::Nothing
            );
        }
    }

    #[test]
    fn a_workspace_view_reads_the_workspace_file() {
        let file: WorkspaceFile = serde_json::from_value(serde_json::json!({
            "name": "x", "status": "active", "isGitRepo": true, "isWorktree": true, "parentId": "p",
            "gitBranch": "feat", "gitRootPath": "/root", "appStates": {}, "metadata": {}, "createdAt": 0, "lastActivity": 0,
        }))
        .unwrap();
        assert_eq!(
            GitWorkspace::from_file("ws", "/wt", &file),
            GitWorkspace {
                id: "ws".to_owned(),
                path: "/wt".to_owned(),
                is_git_repo: true,
                is_worktree: true,
                parent_id: Some("p".to_owned()),
                git_branch: Some("feat".to_owned()),
                git_root_path: Some("/root".to_owned()),
            }
        );
    }
}
