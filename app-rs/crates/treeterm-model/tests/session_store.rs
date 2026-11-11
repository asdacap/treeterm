//! SessionStore against a real in-process daemon: membership, body sync, and tabs.

use std::collections::HashSet;

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use treeterm_core::wire::workspace_file::{WorkspaceFile, WorkspaceStatus, WorktreeSettings};
use treeterm_model::session_store::{RemoveMode, SessionState, SessionStore, WorkspaceEntry};
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::workspace_store::{TabContent, Workspace, WorkspaceStore};
use treeterm_proto::treeterm::WorkspaceRef;

mod common;
use common::{Harness, harness, wait_until, wait_until_app};

fn body(name: &str, parent: Option<&str>) -> String {
    let mut json = serde_json::json!({
        "name": name, "status": "active", "isGitRepo": true, "isWorktree": parent.is_some(),
        "appStates": {}, "metadata": {}, "createdAt": 1, "lastActivity": 1, "parentHash": ""
    });
    if let Some(parent) = parent {
        json["parentId"] = parent.into();
    }
    json.to_string()
}

fn publish(h: &Harness, ids: &[&str]) {
    let path = h.daemon.root().to_string_lossy().into_owned();
    let refs = ids
        .iter()
        .map(|id| WorkspaceRef { id: (*id).into(), path: path.clone() })
        .collect();
    futures::executor::block_on(h.conn.update_session(refs, None, "test-writer"))
        .unwrap()
        .unwrap();
}

fn open_store(cx: &mut TestAppContext, h: &Harness, client: &str) -> Entity<SessionStore> {
    let conn = h.conn.clone();
    let client = client.to_owned();
    let settings = cx.new(|_| SettingsStore::in_memory(Default::default()));
    cx.new(|cx| SessionStore::new(conn, client, settings, cx))
}

fn loaded(s: &SessionStore, id: &str) -> bool {
    matches!(s.entry(id), Some(WorkspaceEntry::Loaded(_)))
}

fn ws(s: &SessionStore, id: &str) -> Entity<WorkspaceStore> {
    s.workspace_store(id).expect("workspace loaded").clone()
}

fn file_on_disk(h: &Harness, id: &str) -> serde_json::Value {
    let bytes = std::fs::read(h.daemon.workspaces_dir().join(format!("{id}.json"))).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn tabs_on_disk(h: &Harness, id: &str) -> Vec<String> {
    file_on_disk(h, id)["appStates"]
        .as_object()
        .map(|tabs| tabs.keys().cloned().collect())
        .unwrap_or_default()
}

/// Wait for a file condition (the daemon writes from tokio threads).
fn wait_for_disk(cx: &mut TestAppContext, done: impl Fn() -> bool) {
    for _ in 0..1000 {
        cx.run_until_parked();
        if done() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("disk condition not reached");
}

#[gpui_kit::test]
fn mirrors_refs_and_loads_workspace_bodies(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("root.json"), body("root", None)).unwrap();
    std::fs::write(h.daemon.workspaces_dir().join("child.json"), body("child", Some("root"))).unwrap();
    publish(&h, &["child", "root", "broken"]);

    let store = open_store(cx, &h, "client-a");
    wait_until(cx, &store, |s| {
        matches!(s.state(), SessionState::Live)
            && loaded(s, "root")
            && loaded(s, "child")
            && matches!(s.entry("broken"), Some(WorkspaceEntry::Error { .. }))
    });

    cx.update(|cx| {
        let s = store.read(cx);
        let rows: Vec<(String, usize)> = s
            .tree_rows(&HashSet::new(), cx)
            .into_iter()
            .map(|r| (r.id, r.depth))
            .collect();
        assert_eq!(rows, vec![("root".into(), 0), ("child".into(), 1), ("broken".into(), 0)]);
        match s.entry("broken") {
            Some(WorkspaceEntry::Error { error, .. }) => assert!(error.contains("missing"), "{error}"),
            _ => unreachable!(),
        }
    });
}

#[gpui_kit::test]
fn follows_ref_removal_and_body_changes(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let path = h.daemon.workspaces_dir().join("a.json");
    std::fs::write(&path, body("first", None)).unwrap();
    publish(&h, &["a", "b"]);

    let store = open_store(cx, &h, "client-a");
    wait_until(cx, &store, |s| loaded(s, "a"));

    std::fs::write(&path, body("second", None)).unwrap();
    wait_until_app(cx, &store, |s, cx| ws(s, "a").read(cx).file().name == "second");

    publish(&h, &["a"]);
    wait_until(cx, &store, |s| s.entry("b").is_none());
    wait_until_app(cx, &store, |s, cx| s.tree_rows(&HashSet::new(), cx).len() == 1);
}

#[gpui_kit::test]
fn adding_a_tab_creates_its_pty_and_persists_it(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("a.json"), body("a", None)).unwrap();
    publish(&h, &["a"]);
    let store = open_store(cx, &h, "client-a");
    wait_until(cx, &store, |s| loaded(s, "a"));

    let workspace = store.read_with(cx, |s, _| ws(s, "a"));
    let tab = workspace.update(cx, |w, cx| w.add_tab("terminal", cx)).unwrap();
    // The PTY id lands in the tab's state, and from there on disk.
    wait_for_disk(cx, || file_on_disk(&h, "a")["appStates"][&tab]["state"]["ptyId"].is_string());
    let disk = file_on_disk(&h, "a");
    assert_eq!(disk["activeTabId"], tab.as_str());
    assert_eq!(disk["appStates"][&tab]["title"], "Terminal 1");
    // Our own write's echo is not re-applied: the terminal stays attached.
    wait_until_app(cx, &workspace, |w, _| matches!(w.tab_content(&tab), Some(TabContent::Terminal(_))));

    workspace.update(cx, |w, cx| w.remove_tab(&tab, cx));
    wait_for_disk(cx, || tabs_on_disk(&h, "a").is_empty());
    assert!(file_on_disk(&h, "a").get("activeTabId").is_none(), "no null for an absent optional");
}

