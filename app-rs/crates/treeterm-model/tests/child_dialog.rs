//! "Add Child Workspace" against a real in-process daemon and git: each tab reads its list on
//! every visit, a tab left before its read finished gets nothing, and the rows' rules.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use gpui_kit::{AppContext as _, Entity, Subscription, TestAppContext};
use treeterm_core::exec::{ExecApi, ExecClient, ExecError, ExecEvent, ExecRequest};
use treeterm_core::git::WorktreeInfo;
use treeterm_core::github::{GitHubApi, GitHubError, HeadRepo, PostCommentsOutcome, PrListItem, PrLookup};
use treeterm_core::review::ReviewComment;
use treeterm_core::wire::workspace_file::{WorkspaceFile, WorktreeSettings};
use treeterm_core::worktree_registry::WorktreeRegistry;
use treeterm_model::child_dialog::{
    AppChoice, ChildDialogEvent, ChildDialogModel, ChildRow, ChildTab, FORK_TOOLTIP, IN_WORKTREE_TOOLTIP, Listing,
    RowState,
};
use treeterm_model::session_store::{AdoptOptions, RemoveMode, SessionState, SessionStore, WorkspaceEntry};
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

type PrReply = Result<Vec<PrListItem>, GitHubError>;

/// GitHub with no PR lookups; each open-PR read waits for the reply the test sends.
#[derive(Default)]
struct FakeGitHub {
    replies: std::sync::Mutex<std::collections::VecDeque<oneshot::Receiver<PrReply>>>,
}

impl FakeGitHub {
    /// The reply to the next open-PR read; sending fails if that read was dropped.
    fn next_reply(&self) -> oneshot::Sender<PrReply> {
        let (tx, rx) = oneshot::channel();
        self.replies.lock().unwrap().push_back(rx);
        tx
    }
}

