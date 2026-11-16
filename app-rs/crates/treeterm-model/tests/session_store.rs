//! SessionStore against a real in-process daemon: membership, body sync, and tabs.

use std::collections::HashSet;

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use treeterm_core::wire::workspace_file::{WorkspaceFile, WorkspaceStatus, WorktreeSettings};
use treeterm_model::review::{Processing, ReviewEvent};
use treeterm_model::session_store::{RemoveMode, SessionState, SessionStore, WorkspaceEntry};
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::workspace_store::{MergeState, TabContent, Workspace, WorkspaceStore};
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
    cx.new(|cx| SessionStore::new(conn, client, settings, None, cx))
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
fn the_debug_json_shows_each_entry_with_its_status(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("root.json"), body("root", None)).unwrap();
    publish(&h, &["root", "broken"]);

    let store = open_store(cx, &h, "client-a");
    wait_until(cx, &store, |s| {
        loaded(s, "root") && matches!(s.entry("broken"), Some(WorkspaceEntry::Error { .. }))
    });

    let json = cx.update(|cx| store.read(cx).debug_json(cx));
    assert!(json["sessionId"].as_str().is_some_and(|id| !id.is_empty()), "{json}");
    assert_eq!(json["workspaces"]["root"]["status"], "loaded");
    assert_eq!(json["workspaces"]["root"]["data"]["id"], "root");
    assert_eq!(json["workspaces"]["root"]["data"]["name"], "root");
    assert_eq!(json["workspaces"]["broken"]["status"], "error");
    assert!(json["workspaces"]["broken"]["error"].is_string());
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
        t.write_input(b"exit 0\r", cx);
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
    let store = cx.new(|cx| SessionStore::new(conn, "client-a".into(), settings.clone(), None, cx));
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

#[gpui_kit::test]
fn a_child_whose_worktree_git_has_forgotten_still_closes(cx: &mut TestAppContext) {
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
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git").args(args).current_dir(&repo).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "init"]);

    let store = open_store(cx, &h, "client-a");
    wait_until(cx, &store, |s| matches!(s.state(), SessionState::Live));
    let root = store.update(cx, |s, cx| s.open_workspace(repo.to_string_lossy().into_owned(), cx));
    wait_until(cx, &store, |s| loaded(s, &root));
    let child = store.update(cx, |s, cx| s.add_child_workspace(&root, "gone".into(), false, None, cx)).unwrap();
    wait_until(cx, &store, |s| loaded(s, &child));
    let child_dir = worktrees_home.join("gone");
    wait_for_disk(cx, || child_dir.exists());
    // Wait for the lock to be released after publishing, so removal is not refused.
    wait_until(cx, &store, |s| s.version() >= 5);

    // The checkout is deleted by hand and git forgets it: `git worktree remove` now fails
    // with "is not a working tree".
    std::fs::remove_dir_all(&child_dir).unwrap();
    git(&["worktree", "prune"]);
    assert_eq!(git(&["worktree", "list", "--porcelain"]).matches("worktree ").count(), 1);

    store.update(cx, |s, cx| s.remove_workspace(&child, RemoveMode::DeleteWorktreeAndBranch, cx));
    wait_until(cx, &store, |s| s.entry(&child).is_none());
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
    // From the child's Review tab, which names its parent for Electron (`parentWorkspaceId`).
    let child_ws = store.read_with(cx, |s, _| ws(s, &child));
    let tab = child_ws.update(cx, |w, cx| w.add_tab("review", cx)).unwrap();
    assert_eq!(child_ws.read_with(cx, |w, _| w.file().app_states[&tab].state["parentWorkspaceId"].clone()), serde_json::json!(root));
    let Some(TabContent::Review(review)) = child_ws.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not a review tab");
    };
    // As a user would: once the tab has loaded. Its git reads in the worktree would race
    // the auto-commit for the index lock.
    wait_until(cx, &review, |r| !r.is_refreshing());
    review.update(cx, |r, cx| r.merge(false, AfterMerge::Remove, cx));
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
    // git's output went to the merged workspace (its Review tab), and went with it.
    wait_until(cx, &child_ws, |w| w.merge_output().is_some_and(|o| o.state == MergeState::Succeeded));
    let output = child_ws.read_with(cx, |w, _| w.merge_output().cloned()).unwrap();
    assert_eq!(output.title(), "Merge feat-merge and remove worktree");
    assert!(output.output.contains("$ git merge feat-merge"), "{}", output.output);
    let root_ws = store.read_with(cx, |s, _| ws(s, &root));
    assert!(root_ws.read_with(cx, |w, _| w.merge_output().is_none()), "the parent shows no merge output");
}

