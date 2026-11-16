//! "Auto Open All Recent Worktrees" against a real in-process daemon and git: detection runs
//! again whenever what it reads changes, the checked count survives a re-detection, and Ok
//! waits for the batch with the controls disabled, showing why it stopped.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::stream::BoxStream;
use gpui_kit::{AppContext as _, Entity, TestAppContext};
use treeterm_core::daemon::{DaemonConnection, SessionLockResult};
use treeterm_core::exec::{ExecApi, ExecClient, ExecError, ExecEvent, ExecRequest};
use treeterm_core::wire::workspace_file::WorktreeSettings;
use treeterm_core::worktree_registry::WorktreeRegistry;
use treeterm_model::auto_open::{AutoOpenEvent, AutoOpenModel, AutoOpenRow};
use treeterm_model::child_dialog::Listing;
use treeterm_model::session_store::{AdoptOptions, SessionState, SessionStore, WorkspaceEntry};
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::traffic::ConnectionTraffic;
use treeterm_proto::treeterm::WorkspaceRef;

mod common;
use common::{Harness, harness, recv, wait_until, wait_until_app};

/// The daemon's commands with `HOME` set to a temporary directory, so the worktree registry
/// lives there and never in the real `~/.treeterm`.
struct HomeExec {
    inner: ExecClient,
    home: String,
}

impl ExecApi for HomeExec {
    fn start(&self, mut request: ExecRequest) -> BoxStream<'static, Result<ExecEvent, ExecError>> {
        request.env.insert("HOME".to_owned(), self.home.clone());
        self.inner.start(request)
    }
}

struct Fixture {
    h: Harness,
    home: tempfile::TempDir,
    store: Entity<SessionStore>,
}

impl Fixture {
    fn new(cx: &mut TestAppContext) -> Self {
        let h = harness();
        let home = tempfile::tempdir().unwrap();
        let settings = cx.new(|_| SettingsStore::in_memory(Default::default()));
        let registry = WorktreeRegistry::new(
            Arc::new(h.conn.fs_client()),
            Arc::new(HomeExec {
                inner: h.conn.exec_client(),
                home: home.path().to_string_lossy().into_owned(),
            }),
        );
        let conn = h.conn.clone();
        let store = cx.new(|cx| {
            let mut store = SessionStore::new(
                conn,
                "client-a".to_owned(),
                settings,
                None,
                cx.new(ConnectionTraffic::new),
                cx,
            );
            store.set_worktree_registry(registry);
            store
        });
        Self { h, home, store }
    }

    /// The daemon host's `~/.treeterm/worktrees.json`.
    fn write_registry(&self, content: &str) {
        let dir = self.home.path().join(".treeterm");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("worktrees.json"), content).unwrap();
    }

    /// The registry remembers `path` on `branch`, named `name`.
    fn remember(&self, path: &str, branch: &str, name: &str) {
        self.write_registry(&format!(
            r#"{{"version":1,"entries":[{{"path":"{path}","branch":"{branch}","displayName":"{name}","description":null,"lastUsedAt":1}}]}}"#
        ));
    }

    /// A git repo with one commit on `main`, opened as a workspace.
    fn open_repo(&self, cx: &mut TestAppContext) -> (PathBuf, String) {
        futures::executor::block_on(self.h.conn.update_session(
            Vec::<WorkspaceRef>::new(),
            None,
            "test-writer",
        ))
        .unwrap()
        .unwrap();
        let repo = self.h.daemon.root().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(
            &repo,
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        // As git lists it (the temporary directory may sit behind a symlink).
        let repo = repo.canonicalize().unwrap();
        wait_until(cx, &self.store, |s| matches!(s.state(), SessionState::Live));
        let path = repo.to_string_lossy().into_owned();
        let id = self.store.update(cx, |s, cx| {
            s.open_workspace(path, WorktreeSettings::default(), cx)
        });
        wait_until(cx, &self.store, |s| {
            matches!(s.entry(&id), Some(WorkspaceEntry::Loaded(_)))
        });
        (repo, id)
    }

    fn dialog(&self, cx: &mut TestAppContext, root: &str) -> Entity<AutoOpenModel> {
        let (store, root) = (self.store.clone(), root.to_owned());
        cx.new(|cx| AutoOpenModel::new(store, root, cx))
    }

    /// Another window's connection, holding the session lock.
    fn lock_elsewhere(&self) -> DaemonConnection {
        let other = futures::executor::block_on(DaemonConnection::open(
            &self.h.rt,
            self.h.daemon.socket_path(),
        ))
        .unwrap()
        .unwrap();
        let locked = futures::executor::block_on(other.lock_session(Duration::from_secs(600)))
            .unwrap()
            .unwrap();
        assert!(matches!(locked, SessionLockResult::Acquired(_)));
        other
    }

    fn workspace_count(&self, cx: &mut TestAppContext) -> usize {
        self.store.read_with(cx, |s, _| s.workspace_ids().count())
    }

    /// The loaded workspace at `path`.
    fn workspace_at(&self, cx: &mut TestAppContext, path: &str) -> String {
        self.store.read_with(cx, |s, cx| {
            s.workspace_ids()
                .find(|id| {
                    s.workspace_store(id)
                        .is_some_and(|w| w.read(cx).workspace().path == path)
                })
                .cloned()
                .unwrap_or_else(|| panic!("{path} is not open"))
        })
    }
}

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