impl GitHubApi for FakeGitHub {
    fn get_pr_info(&self, _: &str, _: &str, _: &str) -> BoxFuture<'static, Result<PrLookup, GitHubError>> {
        async { Err(GitHubError::NoPrForBranch) }.boxed()
    }

    fn list_open_prs(&self, _: &str) -> BoxFuture<'static, PrReply> {
        let reply = self.replies.lock().unwrap().pop_front().expect("a reply for each read");
        async move { reply.await.expect("the test replies") }.boxed()
    }

    fn post_review_comments(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: Vec<ReviewComment>,
    ) -> BoxFuture<'static, Result<PostCommentsOutcome, GitHubError>> {
        async { Err(GitHubError::NoPat) }.boxed()
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
            Arc::new(HomeExec { inner: h.conn.exec_client(), home: home.path().to_string_lossy().into_owned() }),
        );
        let conn = h.conn.clone();
        let store = cx.new(|cx| {
            let mut store = SessionStore::new(conn, "client-a".to_owned(), settings, None, cx.new(ConnectionTraffic::new), cx);
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

    fn publish(&self, ids: &[&str]) {
        let path = self.h.daemon.root().to_string_lossy().into_owned();
        let refs = ids.iter().map(|id| WorkspaceRef { id: (*id).into(), path: path.clone() }).collect();
        futures::executor::block_on(self.h.conn.update_session(refs, None, "test-writer")).unwrap().unwrap();
    }

    /// A git repo with one commit on `main`, opened as a workspace.
    fn open_repo(&self, cx: &mut TestAppContext) -> (PathBuf, String) {
        self.publish(&[]);
        let repo = self.h.daemon.root().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "init"]);
        wait_until(cx, &self.store, |s| matches!(s.state(), SessionState::Live));
        let path = repo.to_string_lossy().into_owned();
        let id = self.store.update(cx, |s, cx| s.open_workspace(path, WorktreeSettings::default(), cx));
        wait_until(cx, &self.store, |s| matches!(s.entry(&id), Some(WorkspaceEntry::Loaded(_))));
        (repo, id)
    }

    fn dialog(&self, cx: &mut TestAppContext, parent: &str, tab: ChildTab) -> Entity<ChildDialogModel> {
        let (store, parent) = (self.store.clone(), parent.to_owned());
        cx.new(|cx| ChildDialogModel::new(store, parent, tab, cx))
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Check out a new `branch` in a worktree next to the repo; returns its path as git lists it.
fn add_worktree(repo: &Path, branch: &str) -> String {
    let path = repo.parent().unwrap().join(format!("wt-{branch}"));
    git(repo, &["worktree", "add", "-q", "-b", branch, path.to_str().unwrap()]);
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

fn rows(cx: &mut TestAppContext, model: &Entity<ChildDialogModel>) -> Listing<ChildRow> {
    model.read_with(cx, |m, cx| m.rows("", cx))
}

fn loaded(cx: &mut TestAppContext, model: &Entity<ChildDialogModel>) -> Vec<ChildRow> {
    wait_until_app(cx, model, |m, cx| matches!(m.rows("", cx), Listing::Loaded(_)));
    match rows(cx, model) {
        Listing::Loaded(rows) => rows,
        Listing::Loading => unreachable!(),
    }
}

fn names(rows: &[ChildRow]) -> Vec<&str> {
    rows.iter().map(|r| r.name.as_str()).collect()
}

/// Run the app for `time`: long enough for a read to come back, had it not been dropped.
fn settle(cx: &mut TestAppContext, time: Duration) {
    let until = std::time::Instant::now() + time;
    while std::time::Instant::now() < until {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui_kit::test]
fn each_visit_to_a_tab_reads_its_list_again(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let f = Fixture::new(cx);
    let (repo, parent) = f.open_repo(cx);
    let model = f.dialog(cx, &parent, ChildTab::Branch);
    assert_eq!(names(&loaded(cx, &model)), ["main"]);

    git(&repo, &["branch", "b2"]);
    assert!(model.update(cx, |m, cx| m.switch(ChildTab::Create, cx)));
    assert!(model.update(cx, |m, cx| m.switch(ChildTab::Branch, cx)));
    assert_eq!(rows(cx, &model), Listing::Loading, "a fresh 'Loading branches...'");
    assert_eq!(names(&loaded(cx, &model)), ["b2", "main"]);

    // Existing and Recent too.
    model.update(cx, |m, cx| m.switch(ChildTab::Existing, cx));
    let before = loaded(cx, &model).len();
    let side = add_worktree(&repo, "side");
    model.update(cx, |m, cx| m.switch(ChildTab::Recent, cx));
    assert!(loaded(cx, &model).is_empty());
    f.write_registry(&format!(
        r#"{{"version":1,"entries":[{{"path":"{side}","branch":"side","displayName":"Side","description":null,"lastUsedAt":1}}]}}"#
    ));
    model.update(cx, |m, cx| m.switch(ChildTab::Existing, cx));
    let existing = loaded(cx, &model);
    assert_eq!(existing.len(), before + 1);
    assert!(existing.iter().any(|r| r.key == side && r.name == "Side"), "named as remembered");
    model.update(cx, |m, cx| m.switch(ChildTab::Recent, cx));
    assert_eq!(names(&loaded(cx, &model)), ["Side"]);
}

#[gpui_kit::test]
fn a_tab_left_before_its_read_finished_gets_neither_rows_nor_error(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let f = Fixture::new(cx);
    let (_repo, parent) = f.open_repo(cx);
    f.write_registry("{broken");
    let model = f.dialog(cx, &parent, ChildTab::Recent);
    model.update(cx, |m, cx| m.switch(ChildTab::Branch, cx));
    assert_eq!(names(&loaded(cx, &model)), ["main"]);
    settle(cx, Duration::from_millis(500));
    assert_eq!(model.read_with(cx, |m, _| m.error().map(str::to_owned)), None);

    // Coming back reads (and fails) again.
    model.update(cx, |m, cx| m.switch(ChildTab::Recent, cx));
    assert_eq!(rows(cx, &model), Listing::Loading);
    wait_until(cx, &model, |m| m.error().is_some());
    let error = model.read_with(cx, |m, _| m.error().unwrap().to_owned());
    assert!(error.starts_with("Failed to load recent workspaces: "), "{error}");
    assert_eq!(rows(cx, &model), Listing::Loaded(Vec::new()));
}

#[gpui_kit::test]
fn existing_reports_a_registry_error_even_when_the_worktree_list_fails(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let f = Fixture::new(cx);
    let not_a_repo = f.h.daemon.root().join("plain");
    std::fs::create_dir_all(&not_a_repo).unwrap();
    let body = serde_json::json!({
        "name": "plain", "status": "active", "isGitRepo": true, "isWorktree": false,
        "gitRootPath": not_a_repo.to_string_lossy(), "appStates": {}, "metadata": {},
        "createdAt": 1, "lastActivity": 1, "parentHash": ""
    });
    std::fs::write(f.h.daemon.workspaces_dir().join("p.json"), body.to_string()).unwrap();
    f.publish(&["p"]);
    wait_until(cx, &f.store, |s| matches!(s.entry("p"), Some(WorkspaceEntry::Loaded(_))));
    f.write_registry("{broken");

    let model = f.dialog(cx, "p", ChildTab::Existing);
    assert!(loaded(cx, &model).is_empty());
    wait_until(cx, &model, |m| m.error().is_some());
    let error = model.read_with(cx, |m, _| m.error().unwrap().to_owned());
    assert!(error.starts_with("Failed to load worktree registry: "), "{error}");
}

#[gpui_kit::test]
fn a_recent_entry_with_an_empty_description_shows_no_description(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let f = Fixture::new(cx);
    let (repo, parent) = f.open_repo(cx);
    let cleared = add_worktree(&repo, "cleared");
    let noted = add_worktree(&repo, "noted");
    f.write_registry(&format!(
        r#"{{"version":1,"entries":[
            {{"path":"{cleared}","branch":"cleared","displayName":null,"description":"","lastUsedAt":2}},
            {{"path":"{noted}","branch":"noted","displayName":null,"description":"notes","lastUsedAt":1}}
        ]}}"#
    ));
    let model = f.dialog(cx, &parent, ChildTab::Recent);
    let rows = loaded(cx, &model);
    assert_eq!(names(&rows), ["cleared", "noted"], "most recently used first");
    assert_eq!(rows[0].description, None);
    assert_eq!(rows[1].description.as_deref(), Some("notes"));
}