#[gpui_kit::test]
fn a_failed_merge_shows_on_the_merged_workspace_only(cx: &mut TestAppContext) {
    use treeterm_model::session_store::AfterMerge;
    cx.executor().allow_parking();
    let h = harness();
    publish(&h, &[]);
    let repo = TempRepo::new(&h);
    let store = open_store(cx, &h, "client-a");
    let root = open_repo_workspace(cx, &store, &repo);
    let child = store.update(cx, |s, cx| s.add_child_workspace(&root, "feat-fail".into(), false, None, cx)).unwrap();
    wait_child(cx, &store, &child);
    // The parent's uncommitted work refuses the merge.
    std::fs::write(repo.path.join("dirty.txt"), "x\n").unwrap();
    let child_ws = store.read_with(cx, |s, _| ws(s, &child));
    let tab = child_ws.update(cx, |w, cx| w.add_tab("review", cx)).unwrap();
    let Some(TabContent::Review(review)) = child_ws.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not a review tab");
    };
    review.update(cx, |r, cx| r.merge(true, AfterMerge::Keep, cx));
    assert_eq!(review.read_with(cx, |r, cx| r.processing(cx)), Processing::Merging);
    // A running merge's output cannot be hidden.
    review.update(cx, |r, cx| r.dismiss_merge_output(cx));
    assert!(review.read_with(cx, |r, cx| r.merge_output(cx).is_some()));

    wait_until(cx, &child_ws, |w| w.merge_output().is_some_and(|o| o.state == MergeState::Failed));
    let output = review.read_with(cx, |r, cx| r.merge_output(cx)).unwrap();
    assert_eq!(output.title(), "Squash merge feat-fail");
    assert!(
        output.output.ends_with("\nMerge failed: Parent workspace has uncommitted changes. Commit or stash them before merging.\n"),
        "{}",
        output.output
    );
    assert_eq!(review.read_with(cx, |r, cx| r.processing(cx)), Processing::Idle);
    assert!(store.read_with(cx, |s, _| matches!(s.entry(&child), Some(WorkspaceEntry::OperationError { .. }))));
    let root_ws = store.read_with(cx, |s, _| ws(s, &root));
    assert!(root_ws.read_with(cx, |w, _| w.merge_output().is_none()), "the parent shows no merge output");
    review.update(cx, |r, cx| r.dismiss_merge_output(cx));
    assert!(child_ws.read_with(cx, |w, _| w.merge_output().is_none()));
}

#[gpui_kit::test]
fn merge_and_keep_reloads_the_review(cx: &mut TestAppContext) {
    use treeterm_model::session_store::AfterMerge;
    cx.executor().allow_parking();
    let h = harness();
    publish(&h, &[]);
    let repo = TempRepo::new(&h);
    let store = open_store(cx, &h, "client-a");
    let root = open_repo_workspace(cx, &store, &repo);
    let child = store.update(cx, |s, cx| s.add_child_workspace(&root, "feat-keep".into(), false, None, cx)).unwrap();
    wait_child(cx, &store, &child);
    let child_ws = store.read_with(cx, |s, _| ws(s, &child));
    let child_dir = std::path::PathBuf::from(child_ws.read_with(cx, |w, _| w.workspace().path.clone()));
    std::fs::write(child_dir.join("kept.txt"), "kept\n").unwrap();
    for args in [&["add", "kept.txt"][..], &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "-m", "kept"]] {
        let out = std::process::Command::new("git").args(args).current_dir(&child_dir).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }
    let tab = child_ws.update(cx, |w, cx| w.add_tab("review", cx)).unwrap();
    let Some(TabContent::Review(review)) = child_ws.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not a review tab");
    };
    wait_until(cx, &review, |r| r.overview().committed == 1);

    review.update(cx, |r, cx| r.merge(false, AfterMerge::Keep, cx));
    wait_until(cx, &child_ws, |w| w.merge_output().is_some_and(|o| o.state != MergeState::Running));
    let output = child_ws.read_with(cx, |w, _| w.merge_output().cloned()).unwrap();
    assert_eq!(output.state, MergeState::Succeeded, "{}", output.output);
    assert_eq!(output.title(), "Merge feat-keep");
    assert_eq!(std::fs::read_to_string(repo.path.join("kept.txt")).unwrap(), "kept\n");
    // The branch is in its base now: nothing left to review.
    wait_until(cx, &review, |r| r.overview().committed == 0);
    assert!(store.read_with(cx, |s, _| loaded(s, &child)), "the workspace is kept");
}

fn git_in(dir: &std::path::Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Write `text` to `file` in `dir` and commit it.
fn commit_file(dir: &std::path::Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).unwrap();
    git_in(dir, &["add", file]);
    git_in(dir, &["commit", "-q", "-m", file]);
}