#[gpui_kit::test]
fn a_tab_whose_program_exits_is_closed(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("a.json"), body("a", None)).unwrap();
    publish(&h, &["a"]);
    let store = open_store(cx, &h, "client-a");
    wait_until(cx, &store, |s| loaded(s, "a"));
    let workspace = store.read_with(cx, |s, _| ws(s, "a"));
    let tab = workspace.update(cx, |w, cx| w.add_tab("terminal", cx)).unwrap();
    wait_until_app(cx, &workspace, |w, _| matches!(w.tab_content(&tab), Some(TabContent::Terminal(_))));
    let Some(TabContent::Terminal(terminal)) = workspace.read_with(cx, |w, _| w.tab_content(&tab)) else {
        unreachable!()
    };
    // Live stream is enough: the PTY buffers input until the (user's own, possibly slow)
    // shell reads it.
    wait_until(cx, &terminal, |t| *t.overlay() == treeterm_model::terminal::Overlay::Hidden);
    // Past the "exited immediately" window, so the exit closes the tab.
    cx.executor().advance_clock(std::time::Duration::from_secs(2));
    terminal.update(cx, |t, cx| {
        t.set_focused(true, cx);
        t.write_input(b"exit 0\r");
    });
    wait_for_disk(cx, || tabs_on_disk(&h, "a").is_empty());
}

#[gpui_kit::test]
fn concurrent_writers_both_keep_their_tabs(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("a.json"), body("a", None)).unwrap();
    publish(&h, &["a"]);
    // Two windows on one session.
    let first = open_store(cx, &h, "client-a");
    let second = open_store(cx, &h, "client-b");
    wait_until(cx, &first, |s| loaded(s, "a"));
    wait_until(cx, &second, |s| loaded(s, "a"));

    let wa = first.read_with(cx, |s, _| ws(s, "a"));
    let wb = second.read_with(cx, |s, _| ws(s, "a"));
    // Metadata edits from both, in the same tick: one write loses the CAS race.
    wa.update(cx, |w, cx| w.update_metadata("fromA", "1", cx));
    wb.update(cx, |w, cx| w.update_metadata("fromB", "1", cx));
    wait_for_disk(cx, || {
        let disk = file_on_disk(&h, "a");
        disk["metadata"]["fromA"] == "1" || disk["metadata"]["fromB"] == "1"
    });
    // The loser adopted the winner; its own edit was replaced, as in Electron. A later
    // edit from either side lands on top of the merged state.
    wb.update(cx, |w, cx| w.update_metadata("later", "1", cx));
    wait_for_disk(cx, || file_on_disk(&h, "a")["metadata"]["later"] == "1");
    wait_until_app(cx, &first, |s, cx| {
        ws(s, "a").read(cx).file().metadata.get("later").map(String::as_str) == Some("1")
    });
}