#[gpui_kit::test]
fn a_branch_in_a_worktree_cannot_be_picked_and_says_why(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let f = Fixture::new(cx);
    let (repo, parent) = f.open_repo(cx);
    add_worktree(&repo, "side");
    git(&repo, &["branch", "free"]);
    let model = f.dialog(cx, &parent, ChildTab::Branch);
    let rows = loaded(cx, &model);
    let state = |name: &str| rows.iter().find(|r| r.name == name).unwrap().state;
    let taken = RowState::Unavailable { badge: "In Worktree", tooltip: IN_WORKTREE_TOOLTIP };
    assert_eq!(state("side"), taken);
    assert_eq!(state("main"), taken);
    assert_eq!(state("free"), RowState::Available);
    assert_eq!(IN_WORKTREE_TOOLTIP, "Branch is already in a worktree");

    model.update(cx, |m, cx| m.pick("side", "", "", cx));
    assert_eq!(model.read_with(cx, |m, _| m.selected_key()), None);
    model.update(cx, |m, cx| m.pick("free", "", "", cx));
    assert_eq!(model.read_with(cx, |m, _| m.selected_key()).as_deref(), Some("free"));
}

#[gpui_kit::test]
fn clicking_the_shown_tab_only_drops_its_error_and_pick(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let f = Fixture::new(cx);
    let (repo, parent) = f.open_repo(cx);
    git(&repo, &["branch", "free"]);
    let model = f.dialog(cx, &parent, ChildTab::Branch);
    let listed = loaded(cx, &model);

    let events = events(cx, &model);
    model.update(cx, |m, cx| m.submit("", "", cx));
    assert_eq!(model.read_with(cx, |m, _| m.error().map(str::to_owned)).as_deref(), Some("Please select a branch"));
    cx.run_until_parked();
    assert!(events.seen.borrow().is_empty());
    model.update(cx, |m, cx| m.pick("free", "", "", cx));
    assert!(!model.read_with(cx, |m, _| m.submit_disabled("")));

    assert!(!model.update(cx, |m, cx| m.switch(ChildTab::Branch, cx)), "nothing new to show");
    assert_eq!(rows(cx, &model), Listing::Loaded(listed), "the list stays");
    model.read_with(cx, |m, _| {
        assert_eq!(m.error(), None);
        assert_eq!(m.selected_key(), None);
        assert!(m.submit_disabled(""));
    });
}

