//! The Files tab's search and go to definition, and the Review tab's comments, viewed files
//! and prompts, against a real in-process daemon and real git repos.

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use treeterm_core::review::{CommitAnchor, Freshness, LineAnchor, Resolution, Side};
use treeterm_model::file_browser::{
    DefinitionLookup, Favourites, FileBrowser, FileView, NAME_SEARCH_DEBOUNCE, NameResults, REPO_SEARCH_DEBOUNCE, RepoResults,
    SearchPanel, SearchRow, Star,
};
use treeterm_model::review::{Direction, Head, Notice, ReviewContent, ReviewModel};
use treeterm_model::session_store::{SessionState, SessionStore, WorkspaceEntry};
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::workspace_store::{TabContent, WorkspaceStore};
use treeterm_proto::treeterm::WorkspaceRef;

mod common;
use common::{Harness, harness, wait_until, wait_until_app};

/// One test at a time. The in-process daemon forks shells for PTYs; a shell forked while
/// another test's git command is starting can hold that command's output pipe open, and
/// the command then never finishes.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
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

    fn write(&self, relative: &str, text: &str) {
        let file = self.path.join(relative);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }

    fn commit_all(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
    }

    fn git(&self, args: &[&str]) -> String {
        let out = std::process::Command::new("git").args(args).current_dir(&self.path).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn root(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        if self.worktrees_home.exists() && std::fs::remove_dir_all(&self.worktrees_home).is_err() {
            eprintln!("could not remove {}", self.worktrees_home.display());
        }
    }
}

/// A Files tab on the repo's workspace, showing `selected`.
fn open_browser(
    cx: &mut TestAppContext,
    h: &Harness,
    repo: &TempRepo,
    selected: &str,
) -> (Entity<SessionStore>, Entity<WorkspaceStore>, Entity<FileBrowser>) {
    let store = open_store(cx, h);
    let workspace = open_repo_workspace(cx, h, &store, repo);
    let state = serde_json::json!({ "selectedPath": format!("{}/{selected}", repo.root()), "expandedDirs": [] });
    let tab = workspace.update(cx, |w, cx| w.add_tab_with("filesystem", state, cx)).unwrap();
    let Some(TabContent::Files(browser)) = workspace.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not a files tab");
    };
    wait_until(cx, &browser, |b| matches!(b.view(), FileView::Text { .. }));
    // The session owns the workspace, and so the tab.
    (store, workspace, browser)
}

/// Byte offset of `needle`'s `nth` occurrence in the shown file.
fn offset_of(browser: &FileBrowser, needle: &str, nth: usize) -> usize {
    let FileView::Text { text, .. } = browser.view() else { panic!("not text") };
    text.match_indices(needle).nth(nth).unwrap().0
}