#[gpui_kit::test]
fn an_added_workspace_is_written_then_published(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish(&h, &[]);
    let first = open_store(cx, &h, "client-a");
    let second = open_store(cx, &h, "client-b");
    wait_until(cx, &first, |s| matches!(s.state(), SessionState::Live));

    let path = h.daemon.root().to_string_lossy().into_owned();
    first.update(cx, |s, cx| {
        let workspace = Workspace {
            id: "ws-new".into(),
            path: path.clone(),
            file: WorkspaceFile {
                name: "new".into(),
                parent_id: None,
                status: WorkspaceStatus::Active,
                is_git_repo: false,
                git_branch: None,
                git_root_path: None,
                is_worktree: false,
                is_detached: Some(false),
                app_states: Default::default(),
                active_tab_id: None,
                metadata: [("sortOrder".to_owned(), "0".to_owned())].into(),
                settings: WorktreeSettings::default(),
                favourite_paths: Vec::new(),
                created_at: 1,
                last_activity: 1,
                extra: Default::default(),
            },
        };
        drop(s.add_workspace(workspace, cx));
    });
    // The other window learns of it through the session and loads its body.
    wait_until_app(cx, &second, |s, cx| {
        s.workspace_store("ws-new").is_some_and(|w| w.read(cx).file().name == "new")
    });
    assert_eq!(file_on_disk(&h, "ws-new")["parentHash"], "");
    first.read_with(cx, |s, _| assert!(s.version() >= 2));
}

#[gpui_kit::test]
fn removing_a_workspace_takes_its_children_and_files_with_it(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("root.json"), body("root", None)).unwrap();
    std::fs::write(h.daemon.workspaces_dir().join("child.json"), body("child", Some("root"))).unwrap();
    std::fs::write(h.daemon.workspaces_dir().join("other.json"), body("other", None)).unwrap();
    publish(&h, &["root", "child", "other"]);
    let store = open_store(cx, &h, "client-a");
    let watcher = open_store(cx, &h, "client-b");
    wait_until(cx, &store, |s| loaded(s, "root") && loaded(s, "child") && loaded(s, "other"));
    wait_until(cx, &watcher, |s| loaded(s, "root"));

    store.update(cx, |s, cx| s.remove_workspace_keep_both("root", cx));
    wait_until(cx, &store, |s| s.entry("root").is_none() && s.entry("child").is_none());
    // Another window sees the membership change.
    wait_until(cx, &watcher, |s| s.entry("root").is_none() && s.entry("child").is_none() && loaded(s, "other"));
    wait_for_disk(cx, || {
        !h.daemon.workspaces_dir().join("root.json").exists() && !h.daemon.workspaces_dir().join("child.json").exists()
    });
    assert!(h.daemon.workspaces_dir().join("other.json").exists());
    // The lock was released.
    store.read_with(cx, |s, _| assert!(s.version() >= 3));
}