/// A child of `repo`'s workspace whose branch and the parent's both changed `a.txt`'s line
/// since it was made, with a Review tab open on it: `(child id, child, its review)`.
fn conflicting_child(
    cx: &mut TestAppContext,
    store: &Entity<SessionStore>,
    repo: &TempRepo,
    name: &str,
) -> (String, Entity<WorkspaceStore>, Entity<treeterm_model::review::ReviewModel>) {
    commit_file(&repo.path, "a.txt", "base\n");
    let root = open_repo_workspace(cx, store, repo);
    let child = store.update(cx, |s, cx| s.add_child_workspace(&root, name.into(), false, None, cx)).unwrap();
    wait_child(cx, store, &child);
    let child_ws = store.read_with(cx, |s, _| ws(s, &child));
    let child_dir = std::path::PathBuf::from(child_ws.read_with(cx, |w, _| w.workspace().path.clone()));
    commit_file(&child_dir, "a.txt", "child\n");
    commit_file(&repo.path, "a.txt", "root\n");
    let tab = child_ws.update(cx, |w, cx| w.add_tab("review", cx)).unwrap();
    let Some(TabContent::Review(review)) = child_ws.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not a review tab");
    };
    (child, child_ws, review)
}

#[gpui_kit::test]
fn the_last_conflicts_stay_while_rechecking(cx: &mut TestAppContext) {
    use treeterm_model::review::{ConflictCheck, Conflicts};
    cx.executor().allow_parking();
    let h = harness();
    publish(&h, &[]);
    let repo = TempRepo::new(&h);
    let store = open_store(cx, &h, "client-a");
    let (_, _, review) = conflicting_child(cx, &store, &repo, "feat-stale");
    wait_until(cx, &review, |r| r.conflicts().has_conflicts() && r.conflicts().check == ConflictCheck::Idle);
    let conflicts = review.read_with(cx, |r, _| r.conflicts().clone());
    assert_eq!(conflicts.last.conflicted_files, vec!["a.txt".to_owned()]);
    assert!(conflicts.parent_has_new_commits);

    // Every state shown during a re-check still has the conflicts and the parent's commits
    // (the banner, "(has conflicts)" and the prompt buttons stay).
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Conflicts>::new()));
    let record = seen.clone();
    let _observe = cx.update(|cx| cx.observe(&review, move |review, cx| record.borrow_mut().push(review.read(cx).conflicts().clone())));
    review.update(cx, |r, cx| r.refresh(cx));
    wait_until(cx, &review, |_| {
        let seen = seen.borrow();
        let checking = seen.iter().position(|c| c.check == ConflictCheck::Checking);
        checking.is_some_and(|at| seen[at..].iter().any(|c| c.check == ConflictCheck::Idle))
    });
    for state in seen.borrow().iter() {
        assert!(state.has_conflicts() && state.parent_has_new_commits, "{state:?}");
    }
}

type Events = std::rc::Rc<std::cell::RefCell<Vec<ReviewEvent>>>;

/// Every event `review` emits from now on.
fn record_events(cx: &mut TestAppContext, review: &Entity<treeterm_model::review::ReviewModel>) -> (Events, gpui_kit::Subscription) {
    let events = Events::default();
    let seen = events.clone();
    let subscription = cx.update(|cx| cx.subscribe(review, move |_, event: &ReviewEvent, _| seen.borrow_mut().push(event.clone())));
    (events, subscription)
}

fn alerts(events: &Events) -> Vec<String> {
    events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            ReviewEvent::Alert(message) => Some(message.clone()),
            _ => None,
        })
        .collect()
}

/// A Review tab on `workspace` with `state`.
fn review_tab(cx: &mut TestAppContext, workspace: &Entity<WorkspaceStore>, state: serde_json::Value) -> Entity<treeterm_model::review::ReviewModel> {
    let tab = workspace.update(cx, |w, cx| w.add_tab_with("review", state, cx)).unwrap();
    let Some(TabContent::Review(review)) = workspace.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not a review tab");
    };
    review
}

/// A detached worktree body under `parent` (`isWorktree` as given).
fn detached_body(name: &str, worktree: bool, parent: &str) -> String {
    serde_json::json!({
        "name": name, "status": "active", "isGitRepo": true, "isWorktree": worktree, "isDetached": true,
        "parentId": parent, "appStates": {}, "metadata": {}, "createdAt": 1, "lastActivity": 1, "parentHash": ""
    })
    .to_string()
}

#[gpui_kit::test]
fn close_and_clean_is_refused_with_an_alert_unless_the_parent_is_a_loaded_repo(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let write = |id: &str, body: String| std::fs::write(h.daemon.workspaces_dir().join(format!("{id}.json")), body).unwrap();
    // "root" is not a git repo (no gitRootPath); "orphan"'s parent is not in the session;
    // "plain" is not a worktree.
    write("root", body("root", None));
    write("nogit", detached_body("nogit", true, "root"));
    write("orphan", detached_body("orphan", true, "missing"));
    write("plain", detached_body("plain", false, "root"));
    publish(&h, &["root", "nogit", "orphan", "plain"]);
    let store = open_store(cx, &h, "client-a");
    wait_until(cx, &store, |s| ["root", "nogit", "orphan", "plain"].iter().all(|id| loaded(s, id)));

    let refusals = [
        ("nogit", "root", "Parent workspace not found or not a git repo"),
        ("orphan", "missing", "Parent workspace not found or not a git repo"),
        ("plain", "root", "Not a worktree workspace"),
    ];
    for (id, parent, error) in refusals {
        let workspace = store.read_with(cx, |s, _| ws(s, id));
        // The tab names its parent, so Close and Clean shows.
        let review = review_tab(cx, &workspace, serde_json::json!({ "parentWorkspaceId": parent }));
        let header = review.read_with(cx, |r, cx| r.header(cx));
        assert!(header.has_parent && header.detached, "{header:?}");
        let (events, _events) = record_events(cx, &review);
        review.update(cx, |r, cx| r.close_and_clean(cx));
        cx.run_until_parked();
        assert_eq!(alerts(&events), vec![format!("Close failed: {error}")], "{id}");
        assert_eq!(review.read_with(cx, |r, cx| r.processing(cx)), Processing::Idle, "{id}");
        assert!(store.read_with(cx, |s, _| loaded(s, id)), "{id} stays");
    }
    assert_eq!(store.update(cx, |s, cx| s.close_and_clean_workspace("gone", cx)), Err("Workspace not found".to_owned()));
}