#[gpui_kit::test]
fn the_files_search_finds_matches_in_the_file_and_the_repo(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("src/lib.rs", "pub fn needle_fn() {}\n\nfn caller() {\n    needle_fn();\n}\n");
    repo.write("notes.txt", "see needle_fn here\n");
    repo.commit_all("files");
    let (_store, _workspace, browser) = open_browser(cx, &h, &repo, "src/lib.rs");

    browser.update(cx, |b, cx| b.open_search("needle_fn", None, cx));
    let matches = browser.read_with(cx, |b, _| b.file_matches());
    assert_eq!(matches.iter().map(|m| (m.line_number, m.column)).collect::<Vec<_>>(), vec![(1, 8), (4, 5)]);
    assert_eq!(matches[0].range, 7..16);
    assert_eq!(browser.read_with(cx, |b, _| b.match_count_label()), "2 found");

    // Enter walks the matches (wrapping), each revealed in the viewer.
    browser.update(cx, |b, cx| b.step_match(true, cx));
    let first = browser.read_with(cx, |b, _| b.reveal_range()).unwrap();
    assert_eq!(first.1, 7..16);
    browser.update(cx, |b, cx| b.step_match(true, cx));
    browser.update(cx, |b, cx| b.step_match(true, cx));
    assert_eq!(browser.read_with(cx, |b, _| b.match_count_label()), "1/2");
    let again = browser.read_with(cx, |b, _| b.reveal_range()).unwrap();
    assert_eq!(again.1, first.1);
    assert!(again.0 > first.0, "the same place is revealed again");
    browser.update(cx, |b, cx| b.step_match(false, cx));
    assert_eq!(browser.read_with(cx, |b, _| b.match_count_label()), "2/2");

    // The repo search starts after the typing pause and lists other files.
    cx.executor().advance_clock(REPO_SEARCH_DEBOUNCE);
    wait_until(cx, &browser, |b| match b.search() {
        SearchPanel::Open(search) => matches!(search.repo, RepoResults::Done { .. } | RepoResults::Failed { .. }),
        SearchPanel::Closed => false,
    });
    let rows = browser.read_with(cx, |b, _| b.search_rows());
    assert_eq!(rows[0], SearchRow::Section("In this file"));
    assert_eq!(rows[1], SearchRow::File("src/lib.rs".into()));
    assert!(matches!(rows[2], SearchRow::InFile { line_number: 1, active: false, .. }));
    assert!(matches!(rows[3], SearchRow::InFile { line_number: 4, active: true, .. }));
    assert_eq!(rows[4], SearchRow::Section("In repo"));
    assert_eq!(rows[5], SearchRow::File("notes.txt".into()), "{rows:?}");
    let SearchRow::InRepo(hit) = rows[6].clone() else { panic!("{rows:?}") };
    let SearchRow::Note(footer) = &rows[7] else { panic!("{rows:?}") };
    assert!(footer.starts_with("1 found · "), "{footer}");
    assert_eq!(rows.len(), 8, "the open file's repo hits are not listed twice");

    // A repo hit opens its file with the match selected.
    browser.update(cx, |b, cx| b.open_repo_hit(&hit, cx));
    wait_until(cx, &browser, |b| matches!(b.view(), FileView::Text { path, .. } if path.ends_with("notes.txt")));
    assert_eq!(browser.read_with(cx, |b, _| b.reveal_range()).unwrap().1, 4..13);
    assert_eq!(browser.read_with(cx, |b, _| b.match_count_label()), "1 found");

    // A new query restarts the search; closing it cancels it.
    browser.update(cx, |b, cx| b.set_query("nothing-like-this", cx));
    let searching = browser.read_with(cx, |b, _| match b.search() {
        SearchPanel::Open(search) => matches!(search.repo, RepoResults::Searching { .. }),
        SearchPanel::Closed => false,
    });
    assert!(searching);
    browser.update(cx, |b, cx| b.close_search(cx));
    assert_eq!(browser.read_with(cx, |b, _| b.search().clone()), SearchPanel::Closed);
    assert!(browser.read_with(cx, |b, _| b.file_matches()).is_empty());
}