#[gpui_kit::test]
fn settings_changes_reach_live_terminals(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("a.json"), body("a", None)).unwrap();
    publish(&h, &["a"]);
    let settings = cx.new(|_| SettingsStore::in_memory(Default::default()));
    let conn = h.conn.clone();
    let store = cx.new(|cx| SessionStore::new(conn, "client-a".into(), settings.clone(), cx));
    wait_until(cx, &store, |s| loaded(s, "a"));
    let workspace = store.read_with(cx, |s, _| ws(s, "a"));
    let tab = workspace.update(cx, |w, cx| w.add_tab("terminal", cx)).unwrap();
    wait_until_app(cx, &workspace, |w, _| matches!(w.tab_content(&tab), Some(TabContent::Terminal(_))));

    settings.update(cx, |s, cx| s.update(|settings| settings.terminal.font_size = 17.0, cx));
    cx.run_until_parked();
    let Some(TabContent::Terminal(terminal)) = workspace.read_with(cx, |w, _| w.tab_content(&tab)) else {
        unreachable!()
    };
    terminal.read_with(cx, |t, _| assert_eq!(t.options().settings.font_size, 17.0));
}

#[gpui_kit::test]
fn opening_a_directory_adds_a_root_workspace_with_git_info_and_a_terminal(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish(&h, &[]);
    let repo = h.daemon.root().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git").args(args).current_dir(&repo).status().unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "init"]);

    let first = open_store(cx, &h, "client-a");
    let second = open_store(cx, &h, "client-b");
    wait_until(cx, &first, |s| matches!(s.state(), SessionState::Live));
    let path = repo.to_string_lossy().into_owned();
    let id = first.update(cx, |s, cx| s.open_workspace(path.clone(), cx));
    assert!(first.read_with(cx, |s, _| matches!(s.entry(&id), Some(WorkspaceEntry::Loading { .. }))));

    wait_until_app(cx, &second, |s, cx| s.workspace_store(&id).is_some_and(|w| w.read(cx).file().name == "repo"));
    let disk = file_on_disk(&h, &id);
    assert_eq!(disk["isGitRepo"], true);
    assert_eq!(disk["gitBranch"], "main");
    assert_eq!(disk["metadata"]["sortOrder"], "0");
    let tab = disk["activeTabId"].as_str().unwrap().to_owned();
    assert_eq!(disk["appStates"][&tab]["applicationId"], "terminal");
    wait_for_disk(cx, || file_on_disk(&h, &id)["appStates"][&tab]["state"]["ptyId"].is_string());
}

#[gpui_kit::test]
fn a_child_workspace_is_a_new_worktree_from_the_parent_branch(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish(&h, &[]);
    // Worktrees go under the real ~/.treeterm/worktrees/<repo name>/: use a unique repo
    // name and remove what the test creates there.
    let repo_name = format!("tt-test-{}", uuid::Uuid::new_v4().simple());
    let repo = h.daemon.root().join(&repo_name);
    let worktrees_home = dirs::home_dir().unwrap().join(".treeterm/worktrees").join(&repo_name);
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            if self.0.exists() && std::fs::remove_dir_all(&self.0).is_err() {
                eprintln!("could not remove {}", self.0.display());
            }
        }
    }
    let _cleanup = Cleanup(worktrees_home.clone());
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git").args(args).current_dir(&repo).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "init"]);

    let store = open_store(cx, &h, "client-a");
    wait_until(cx, &store, |s| matches!(s.state(), SessionState::Live));
    let path = repo.to_string_lossy().into_owned();
    let root = store.update(cx, |s, cx| s.open_workspace(path, cx));
    wait_until(cx, &store, |s| loaded(s, &root));

    let child = store
        .update(cx, |s, cx| s.add_child_workspace(&root, "feature-x".into(), false, Some("try it".into()), cx))
        .unwrap();
    wait_until(cx, &store, |s| loaded(s, &child) || matches!(s.entry(&child), Some(WorkspaceEntry::Error { .. })));
    if let Some(WorkspaceEntry::Error { error, .. }) = store.read_with(cx, |s, _| match s.entry(&child) {
        Some(WorkspaceEntry::Error { error, name }) => Some(WorkspaceEntry::Error { error: error.clone(), name: name.clone() }),
        _ => None,
    }) {
        panic!("child failed: {error}");
    }
    wait_for_disk(cx, || h.daemon.workspaces_dir().join(format!("{child}.json")).exists());
    let disk = file_on_disk(&h, &child);
    assert_eq!(disk["parentId"], root.as_str());
    assert_eq!(disk["isWorktree"], true);
    assert_eq!(disk["metadata"]["description"], "try it");
    let worktrees = git(&["worktree", "list"]);
    assert!(worktrees.lines().count() >= 2, "{worktrees}");
    assert!(worktrees_home.join("feature-x").exists());
    // The lock was released after publishing.
    wait_until(cx, &store, |s| s.version() >= 5);
    cx.update(|cx| {
        let rows: Vec<String> = store.read(cx).tree_rows(&HashSet::new(), cx).into_iter().map(|r| r.id).collect();
        assert_eq!(rows, vec![root.clone(), child.clone()]);
    });

    // Removing it deletes the worktree and its branch.
    store.update(cx, |s, cx| s.remove_workspace(&child, RemoveMode::DeleteWorktreeAndBranch, cx));
    wait_until(cx, &store, |s| s.entry(&child).is_none());
    wait_for_disk(cx, || !worktrees_home.join("feature-x").exists());
    assert!(!git(&["branch", "--list", "feature-x"]).contains("feature-x"));
    assert!(loaded_now(&store, cx, &root));
}