/// Check out a new `branch` (from `base`) in a worktree next to the repo; returns its path as
/// git lists it.
fn add_worktree(repo: &Path, branch: &str, base: &str) -> String {
    let path = repo.parent().unwrap().join(format!("wt-{branch}"));
    git(
        repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            branch,
            path.to_str().unwrap(),
            base,
        ],
    );
    let listed = git(repo, &["worktree", "list", "--porcelain"]);
    let mut current = None;
    for line in listed.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current = Some(path.to_owned());
        }
        if line == format!("branch refs/heads/{branch}") {
            return current.unwrap();
        }
    }
    panic!("{branch} not in the worktree list:\n{listed}");
}

fn rows(cx: &mut TestAppContext, model: &Entity<AutoOpenModel>) -> Listing<AutoOpenRow> {
    model.read_with(cx, |m, _| m.rows())
}

/// The rows once detection is done.
fn detected(cx: &mut TestAppContext, model: &Entity<AutoOpenModel>) -> Vec<AutoOpenRow> {
    wait_until(cx, model, |m| matches!(m.rows(), Listing::Loaded(_)));
    match rows(cx, model) {
        Listing::Loaded(rows) => rows,
        Listing::Loading => unreachable!(),
    }
}

fn paths(rows: &[AutoOpenRow]) -> Vec<&str> {
    rows.iter().map(|r| r.path.as_str()).collect()
}

/// Every event the dialog reports, in order.
fn record(
    cx: &mut TestAppContext,
    model: &Entity<AutoOpenModel>,
) -> Rc<RefCell<Vec<AutoOpenEvent>>> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|cx| {
        cx.subscribe(model, move |_, event: &AutoOpenEvent, _| {
            sink.borrow_mut().push(*event)
        })
        .detach()
    });
    events
}