#[gpui_kit::test]
fn the_tree_searches_file_names_after_a_pause(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("src/needle_one.rs", "1\n");
    repo.write("docs/Needle.md", "2\n");
    repo.write("other.txt", "3\n");
    repo.commit_all("files");
    let (_store, _workspace, browser) = open_browser(cx, &h, &repo, "other.txt");

    browser.update(cx, |b, cx| b.set_name_query("needle", cx));
    let search = browser.read_with(cx, |b, _| b.name_search().clone());
    assert!(search.is_active());
    assert_eq!(search.results, NameResults::Idle, "nothing runs before the typing pause");
    cx.executor().advance_clock(NAME_SEARCH_DEBOUNCE);
    wait_until(cx, &browser, |b| matches!(b.name_search().results, NameResults::Found(_) | NameResults::Failed(_)));
    let NameResults::Found(entries) = browser.read_with(cx, |b, _| b.name_search().results.clone()) else {
        panic!("search failed")
    };
    let mut names: Vec<String> = entries.iter().map(|e| e.relative_path.clone()).collect();
    names.sort();
    assert_eq!(names, vec!["docs/Needle.md".to_owned(), "src/needle_one.rs".to_owned()]);

    // A directory result expands and is selected; a file opens.
    let docs = format!("{}/docs", repo.root());
    browser.update(cx, |b, cx| b.set_name_query("doc", cx));
    cx.executor().advance_clock(NAME_SEARCH_DEBOUNCE);
    wait_until(cx, &browser, |b| matches!(&b.name_search().results, NameResults::Found(e) if e.iter().any(|e| e.is_directory)));
    let dir = browser.read_with(cx, |b, _| match &b.name_search().results {
        NameResults::Found(entries) => entries.iter().find(|e| e.is_directory).cloned().unwrap(),
        _ => unreachable!(),
    });
    browser.update(cx, |b, cx| b.open_name_result(&dir, cx));
    assert_eq!(browser.read_with(cx, |b, _| b.selected().map(str::to_owned)), Some(docs.clone()));
    assert!(browser.read_with(cx, |b, _| b.rows().iter().any(|r| r.path == docs && r.expanded)));

    // Clearing (or blanking) goes back to the tree.
    browser.update(cx, |b, cx| b.set_name_query("   ", cx));
    cx.executor().advance_clock(NAME_SEARCH_DEBOUNCE);
    wait_until(cx, &browser, |b| b.name_search().results == NameResults::Idle);
    browser.update(cx, |b, cx| b.clear_name_search(cx));
    assert!(!browser.read_with(cx, |b, _| b.name_search().is_active()));
}

#[gpui_kit::test]
fn the_search_panel_opens_on_the_selection_or_the_word_under_the_cursor(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.rs", "let alpha = beta;\n");
    repo.commit_all("a");
    let (_store, _workspace, browser) = open_browser(cx, &h, &repo, "a.rs");
    let query = |browser: &FileBrowser| match browser.search() {
        SearchPanel::Open(search) => Some(search.query.clone()),
        SearchPanel::Closed => None,
    };

    browser.update(cx, |b, cx| b.open_search("", None, cx));
    assert_eq!(browser.read_with(cx, |b, _| query(b)), Some(String::new()));
    // Multi-line selections are not queries; an open panel keeps its query.
    browser.update(cx, |b, cx| b.set_query("alp", cx));
    browser.update(cx, |b, cx| b.open_search("a\nb", None, cx));
    assert_eq!(browser.read_with(cx, |b, _| query(b)), Some("alp".into()));
    let beta = browser.read_with(cx, |b, _| offset_of(b, "beta", 0)) + 2;
    browser.update(cx, |b, cx| b.open_search("", Some(beta), cx));
    assert_eq!(browser.read_with(cx, |b, _| query(b)), Some("beta".into()));
    browser.update(cx, |b, cx| b.open_search("alpha", Some(beta), cx));
    assert_eq!(browser.read_with(cx, |b, _| query(b)), Some("alpha".into()));
}