fn loaded_now(store: &Entity<SessionStore>, cx: &mut TestAppContext, id: &str) -> bool {
    store.read_with(cx, |s, _| loaded(s, id))
}

#[gpui_kit::test]
fn merging_a_child_brings_its_commits_into_the_parent_and_removes_it(cx: &mut TestAppContext) {
    use treeterm_model::session_store::AfterMerge;
    cx.executor().allow_parking();
    let h = harness();
    publish(&h, &[]);
    let repo_name = format!("tt-test-{}", uuid::Uuid::new_v4().simple());
    let repo = h.daemon.root().join(&repo_name);
    let worktrees_home = dirs::home_dir().unwrap().join(".treeterm/worktrees").join(&repo_name);
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            if self.0.exists() && std::fs::remove_dir_all(&self.0).is_err() {
                eprintln!("could not remove {}", self.0.display());
            }
        }
    }
    let _cleanup = Cleanup(worktrees_home.clone());
    std::fs::create_dir_all(&repo).unwrap();
    let git_in = |dir: &std::path::Path, args: &[&str]| {
        let out = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    git_in(&repo, &["init", "-q", "-b", "main"]);
    git_in(&repo, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "init"]);

    let store = open_store(cx, &h, "client-a");
    wait_until(cx, &store, |s| matches!(s.state(), SessionState::Live));
    let root = store.update(cx, |s, cx| s.open_workspace(repo.to_string_lossy().into_owned(), cx));
    wait_until(cx, &store, |s| loaded(s, &root));
    let child = store.update(cx, |s, cx| s.add_child_workspace(&root, "feat-merge".into(), false, None, cx)).unwrap();
    wait_until(cx, &store, |s| loaded(s, &child));
    let child_dir = worktrees_home.join("feat-merge");
    wait_for_disk(cx, || child_dir.exists());

    // Uncommitted work in the child is auto-committed before a merge that removes it.
    std::fs::write(child_dir.join("feature.txt"), "done\n").unwrap();
    std::process::Command::new("git").args(["config", "user.email", "t@t"]).current_dir(&repo).status().unwrap();
    std::process::Command::new("git").args(["config", "user.name", "t"]).current_dir(&repo).status().unwrap();
    store.update(cx, |s, cx| s.merge_workspace(&child, false, AfterMerge::Remove, cx));
    wait_until(cx, &store, |s| s.entry(&child).is_none() || matches!(s.entry(&child), Some(WorkspaceEntry::OperationError { .. })));
    if let Some(error) = store.read_with(cx, |s, _| match s.entry(&child) {
        Some(WorkspaceEntry::OperationError { error, .. }) => Some(error.clone()),
        _ => None,
    }) {
        panic!("merge failed: {error}");
    }
    assert_eq!(std::fs::read_to_string(repo.join("feature.txt")).unwrap(), "done\n");
    assert!(git_in(&repo, &["log", "--oneline"]).contains("WIP: Auto-commit before merge from feat-merge"));
    wait_for_disk(cx, || !child_dir.exists());
    assert!(!git_in(&repo, &["branch", "--list", "feat-merge"]).contains("feat-merge"));
}