/// Run the app for `time`: long enough for a detection to come back, had it started.
fn settle(cx: &mut TestAppContext, time: Duration) {
    let until = std::time::Instant::now() + time;
    while std::time::Instant::now() < until {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn error(cx: &mut TestAppContext, model: &Entity<AutoOpenModel>) -> Option<String> {
    model.read_with(cx, |m, _| m.error().map(str::to_owned))
}

#[gpui_kit::test]
fn remembered_worktrees_are_listed_checked_and_include_unknown_adds_the_rest(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let f = Fixture::new(cx);
    let (repo, root) = f.open_repo(cx);
    let known = add_worktree(&repo, "known", "main");
    let stray = add_worktree(&repo, "stray", "main");
    f.remember(&known, "known", "Known work");

    let model = f.dialog(cx, &root);
    assert_eq!(rows(cx, &model), Listing::Loading, "Detecting worktrees...");
    let listed = detected(cx, &model);
    assert_eq!(paths(&listed), [known.as_str()]);
    assert_eq!(listed[0].name(), "Known work");
    model.read_with(cx, |m, cx| {
        assert!(m.is_checked(&known));
        assert_eq!(
            (m.count(), m.confirm_label(), m.can_confirm()),
            (1, "Ok (1)".to_owned(), true)
        );
        assert_eq!(m.root_name(cx), "repo");
    });

    // Re-detecting keeps the count, with Ok disabled.
    model.update(cx, |m, cx| m.set_include_unknown(true, cx));
    assert_eq!(rows(cx, &model), Listing::Loading);
    model.read_with(cx, |m, _| {
        assert!(m.include_unknown());
        assert_eq!(
            (m.count(), m.confirm_label(), m.can_confirm()),
            (1, "Ok (1)".to_owned(), false)
        );
    });
    let listed = detected(cx, &model);
    assert_eq!(listed.len(), 2);
    let unknown = listed
        .iter()
        .find(|r| r.path == stray)
        .expect("the unknown worktree");
    assert!(unknown.entry.is_none());
    assert_eq!(unknown.name(), "stray");
    assert_eq!(
        model.read_with(cx, |m, _| (m.count(), m.is_checked(&stray))),
        (2, true)
    );

    // A row click toggles it.
    model.update(cx, |m, cx| m.toggle(&stray, cx));
    assert_eq!(
        model.read_with(cx, |m, _| (m.count(), m.is_checked(&stray))),
        (1, false)
    );
}

#[gpui_kit::test]
fn detection_runs_again_when_a_workspace_opens_or_closes_or_the_root_changes_branch(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let f = Fixture::new(cx);
    let (repo, root) = f.open_repo(cx);
    let a = add_worktree(&repo, "a", "main");
    let b = add_worktree(&repo, "b", "main");
    let model = f.dialog(cx, &root);
    model.update(cx, |m, cx| m.set_include_unknown(true, cx));
    assert_eq!(detected(cx, &model).len(), 2);

    // Opened elsewhere: its row goes.
    let worktree = treeterm_core::git::WorktreeInfo {
        path: a.clone(),
        branch: "a".into(),
    };
    let opened = f.store.update(cx, |s, cx| {
        s.adopt_worktree(&root, worktree, AdoptOptions::default(), cx)
    });
    let child = recv(cx, opened).unwrap();
    wait_until(
        cx,
        &model,
        |m| matches!(m.rows(), Listing::Loaded(rows) if rows.len() == 1),
    );
    assert_eq!(paths(&detected(cx, &model)), [b.as_str()]);

    // Closed again: it comes back.
    f.store
        .update(cx, |s, cx| s.remove_workspace_keep_both(&child, cx));
    wait_until(
        cx,
        &model,
        |m| matches!(m.rows(), Listing::Loaded(rows) if rows.len() == 2),
    );

    // The root on another branch: detected again (here failing, as the registry now is).
    f.write_registry("{broken");
    settle(cx, Duration::from_millis(300));
    assert_eq!(error(cx, &model), None, "nothing it reads has changed");
    let switch = |cx: &mut TestAppContext, branch: &str| {
        let store = f
            .store
            .read_with(cx, |s, _| s.workspace_store(&root).unwrap().clone());
        store.update(cx, |w, cx| w.set_git_branch(branch, cx));
    };
    git(&repo, &["checkout", "-q", "-b", "other"]);
    switch(cx, "other");
    wait_until(cx, &model, |m| m.error().is_some());
    let failed = error(cx, &model).unwrap();
    assert!(
        failed.starts_with("Failed to detect worktrees: "),
        "{failed}"
    );
    assert_eq!(rows(cx, &model), Listing::Loaded(Vec::new()));

    // And back: the next detection clears the error.
    std::fs::remove_file(f.home.path().join(".treeterm/worktrees.json")).unwrap();
    git(&repo, &["checkout", "-q", "main"]);
    switch(cx, "main");
    wait_until(cx, &model, |m| m.error().is_none());
    assert_eq!(detected(cx, &model).len(), 2);
}

#[gpui_kit::test]
fn a_failed_detection_keeps_the_count_and_ok_then_opens_an_empty_batch(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let f = Fixture::new(cx);
    let (repo, root) = f.open_repo(cx);
    let known = add_worktree(&repo, "known", "main");
    f.remember(&known, "known", "Known");
    let model = f.dialog(cx, &root);
    let events = record(cx, &model);
    assert_eq!(detected(cx, &model).len(), 1);

    f.write_registry("{broken");
    model.update(cx, |m, cx| m.set_include_unknown(true, cx));
    wait_until(cx, &model, |m| m.error().is_some());
    let failed = error(cx, &model).unwrap();
    assert!(
        failed.starts_with("Failed to detect worktrees: "),
        "{failed}"
    );
    assert_eq!(
        rows(cx, &model),
        Listing::Loaded(Vec::new()),
        "No recent worktrees found"
    );
    // `checkedPaths` stays: Ok still counts it, and is enabled.
    model.read_with(cx, |m, _| {
        assert_eq!(
            (m.count(), m.confirm_label(), m.can_confirm()),
            (1, "Ok (1)".to_owned(), true)
        )
    });

    // Each detection clears the error first.
    model.update(cx, |m, cx| m.set_include_unknown(false, cx));
    assert_eq!(
        (error(cx, &model), rows(cx, &model)),
        (None, Listing::Loading)
    );
    wait_until(cx, &model, |m| m.error().is_some());

    // Ok opens nothing and closes the dialog.
    model.update(cx, |m, cx| m.confirm(cx));
    model.read_with(cx, |m, _| {
        assert!(m.processing());
        assert_eq!(m.error(), None, "cleared by Ok");
    });
    wait_until_app(cx, &model, |_, _| !events.borrow().is_empty());
    assert_eq!(*events.borrow(), [AutoOpenEvent::Opened]);
    assert_eq!(f.workspace_count(cx), 1);
}

#[gpui_kit::test]
fn ok_waits_for_the_batch_with_the_controls_disabled_and_shows_a_lock_held_elsewhere(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let f = Fixture::new(cx);
    let (repo, root) = f.open_repo(cx);
    let commit = |dir: &str| {
        git(
            Path::new(dir),
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "work",
            ],
        )
    };
    let feat = add_worktree(&repo, "feat", "main");
    commit(&feat);
    let sub = add_worktree(&repo, "sub", "feat");
    commit(&sub);
    let model = f.dialog(cx, &root);
    let events = record(cx, &model);
    model.update(cx, |m, cx| m.set_include_unknown(true, cx));
    let listed = detected(cx, &model);
    // `sub` sits under `feat`, which it branched from.
    assert_eq!(paths(&listed), [feat.as_str(), sub.as_str()]);
    assert_eq!(listed[1].parent_path.as_deref(), Some(feat.as_str()));
    assert_eq!(listed[1].depth, 1);

    let other = f.lock_elsewhere();
    model.update(cx, |m, cx| m.confirm(cx));
    model.read_with(cx, |m, _| {
        assert!(m.processing());
        assert_eq!(
            (m.confirm_label(), m.can_confirm()),
            ("Opening... Please wait".to_owned(), false)
        );
    });
    // Rows and the include-unknown checkbox are disabled meanwhile; Ok does nothing more.
    model.update(cx, |m, cx| {
        m.toggle(&feat, cx);
        m.set_include_unknown(false, cx);
        m.confirm(cx);
    });
    model.read_with(cx, |m, _| {
        assert!(m.is_checked(&feat), "the row click was ignored");
        assert!(m.include_unknown(), "the toggle was ignored");
        assert!(
            matches!(m.rows(), Listing::Loaded(_)),
            "no detection started"
        );
    });

    // The lock is refused: the dialog stays, with the error and its controls back.
    wait_until(cx, &model, |m| !m.processing());
    assert_eq!(
        error(cx, &model).as_deref(),
        Some("Session is locked by another window")
    );
    assert!(events.borrow().is_empty());
    assert_eq!(f.workspace_count(cx), 1, "nothing was added");
    model.update(cx, |m, cx| m.toggle(&feat, cx));
    model.read_with(cx, |m, _| {
        assert_eq!((m.count(), m.confirm_label()), (1, "Ok (1)".to_owned()))
    });
    model.update(cx, |m, cx| m.toggle(&feat, cx));

    // Once the other window lets go, the batch opens both, each under its parent, and the
    // last one is shown.
    futures::executor::block_on(other.unlock_session())
        .unwrap()
        .unwrap();
    model.update(cx, |m, cx| m.confirm(cx));
    assert_eq!(error(cx, &model), None, "cleared by Ok");
    wait_until_app(cx, &model, |_, _| !events.borrow().is_empty());
    assert_eq!(*events.borrow(), [AutoOpenEvent::Opened]);
    let (feat_id, sub_id) = (f.workspace_at(cx, &feat), f.workspace_at(cx, &sub));
    f.store.read_with(cx, |s, cx| {
        let file = |id: &str| s.workspace_store(id).unwrap().read(cx).file().clone();
        assert_eq!(
            (file(&feat_id).name, file(&feat_id).parent_id),
            ("feat".to_owned(), Some(root.clone()))
        );
        assert_eq!(
            (file(&sub_id).name, file(&sub_id).parent_id),
            ("sub".to_owned(), Some(feat_id.clone()))
        );
        assert_eq!(s.active_workspace(), Some(sub_id.as_str()));
    });
    // The batch's lock is released.
    let again = futures::executor::block_on(other.lock_session(Duration::from_secs(1)))
        .unwrap()
        .unwrap();
    assert!(matches!(again, SessionLockResult::Acquired(_)));
}

#[gpui_kit::test]
fn the_dialog_goes_when_its_root_does(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let f = Fixture::new(cx);
    let (_repo, root) = f.open_repo(cx);
    let model = f.dialog(cx, &root);
    let events = record(cx, &model);
    assert!(detected(cx, &model).is_empty());
    f.store
        .update(cx, |s, cx| s.remove_workspace_keep_both(&root, cx));
    wait_until_app(cx, &model, |_, _| !events.borrow().is_empty());
    assert_eq!(events.borrow()[0], AutoOpenEvent::RootGone);
    assert_eq!(
        error(cx, &model).as_deref(),
        Some("Root workspace not found")
    );
}