#[gpui_kit::test]
fn close_and_clean_disables_the_review_until_the_workspace_is_gone(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish(&h, &[]);
    let repo = TempRepo::new(&h);
    let store = open_store(cx, &h, "client-a");
    let root = open_repo_workspace(cx, &store, &repo);
    let child = store.update(cx, |s, cx| s.add_child_workspace(&root, "det-close".into(), true, None, cx)).unwrap();
    wait_child(cx, &store, &child);
    // The child's creation released the lock (else the removal would be refused).
    wait_until(cx, &store, |s| !s.is_locked());
    let child_ws = store.read_with(cx, |s, _| ws(s, &child));
    let child_dir = std::path::PathBuf::from(child_ws.read_with(cx, |w, _| w.workspace().path.clone()));
    let review = review_tab(cx, &child_ws, serde_json::json!({}));
    wait_until(cx, &review, |r| !r.is_refreshing());
    let header = review.read_with(cx, |r, cx| r.header(cx));
    assert!(header.has_parent && header.detached, "{header:?}");

    let (events, _events) = record_events(cx, &review);
    review.update(cx, |r, cx| r.close_and_clean(cx));
    // "Closing...": every action waits.
    assert_eq!(review.read_with(cx, |r, cx| r.processing(cx)), Processing::Removing);
    wait_until(cx, &store, |s| s.entry(&child).is_none());
    wait_for_disk(cx, || !child_dir.exists());
    assert!(alerts(&events).is_empty());
    assert!(loaded_now(&store, cx, &root));
}

#[gpui_kit::test]
fn a_close_and_clean_that_cannot_run_shows_on_the_workspace_and_frees_the_review(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    let root = serde_json::json!({
        "name": "root", "status": "active", "isGitRepo": true, "isWorktree": false, "gitBranch": "main",
        "gitRootPath": h.daemon.root().to_string_lossy(), "appStates": {}, "metadata": {}, "createdAt": 1,
        "lastActivity": 1, "parentHash": ""
    });
    std::fs::write(h.daemon.workspaces_dir().join("root.json"), root.to_string()).unwrap();
    std::fs::write(h.daemon.workspaces_dir().join("child.json"), detached_body("child", true, "root")).unwrap();
    publish(&h, &["root", "child"]);
    let store = open_store(cx, &h, "client-a");
    wait_until(cx, &store, |s| loaded(s, "root") && loaded(s, "child"));
    // Another connection holds the session lock: the removal cannot start.
    let other = futures::executor::block_on(treeterm_core::daemon::DaemonConnection::open(&h.rt, h.daemon.socket_path()))
        .unwrap()
        .unwrap();
    let locked = futures::executor::block_on(other.lock_session(std::time::Duration::from_secs(600))).unwrap().unwrap();
    assert!(matches!(locked, treeterm_core::daemon::SessionLockResult::Acquired(_)));
    let child_ws = store.read_with(cx, |s, _| ws(s, "child"));
    let review = review_tab(cx, &child_ws, serde_json::json!({}));
    let (events, _events) = record_events(cx, &review);

    review.update(cx, |r, cx| r.close_and_clean(cx));
    assert_eq!(review.read_with(cx, |r, cx| r.processing(cx)), Processing::Removing);
    wait_until(cx, &store, |s| matches!(s.entry("child"), Some(WorkspaceEntry::OperationError { .. })));
    let error = store.read_with(cx, |s, _| match s.entry("child") {
        Some(WorkspaceEntry::OperationError { error, .. }) => error.clone(),
        _ => unreachable!(),
    });
    assert_eq!(error, "Session is locked by another window");
    // The failure is the workspace's warning, not an alert; the actions work again.
    assert!(alerts(&events).is_empty());
    assert_eq!(review.read_with(cx, |r, cx| r.processing(cx)), Processing::Idle);
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
    assert!(!editor.read_with(cx, |e, _| e.is_preview()), "only markdown opens on its preview");
    editor.update(cx, |e, cx| e.set_content("hello world\n".into(), cx));
    assert!(editor.read_with(cx, |e, _| e.is_dirty()));
    // The tab title marks the unsaved edit at once.
    let title = |cx: &mut TestAppContext| workspace.read_with(cx, |w, _| w.file().app_states[&tab].title.clone());
    assert_eq!(title(cx), "notes.txt \u{2022}");
    editor.update(cx, |e, cx| e.save(cx));
    wait_until(cx, &editor, |e| !e.is_dirty());
    assert_eq!(title(cx), "notes.txt");
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

#[gpui_kit::test]
fn active_child_workspaces_count_as_unmerged(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("root.json"), body("root", None)).unwrap();
    std::fs::write(h.daemon.workspaces_dir().join("kid.json"), body("kid", Some("root"))).unwrap();
    let mut merged: serde_json::Value = serde_json::from_str(&body("done", Some("root"))).unwrap();
    merged["status"] = "merged".into();
    std::fs::write(h.daemon.workspaces_dir().join("done.json"), merged.to_string()).unwrap();
    publish(&h, &["root", "kid", "done"]);
    let store = open_store(cx, &h, "client-a");
    wait_until(cx, &store, |s| loaded(s, "root") && loaded(s, "kid") && loaded(s, "done"));
    let unmerged = cx.read(|cx| store.read(cx).unmerged_sub_workspaces(cx));
    assert_eq!(unmerged, vec![("kid".to_owned(), None)]);
}