/// A temp git repo with one commit, plus cleanup of what tests create for it under the
/// real `~/.treeterm/worktrees/<repo name>/`.
struct TempRepo {
    path: std::path::PathBuf,
    worktrees_home: std::path::PathBuf,
}

impl TempRepo {
    fn new(h: &Harness) -> Self {
        let name = format!("tt-test-{}", uuid::Uuid::new_v4().simple());
        let path = h.daemon.root().join(&name);
        std::fs::create_dir_all(&path).unwrap();
        let repo = Self {
            worktrees_home: dirs::home_dir().unwrap().join(".treeterm/worktrees").join(&name),
            path,
        };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.email", "t@t"]);
        repo.git(&["config", "user.name", "t"]);
        repo.git(&["commit", "-q", "--allow-empty", "-m", "init"]);
        repo
    }

    fn git(&self, args: &[&str]) -> String {
        let out = std::process::Command::new("git").args(args).current_dir(&self.path).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        if self.worktrees_home.exists() && std::fs::remove_dir_all(&self.worktrees_home).is_err() {
            eprintln!("could not remove {}", self.worktrees_home.display());
        }
    }
}

fn open_repo_workspace(cx: &mut TestAppContext, store: &Entity<SessionStore>, repo: &TempRepo) -> String {
    wait_until(cx, store, |s| matches!(s.state(), SessionState::Live));
    let path = repo.path.to_string_lossy().into_owned();
    let id = store.update(cx, |s, cx| s.open_workspace(path, cx));
    wait_until(cx, store, |s| loaded(s, &id));
    id
}

fn wait_child(cx: &mut TestAppContext, store: &Entity<SessionStore>, id: &str) {
    wait_until(cx, store, |s| loaded(s, id) || matches!(s.entry(id), Some(WorkspaceEntry::Error { .. })));
    let error = store.read_with(cx, |s, _| match s.entry(id) {
        Some(WorkspaceEntry::Error { error, .. }) => Some(error.clone()),
        _ => None,
    });
    if let Some(error) = error {
        panic!("child failed: {error}");
    }
}

#[gpui_kit::test]
fn a_child_can_check_out_an_existing_branch(cx: &mut TestAppContext) {
    use treeterm_model::session_store::{ChildOptions, ChildSource};
    cx.executor().allow_parking();
    let h = harness();
    publish(&h, &[]);
    let repo = TempRepo::new(&h);
    repo.git(&["branch", "team/feature-y"]);
    let store = open_store(cx, &h, "client-a");
    let root = open_repo_workspace(cx, &store, &repo);

    let child = store
        .update(cx, |s, cx| {
            s.create_child(
                &root,
                ChildSource::ExistingBranch { branch: "team/feature-y".into() },
                ChildOptions::default(),
                cx,
            )
        })
        .unwrap();
    wait_child(cx, &store, &child);
    wait_for_disk(cx, || h.daemon.workspaces_dir().join(format!("{child}.json")).exists());
    let disk = file_on_disk(&h, &child);
    assert_eq!(disk["name"], "feature-y");
    assert_eq!(disk["gitBranch"], "team/feature-y");
    assert_eq!(disk["metadata"]["branchIsUserDefined"], "true");
}

#[gpui_kit::test]
fn an_existing_worktree_can_be_adopted_once(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish(&h, &[]);
    let repo = TempRepo::new(&h);
    let outside = h.daemon.root().join("outside-wt");
    repo.git(&["worktree", "add", "-q", "-b", "side", outside.to_str().unwrap()]);
    let store = open_store(cx, &h, "client-a");
    let root = open_repo_workspace(cx, &store, &repo);

    let worktree = treeterm_core::git::WorktreeInfo {
        path: outside.to_string_lossy().into_owned(),
        branch: "side".into(),
    };
    let child = store
        .update(cx, |s, cx| s.adopt_existing_worktree(&root, worktree.clone(), None, cx))
        .unwrap();
    wait_child(cx, &store, &child);
    let again = store.update(cx, |s, cx| s.adopt_existing_worktree(&root, worktree, None, cx));
    assert_eq!(again.unwrap_err(), "This worktree is already open");
    wait_for_disk(cx, || h.daemon.workspaces_dir().join(format!("{child}.json")).exists());
    assert_eq!(file_on_disk(&h, &child)["gitBranch"], "side");
}