#[gpui_kit::test]
fn go_to_definition_opens_one_result_and_offers_several(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("src/lib.rs", "pub fn needle_fn() {}\nfn twice() {}\n\nfn caller() {\n    needle_fn();\n    twice();\n}\n");
    repo.write("src/other.rs", "\n\nfn twice() {}\n");
    repo.write("notes.txt", "needle_fn\n");
    repo.commit_all("files");
    let (_store, workspace, browser) = open_browser(cx, &h, &repo, "src/lib.rs");

    // One definition in the open file: the viewer moves there.
    let call = browser.read_with(cx, |b, _| offset_of(b, "needle_fn", 1)) + 3;
    browser.update(cx, |b, cx| b.go_to_definition(call, cx));
    assert!(matches!(browser.read_with(cx, |b, _| b.definition().clone()), DefinitionLookup::Searching { symbol } if symbol == "needle_fn"));
    wait_until(cx, &browser, |b| *b.definition() == DefinitionLookup::Idle);
    let reveal = browser.read_with(cx, |b, _| b.reveal().cloned()).unwrap();
    assert!(reveal.path.ends_with("src/lib.rs"));
    assert_eq!(reveal.line_number, 1);

    // Several: the user picks one; another file opens in an Editor tab at the line.
    let twice = browser.read_with(cx, |b, _| offset_of(b, "twice", 1));
    browser.update(cx, |b, cx| b.go_to_definition(twice, cx));
    wait_until(cx, &browser, |b| matches!(b.definition(), DefinitionLookup::Choose { .. }));
    let DefinitionLookup::Choose { symbol, locations } = browser.read_with(cx, |b, _| b.definition().clone()) else {
        unreachable!()
    };
    assert_eq!(symbol, "twice");
    let mut found: Vec<(String, u64)> = locations.iter().map(|l| (l.file_path.clone(), l.line_number)).collect();
    found.sort();
    assert_eq!(found, vec![("src/lib.rs".to_owned(), 2), ("src/other.rs".to_owned(), 3)]);
    let other = locations.iter().position(|l| l.file_path == "src/other.rs").unwrap();
    browser.update(cx, |b, cx| b.choose_definition(other, cx));
    assert_eq!(browser.read_with(cx, |b, _| b.definition().clone()), DefinitionLookup::Idle);
    let editor = workspace.read_with(cx, |w, _| {
        w.file().app_states.values().find(|tab| tab.application_id == "editor").map(|tab| tab.state.clone())
    });
    let editor = editor.expect("an editor tab");
    assert!(editor["filePath"].as_str().unwrap().ends_with("src/other.rs"), "{editor}");
    assert_eq!(editor["scrollToLine"], 3);
    assert!(browser.read_with(cx, |b, _| matches!(b.view(), FileView::Text { path, .. } if path.ends_with("src/lib.rs"))));

    // Nothing defines a keyword; plain text has no definitions; a non-word does nothing.
    let paren = browser.read_with(cx, |b, _| offset_of(b, " {}", 0)) + 1;
    browser.update(cx, |b, cx| b.go_to_definition(paren, cx));
    assert_eq!(browser.read_with(cx, |b, _| b.definition().clone()), DefinitionLookup::Idle);
    let fn_word = browser.read_with(cx, |b, _| offset_of(b, "fn", 0));
    browser.update(cx, |b, cx| b.go_to_definition(fn_word, cx));
    wait_until(cx, &browser, |b| !matches!(b.definition(), DefinitionLookup::Searching { .. }));
    assert_eq!(browser.read_with(cx, |b, _| b.definition().clone()), DefinitionLookup::NotFound { symbol: "fn".into() });
    browser.update(cx, |b, cx| b.dismiss_definition(cx));
    let notes = format!("{}/notes.txt", repo.root());
    browser.update(cx, |b, cx| b.open(&notes, false, cx));
    wait_until(cx, &browser, |b| matches!(b.view(), FileView::Text { path, .. } if path.ends_with("notes.txt")));
    browser.update(cx, |b, cx| b.go_to_definition(1, cx));
    assert_eq!(
        browser.read_with(cx, |b, _| b.definition().clone()),
        DefinitionLookup::Unavailable { language: "plaintext" }
    );
}