#[gpui_kit::test]
fn an_ai_harness_tab_is_classified_by_its_analyzer(cx: &mut TestAppContext) {
    use std::sync::Arc;
    use treeterm_core::llm::FakeLlm;
    use treeterm_core::settings::ClassifierProvider;
    use treeterm_model::analyzer::LlmService;
    use treeterm_model::workspace_store::ActivityState;
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("a.json"), body("a", None)).unwrap();
    publish(&h, &["a"]);

    let mut settings = treeterm_core::settings::from_value(serde_json::json!({
        "aiHarness": { "instances": [{
            "id": "bot", "name": "Bot", "icon": "B",
            "command": "sh -c 'echo \"Allow write to config.toml? (y/n)\"; sleep 60'",
            "isDefault": false, "enableSandbox": false, "allowNetwork": true,
            "backgroundColor": "#1a1a24", "disableScrollbar": false, "keepOnExit": true
        }]}
    }))
    .unwrap();
    settings.llm.api_key = "test-key".into();
    settings.llm.base_url = "http://localhost".into();
    settings.terminal_analyzer.provider = ClassifierProvider::ChatCompletions;
    settings.terminal_analyzer.model = "test-model".into();
    settings.terminal_analyzer.idle_debounce = std::time::Duration::from_millis(300);
    let fake = FakeLlm::default();
    fake.on_complete_chat(|_, _| Ok(r#"{"state":"permission_request","reason":"asks to write config.toml"}"#.into()));
    let llm = LlmService::new(Arc::new(fake.clone()), h.rt.clone());
    let conn = h.conn.clone();
    let settings_store = cx.new(|_| SettingsStore::in_memory(settings));
    let store = cx.new(|cx| SessionStore::new(conn, "client-a".into(), settings_store, Some(llm), cx));
    wait_until(cx, &store, |s| loaded(s, "a"));
    let workspace = store.read_with(cx, |s, _| ws(s, "a"));

    let tab = workspace.update(cx, |w, cx| w.add_tab("aiharness-bot", cx)).unwrap();
    wait_until_app(cx, &workspace, |w, _| w.analyzer(&tab).is_some());
    // The screen settles (first on the shell prompt, then on the question); each idle edge
    // (fake clock) gets the screen classified.
    let asked = || {
        fake.chat_calls()
            .iter()
            .any(|(messages, _)| messages.iter().any(|m| m.content.contains("Allow write to config.toml?")))
    };
    for _ in 0..1000 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        cx.executor().advance_clock(std::time::Duration::from_millis(100));
        cx.run_until_parked();
        if asked() && workspace.read_with(cx, |w, _| w.activity(&tab)) == ActivityState::PermissionRequest {
            break;
        }
    }
    let screen = workspace.read_with(cx, |w, cx| match w.tab_content(&tab) {
        Some(TabContent::Terminal(t)) => t.read(cx).screen_text(),
        _ => String::new(),
    });
    assert!(asked(), "the question was never classified; {} calls; screen:\n{screen}", fake.chat_calls().len());
    assert_eq!(workspace.read_with(cx, |w, _| w.activity(&tab)), ActivityState::PermissionRequest);
    let analyzer = workspace.read_with(cx, |w, _| w.analyzer(&tab)).unwrap();
    assert_eq!(analyzer.read_with(cx, |a, _| a.analyzer().reason().to_owned()), "asks to write config.toml");
}