#[gpui_kit::test]
fn an_editor_tab_loads_edits_and_saves_a_file(cx: &mut TestAppContext) {
    use treeterm_model::text_editor::EditorDoc;
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("a.json"), body("a", None)).unwrap();
    publish(&h, &["a"]);
    let file = h.daemon.root().join("notes.txt");
    std::fs::write(&file, "hello\n").unwrap();
    let store = open_store(cx, &h, "client-a");
    wait_until(cx, &store, |s| loaded(s, "a"));
    let workspace = store.read_with(cx, |s, _| ws(s, "a"));

    let path = file.to_string_lossy().into_owned();
    let tab = workspace.update(cx, |w, cx| w.open_in_editor(&path, cx)).unwrap();
    let Some(TabContent::Editor(editor)) = workspace.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not an editor tab");
    };
    wait_until(cx, &editor, |e| matches!(e.doc(), EditorDoc::Ready { .. }));
    editor.update(cx, |e, cx| e.set_content("hello world\n".into(), cx));
    assert!(editor.read_with(cx, |e, _| e.is_dirty()));
    editor.update(cx, |e, cx| e.save(cx));
    wait_until(cx, &editor, |e| !e.is_dirty());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello world\n");
    wait_for_disk(cx, || {
        let state = &file_on_disk(&h, "a")["appStates"][&tab]["state"];
        state["status"] == "ready" && state["isDirty"] == false && state["currentContent"] == "hello world\n"
    });
    assert_eq!(file_on_disk(&h, "a")["appStates"][&tab]["title"], "notes.txt");
}

#[gpui_kit::test]
fn the_review_tab_stages_and_commits_uncommitted_changes(cx: &mut TestAppContext) {
    use treeterm_core::git::Staging;
    use treeterm_model::review::{ReviewContent, StageTarget};
    cx.executor().allow_parking();
    let h = harness();
    publish(&h, &[]);
    let repo = TempRepo::new(&h);
    std::fs::write(repo.path.join("new.txt"), "fresh\n").unwrap();
    let store = open_store(cx, &h, "client-a");
    let root = open_repo_workspace(cx, &store, &repo);
    let workspace = store.read_with(cx, |s, _| ws(s, &root));

    let tab = workspace
        .update(cx, |w, cx| w.add_tab_with("review", serde_json::json!({ "viewMode": "uncommitted" }), cx))
        .unwrap();
    let Some(TabContent::Review(review)) = workspace.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not a review tab");
    };
    let staged_files = |content: &ReviewContent, side: Staging| match content {
        ReviewContent::Uncommitted(u) => {
            u.changes.files.iter().filter(|f| f.entry.staged == side).map(|f| f.entry.path.clone()).collect()
        }
        _ => Vec::new(),
    };
    wait_until(cx, &review, |r| staged_files(r.content(), Staging::Unstaged) == vec!["new.txt".to_owned()]);
    let ReviewContent::Uncommitted(uncommitted) = review.read_with(cx, |r, _| r.content().clone()) else {
        unreachable!()
    };
    assert!(uncommitted.patch.contains("+fresh"), "{}", uncommitted.patch);

    review.update(cx, |r, cx| r.stage(StageTarget::File("new.txt".into()), cx));
    wait_until(cx, &review, |r| staged_files(r.content(), Staging::Staged) == vec!["new.txt".to_owned()]);

    review.update(cx, |r, cx| r.commit("   ", cx));
    assert_eq!(
        review.read_with(cx, |r, _| r.commit_op().clone()),
        treeterm_model::review::GitOp::Failed("Commit message is required".into())
    );
    review.update(cx, |r, cx| r.commit("add new file", cx));
    wait_until(cx, &review, |r| match r.content() {
        ReviewContent::Uncommitted(u) => u.changes.files.is_empty(),
        _ => false,
    });
    assert!(repo.git(&["log", "--oneline"]).contains("add new file"));
}
