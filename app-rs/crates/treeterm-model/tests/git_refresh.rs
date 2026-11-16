//! When a workspace's git status (its `GitController`) refreshes, as Electron's `refreshGit`
//! callers do: every workspace on load, every select (the shown one too), the AI going idle,
//! Review's refresh / stage / unstage / commit, and merges (the parent, and a kept child).
//! And the header's Pull (`pullFromRemote`), whose outcome the button alerts when it fails.
//!
//! The scheduler's clock is the test clock: within the cooldown since a run started,
//! requests wait for [`cool_down`], so each check below sees what one trigger asked for.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{AppContext as _, Entity, Task, TestAppContext};
use treeterm_core::git_status::{
    GitStatus, ParentConflicts, ParentDiff, REFRESH_COOLDOWN, WorkingTree,
};
use treeterm_core::github::UnavailableGitHub;
use treeterm_model::git_controller::{GitController, PULL_GONE, PullState};
use treeterm_model::review::{GitOp, ReviewEvent, ReviewModel, StageTarget};
use treeterm_model::session_store::{AfterMerge, SessionState, SessionStore, WorkspaceEntry};
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::traffic::ConnectionTraffic;
use treeterm_model::workspace_store::{MergeState, TabContent, WorkspaceStore};

mod common;
use common::{Harness, harness, wait_until, wait_until_app};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Write `text` to `file` in `dir` and commit it.
fn commit_file(dir: &Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", file]);
}