#[gpui_kit::test]
fn finished_work_in_an_unviewed_workspace_is_marked_unread_until_viewed(cx: &mut TestAppContext) {
    use treeterm_model::session_store::SessionEvent;
    use treeterm_model::workspace_store::ActivityState;
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("a.json"), body("a", None)).unwrap();
    std::fs::write(h.daemon.workspaces_dir().join("b.json"), body("b", None)).unwrap();
    publish(&h, &["a", "b"]);
    let mut settings = treeterm_core::settings::Settings::default();
    settings.terminal_analyzer.idle_debounce = std::time::Duration::from_millis(300);
    let conn = h.conn.clone();
    let settings_store = cx.new(|_| SettingsStore::in_memory(settings));
    let store = cx.new(|cx| SessionStore::new(conn, "client-a".into(), settings_store, None, cx));
    let recorded = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let sink = recorded.clone();
    cx.update(|cx| {
        cx.subscribe(&store, move |_, event: &SessionEvent, _| {
            if let SessionEvent::AttentionRecorded(id) = event {
                sink.borrow_mut().push(id.clone());
            }
        })
        .detach()
    });
    wait_until(cx, &store, |s| loaded(s, "a") && loaded(s, "b"));
    store.update(cx, |s, cx| s.set_viewed_workspace(Some("a".into()), cx));
    let b = store.read_with(cx, |s, _| ws(s, "b"));
    let tab = b.update(cx, |w, cx| w.add_tab("terminal", cx)).unwrap();
    wait_until_app(cx, &b, |w, _| matches!(w.tab_content(&tab), Some(TabContent::Terminal(_))));
    let Some(TabContent::Terminal(terminal)) = b.read_with(cx, |w, _| w.tab_content(&tab)) else {
        unreachable!()
    };
    // Let the shell print its prompt and that first burst settle (not reported as work).
    wait_until_app(cx, &terminal, |t, _| !t.screen_text().trim().is_empty());
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        cx.executor().advance_clock(std::time::Duration::from_millis(100));
        cx.run_until_parked();
    }
    terminal.update(cx, |t, cx| {
        t.set_focused(true, cx);
        t.write_input(b"echo attention-check\r", cx);
    });
    for _ in 0..300 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        cx.executor().advance_clock(std::time::Duration::from_millis(100));
        cx.run_until_parked();
        if !recorded.borrow().is_empty() {
            break;
        }
    }
    // A short burst goes Working → Idle within one update; the transitions show it.
    let went_through_working = b.read_with(cx, |w, _| {
        w.activity_states().transitions(&tab).iter().any(|t| t.to == ActivityState::Working)
    });
    assert!(went_through_working, "the command's output never counted as work");
    assert_eq!(*recorded.borrow(), vec!["b".to_owned()]);
    assert!(b.read_with(cx, |w, _| w.has_unread_attention()));
    wait_for_disk(cx, || file_on_disk(&h, "b")["metadata"]["workspaceAttention"].is_string());

    // Viewing it marks it read.
    store.update(cx, |s, cx| s.set_viewed_workspace(Some("b".into()), cx));
    wait_until_app(cx, &b, |w, _| !w.has_unread_attention());
    wait_for_disk(cx, || {
        let raw = file_on_disk(&h, "b")["metadata"]["workspaceAttention"].as_str().map(str::to_owned);
        raw.and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .is_some_and(|a| a["revision"] == a["acknowledgedRevision"])
    });
}

#[gpui_kit::test]
fn a_web_app_gets_a_free_port_starts_its_server_and_is_ready(cx: &mut TestAppContext) {
    use treeterm_model::web_app::WebAppPhase;
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("a.json"), body("a", None)).unwrap();
    publish(&h, &["a"]);
    let settings = treeterm_core::settings::from_value(serde_json::json!({
        "webApp": { "instances": [{
            "id": "py", "name": "Py", "icon": "P", "command": "python3 -m http.server $PORT --bind 127.0.0.1",
            "isDefault": false, "keepOnExit": false
        }]}
    }))
    .unwrap();
    let conn = h.conn.clone();
    let settings_store = cx.new(|_| SettingsStore::in_memory(settings));
    let store = cx.new(|cx| SessionStore::new(conn, "client-a".into(), settings_store, None, cx));
    wait_until(cx, &store, |s| loaded(s, "a"));
    let workspace = store.read_with(cx, |s, _| ws(s, "a"));
    let tab = workspace.update(cx, |w, cx| w.add_tab("webapp-py", cx)).unwrap();
    let Some(TabContent::WebApp { app, .. }) = workspace.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not a web app tab");
    };
    // Real time: curl probes run on the daemon, the poll interval on the executor clock.
    let mut phase = WebAppPhase::AllocatingPort;
    for _ in 0..600 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        cx.executor().advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
        phase = app.read_with(cx, |a, _| a.phase().clone());
        if matches!(phase, WebAppPhase::Ready { .. } | WebAppPhase::Error(_)) {
            break;
        }
    }
    let port = app.read_with(cx, |a, _| a.port()).expect("a port was assigned");
    assert_eq!(phase, WebAppPhase::Ready { url: format!("http://localhost:{port}/") });
    assert!((20000..45000).contains(&port));
    wait_for_disk(cx, || file_on_disk(&h, "a")["appStates"][&tab]["state"]["port"]["port"] == port);
    assert!(matches!(
        workspace.read_with(cx, |w, _| w.tab_content(&tab)),
        Some(TabContent::WebApp { terminal: Some(_), .. })
    ));
}