#[gpui_kit::test]
fn favourites_are_starred_in_the_tree_and_listed_above_it(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("src/a.rs", "a\n");
    repo.write("src/deep/b.rs", "b\n");
    repo.write("README.md", "r\n");
    repo.commit_all("files");
    let (_store, workspace, browser) = open_browser(cx, &h, &repo, "README.md");
    let root = repo.root();
    let (src, a, readme) = (format!("{root}/src"), format!("{root}/src/a.rs"), format!("{root}/README.md"));
    let listed = |b: &FileBrowser| match b.favourites() {
        Favourites::Ready(files) => {
            let mut paths: Vec<String> = files.iter().map(|f| f.relative_path.clone()).collect();
            paths.sort();
            Some(paths)
        }
        Favourites::Empty | Favourites::Loading | Favourites::Failed(_) => None,
    };
    assert_eq!(browser.read_with(cx, |b, _| b.favourites().clone()), Favourites::Empty);
    assert_eq!(browser.read_with(cx, |b, cx| b.star(&src, cx)), Star::Off);

    // A favourite folder lists every file under it and covers them.
    browser.update(cx, |b, cx| b.toggle_favourite(&src, cx));
    wait_until(cx, &browser, |b| listed(b) == Some(vec!["src/a.rs".into(), "src/deep/b.rs".into()]));
    assert_eq!(browser.read_with(cx, |b, cx| b.star(&src, cx)), Star::On);
    assert_eq!(browser.read_with(cx, |b, cx| b.star(&a, cx)), Star::Covered);
    browser.update(cx, |b, cx| b.toggle_favourite(&a, cx));
    assert_eq!(workspace.read_with(cx, |w, _| w.favourite_paths().to_vec()), vec!["src".to_owned()]);

    browser.update(cx, |b, cx| b.toggle_favourite(&readme, cx));
    wait_until(cx, &browser, |b| listed(b).is_some_and(|paths| paths.len() == 3));
    assert_eq!(workspace.read_with(cx, |w, _| w.favourite_paths().to_vec()), vec!["src".to_owned(), "README.md".to_owned()]);

    // Unstarring.
    browser.update(cx, |b, cx| b.toggle_favourite(&src, cx));
    wait_until(cx, &browser, |b| listed(b) == Some(vec!["README.md".into()]));
    browser.update(cx, |b, cx| b.toggle_favourite(&readme, cx));
    assert_eq!(browser.read_with(cx, |b, _| b.favourites().clone()), Favourites::Empty);
    assert!(workspace.read_with(cx, |w, _| w.favourite_paths().is_empty()));
}

// ---- review ----------------------------------------------------------------------------

fn open_store(cx: &mut TestAppContext, h: &Harness) -> Entity<SessionStore> {
    let settings = treeterm_core::settings::from_value(serde_json::json!({
        "aiHarness": { "instances": [{
            "id": "bot", "name": "Bot", "icon": "B", "command": "cat",
            "isDefault": false, "enableSandbox": false, "allowNetwork": true,
            "backgroundColor": "#1a1a24", "disableScrollbar": false, "keepOnExit": true
        }]}
    }))
    .unwrap();
    let conn = h.conn.clone();
    let settings = cx.new(|_| SettingsStore::in_memory(settings));
    cx.new(|cx| SessionStore::new(conn, "client-a".into(), settings, None, cx))
}

/// The repo as a workspace with no tabs (no terminal whose shell could start while a test's
/// git commands run; see [`serial`]).
fn open_repo_workspace(cx: &mut TestAppContext, h: &Harness, store: &Entity<SessionStore>, repo: &TempRepo) -> Entity<WorkspaceStore> {
    let id = format!("ws-{}", uuid::Uuid::new_v4().simple());
    let body = serde_json::json!({
        "name": "repo", "status": "active", "isGitRepo": true, "isWorktree": false, "gitBranch": "main",
        "appStates": {}, "metadata": {}, "createdAt": 1, "lastActivity": 1, "parentHash": ""
    });
    std::fs::write(h.daemon.workspaces_dir().join(format!("{id}.json")), body.to_string()).unwrap();
    let refs = vec![WorkspaceRef { id: id.clone(), path: repo.root() }];
    futures::executor::block_on(h.conn.update_session(refs, None, "test-writer")).unwrap().unwrap();
    wait_until(cx, store, |s| matches!(s.state(), SessionState::Live) && matches!(s.entry(&id), Some(WorkspaceEntry::Loaded(_))));
    store.read_with(cx, |s, _| s.workspace_store(&id).unwrap().clone())
}

fn open_review(cx: &mut TestAppContext, workspace: &Entity<WorkspaceStore>) -> Entity<ReviewModel> {
    let tab = workspace
        .update(cx, |w, cx| w.add_tab_with("review", serde_json::json!({ "viewMode": "uncommitted" }), cx))
        .unwrap();
    let Some(TabContent::Review(review)) = workspace.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not a review tab");
    };
    review
}