#[gpui_kit::test]
fn an_existing_pick_outlives_a_reread_and_opens_the_worktree(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let f = Fixture::new(cx);
    let (repo, parent) = f.open_repo(cx);
    let side = add_worktree(&repo, "side");
    f.write_registry(&format!(
        r#"{{"version":1,"entries":[{{"path":"{side}","branch":"side","displayName":"Side","description":"kept","lastUsedAt":1}}]}}"#
    ));
    let model = f.dialog(cx, &parent, ChildTab::Existing);
    loaded(cx, &model);
    // Only the empty fields are filled in.
    let fill = model.update(cx, |m, cx| m.pick(&side, "typed", "", cx));
    assert_eq!((fill.name, fill.description.as_deref()), (None, Some("kept")));

    model.update(cx, |m, cx| m.switch(ChildTab::Branch, cx));
    model.update(cx, |m, cx| m.switch(ChildTab::Existing, cx));
    assert_eq!(rows(cx, &model), Listing::Loading);
    model.read_with(cx, |m, _| {
        assert_eq!(m.selected_key().as_deref(), Some(side.as_str()));
        assert!(!m.submit_disabled(""));
    });
    let events = events(cx, &model);
    model.update(cx, |m, cx| m.submit("", "", cx));
    let child = events.created(cx);
    wait_until_app(cx, &f.store, |s, cx| {
        s.workspace_store(&child).is_some_and(|w| w.read(cx).workspace().path == side)
    });
}

/// What a dialog model reported.
struct Events {
    seen: Rc<RefCell<Vec<ChildDialogEvent>>>,
    _subscription: Subscription,
}

impl Events {
    /// Wait for the dialog's `Created`.
    fn created(&self, cx: &mut TestAppContext) -> String {
        for _ in 0..1000 {
            cx.run_until_parked();
            if let Some(ChildDialogEvent::Created(id)) = self.seen.borrow().last() {
                return id.clone();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("not created: {:?}", self.seen.borrow());
    }
}

fn events(cx: &mut TestAppContext, model: &Entity<ChildDialogModel>) -> Events {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let subscription =
        cx.update(|cx| cx.subscribe(model, move |_, event: &ChildDialogEvent, _| sink.borrow_mut().push(event.clone())));
    Events { seen, _subscription: subscription }
}

/// The picked worktree's workspace file, once the dialog reported it.
fn adopted_file(cx: &mut TestAppContext, f: &Fixture, events: &Events) -> WorkspaceFile {
    let child = events.created(cx);
    f.store.read_with(cx, |s, cx| s.workspace_store(&child).expect("open").read(cx).file().clone())
}

#[gpui_kit::test]
fn open_existing_waits_in_the_dialog_and_uses_the_chosen_application(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let f = Fixture::new(cx);
    let (repo, parent) = f.open_repo(cx);
    let side = add_worktree(&repo, "side");
    let model = f.dialog(cx, &parent, ChildTab::Existing);
    loaded(cx, &model);
    let apps = model.read_with(cx, |m, cx| m.available_apps(cx));
    assert!(apps.iter().any(|(id, _)| id == "filesystem"), "{apps:?}");
    model.update(cx, |m, cx| {
        m.pick(&side, "", "", cx);
        m.set_app_choice(AppChoice::Custom(Some("filesystem".into())), cx);
    });
    let events = events(cx, &model);
    let before = f.store.read_with(cx, |s, _| s.workspace_ids().count());

    model.update(cx, |m, cx| m.submit("", "  notes ", cx));
    model.read_with(cx, |m, _| {
        assert_eq!(m.processing(), Some("Opening existing worktree..."));
        assert_eq!(m.submit_label(), "Opening... Please wait");
        assert!(m.submit_disabled(""), "Open is disabled");
    });
    // Another submit (Enter) does nothing; switching tabs keeps the wait.
    model.update(cx, |m, cx| m.submit("", "", cx));
    model.update(cx, |m, cx| m.switch(ChildTab::Create, cx));
    assert_eq!(model.read_with(cx, |m, _| m.submit_label()), "Creating... Please wait");
    model.update(cx, |m, cx| m.switch(ChildTab::Existing, cx));

    let file = adopted_file(cx, &f, &events);
    assert_eq!(events.seen.borrow().len(), 1, "added once");
    assert_eq!(f.store.read_with(cx, |s, _| s.workspace_ids().count()), before + 1);
    // Named after the branch, with the dialog's description and default application.
    assert_eq!((file.name.as_str(), file.display_name()), ("side", "side"));
    assert_eq!(file.metadata.get("description").map(String::as_str), Some("notes"));
    assert_eq!(file.settings.default_application_id, "filesystem");
    let tab = file.active_tab_id.as_deref().unwrap();
    assert_eq!(file.app_states[tab].application_id, "filesystem");
    model.read_with(cx, |m, _| {
        assert_eq!(m.processing(), None);
        assert_eq!(m.submit_label(), "Open");
    });
}

#[gpui_kit::test]
fn recent_workspace_keeps_a_typed_name_as_the_display_name(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let f = Fixture::new(cx);
    let (repo, parent) = f.open_repo(cx);
    let plain = add_worktree(&repo, "team/plain");
    let named = add_worktree(&repo, "named");
    f.write_registry(&format!(
        r#"{{"version":1,"entries":[
            {{"path":"{plain}","branch":"team/plain","displayName":null,"description":null,"lastUsedAt":2}},
            {{"path":"{named}","branch":"named","displayName":"Named","description":null,"lastUsedAt":1}}
        ]}}"#
    ));
    let model = f.dialog(cx, &parent, ChildTab::Recent);
    loaded(cx, &model);

    // A row with no remembered name fills in its branch: no display name then.
    let fill = model.update(cx, |m, cx| m.pick(&plain, "", "", cx));
    let name = fill.name.unwrap();
    assert_eq!(name, "team/plain");
    let events = events(cx, &model);
    model.update(cx, |m, cx| m.submit(&name, "", cx));
    assert_eq!(model.read_with(cx, |m, _| m.processing()), Some("Opening recent workspace..."));
    let file = adopted_file(cx, &f, &events);
    assert_eq!((file.name.as_str(), file.display_name()), ("team/plain", "team/plain"));
    assert_eq!(file.settings.default_application_id, "", "no custom application chosen");

    // A remembered name is filled in and kept as the display name.
    let fill = model.update(cx, |m, cx| m.pick(&named, "", "", cx));
    events.seen.borrow_mut().clear();
    model.update(cx, |m, cx| m.submit(fill.name.as_deref().unwrap(), "", cx));
    let file = adopted_file(cx, &f, &events);
    assert_eq!((file.name.as_str(), file.display_name()), ("named", "Named"));
}