#[gpui_kit::test]
fn a_local_browser_tab_opens_its_remembered_page_directly(cx: &mut TestAppContext) {
    use treeterm_model::browser_page::BrowserPhase;
    use treeterm_model::workspace_store::PageHost;
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("a.json"), body("a", None)).unwrap();
    publish(&h, &["a"]);
    let settings = treeterm_core::settings::from_value(serde_json::json!({
        "browser": { "instances": [{ "id": "docs", "name": "Docs", "icon": "B", "url": "http://localhost:3000/", "isDefault": false }] }
    }))
    .unwrap();
    let conn = h.conn.clone();
    let settings_store = cx.new(|_| SettingsStore::in_memory(settings));
    let store = cx.new(|cx| SessionStore::new(conn, "client-a".into(), settings_store, None, cx));
    wait_until(cx, &store, |s| loaded(s, "a"));
    let workspace = store.read_with(cx, |s, _| ws(s, "a"));
    let tab = workspace.update(cx, |w, cx| w.add_tab_with("browser-docs", serde_json::json!({ "path": "/guide?x=1" }), cx)).unwrap();
    let Some(TabContent::Browser { page, forward }) = workspace.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not a browser tab");
    };
    // No forward on this machine: the page is ready at once, on the page the tab remembers.
    assert!(matches!(forward, PageHost::Local));
    let ready = BrowserPhase::Ready { url: "http://localhost:3000/guide?x=1".into(), home: "http://localhost:3000/".into() };
    assert_eq!(page.read_with(cx, |p, _| p.phase().clone()), ready);
    page.update(cx, |p, cx| p.restart_forward(cx));
    page.update(cx, |p, cx| p.retry(cx));
    assert_eq!(page.read_with(cx, |p, _| p.phase().clone()), ready);
    workspace.update(cx, |w, cx| w.remove_tab(&tab, cx));
    assert!(workspace.read_with(cx, |w, _| w.tab_content(&tab)).is_none());
}