/// [`wait_until_app`] for a review, describing it when the condition is not reached.
fn wait_review(cx: &mut TestAppContext, review: &Entity<ReviewModel>, what: &str, done: impl Fn(&ReviewModel, &gpui_kit::App) -> bool) {
    for _ in 0..1000 {
        cx.run_until_parked();
        if review.read_with(cx, |r, cx| done(r, cx)) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let state = review.read_with(cx, |r, cx| {
        format!("content {:?}\nhead {:?}\nnotice {:?}\nadding {:?}\ndraft {:?}\ncomments {:?}", r.content(), r.head(), r.notice(), r.adding(), r.draft(), r.comments(cx))
    });
    panic!("{what} not reached:\n{state}");
}

fn metadata(cx: &mut TestAppContext, workspace: &Entity<WorkspaceStore>, key: &str) -> Option<serde_json::Value> {
    workspace.read_with(cx, |w, _| w.file().metadata.get(key).map(|v| serde_json::from_str(v).unwrap()))
}

#[gpui_kit::test]
fn review_files_are_marked_viewed_in_workspace_metadata(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "one\ntwo\n");
    repo.write("b.txt", "bee\n");
    repo.commit_all("base");
    repo.write("a.txt", "one\nTWO\n");
    repo.write("b.txt", "BEE\n");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let review = open_review(cx, &workspace);
    wait_review(cx, &review, "two changed files", |r, _| matches!(r.content(), ReviewContent::Uncommitted(u) if u.changes.files.len() == 2));

    assert_eq!(review.read_with(cx, |r, cx| r.viewed_count(cx).unwrap()), (0, 2));
    review.update(cx, |r, cx| r.toggle_viewed("a.txt", cx));
    assert_eq!(
        metadata(cx, &workspace, "reviewViewedFiles"),
        Some(serde_json::json!({ "a.txt": { "kind": "text", "additions": 1, "deletions": 1 } }))
    );
    assert_eq!(review.read_with(cx, |r, cx| r.viewed_count(cx).unwrap()), (1, 2));

    // "Next unviewed" skips viewed files and stops at the end.
    assert_eq!(review.update(cx, |r, cx| r.jump_unviewed(Direction::Next, cx)), Some("b.txt".into()));
    assert_eq!(review.update(cx, |r, cx| r.jump_unviewed(Direction::Next, cx)), None);
    assert_eq!(review.update(cx, |r, cx| r.jump_unviewed(Direction::Previous, cx)), None);
    review.update(cx, |r, cx| r.toggle_viewed("b.txt", cx));
    assert_eq!(review.read_with(cx, |r, cx| r.viewed_count(cx).unwrap()), (2, 2));

    // A file that changes again loses its mark on the next load.
    repo.write("a.txt", "one\nTWO\nthree\n");
    review.update(cx, |r, cx| r.refresh(cx));
    wait_review(cx, &review, "a mark dropped", |r, cx| r.viewed_count(cx).unwrap() == (1, 2));
    assert!(review.read_with(cx, |r, cx| r.viewed_files(cx).unwrap().contains_key("b.txt")));

    // Unmarking.
    review.update(cx, |r, cx| r.toggle_viewed("b.txt", cx));
    assert_eq!(metadata(cx, &workspace, "reviewViewedFiles"), Some(serde_json::json!({})));
}