#[gpui_kit::test]
fn a_refused_open_shows_its_error_and_lets_the_user_try_again(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let f = Fixture::new(cx);
    let (repo, parent) = f.open_repo(cx);
    let side = add_worktree(&repo, "side");
    let model = f.dialog(cx, &parent, ChildTab::Existing);
    loaded(cx, &model);
    model.update(cx, |m, cx| m.pick(&side, "", "", cx));
    let events = events(cx, &model);
    let ids = |cx: &mut TestAppContext| f.store.read_with(cx, |s, _| s.workspace_ids().cloned().collect::<Vec<_>>());
    let before = ids(cx);

    // Another window holds the session lock.
    let other = futures::executor::block_on(treeterm_core::daemon::DaemonConnection::open(&f.h.rt, f.h.daemon.socket_path()))
        .unwrap()
        .unwrap();
    futures::executor::block_on(other.lock_session(Duration::from_secs(600))).unwrap().unwrap();
    model.update(cx, |m, cx| m.submit("", "", cx));
    assert!(model.read_with(cx, |m, _| m.processing().is_some()));
    wait_until(cx, &model, |m| m.processing().is_none());
    model.read_with(cx, |m, _| {
        assert_eq!(m.error(), Some("Session is locked by another window"));
        assert!(!m.submit_disabled(""), "Open can be tried again");
    });
    assert_eq!(ids(cx), before, "nothing was added");

    // Opened elsewhere meanwhile: refused, and the error replaces the last one.
    futures::executor::block_on(other.unlock_session()).unwrap().unwrap();
    let info = WorktreeInfo { path: side.clone(), branch: "side".into() };
    let opened = f.store.update(cx, |s, cx| s.adopt_worktree(&parent, info, AdoptOptions::default(), cx));
    assert!(recv(cx, opened).is_ok());
    model.update(cx, |m, cx| m.submit("", "", cx));
    wait_until(cx, &model, |m| m.processing().is_none());
    assert_eq!(model.read_with(cx, |m, _| m.error().map(str::to_owned)).as_deref(), Some("This worktree is already open"));
    assert!(events.seen.borrow().is_empty());
}

#[gpui_kit::test]
fn the_dialog_goes_when_its_parent_does(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let f = Fixture::new(cx);
    let (_repo, parent) = f.open_repo(cx);
    let model = f.dialog(cx, &parent, ChildTab::Branch);
    loaded(cx, &model);
    let events = events(cx, &model);
    f.store.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert!(events.seen.borrow().is_empty(), "the parent is still there");

    f.store.update(cx, |s, cx| s.remove_workspace(&parent, RemoveMode::KeepWorktreeAndBranch, cx));
    wait_until(cx, &f.store, |s| s.workspace_store(&parent).is_none());
    cx.run_until_parked();
    assert!(events.seen.borrow().contains(&ChildDialogEvent::ParentGone));
}