/// A repo with `a.txt` committed, in the daemon's temp root. Its worktrees go in its own
/// ignored `.worktrees/`, so nothing lands in the real `~/.treeterm`.
fn new_repo(h: &Harness) -> PathBuf {
    let path = h
        .daemon
        .root()
        .join(format!("repo-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "t@t"]);
    git(&path, &["config", "user.name", "t"]);
    std::fs::write(path.join(".gitignore"), ".worktrees/\n").unwrap();
    std::fs::write(path.join("a.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    path
}

/// An empty session on the daemon.
fn publish_empty(h: &Harness) {
    futures::executor::block_on(h.conn.update_session(Vec::new(), None, "test-writer"))
        .unwrap()
        .unwrap();
}

/// A window on the session whose PR lookups fail at once (no `gh`, no network), as when the
/// GitHub client cannot be built.
fn open_store(cx: &mut TestAppContext, h: &Harness, client: &str) -> Entity<SessionStore> {
    let (conn, client) = (h.conn.clone(), client.to_owned());
    let settings = cx.new(|_| SettingsStore::in_memory(Default::default()));
    let store = cx.new(|cx| {
        SessionStore::new(
            conn,
            client,
            settings,
            None,
            cx.new(ConnectionTraffic::new),
            cx,
        )
    });
    store.update(cx, |s, _| {
        s.set_github_api(Arc::new(UnavailableGitHub::new("tls".to_owned())))
    });
    wait_until(cx, &store, |s| matches!(s.state(), SessionState::Live));
    store
}

fn loaded(s: &SessionStore, id: &str) -> bool {
    matches!(s.entry(id), Some(WorkspaceEntry::Loaded(_)))
}

fn open(cx: &mut TestAppContext, store: &Entity<SessionStore>, path: &Path) -> String {
    let id = store.update(cx, |s, cx| {
        s.open_workspace(path.to_string_lossy().into_owned(), Default::default(), cx)
    });
    wait_until(cx, store, |s| loaded(s, &id));
    id
}

/// A new worktree child of `parent`, added without selecting it.
fn add_child(
    cx: &mut TestAppContext,
    store: &Entity<SessionStore>,
    parent: &str,
    name: &str,
) -> String {
    let id = store
        .update(cx, |s, cx| {
            s.add_child_workspace(parent, name.into(), false, None, cx)
        })
        .unwrap();
    wait_until(cx, store, |s| {
        loaded(s, &id) || matches!(s.entry(&id), Some(WorkspaceEntry::Error { .. }))
    });
    if let Some(error) = error_of(cx, store, &id) {
        panic!("child failed: {error}");
    }
    id
}

fn error_of(cx: &mut TestAppContext, store: &Entity<SessionStore>, id: &str) -> Option<String> {
    store.read_with(cx, |s, _| match s.entry(id) {
        Some(
            WorkspaceEntry::Error { error, .. } | WorkspaceEntry::OperationError { error, .. },
        ) => Some(error.clone()),
        _ => None,
    })
}

fn workspace(
    cx: &mut TestAppContext,
    store: &Entity<SessionStore>,
    id: &str,
) -> Entity<WorkspaceStore> {
    store.read_with(cx, |s, _| {
        s.workspace_store(id).expect("workspace loaded").clone()
    })
}

fn dir(cx: &mut TestAppContext, store: &Entity<SessionStore>, id: &str) -> PathBuf {
    let workspace = workspace(cx, store, id);
    PathBuf::from(workspace.read_with(cx, |w, _| w.workspace().path.clone()))
}

fn status(cx: &mut TestAppContext, store: &Entity<SessionStore>, id: &str) -> GitStatus {
    store.read_with(cx, |s, cx| {
        s.git_controller(id)
            .expect("a git controller")
            .read(cx)
            .status()
            .clone()
    })
}

fn wait_status(
    cx: &mut TestAppContext,
    store: &Entity<SessionStore>,
    id: &str,
    done: impl Fn(&GitStatus) -> bool,
) {
    wait_until_app(cx, store, |s, cx| {
        s.git_controller(id)
            .is_some_and(|c| done(c.read(cx).status()))
    });
}

/// Until none of these workspaces' refreshes runs: a git write meanwhile could race their
/// `git status` for the index lock.
fn settle(cx: &mut TestAppContext, store: &Entity<SessionStore>, ids: &[&str]) {
    wait_until_app(cx, store, |s, cx| {
        ids.iter().all(|id| {
            s.git_controller(id)
                .is_some_and(|c| !c.read(cx).is_refreshing())
        })
    });
}

/// The cooldown since the last runs is over: a request waiting for it runs now, and the
/// next one runs at once.
fn cool_down(cx: &mut TestAppContext) {
    cx.executor().advance_clock(REFRESH_COOLDOWN);
}

#[gpui_kit::test]
fn every_workspace_refreshes_when_it_loads(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish_empty(&h);
    let (a, b) = (new_repo(&h), new_repo(&h));
    std::fs::write(a.join("untracked.txt"), "x\n").unwrap();
    std::fs::write(b.join("untracked.txt"), "x\n").unwrap();
    let store = open_store(cx, &h, "client-a");
    let (id_a, id_b) = (open(cx, &store, &a), open(cx, &store, &b));
    // Neither was ever viewed.
    wait_status(cx, &store, &id_a, |s| s.working_tree == WorkingTree::Dirty);
    wait_status(cx, &store, &id_b, |s| s.working_tree == WorkingTree::Dirty);

    // A child forked without being selected: its branch is known to match its parent's.
    let child = add_child(cx, &store, &id_a, "feat");
    wait_status(cx, &store, &child, |s| s.parent_diff == ParentDiff::Clean);
    // Without a GitHub client, the controller still runs; only the PR step fails.
    settle(cx, &store, &[&child]);
    let failures = store.read_with(cx, |s, cx| {
        s.git_controller(&child)
            .unwrap()
            .read(cx)
            .failures()
            .to_vec()
    });
    assert!(
        failures
            .contains(&"loading the pull request failed: GitHub is not available: tls".to_owned()),
        "{failures:?}"
    );

    // Rebuilt from the session in another window (`reconstructWorkspace`): the same.
    let other = open_store(cx, &h, "client-b");
    wait_until(cx, &other, |s| {
        loaded(s, &id_a) && loaded(s, &id_b) && loaded(s, &child)
    });
    wait_status(cx, &other, &id_a, |s| s.working_tree == WorkingTree::Dirty);
    wait_status(cx, &other, &id_b, |s| s.working_tree == WorkingTree::Dirty);
    wait_status(cx, &other, &child, |s| s.parent_diff == ParentDiff::Clean);
}

#[gpui_kit::test]
fn selecting_a_workspace_refreshes_it_even_when_it_is_shown_already(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish_empty(&h);
    let repo = new_repo(&h);
    let store = open_store(cx, &h, "client-a");
    let id = open(cx, &store, &repo);
    settle(cx, &store, &[&id]);
    store.update(cx, |s, cx| s.set_active_workspace(id.clone(), cx));
    cool_down(cx);
    settle(cx, &store, &[&id]);
    assert_eq!(status(cx, &store, &id).working_tree, WorkingTree::Clean);

    std::fs::write(repo.join("untracked.txt"), "x\n").unwrap();
    // A click on the row shown already.
    store.update(cx, |s, cx| s.set_active_workspace(id.clone(), cx));
    cool_down(cx);
    wait_status(cx, &store, &id, |s| s.working_tree == WorkingTree::Dirty);
}

#[gpui_kit::test]
fn the_ai_going_idle_refreshes_the_git_status(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish_empty(&h);
    let repo = new_repo(&h);
    let store = open_store(cx, &h, "client-a");
    let id = open(cx, &store, &repo);
    settle(cx, &store, &[&id]);
    assert_eq!(status(cx, &store, &id).working_tree, WorkingTree::Clean);

    std::fs::write(repo.join("made-by-the-ai.txt"), "x\n").unwrap();
    // What `AnalyzerEvent::RefreshGit` (Idle or Completed) runs.
    let ws = workspace(cx, &store, &id);
    ws.update(cx, |w, cx| w.refresh_git(cx));
    cool_down(cx);
    wait_status(cx, &store, &id, |s| s.working_tree == WorkingTree::Dirty);
}

type Events = std::rc::Rc<std::cell::RefCell<Vec<ReviewEvent>>>;

fn git_changes(events: &Events) -> usize {
    events
        .borrow()
        .iter()
        .filter(|e| **e == ReviewEvent::GitChanged)
        .count()
}

#[gpui_kit::test]
fn review_refresh_stage_unstage_and_commit_refresh_the_git_status(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish_empty(&h);
    let repo = new_repo(&h);
    std::fs::write(repo.join("new.txt"), "fresh\n").unwrap();
    let store = open_store(cx, &h, "client-a");
    let id = open(cx, &store, &repo);
    wait_status(cx, &store, &id, |s| s.working_tree == WorkingTree::Dirty);
    settle(cx, &store, &[&id]);
    let ws = workspace(cx, &store, &id);
    let tab = ws.update(cx, |w, cx| w.add_tab("review", cx)).unwrap();
    let Some(TabContent::Review(review)) = ws.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not a review tab");
    };
    wait_until(cx, &review, |r| {
        !r.is_loading() && r.overview().uncommitted == 1
    });
    let events = Events::default();
    let seen = events.clone();
    let _events = cx.update(|cx| {
        cx.subscribe(&review, move |_, e: &ReviewEvent, _| {
            seen.borrow_mut().push(e.clone())
        })
    });
    let stage =
        |cx: &mut TestAppContext, review: &Entity<ReviewModel>, add: bool, target: StageTarget| {
            review.update(cx, |r, cx| {
                if add {
                    r.stage(target, cx);
                } else {
                    r.unstage(target, cx);
                }
            });
            // And the reload after it (only `refresh` spins): its `git status` would race the
            // next stage for the index lock.
            wait_until(cx, review, |r| {
                *r.staging() != GitOp::Running && !r.is_loading()
            });
        };

    // A failed stage asks for nothing.
    stage(cx, &review, true, StageTarget::File("missing.txt".into()));
    assert!(matches!(
        review.read_with(cx, |r, _| r.staging().clone()),
        GitOp::Failed(_)
    ));
    assert_eq!(git_changes(&events), 0);
    // Stage, Unstage, Stage All, Unstage All each ask once.
    for (add, target) in [
        (true, StageTarget::File("new.txt".into())),
        (false, StageTarget::File("new.txt".into())),
        (true, StageTarget::All),
        (false, StageTarget::All),
    ] {
        let before = git_changes(&events);
        stage(cx, &review, add, target);
        assert_eq!(
            review.read_with(cx, |r, _| r.staging().clone()),
            GitOp::Idle
        );
        assert_eq!(git_changes(&events), before + 1);
    }

    // What they asked for runs once the cooldown is over: staged is still uncommitted.
    stage(cx, &review, true, StageTarget::All);
    cool_down(cx);
    settle(cx, &store, &[&id]);
    wait_until(cx, &review, |r| !r.is_loading());
    assert_eq!(status(cx, &store, &id).working_tree, WorkingTree::Dirty);

    // Committed: the header's "Uncommitted" goes without reselecting the workspace.
    review.update(cx, |r, cx| r.commit("add new.txt", cx));
    wait_until(cx, &review, |r| {
        *r.commit_op() == GitOp::Idle && r.overview().uncommitted == 0
    });
    cool_down(cx);
    wait_status(cx, &store, &id, |s| s.working_tree == WorkingTree::Clean);
    settle(cx, &store, &[&id]);

    // "Refresh changes" (also run when the tab is shown again).
    std::fs::write(repo.join("later.txt"), "x\n").unwrap();
    let before = git_changes(&events);
    review.update(cx, |r, cx| r.refresh(cx));
    assert_eq!(git_changes(&events), before + 1);
    cool_down(cx);
    wait_status(cx, &store, &id, |s| s.working_tree == WorkingTree::Dirty);
}

#[gpui_kit::test]
fn merges_refresh_the_parent_and_the_kept_child(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish_empty(&h);
    let repo = new_repo(&h);
    let store = open_store(cx, &h, "client-a");
    let root = open(cx, &store, &repo);
    let mid = add_child(cx, &store, &root, "mid");
    let keep = add_child(cx, &store, &mid, "keep");
    wait_status(cx, &store, &mid, |s| s.parent_diff == ParentDiff::Clean);
    wait_status(cx, &store, &keep, |s| s.parent_diff == ParentDiff::Clean);
    settle(cx, &store, &[&root, &mid, &keep]);

    // `keep` gets a commit of its own, and knows it.
    commit_file(&dir(cx, &store, &keep), "k.txt", "k\n");
    cool_down(cx);
    store.update(cx, |s, cx| s.refresh_git_status(&keep, cx));
    wait_status(cx, &store, &keep, |s| s.parent_diff == ParentDiff::NotClean);
    settle(cx, &store, &[&keep]);

    // Merge and keep: the child has nothing left to merge, its parent has a commit its own
    // parent lacks.
    cool_down(cx);
    store.update(cx, |s, cx| {
        s.merge_workspace(&keep, false, AfterMerge::Keep, cx)
    });
    let keep_ws = workspace(cx, &store, &keep);
    wait_until(cx, &keep_ws, |w| {
        w.merge_output()
            .is_some_and(|o| o.state != MergeState::Running)
    });
    let output = keep_ws
        .read_with(cx, |w, _| w.merge_output().cloned())
        .unwrap();
    assert_eq!(output.state, MergeState::Succeeded, "{}", output.output);
    wait_status(cx, &store, &keep, |s| s.parent_diff == ParentDiff::Clean);
    wait_status(cx, &store, &mid, |s| s.parent_diff == ParentDiff::NotClean);

    // Merge and remove: once `gone`'s change to a.txt is in `mid`, `mid` conflicts with the
    // root, which changed it too.
    let gone = add_child(cx, &store, &mid, "gone");
    settle(cx, &store, &[&root, &mid, &keep, &gone]);
    commit_file(&dir(cx, &store, &gone), "a.txt", "gone\n");
    commit_file(&repo, "a.txt", "root\n");
    assert_eq!(
        status(cx, &store, &mid).parent_conflicts,
        ParentConflicts::None
    );
    cool_down(cx);
    store.update(cx, |s, cx| {
        s.merge_workspace(&gone, false, AfterMerge::Remove, cx)
    });
    wait_until(cx, &store, |s| {
        s.entry(&gone).is_none()
            || matches!(s.entry(&gone), Some(WorkspaceEntry::OperationError { .. }))
    });
    if let Some(error) = error_of(cx, &store, &gone) {
        panic!("merge failed: {error}");
    }
    wait_status(cx, &store, &mid, |s| {
        s.parent_conflicts == ParentConflicts::Conflicting
    });
    settle(cx, &store, &[&root, &mid, &keep]);

    // A merge that fails (the parent has uncommitted work) refreshes neither.
    std::fs::write(dir(cx, &store, &mid).join("dirty.txt"), "x\n").unwrap();
    std::fs::write(dir(cx, &store, &keep).join("dirty.txt"), "x\n").unwrap();
    cool_down(cx);
    store.update(cx, |s, cx| {
        s.merge_workspace(&keep, false, AfterMerge::Keep, cx)
    });
    wait_until(cx, &keep_ws, |w| {
        w.merge_output()
            .is_some_and(|o| o.state == MergeState::Failed)
    });
    for _ in 0..30 {
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(status(cx, &store, &mid).working_tree, WorkingTree::Clean);
    assert_eq!(status(cx, &store, &keep).working_tree, WorkingTree::Clean);
}

/// Pump the GPUI test scheduler until `task` resolves.
fn finish<T: 'static>(cx: &mut TestAppContext, task: Task<T>) -> T {
    let slot = Rc::new(RefCell::new(None));
    let sink = slot.clone();
    cx.foreground_executor()
        .spawn(async move { *sink.borrow_mut() = Some(task.await) })
        .detach();
    for _ in 0..1000 {
        cx.run_until_parked();
        if let Some(value) = slot.borrow_mut().take() {
            return value;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("task did not finish");
}

fn controller(
    cx: &mut TestAppContext,
    store: &Entity<SessionStore>,
    id: &str,
) -> Entity<GitController> {
    store.read_with(cx, |s, _| {
        s.git_controller(id).expect("a git controller").clone()
    })
}

/// The header's Pull click: the pull starts (none runs yet).
fn start_pull(
    cx: &mut TestAppContext,
    controller: &Entity<GitController>,
) -> Task<Result<(), String>> {
    controller
        .update(cx, |git, cx| git.pull(cx))
        .expect("no pull runs yet")
}

#[gpui_kit::test]
fn a_failed_pull_resolves_with_gits_error_and_one_pull_runs_at_a_time(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish_empty(&h);
    // No upstream to pull from.
    let repo = new_repo(&h);
    let store = open_store(cx, &h, "client-a");
    let id = open(cx, &store, &repo);
    settle(cx, &store, &[&id]);
    let git = controller(cx, &store, &id);

    let pull = start_pull(cx, &git);
    assert_eq!(git.read_with(cx, |g, _| g.pull_state()), PullState::Pulling);
    // `disabled={pullLoading}`: a click meanwhile starts nothing.
    assert!(git.update(cx, |g, cx| g.pull(cx)).is_none());

    // `alert(result.error)`: git's own words.
    let error = finish(cx, pull).expect_err("there is no upstream");
    assert!(
        error.starts_with("There is no tracking information for the current branch."),
        "{error}"
    );
    assert_eq!(git.read_with(cx, |g, _| g.pull_state()), PullState::Idle);
    // And the next click pulls again.
    let again = start_pull(cx, &git);
    assert!(finish(cx, again).is_err());
}

#[gpui_kit::test]
fn a_pull_brings_in_the_commits_behind_upstream(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish_empty(&h);
    let root = h.daemon.root();
    let remote = root.join(format!("remote-{}.git", uuid::Uuid::new_v4().simple()));
    git(
        root,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            &remote.to_string_lossy(),
        ],
    );
    let upstream = new_repo(&h);
    git(
        &upstream,
        &["remote", "add", "origin", &remote.to_string_lossy()],
    );
    git(&upstream, &["push", "-q", "-u", "origin", "main"]);
    let clone = root.join(format!("clone-{}", uuid::Uuid::new_v4().simple()));
    git(
        root,
        &[
            "clone",
            "-q",
            &remote.to_string_lossy(),
            &clone.to_string_lossy(),
        ],
    );
    let store = open_store(cx, &h, "client-a");
    let id = open(cx, &store, &clone);
    settle(cx, &store, &[&id]);

    commit_file(&upstream, "b.txt", "upstream\n");
    git(&upstream, &["push", "-q"]);
    cool_down(cx);
    store.update(cx, |s, cx| s.refresh_git_status(&id, cx));
    wait_status(cx, &store, &id, |s| s.behind_count == 1);
    settle(cx, &store, &[&id]);

    let git_controller = controller(cx, &store, &id);
    let pull = start_pull(cx, &git_controller);
    assert_eq!(finish(cx, pull), Ok(()));
    assert_eq!(status(cx, &store, &id).behind_count, 0);
    assert_eq!(
        git_controller.read_with(cx, |g, _| g.pull_state()),
        PullState::Idle
    );
    assert_eq!(
        std::fs::read_to_string(clone.join("b.txt")).unwrap(),
        "upstream\n"
    );
}

#[gpui_kit::test]
fn pulling_outside_a_git_repo_fails_with_not_a_git_repo(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish_empty(&h);
    let plain = h
        .daemon
        .root()
        .join(format!("plain-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&plain).unwrap();
    let store = open_store(cx, &h, "client-a");
    let id = open(cx, &store, &plain);
    settle(cx, &store, &[&id]);

    let git = controller(cx, &store, &id);
    let pull = start_pull(cx, &git);
    assert_eq!(finish(cx, pull), Err("Not a git repo".to_owned()));
}

#[gpui_kit::test]
fn a_pull_whose_workspace_goes_first_says_so(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish_empty(&h);
    let store = open_store(cx, &h, "client-a");
    let git = cx.new(|_| {
        GitController::new(
            "gone".to_owned(),
            store.downgrade(),
            h.conn.git(),
            Arc::new(UnavailableGitHub::new("tls".to_owned())),
        )
    });
    let pull = start_pull(cx, &git);
    // The workspace leaves the session (its controller with it) before the pull ran.
    drop(git);
    cx.update(|_| {});
    assert_eq!(finish(cx, pull), Err(PULL_GONE.to_owned()));
}