#[gpui_kit::test]
fn review_comments_are_added_addressed_prompted_and_deleted(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "one\ntwo\nthree\n");
    repo.commit_all("base");
    repo.write("a.txt", "one\nTWO\nthree\n");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let review = open_review(cx, &workspace);
    wait_review(cx, &review, "loaded", |r, _| matches!(r.head(), Head::Known(_)) && matches!(r.content(), ReviewContent::Uncommitted(_)));
    let Head::Known(head) = review.read_with(cx, |r, _| r.head().clone()) else { unreachable!() };

    // Blank text is not saved.
    review.update(cx, |r, cx| r.start_comment("a.txt", Side::Modified, 2, cx));
    review.update(cx, |r, cx| r.submit_comment("   ", cx));
    assert!(review.read_with(cx, |r, _| r.draft().is_some()));
    review.update(cx, |r, cx| r.submit_comment("rename this", cx));
    wait_review(cx, &review, "comment saved", |r, _| r.draft().is_none());
    let comments = review.read_with(cx, |r, cx| r.comments(cx).unwrap());
    assert_eq!(comments.len(), 1);
    let comment = comments[0].clone();
    assert_eq!((comment.file_path.as_str(), comment.line_number, comment.text.as_str()), ("a.txt", 2, "rename this"));
    assert_eq!(comment.side, Side::Modified);
    assert_eq!(comment.commit, CommitAnchor::Head(head.clone()));
    // An uncommitted line blames as all zeros.
    assert!(matches!(&comment.line, LineAnchor::Blamed(hash) if hash.chars().all(|c| c == '0')), "{:?}", comment.line);
    assert_eq!((comment.freshness, comment.resolution), (Freshness::Current, Resolution::Open));
    // Stored the way Electron writes it.
    let raw = workspace.read_with(cx, |w, _| w.file().metadata["reviewComments"].clone());
    let prefix = format!(r#"[{{"filePath":"a.txt","lineNumber":2,"text":"rename this","commitHash":"{head}","isOutdated":false,"addressed":false,"side":"modified","lineCommitHash":""#);
    assert!(raw.starts_with(&prefix), "{raw}");
    assert!(raw.contains(&format!(r#""id":"{}","createdAt":"#, comment.id)), "{raw}");

    review.update(cx, |r, cx| r.toggle_addressed(&comment.id, cx));
    assert_eq!(review.read_with(cx, |r, cx| r.comments(cx).unwrap()[0].resolution), Resolution::Addressed);
    review.update(cx, |r, cx| r.toggle_addressed(&comment.id, cx));
    assert_eq!(review.read_with(cx, |r, cx| r.comments(cx).unwrap()[0].resolution), Resolution::Open);

    // Without a harness the prompt fails and nothing is marked.
    review.update(cx, |r, cx| r.prompt_open_comments(cx));
    assert!(matches!(review.read_with(cx, |r, _| r.notice().clone()), Notice::Error(e) if e.contains("No AI harness")));
    assert_eq!(review.read_with(cx, |r, cx| r.comments(cx).unwrap()[0].resolution), Resolution::Open);
    review.update(cx, |r, cx| r.dismiss_notice(cx));

    // With one, the prompt is typed into it, the tab is shown and the comment is addressed.
    let bot = workspace.update(cx, |w, cx| w.add_tab("aiharness-bot", cx)).unwrap();
    let review_tab = workspace.read_with(cx, |w, _| {
        w.file().app_states.iter().find(|(_, tab)| tab.application_id == "review").unwrap().0.clone()
    });
    workspace.update(cx, |w, cx| w.set_active_tab(&review_tab, cx));
    wait_until_app(cx, &workspace, |w, _| matches!(w.tab_content(&bot), Some(TabContent::Terminal(_))));
    review.update(cx, |r, cx| r.prompt_open_comments(cx));
    assert_eq!(review.read_with(cx, |r, _| r.notice().clone()), Notice::Clear);
    assert_eq!(review.read_with(cx, |r, cx| r.comments(cx).unwrap()[0].resolution), Resolution::Addressed);
    assert_eq!(workspace.read_with(cx, |w, _| w.file().active_tab_id.clone()), Some(bot.clone()));
    let screen = |w: &WorkspaceStore, cx: &gpui_kit::App| match w.tab_content(&bot) {
        Some(TabContent::Terminal(t)) => t.read(cx).screen_text(),
        _ => String::new(),
    };
    wait_until_app(cx, &workspace, |w, cx| screen(w, cx).contains("rename this"));
    assert!(workspace.read_with(cx, |w, cx| screen(w, cx)).contains("Please address the following review comments:"));

    // Committing moves the line's blame: the comment goes outdated.
    repo.commit_all("rename");
    review.update(cx, |r, cx| r.refresh(cx));
    wait_review(cx, &review, "outdated", |r, cx| r.comments(cx).unwrap()[0].freshness == Freshness::Outdated);

    review.update(cx, |r, cx| r.delete_comment(&comment.id, cx));
    assert!(review.read_with(cx, |r, cx| r.comments(cx).unwrap()).is_empty());
    assert_eq!(metadata(cx, &workspace, "reviewComments"), Some(serde_json::json!([])));
}