#[gpui_kit::test]
fn the_parent_name_follows_the_parent(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let f = Fixture::new(cx);
    let path = f.h.daemon.workspaces_dir().join("a.json");
    let body = |name: &str| {
        serde_json::json!({
            "name": name, "status": "active", "isGitRepo": false, "isWorktree": false,
            "appStates": {}, "metadata": {}, "createdAt": 1, "lastActivity": 1, "parentHash": ""
        })
        .to_string()
    };
    std::fs::write(&path, body("first")).unwrap();
    f.publish(&["a"]);
    wait_until(cx, &f.store, |s| matches!(s.entry("a"), Some(WorkspaceEntry::Loaded(_))));
    let model = f.dialog(cx, "a", ChildTab::Create);
    assert_eq!(model.read_with(cx, |m, cx| m.parent_name(cx)), "first");
    let notified = std::rc::Rc::new(std::cell::Cell::new(0));
    let count = notified.clone();
    let _observe = cx.update(|cx| cx.observe(&model, move |_, _| count.set(count.get() + 1)));

    std::fs::write(&path, body("second")).unwrap();
    wait_until_app(cx, &model, |m, cx| m.parent_name(cx) == "second");
    assert!(notified.get() > 0, "the dialog is told to redraw");
}

#[gpui_kit::test]
fn the_pr_tab_reads_on_each_visit_and_drops_a_reply_that_comes_too_late(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let f = Fixture::new(cx);
    let github = Arc::new(FakeGitHub::default());
    f.store.update(cx, |s, _| s.set_github_api(github.clone()));
    let (_repo, parent) = f.open_repo(cx);
    let error = |cx: &mut TestAppContext, model: &Entity<ChildDialogModel>| {
        model.read_with(cx, |m, _| m.error().map(str::to_owned))
    };

    // Left before GitHub answered: the read is dropped, so its error never shows.
    let first = github.next_reply();
    let model = f.dialog(cx, &parent, ChildTab::Pr);
    cx.run_until_parked();
    model.update(cx, |m, cx| m.switch(ChildTab::Branch, cx));
    loaded(cx, &model);
    assert!(first.send(Err(GitHubError::NoPat)).is_err(), "the read was dropped");
    cx.run_until_parked();
    assert_eq!(error(cx, &model), None);

    // Coming back reads again; a failure shows (it is not taken for an empty list).
    let second = github.next_reply();
    model.update(cx, |m, cx| m.switch(ChildTab::Pr, cx));
    assert_eq!(rows(cx, &model), Listing::Loading);
    cx.run_until_parked();
    second.send(Err(GitHubError::NoPat)).unwrap();
    wait_until(cx, &model, |m| m.error().is_some());
    assert_eq!(
        error(cx, &model).as_deref(),
        Some("Failed to load pull requests: No GitHub PAT configured. Set one in Settings > GitHub.")
    );
    assert_eq!(rows(cx, &model), Listing::Loaded(Vec::new()));

    // And again: the list, where a fork's PR cannot be picked.
    let pr = |number: u64, head_repo| PrListItem {
        number,
        title: format!("PR {number}"),
        author: String::new(),
        head_ref_name: format!("feature-{number}"),
        head_repo,
    };
    model.update(cx, |m, cx| m.switch(ChildTab::Branch, cx));
    let third = github.next_reply();
    model.update(cx, |m, cx| m.switch(ChildTab::Pr, cx));
    assert_eq!(error(cx, &model), None);
    cx.run_until_parked();
    third.send(Ok(vec![pr(7, HeadRepo::Origin), pr(8, HeadRepo::Fork)])).unwrap();
    let listed = loaded(cx, &model);
    assert_eq!(names(&listed), ["#7 PR 7", "#8 PR 8"]);
    assert_eq!(listed[0].branch.as_deref(), Some("feature-7"));
    assert_eq!(listed[1].state, RowState::Unavailable { badge: "Fork", tooltip: FORK_TOOLTIP });
    model.update(cx, |m, cx| m.pick("8", "", "", cx));
    assert_eq!(model.read_with(cx, |m, _| m.selected_key()), None);
    model.update(cx, |m, cx| m.pick("7", "", "", cx));
    assert_eq!(model.read_with(cx, |m, _| m.selected_key()).as_deref(), Some("7"));
}