#[gpui_kit::test]
fn auto_open_adopts_worktrees_under_their_detected_parents(cx: &mut TestAppContext) {
    use treeterm_model::session_store::AutoOpenItem;
    cx.executor().allow_parking();
    let h = harness();
    publish(&h, &[]);
    let repo = TempRepo::new(&h);
    let feat = h.daemon.root().join("wt-feat");
    let sub = h.daemon.root().join("wt-sub");
    repo.git(&["worktree", "add", "-q", "-b", "feat", feat.to_str().unwrap()]);
    repo.git(&["worktree", "add", "-q", "-b", "sub", sub.to_str().unwrap(), "feat"]);
    let store = open_store(cx, &h, "client-a");
    let root = open_repo_workspace(cx, &store, &repo);
    let path = |p: &std::path::Path| p.to_string_lossy().into_owned();
    let items = vec![
        // Child first: it must still be created after its parent.
        AutoOpenItem {
            path: path(&sub),
            branch: "sub".into(),
            parent_path: Some(path(&feat)),
            display_name: Some("Sub work".into()),
            description: None,
        },
        AutoOpenItem { path: path(&feat), branch: "feat".into(), parent_path: None, display_name: None, description: Some("the feature".into()) },
    ];
    store.update(cx, |s, cx| s.auto_open_worktrees(&root, items, cx)).unwrap();
    let find = |s: &SessionStore, p: &str, cx: &gpui_kit::App| {
        s.workspace_ids().find(|id| s.workspace_store(id).is_some_and(|w| w.read(cx).workspace().path == p)).cloned()
    };
    for _ in 0..500 {
        // Auto-open polls for each parent to finish loading (executor clock).
        cx.executor().advance_clock(std::time::Duration::from_millis(100));
        cx.run_until_parked();
        if cx.read(|cx| find(store.read(cx), &path(&feat), cx).is_some() && find(store.read(cx), &path(&sub), cx).is_some()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let entries = cx.read(|cx| {
        let s = store.read(cx);
        s.workspace_ids()
            .map(|id| match s.entry(id) {
                Some(WorkspaceEntry::Loaded(w)) => format!("{id}: {}", w.read(cx).workspace().path),
                Some(WorkspaceEntry::Error { error, .. }) => format!("{id}: error {error}"),
                Some(WorkspaceEntry::Loading { message, .. }) => format!("{id}: loading {message}"),
                Some(WorkspaceEntry::OperationError { error, .. }) => format!("{id}: op error {error}"),
                None => format!("{id}: none"),
            })
            .collect::<Vec<_>>()
    });
    assert!(entries.len() == 3, "{entries:#?} want {} and {}", path(&feat), path(&sub));
    let (feat_id, sub_id) = cx.read(|cx| {
        let s = store.read(cx);
        (find(s, &path(&feat), cx).unwrap(), find(s, &path(&sub), cx).unwrap())
    });
    let file_of = |id: &str| store.read_with(cx, |s, cx| ws(s, id).read(cx).file().clone());
    assert_eq!(file_of(&feat_id).parent_id.as_deref(), Some(root.as_str()));
    assert_eq!(file_of(&feat_id).metadata.get("description").map(String::as_str), Some("the feature"));
    assert_eq!(file_of(&sub_id).parent_id.as_deref(), Some(feat_id.as_str()));
    assert_eq!(file_of(&sub_id).display_name(), "Sub work");
    assert_eq!(file_of(&sub_id).metadata.get("branchIsUserDefined").map(String::as_str), Some("true"));
}

#[gpui_kit::test]
fn a_lock_held_elsewhere_shows_and_can_be_forced_open(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    publish(&h, &[]);
    let store = open_store(cx, &h, "client-a");
    wait_until(cx, &store, |s| matches!(s.state(), SessionState::Live));
    // Another connection takes the lock and never releases it.
    let other = futures::executor::block_on(treeterm_core::daemon::DaemonConnection::open(&h.rt, h.daemon.socket_path()))
        .unwrap()
        .unwrap();
    let locked = futures::executor::block_on(other.lock_session(std::time::Duration::from_secs(600))).unwrap().unwrap();
    assert!(matches!(locked, treeterm_core::daemon::SessionLockResult::Acquired(_)));
    wait_until(cx, &store, |s| s.is_locked());
    store.update(cx, |s, cx| s.force_unlock(cx));
    wait_until(cx, &store, |s| !s.is_locked());
}

#[gpui_kit::test]
fn a_file_downloads_to_a_local_path(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let h = harness();
    std::fs::write(h.daemon.workspaces_dir().join("a.json"), body("a", None)).unwrap();
    publish(&h, &["a"]);
    let source = h.daemon.root().join("data.bin");
    std::fs::write(&source, [0u8, 1, 2, 255, 7]).unwrap();
    let store = open_store(cx, &h, "client-a");
    wait_until(cx, &store, |s| loaded(s, "a"));
    let workspace = store.read_with(cx, |s, _| ws(s, "a"));
    let tab = workspace.update(cx, |w, cx| w.add_tab("filesystem", cx)).unwrap();
    let Some(TabContent::Files(files)) = workspace.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not a files tab");
    };
    let out = tempfile::tempdir().unwrap();
    let destination = out.path().join("copy.bin");
    files.update(cx, |f, cx| f.download(&source.to_string_lossy(), destination.clone(), cx));
    wait_until(cx, &files, |f| *f.download_state() == treeterm_model::file_browser::DownloadState::Idle && destination.exists());
    assert_eq!(std::fs::read(&destination).unwrap(), vec![0u8, 1, 2, 255, 7]);
}

#[gpui_kit::test]
fn review_shows_both_sides_of_a_changed_image(cx: &mut TestAppContext) {
    use treeterm_model::review::{ImageSides, ReviewContent};
    cx.executor().allow_parking();
    let h = harness();
    publish(&h, &[]);
    let repo = TempRepo::new(&h);
    // Not real PNGs: the review only moves the bytes.
    std::fs::write(repo.path.join("logo.png"), b"\x89PNG-before").unwrap();
    repo.git(&["add", "logo.png"]);
    repo.git(&["commit", "-q", "-m", "logo"]);
    std::fs::write(repo.path.join("logo.png"), b"\x89PNG-after!").unwrap();
    let store = open_store(cx, &h, "client-a");
    let root = open_repo_workspace(cx, &store, &repo);
    let workspace = store.read_with(cx, |s, _| ws(s, &root));
    let tab = workspace
        .update(cx, |w, cx| w.add_tab_with("review", serde_json::json!({ "viewMode": "uncommitted" }), cx))
        .unwrap();
    let Some(TabContent::Review(review)) = workspace.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not a review tab");
    };
    wait_until(cx, &review, |r| matches!(r.content(), ReviewContent::Uncommitted(_)));
    // Every image file shown loads inline under its header.
    wait_until(cx, &review, |r| matches!(r.image("logo.png"), Some(ImageSides::Loaded { .. })));
    review.update(cx, |r, cx| r.focus_file("logo.png", cx));
    wait_until(cx, &review, |r| r.image_diff().is_some_and(|d| !matches!(d.sides, ImageSides::Loading)));
    let sides = review.read_with(cx, |r, _| r.image_diff().unwrap().sides.clone());
    let ImageSides::Loaded { before, after } = sides else {
        panic!("not loaded: {sides:?}");
    };
    assert_eq!(before.as_deref().map(Vec::as_slice), Some(&b"\x89PNG-before"[..]));
    assert_eq!(after.as_deref().map(Vec::as_slice), Some(&b"\x89PNG-after!"[..]));
}
