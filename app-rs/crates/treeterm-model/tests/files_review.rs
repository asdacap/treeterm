//! The Files tab's search, go to definition and comments, and the Review tab's comments,
//! viewed files and prompts, against a real in-process daemon and real git repos.

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use treeterm_core::files::HiddenEntries;
use treeterm_core::review::{CommitAnchor, Freshness, LineAnchor, Resolution, Side};
use treeterm_core::search::DefinitionLocation;
use treeterm_model::file_browser::{
    Anchor, DefinitionLookup, Favourites, FileBrowser, FileView, Host, LineDraft, Listing, NAME_SEARCH_DEBOUNCE,
    NameResults, REPO_SEARCH_DEBOUNCE, RepoResults, SearchOpen, SearchOpened, SearchPanel, SearchRow, Star,
};
use treeterm_model::review::{
    DiffFind, Direction, FavouriteSelection, GitOp, Head, Notice, ReviewContent, ReviewMode, ReviewModel,
};
use treeterm_model::review_find::DiffLayout;
use treeterm_model::session_store::{SessionState, SessionStore, WorkspaceEntry};
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::text_editor::{EditorDoc, ScrollTarget, TextEditor};
use treeterm_model::traffic::ConnectionTraffic;
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
        let repo = Self::without_commits(h);
        repo.git(&["commit", "-q", "--allow-empty", "-m", "init"]);
        repo
    }

    /// A repo whose `HEAD` cannot be read yet.
    fn without_commits(h: &Harness) -> Self {
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

    /// A worktree beside the repo with `branch` checked out, for a workspace whose branch
    /// must stay what it records: every workspace re-reads its branch from git when its
    /// status refreshes (on load, and after a Review refresh), so two workspaces on one
    /// checkout would both end up on its branch.
    fn worktree(&self, branch: &str) -> String {
        let name = self.path.file_name().unwrap().to_string_lossy();
        let path = self.path.with_file_name(format!("{name}-{branch}"));
        self.git(&["worktree", "add", "-q", &path.to_string_lossy(), branch]);
        path.to_string_lossy().into_owned()
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

    let opened = browser.update(cx, |b, cx| b.open_search(SearchOpen::Shortcut { selection: "needle_fn" }, cx));
    assert_eq!(opened, SearchOpened::Focus);
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
fn picking_the_file_shown_leaves_it_as_it_is(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.rs", "let word = 1;\nlet word = 2;\n");
    repo.write("big.txt", &"x".repeat(1024 * 1024 + 1));
    repo.commit_all("files");
    let (_store, _workspace, browser) = open_browser(cx, &h, &repo, "a.rs");
    let (a, big) = (format!("{}/a.rs", repo.root()), format!("{}/big.txt", repo.root()));

    // Not read again: no "Loading...", the selected match stays, and so does the text read
    // before the file changed on disk.
    let opened = browser.update(cx, |b, cx| b.open_search(SearchOpen::Shortcut { selection: "word" }, cx));
    assert_eq!(opened, SearchOpened::Focus);
    browser.update(cx, |b, cx| b.select_match(1, cx));
    repo.write("a.rs", "changed\n");
    let reads = browser.read_with(cx, |b, _| b.reads());
    browser.update(cx, |b, cx| b.select_file(&a, cx));
    assert!(matches!(browser.read_with(cx, |b, _| b.view().clone()), FileView::Text { text, .. } if text.starts_with("let word")));
    assert_eq!(browser.read_with(cx, |b, _| b.reads()), reads);
    assert_eq!(browser.read_with(cx, |b, _| b.match_count_label()), "2/2");
    assert!(browser.read_with(cx, |b, _| b.reveal_range()).is_some());

    // Reading it again (Review's Refresh) shows what is on disk now.
    browser.update(cx, |b, cx| b.open(&a, false, cx));
    assert!(browser.read_with(cx, |b, _| b.reads()) > reads);
    wait_until(cx, &browser, |b| matches!(b.view(), FileView::Text { text, .. } if text == "changed\n"));

    // A file opened anyway stays open when picked again.
    browser.update(cx, |b, cx| b.select_file(&big, cx));
    wait_until(cx, &browser, |b| !matches!(b.view(), FileView::Loading(_)));
    assert!(matches!(browser.read_with(cx, |b, _| b.view().clone()), FileView::TooLarge { .. }));
    browser.update(cx, |b, cx| b.open(&big, true, cx));
    wait_until(cx, &browser, |b| matches!(b.view(), FileView::Text { .. }));
    browser.update(cx, |b, cx| b.select_file(&big, cx));
    assert!(matches!(browser.read_with(cx, |b, _| b.view().clone()), FileView::Text { .. }));

    // Another file is read.
    browser.update(cx, |b, cx| b.select_file(&a, cx));
    assert!(matches!(browser.read_with(cx, |b, _| b.view().clone()), FileView::Loading(path) if path == a));
}

#[gpui_kit::test]
fn icons_and_avif_files_show_as_images(cx: &mut TestAppContext) {
    use treeterm_core::files::ImageFormat;
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "a\n");
    // Both headers hold NUL bytes, which would make them binary files if read as text.
    let ico = b"\0\0\x01\0\x01\0\x10\x10\0\0\x01\0\x20\0".to_vec();
    let avif = b"\0\0\0\x1cftypavif\0\0\0\0avifmif1miaf".to_vec();
    std::fs::write(repo.path.join("favicon.ICO"), &ico).unwrap();
    std::fs::write(repo.path.join("photo.avif"), &avif).unwrap();
    repo.commit_all("files");
    let (_store, _workspace, browser) = open_browser(cx, &h, &repo, "a.txt");

    for (name, bytes, expected) in [("favicon.ICO", &ico, ImageFormat::Ico), ("photo.avif", &avif, ImageFormat::Avif)] {
        let path = format!("{}/{name}", repo.root());
        browser.update(cx, |b, cx| b.select_file(&path, cx));
        wait_until(cx, &browser, |b| !matches!(b.view(), FileView::Loading(_)));
        match browser.read_with(cx, |b, _| b.view().clone()) {
            FileView::Image { path: shown, bytes: read, format } => {
                assert_eq!((shown, format), (path, expected));
                assert_eq!(*read, *bytes);
            }
            other => panic!("{name}: {other:?}"),
        }
    }
}

#[gpui_kit::test]
fn a_model_shown_again_is_read_again(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "a\n");
    repo.write("part.stl", "solid one\nendsolid one\n");
    repo.commit_all("files");
    let (_store, _workspace, browser) = open_browser(cx, &h, &repo, "a.txt");
    let (text, part) = (format!("{}/a.txt", repo.root()), format!("{}/part.stl", repo.root()));
    let model_bytes = |browser: &Entity<FileBrowser>, cx: &mut TestAppContext| match browser.read_with(cx, |b, _| b.view().clone()) {
        FileView::Model { bytes, .. } => bytes,
        other => panic!("{other:?}"),
    };

    browser.update(cx, |b, cx| b.select_file(&part, cx));
    wait_until(cx, &browser, |b| matches!(b.view(), FileView::Model { .. }));
    let first = model_bytes(&browser, cx);
    assert_eq!(first.as_slice(), b"solid one\nendsolid one\n");

    // Another file, then the model again: its bytes as they are on disk now, in a new read
    // (the viewer builds a fresh page for it, as Electron remounts its viewer).
    repo.write("part.stl", "solid two\nendsolid two\n");
    browser.update(cx, |b, cx| b.select_file(&text, cx));
    wait_until(cx, &browser, |b| matches!(b.view(), FileView::Text { .. }));
    browser.update(cx, |b, cx| b.select_file(&part, cx));
    wait_until(cx, &browser, |b| matches!(b.view(), FileView::Model { .. }));
    let second = model_bytes(&browser, cx);
    assert_eq!(second.as_slice(), b"solid two\nendsolid two\n");
    assert!(!std::sync::Arc::ptr_eq(&first, &second));
}

#[gpui_kit::test]
fn a_files_tab_mounted_again_reads_its_file_again(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "a\n");
    repo.write("part.stl", "solid one\nendsolid one\n");
    repo.commit_all("files");
    let (_store, _workspace, browser) = open_browser(cx, &h, &repo, "a.txt");
    let part = format!("{}/part.stl", repo.root());
    browser.update(cx, |b, cx| b.select_file(&part, cx));
    wait_until(cx, &browser, |b| matches!(b.view(), FileView::Model { .. }));
    let reads = browser.read_with(cx, |b, _| b.reads());

    // Its workspace shown again: the model as it is on disk now, in a new read, still the
    // file selected.
    repo.write("part.stl", "solid two\nendsolid two\n");
    browser.update(cx, |b, cx| b.read_again(cx));
    assert!(matches!(browser.read_with(cx, |b, _| b.view().clone()), FileView::Loading(path) if path == part));
    wait_until(cx, &browser, |b| matches!(b.view(), FileView::Model { .. }));
    match browser.read_with(cx, |b, _| b.view().clone()) {
        FileView::Model { path, bytes, .. } => {
            assert_eq!(path, part);
            assert_eq!(bytes.as_slice(), b"solid two\nendsolid two\n");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(browser.read_with(cx, |b, _| (b.reads(), b.selected().map(str::to_owned))), (reads + 1, Some(part)));
}

#[gpui_kit::test]
fn a_folder_that_failed_to_load_loads_again_when_reopened(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "a\n");
    repo.commit_all("a");
    let (_store, _workspace, browser) = open_browser(cx, &h, &repo, "a.txt");
    let late = format!("{}/late", repo.root());
    let listing = |b: &FileBrowser| b.listing(&late).cloned();

    // The daemon's reason, as it gave it.
    browser.update(cx, |b, cx| b.toggle_dir(&late, cx));
    wait_until(cx, &browser, |b| matches!(listing(b), Some(Listing::Failed(_))));
    let Some(Listing::Failed(reason)) = browser.read_with(cx, |b, _| listing(b)) else { unreachable!() };
    assert!(reason.contains("No such file"), "{reason}");

    // Collapsed and opened again: "Loading..." in place of the old error, then the entries.
    browser.update(cx, |b, cx| b.toggle_dir(&late, cx));
    repo.write("late/b.txt", "b\n");
    browser.update(cx, |b, cx| b.toggle_dir(&late, cx));
    assert!(matches!(browser.read_with(cx, |b, _| listing(b)), Some(Listing::Loading)));
    wait_until(cx, &browser, |b| matches!(listing(b), Some(Listing::Loaded(entries)) if entries.len() == 1));

    // A loaded folder keeps its entries while it is read again.
    browser.update(cx, |b, cx| b.refresh(cx));
    assert!(matches!(browser.read_with(cx, |b, _| listing(b)), Some(Listing::Loaded(_))));
}

#[gpui_kit::test]
fn hidden_files_show_in_the_tree_when_asked_and_the_tab_remembers(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "a\n");
    repo.write(".env", "KEY=1\n");
    repo.write(".config/tool.toml", "x = 1\n");
    repo.commit_all("files");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let (a, config) = (format!("{}/a.txt", repo.root()), format!("{}/.config", repo.root()));
    let names = |b: &FileBrowser| b.rows().into_iter().map(|row| row.name).collect::<Vec<_>>();
    let all = [".config", "tool.toml", ".git", ".env", "a.txt"];

    // Not shown by default, even an expanded hidden directory.
    let (tab, browser) =
        files_tab(cx, &workspace, serde_json::json!({ "selectedPath": a, "expandedDirs": [config] }));
    wait_until(cx, &browser, |b| {
        matches!(b.root_listing(), Some(Listing::Loaded(_))) && matches!(b.listing(&config), Some(Listing::Loaded(_)))
    });
    assert_eq!(browser.read_with(cx, |b, _| names(b)), ["a.txt"]);
    assert_eq!(browser.read_with(cx, |b, _| b.hidden()), HiddenEntries::Skip);
    // Asked for what it already does: nothing is saved.
    browser.update(cx, |b, cx| b.set_hidden(HiddenEntries::Skip, cx));
    cx.run_until_parked();
    assert!(tab_state(cx, &workspace, &tab).get("showHidden").is_none());

    // Shown at once: the daemon listed them all along, nothing is read again.
    browser.update(cx, |b, cx| b.set_hidden(HiddenEntries::Include, cx));
    assert_eq!(browser.read_with(cx, |b, _| names(b)), all);
    cx.run_until_parked();
    let saved = tab_state(cx, &workspace, &tab);
    assert_eq!(saved["showHidden"], serde_json::json!(true));

    // A tab built from that state shows them at once.
    let (rebuilt_tab, rebuilt) = files_tab(cx, &workspace, saved.clone());
    assert_eq!(rebuilt.read_with(cx, |b, _| b.hidden()), HiddenEntries::Include);
    wait_until(cx, &rebuilt, |b| matches!(b.listing(&config), Some(Listing::Loaded(_))));
    assert_eq!(rebuilt.read_with(cx, |b, _| names(b)), all);

    // Another window hid them: they go here too, and nothing is written back.
    let mut theirs = saved;
    theirs["showHidden"] = serde_json::json!(false);
    browser.update(cx, |b, cx| b.follow_state(&theirs, cx));
    assert_eq!(browser.read_with(cx, |b, _| names(b)), ["a.txt"]);
    cx.run_until_parked();
    assert_eq!(tab_state(cx, &workspace, &tab)["showHidden"], serde_json::json!(true));

    // Hidden again from the tab itself: saved as such.
    rebuilt.update(cx, |b, cx| b.set_hidden(HiddenEntries::Skip, cx));
    assert_eq!(rebuilt.read_with(cx, |b, _| names(b)), ["a.txt"]);
    cx.run_until_parked();
    assert_eq!(tab_state(cx, &workspace, &rebuilt_tab)["showHidden"], serde_json::json!(false));
}

#[gpui_kit::test]
fn the_search_panel_opens_on_the_selection_or_the_word_under_the_cursor(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.rs", "let alpha = beta;\n");
    repo.write("style.css", "a { background-color: red }\n");
    repo.commit_all("a");
    let (_store, _workspace, browser) = open_browser(cx, &h, &repo, "a.rs");
    let query = |browser: &FileBrowser| match browser.search() {
        SearchPanel::Open(search) => Some(search.query.clone()),
        SearchPanel::Closed => None,
    };
    let open = |cx: &mut TestAppContext, how: SearchOpen<'_>| browser.update(cx, |b, cx| b.open_search(how, cx));

    // The context menu off a word does nothing: the panel stays closed.
    let equals = browser.read_with(cx, |b, _| offset_of(b, " = ", 0)) + 1;
    assert_eq!(open(cx, SearchOpen::ContextMenu { selection: "", cursor: equals }), SearchOpened::Ignored);
    assert_eq!(browser.read_with(cx, |b, _| query(b)), None);
    // Cmd/Ctrl+F with nothing selected opens it empty.
    assert_eq!(open(cx, SearchOpen::Shortcut { selection: "" }), SearchOpened::Focus);
    assert_eq!(browser.read_with(cx, |b, _| query(b)), Some(String::new()));
    // Multi-line selections are not queries; an open panel keeps its query, and the
    // context menu off a word leaves it alone.
    browser.update(cx, |b, cx| b.set_query("alp", cx));
    assert_eq!(open(cx, SearchOpen::Shortcut { selection: "a\nb" }), SearchOpened::Focus);
    assert_eq!(browser.read_with(cx, |b, _| query(b)), Some("alp".into()));
    assert_eq!(open(cx, SearchOpen::ContextMenu { selection: "a\nb", cursor: equals }), SearchOpened::Ignored);
    assert_eq!(browser.read_with(cx, |b, _| query(b)), Some("alp".into()));
    let beta = browser.read_with(cx, |b, _| offset_of(b, "beta", 0)) + 2;
    assert_eq!(open(cx, SearchOpen::ContextMenu { selection: "", cursor: beta }), SearchOpened::Focus);
    assert_eq!(browser.read_with(cx, |b, _| query(b)), Some("beta".into()));
    assert_eq!(open(cx, SearchOpen::ContextMenu { selection: "alpha", cursor: beta }), SearchOpened::Focus);
    assert_eq!(browser.read_with(cx, |b, _| query(b)), Some("alpha".into()));

    // The word is read as Monaco reads the file's language: a CSS one holds `-`.
    let css = format!("{}/style.css", repo.root());
    browser.update(cx, |b, cx| b.select_file(&css, cx));
    wait_until(cx, &browser, |b| matches!(b.view(), FileView::Text { path, .. } if path.ends_with("style.css")));
    let background = browser.read_with(cx, |b, _| offset_of(b, "background", 0)) + 2;
    assert_eq!(open(cx, SearchOpen::ContextMenu { selection: "", cursor: background }), SearchOpened::Focus);
    assert_eq!(browser.read_with(cx, |b, _| query(b)), Some("background-color".into()));
}

#[gpui_kit::test]
fn the_search_word_is_read_in_the_language_embedded_under_the_cursor(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write(
        "card.vue",
        "<template>\n  <p>{{ foo?.bar }}</p>\n</template>\n<script>\nexport default {\n  \
         done() { this.$emit('done', foo?.bar) },\n}\n</script>\n<style scoped>\n.btn { color: red }\n</style>\n",
    );
    repo.commit_all("a");
    let (_store, _workspace, browser) = open_browser(cx, &h, &repo, "card.vue");
    let search_at = |cx: &mut TestAppContext, needle: &str, nth: usize, into: usize| {
        let cursor = browser.read_with(cx, |b, _| offset_of(b, needle, nth)) + into;
        let opened = browser.update(cx, |b, cx| b.open_search(SearchOpen::ContextMenu { selection: "", cursor }, cx));
        assert_eq!(opened, SearchOpened::Focus);
        browser.read_with(cx, |b, _| match b.search() {
            SearchPanel::Open(search) => search.query.clone(),
            SearchPanel::Closed => panic!("the panel is closed"),
        })
    };

    // Monaco reads a Vue file as HTML, whose words hold `?`, and its script block as
    // JavaScript, whose words end at `?` and hold `$`, and its style block as CSS.
    assert_eq!(search_at(cx, "foo?.bar", 0, 1), "foo?");
    assert_eq!(search_at(cx, "foo?.bar", 1, 1), "foo");
    assert_eq!(search_at(cx, "$emit", 0, 2), "$emit");
    assert_eq!(search_at(cx, ".btn", 0, 2), ".btn");
}

#[gpui_kit::test]
fn searching_for_the_query_again_drops_the_selected_match(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.rs", "let word = 1;\nlet word = 2;\n");
    repo.commit_all("a");
    let (_store, _workspace, browser) = open_browser(cx, &h, &repo, "a.rs");
    let open = |cx: &mut TestAppContext, how: SearchOpen<'_>| browser.update(cx, |b, cx| b.open_search(how, cx));
    let label = |cx: &mut TestAppContext| browser.read_with(cx, |b, _| b.match_count_label());
    let repo_results = |cx: &mut TestAppContext| {
        browser.read_with(cx, |b, _| match b.search() {
            SearchPanel::Open(search) => search.repo.clone(),
            SearchPanel::Closed => panic!("the panel is closed"),
        })
    };

    assert_eq!(open(cx, SearchOpen::Shortcut { selection: "word" }), SearchOpened::Focus);
    cx.executor().advance_clock(REPO_SEARCH_DEBOUNCE);
    wait_until(cx, &browser, |b| match b.search() {
        SearchPanel::Open(search) => matches!(search.repo, RepoResults::Done { .. } | RepoResults::Failed { .. }),
        SearchPanel::Closed => false,
    });
    let searched = repo_results(cx);
    browser.update(cx, |b, cx| b.select_match(1, cx));
    assert_eq!(label(cx), "2/2");

    // Cmd/Ctrl+F with nothing selected only focuses the panel.
    assert_eq!(open(cx, SearchOpen::Shortcut { selection: "" }), SearchOpened::Focus);
    assert_eq!(label(cx), "2/2");
    // The same word again: back to "2 found", and the repo search is not run again.
    let second = browser.read_with(cx, |b, _| offset_of(b, "word", 1)) + 1;
    assert_eq!(open(cx, SearchOpen::ContextMenu { selection: "", cursor: second }), SearchOpened::Focus);
    assert_eq!(label(cx), "2 found");
    assert_eq!(repo_results(cx), searched);
    browser.update(cx, |b, cx| b.select_match(0, cx));
    assert_eq!(label(cx), "1/2");
    assert_eq!(open(cx, SearchOpen::Shortcut { selection: "word" }), SearchOpened::Focus);
    assert_eq!(label(cx), "2 found");
    assert_eq!(repo_results(cx), searched);
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

/// An Editor tab on `path`, once its file is loaded.
fn open_editor(cx: &mut TestAppContext, workspace: &Entity<WorkspaceStore>, path: &str) -> Entity<TextEditor> {
    let tab = workspace.update(cx, |w, cx| w.open_in_editor(path, cx)).unwrap();
    let Some(TabContent::Editor(editor)) = workspace.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not an editor tab");
    };
    wait_until(cx, &editor, |e| matches!(e.doc(), EditorDoc::Ready { .. }));
    editor
}

/// The Editor tabs' states, in no particular order.
fn editor_states(cx: &mut TestAppContext, workspace: &Entity<WorkspaceStore>) -> Vec<serde_json::Value> {
    workspace.read_with(cx, |w, _| {
        w.file().app_states.values().filter(|tab| tab.application_id == "editor").map(|tab| tab.state.clone()).collect()
    })
}

/// The Editor tab's text as it is now.
fn editor_text(cx: &mut TestAppContext, editor: &Entity<TextEditor>) -> String {
    editor.read_with(cx, |e, _| match e.doc() {
        EditorDoc::Ready { current, .. } => current.clone(),
        EditorDoc::Loading | EditorDoc::Error(_) => panic!("not loaded"),
    })
}

#[gpui_kit::test]
fn go_to_definition_in_an_editor_tab_opens_other_files_and_moves_within_its_own(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("src/a.rs", "pub fn helper() {}\n\nfn twice() {}\n");
    repo.write("src/b.rs", "fn local() {}\n\nfn twice() {}\n\nfn main() {\n    helper();\n    local();\n    twice();\n}\n");
    repo.write("notes.txt", "helper\n");
    repo.commit_all("files");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let editor = open_editor(cx, &workspace, &format!("{}/src/b.rs", repo.root()));
    let text = editor_text(cx, &editor);
    let opens_at = |cx: &mut TestAppContext, file: &str| -> Vec<serde_json::Value> {
        editor_states(cx, &workspace)
            .into_iter()
            .filter(|state| state["filePath"].as_str().unwrap().ends_with(file))
            .map(|state| state["scrollToLine"].clone())
            .collect()
    };

    // A definition in another file opens it in a new Editor tab at its line.
    let call = text.find("helper()").unwrap() + 2;
    editor.update(cx, |e, cx| e.go_to_definition(call, cx));
    assert_eq!(editor.read_with(cx, |e, _| e.definition().clone()), DefinitionLookup::Searching { symbol: "helper".into() });
    wait_until(cx, &editor, |e| *e.definition() == DefinitionLookup::Idle);
    assert_eq!(opens_at(cx, "src/a.rs"), [serde_json::json!(1)]);

    // One in its own file moves the editor there; no tab opens.
    let call = text.find("local();").unwrap();
    editor.update(cx, |e, cx| e.go_to_definition(call, cx));
    wait_until(cx, &editor, |e| *e.definition() == DefinitionLookup::Idle);
    assert_eq!(editor.update(cx, |e, cx| e.take_scroll_target(cx)), Some(ScrollTarget::Line(1)));
    assert_eq!(editor_states(cx, &workspace).len(), 2);

    // Several: the chosen one opens.
    let call = text.find("twice();").unwrap();
    editor.update(cx, |e, cx| e.go_to_definition(call, cx));
    wait_until(cx, &editor, |e| matches!(e.definition(), DefinitionLookup::Choose { .. }));
    let DefinitionLookup::Choose { symbol, locations } = editor.read_with(cx, |e, _| e.definition().clone()) else {
        unreachable!()
    };
    assert_eq!(symbol, "twice");
    let mut found: Vec<(String, u64)> = locations.iter().map(|l| (l.file_path.clone(), l.line_number)).collect();
    found.sort();
    assert_eq!(found, vec![("src/a.rs".to_owned(), 3), ("src/b.rs".to_owned(), 3)]);
    let other = locations.iter().position(|l| l.file_path == "src/a.rs").unwrap();
    editor.update(cx, |e, cx| e.choose_definition(other, cx));
    assert_eq!(editor.read_with(cx, |e, _| e.definition().clone()), DefinitionLookup::Idle);
    let mut lines = opens_at(cx, "src/a.rs");
    lines.sort_by_key(|line| line.as_u64());
    assert_eq!(lines, [serde_json::json!(1), serde_json::json!(3)]);

    // The text is searched as it is, unsaved edits included.
    editor.update(cx, |e, cx| e.set_content(format!("helper\n{text}"), cx));
    editor.update(cx, |e, cx| e.go_to_definition(2, cx));
    assert_eq!(editor.read_with(cx, |e, _| e.definition().clone()), DefinitionLookup::Searching { symbol: "helper".into() });
    wait_until(cx, &editor, |e| *e.definition() == DefinitionLookup::Idle);
    assert_eq!(opens_at(cx, "src/a.rs").len(), 3);

    // Off a word nothing is looked up; a failed lookup is dismissed.
    let edited = "helper\n".len();
    let space = edited + text.find(") {").unwrap() + 1;
    editor.update(cx, |e, cx| e.go_to_definition(space, cx));
    assert_eq!(editor.read_with(cx, |e, _| e.definition().clone()), DefinitionLookup::Idle);
    let fn_word = edited + text.find("fn").unwrap();
    editor.update(cx, |e, cx| e.go_to_definition(fn_word, cx));
    wait_until(cx, &editor, |e| !matches!(e.definition(), DefinitionLookup::Searching { .. }));
    assert_eq!(editor.read_with(cx, |e, _| e.definition().clone()), DefinitionLookup::NotFound { symbol: "fn".into() });
    editor.update(cx, |e, cx| e.dismiss_definition(cx));
    assert_eq!(editor.read_with(cx, |e, _| e.definition().clone()), DefinitionLookup::Idle);

    // Plain text has no definitions, nor the context menu's item.
    let notes = open_editor(cx, &workspace, &format!("{}/notes.txt", repo.root()));
    assert!(editor.read_with(cx, |e, _| e.has_definitions()));
    assert!(!notes.read_with(cx, |e, _| e.has_definitions()));
    notes.update(cx, |e, cx| e.go_to_definition(1, cx));
    assert_eq!(notes.read_with(cx, |e, _| e.definition().clone()), DefinitionLookup::Unavailable { language: "plaintext" });
}

/// A tab of `application_id` built from `state` (merged over the application's own), and
/// its id.
fn tab_from(cx: &mut TestAppContext, workspace: &Entity<WorkspaceStore>, application_id: &str, state: serde_json::Value) -> (String, TabContent) {
    let tab = workspace.update(cx, |w, cx| w.add_tab_with(application_id, state, cx)).unwrap();
    let content = workspace.read_with(cx, |w, _| w.tab_content(&tab)).unwrap();
    (tab, content)
}

/// A Files tab built from `state`, once its file is shown as text.
fn files_tab(cx: &mut TestAppContext, workspace: &Entity<WorkspaceStore>, state: serde_json::Value) -> (String, Entity<FileBrowser>) {
    let (tab, TabContent::Files(browser)) = tab_from(cx, workspace, "filesystem", state) else {
        panic!("not a files tab");
    };
    wait_until(cx, &browser, |b| matches!(b.view(), FileView::Text { .. }));
    (tab, browser)
}

#[gpui_kit::test]
fn peek_definition_lists_even_a_lone_definition_and_the_chosen_one_opens(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("src/a.rs", "pub fn helper() {}\n");
    repo.write("src/b.rs", "fn main() {\n    helper();\n}\n");
    repo.commit_all("files");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let lone = vec![DefinitionLocation { file_path: "src/a.rs".into(), line_number: 1 }];
    let peeked = DefinitionLookup::Choose { symbol: "helper".into(), locations: lone };

    // The Editor tab: listed, then opened in an Editor tab at its line when chosen.
    let editor = open_editor(cx, &workspace, &format!("{}/src/b.rs", repo.root()));
    let call = editor_text(cx, &editor).find("helper()").unwrap();
    editor.update(cx, |e, cx| e.peek_definition(call, cx));
    wait_until(cx, &editor, |e| !matches!(e.definition(), DefinitionLookup::Searching { .. }));
    assert_eq!(editor.read_with(cx, |e, _| e.definition().clone()), peeked);
    editor.update(cx, |e, cx| e.choose_definition(0, cx));
    let opened: Vec<serde_json::Value> = editor_states(cx, &workspace)
        .into_iter()
        .filter(|state| state["filePath"].as_str().unwrap().ends_with("src/a.rs"))
        .map(|state| state["scrollToLine"].clone())
        .collect();
    assert_eq!(opened, [serde_json::json!(1)]);

    // The Files viewer lists it too.
    let (_, browser) = files_tab(cx, &workspace, serde_json::json!({ "selectedPath": format!("{}/src/b.rs", repo.root()), "expandedDirs": [] }));
    browser.update(cx, |b, cx| b.peek_definition(call, cx));
    wait_until(cx, &browser, |b| !matches!(b.definition(), DefinitionLookup::Searching { .. }));
    assert_eq!(browser.read_with(cx, |b, _| b.definition().clone()), peeked);
}

#[gpui_kit::test]
fn an_editor_suggests_words_of_the_workspaces_other_open_files_of_its_language(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.rs", "fn alpha() {}\n");
    repo.write("b.rs", "fn beta() {}\n");
    repo.write("c.rs", "fn gamma() {}\n");
    repo.write("notes.md", "delta\n");
    repo.commit_all("files");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let a = open_editor(cx, &workspace, &format!("{}/a.rs", repo.root()));
    let b = open_editor(cx, &workspace, &format!("{}/b.rs", repo.root()));
    open_editor(cx, &workspace, &format!("{}/notes.md", repo.root()));
    files_tab(cx, &workspace, serde_json::json!({ "selectedPath": format!("{}/c.rs", repo.root()), "expandedDirs": [] }));
    // Unsaved edits count: the other editor's text as it is now.
    b.update(cx, |b, cx| b.set_content("fn beta() { epsilon }\n".into(), cx));
    let me = a.entity_id();
    let mut others = a.read_with(cx, |a, cx| a.other_open_texts(me, cx));
    others.sort();
    assert_eq!(others, ["fn beta() { epsilon }\n", "fn gamma() {}\n"]);
}

/// An Editor tab built from `state`, once its file is loaded or failed to load.
fn editor_tab(cx: &mut TestAppContext, workspace: &Entity<WorkspaceStore>, state: serde_json::Value) -> (String, Entity<TextEditor>) {
    let (tab, TabContent::Editor(editor)) = tab_from(cx, workspace, "editor", state) else {
        panic!("not an editor tab");
    };
    wait_until(cx, &editor, |e| !matches!(e.doc(), EditorDoc::Loading));
    (tab, editor)
}

#[gpui_kit::test]
fn the_files_viewer_keeps_its_scroll_offset_for_the_file_it_was_left_on(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", &"line\n".repeat(200));
    repo.write("b.txt", &"other\n".repeat(200));
    repo.commit_all("files");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let (a, b) = (format!("{}/a.txt", repo.root()), format!("{}/b.txt", repo.root()));

    // Built with a saved offset: shown once in its file, after it has loaded.
    let (tab, browser) = files_tab(cx, &workspace, serde_json::json!({ "selectedPath": a, "expandedDirs": [], "scrollTop": 300 }));
    assert_eq!(browser.read_with(cx, |m, _| m.scroll_restore(&b)), None);
    assert_eq!(browser.read_with(cx, |m, _| m.scroll_restore(&a)), Some(300.));
    // Hidden before the viewer got there: the saved offset stays.
    browser.update(cx, |m, cx| m.save_scroll_top(0., cx));
    cx.run_until_parked();
    assert_eq!(tab_state(cx, &workspace, &tab)["scrollTop"], serde_json::json!(300));
    assert_eq!(browser.update(cx, |m, _| m.take_scroll_restore(&a)), Some(300.));
    assert_eq!(browser.update(cx, |m, _| m.take_scroll_restore(&a)), None);

    // Hidden: where it was left is saved, in whole pixels.
    browser.update(cx, |m, cx| m.save_scroll_top(120.4, cx));
    cx.run_until_parked();
    assert_eq!(tab_state(cx, &workspace, &tab)["scrollTop"], serde_json::json!(120));
    // A tab built from that state scrolls there again.
    let saved = tab_state(cx, &workspace, &tab);
    let (_, rebuilt) = files_tab(cx, &workspace, saved);
    assert_eq!(rebuilt.read_with(cx, |m, _| m.scroll_restore(&a)), Some(120.));

    // Another file picked: the offset, which was the last file's, is dropped.
    browser.update(cx, |m, cx| m.select_file(&b, cx));
    cx.run_until_parked();
    let state = tab_state(cx, &workspace, &tab);
    assert_eq!(state["selectedPath"], serde_json::json!(b), "{state}");
    assert!(state.get("scrollTop").is_none(), "{state}");
    // The pending offset of the rebuilt tab goes with its file too.
    rebuilt.update(cx, |m, cx| m.select_file(&b, cx));
    assert_eq!(rebuilt.read_with(cx, |m, _| m.scroll_restore(&a)), None);
    // Read again ("Open anyway"), the same file keeps its offset.
    browser.update(cx, |m, cx| m.save_scroll_top(60., cx));
    browser.update(cx, |m, cx| m.open(&b, true, cx));
    cx.run_until_parked();
    assert_eq!(tab_state(cx, &workspace, &tab)["scrollTop"], serde_json::json!(60));

    // Another window picking another file: the offset the state has for it is shown there.
    browser.update(cx, |m, cx| {
        m.follow_state(&serde_json::json!({ "selectedPath": a, "expandedDirs": [], "scrollTop": 40 }), cx);
    });
    wait_until(cx, &browser, |m| matches!(m.view(), FileView::Text { path, .. } if *path == a));
    assert_eq!(browser.read_with(cx, |m, _| m.scroll_restore(&a)), Some(40.));
    // A place to move to (a search hit) wins over it.
    browser.update(cx, |m, cx| m.open_location("a.txt", 150, 1, 0, cx));
    assert_eq!(browser.read_with(cx, |m, _| m.scroll_restore(&a)), None);
}

#[gpui_kit::test]
fn a_files_tab_opened_at_a_line_moves_there_once(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", &"line\n".repeat(20));
    repo.write("b.txt", "other\n");
    repo.commit_all("files");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let (a, b) = (format!("{}/a.txt", repo.root()), format!("{}/b.txt", repo.root()));

    // The line wins over the saved offset.
    let state = serde_json::json!({ "selectedPath": a, "expandedDirs": [], "scrollToLine": 5, "scrollTop": 120 });
    let (tab, browser) = files_tab(cx, &workspace, state.clone());
    let reveal = browser.read_with(cx, |m, _| m.reveal().cloned()).unwrap();
    assert_eq!((reveal.path.as_str(), reveal.line_number), (a.as_str(), 5));
    assert_eq!(browser.read_with(cx, |m, _| m.scroll_restore(&a)), None);
    // Another move (a search hit) leaves the line in the state.
    browser.update(cx, |m, cx| m.reveal_applied(reveal.seq + 1, cx));
    cx.run_until_parked();
    assert_eq!(tab_state(cx, &workspace, &tab)["scrollToLine"], serde_json::json!(5));
    // Once the viewer has moved there it is used up, and a tab built again does not move.
    browser.update(cx, |m, cx| m.reveal_applied(reveal.seq, cx));
    cx.run_until_parked();
    let after = tab_state(cx, &workspace, &tab);
    assert!(after.get("scrollToLine").is_none(), "{after}");
    assert_eq!(after["scrollTop"], serde_json::json!(120));
    let (_, rebuilt) = files_tab(cx, &workspace, after);
    assert_eq!(rebuilt.read_with(cx, |m, _| m.reveal().cloned()), None);
    // The same line asked for again by another window moves there again.
    browser.update(cx, |m, cx| m.follow_state(&state, cx));
    let again = browser.read_with(cx, |m, _| m.reveal().cloned()).unwrap();
    assert_eq!((again.line_number, again.seq > reveal.seq), (5, true));

    // Another file picked before the viewer got to the line: the line goes with it.
    let (tab, browser) = files_tab(cx, &workspace, state);
    browser.update(cx, |m, cx| m.select_file(&b, cx));
    cx.run_until_parked();
    let after = tab_state(cx, &workspace, &tab);
    assert!(after.get("scrollToLine").is_none() && after.get("scrollTop").is_none(), "{after}");
    assert_eq!(browser.read_with(cx, |m, _| m.reveal().cloned()), None);
}

#[gpui_kit::test]
fn a_files_tab_without_a_state_is_an_invalid_tab(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "one\n");
    repo.commit_all("files");
    let store = open_store(cx, &h);
    let app_states = serde_json::json!({ "t1": { "applicationId": "filesystem", "title": "Files", "state": null } });
    let workspace = open_repo_workspace_with(cx, &h, &store, &repo, app_states);
    let Some(TabContent::Files(browser)) = workspace.read_with(cx, |w, _| w.tab_content("t1")) else {
        panic!("not a files tab");
    };
    cx.run_until_parked();
    browser.read_with(cx, |m, _| {
        assert!(m.is_invalid());
        assert!(m.root_listing().is_none() && m.rows().is_empty());
        assert!(matches!(m.view(), FileView::Nothing));
    });
    assert_eq!(tab_state(cx, &workspace, "t1"), serde_json::Value::Null);
    // Shown, it still reads no `HEAD` for comments.
    browser.update(cx, |m, cx| m.set_visible(true, cx));
    cx.run_until_parked();
    assert!(!matches!(browser.read_with(cx, |m, _| m.anchor().clone()), Anchor::Head(_)));

    // A state with a selection is valid.
    let (_, valid) = files_tab(cx, &workspace, serde_json::json!({ "selectedPath": format!("{}/a.txt", repo.root()) }));
    assert!(!valid.read_with(cx, |m, _| m.is_invalid()));
}

#[gpui_kit::test]
fn an_editor_tab_keeps_its_scroll_offset_and_moves_to_its_line_once(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.rs", &"fn a() {}\n".repeat(100));
    repo.commit_all("files");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let a = format!("{}/a.rs", repo.root());
    let text = "fn a() {}\n".repeat(100);
    let ready = serde_json::json!({
        "status": "ready", "filePath": a, "originalContent": text, "currentContent": text,
        "language": "rust", "isDirty": false, "viewMode": "editor", "scrollTop": 200
    });

    // Built from a loaded state: its offset is shown once.
    let (tab, editor) = editor_tab(cx, &workspace, ready.clone());
    // Hidden before the editor got there: the saved offset stays.
    editor.update(cx, |e, cx| e.save_scroll_top(0., cx));
    cx.run_until_parked();
    assert_eq!(tab_state(cx, &workspace, &tab)["scrollTop"], serde_json::json!(200));
    assert_eq!(editor.update(cx, |e, cx| e.take_scroll_target(cx)), Some(ScrollTarget::Top(200.)));
    assert_eq!(editor.update(cx, |e, cx| e.take_scroll_target(cx)), None);
    // Hidden: where it was left is saved, the rest of the state kept.
    editor.update(cx, |e, cx| e.save_scroll_top(150., cx));
    cx.run_until_parked();
    let state = tab_state(cx, &workspace, &tab);
    assert_eq!((state["scrollTop"].clone(), state["status"].clone()), (serde_json::json!(150), serde_json::json!("ready")));

    // A line asked for wins, and is then removed: a tab built again goes to the offset.
    let mut at_line = ready.clone();
    at_line["scrollToLine"] = serde_json::json!(3);
    let (tab, editor) = editor_tab(cx, &workspace, at_line);
    assert_eq!(editor.update(cx, |e, cx| e.take_scroll_target(cx)), Some(ScrollTarget::Line(3)));
    assert_eq!(editor.update(cx, |e, cx| e.take_scroll_target(cx)), None);
    cx.run_until_parked();
    let state = tab_state(cx, &workspace, &tab);
    assert!(state.get("scrollToLine").is_none(), "{state}");
    let (_, rebuilt) = editor_tab(cx, &workspace, state);
    assert_eq!(rebuilt.update(cx, |e, cx| e.take_scroll_target(cx)), Some(ScrollTarget::Top(200.)));

    // Opened at a line (go to definition): moved there once it has loaded, then removed.
    let tab = workspace.update(cx, |w, cx| w.open_in_editor_at(&a, 7, cx)).unwrap();
    let Some(TabContent::Editor(editor)) = workspace.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not an editor tab");
    };
    wait_until(cx, &editor, |e| matches!(e.doc(), EditorDoc::Ready { .. }));
    cx.run_until_parked();
    assert_eq!(tab_state(cx, &workspace, &tab)["scrollToLine"], serde_json::json!(7));
    assert_eq!(editor.update(cx, |e, cx| e.take_scroll_target(cx)), Some(ScrollTarget::Line(7)));
    cx.run_until_parked();
    let state = tab_state(cx, &workspace, &tab);
    assert!(state.get("scrollToLine").is_none(), "{state}");
    let (_, rebuilt) = editor_tab(cx, &workspace, state);
    assert_eq!(rebuilt.update(cx, |e, cx| e.take_scroll_target(cx)), None);

    // Only a loaded state's offset counts: one still loading opens at the top.
    let (_, loaded) = editor_tab(cx, &workspace, serde_json::json!({ "status": "loading", "filePath": a, "scrollTop": 200 }));
    assert!(matches!(loaded.read_with(cx, |e, _| e.doc().clone()), EditorDoc::Ready { .. }));
    assert_eq!(loaded.update(cx, |e, cx| e.take_scroll_target(cx)), None);
    // A file that failed to load saves no offset.
    let missing = format!("{}/missing.rs", repo.root());
    let (tab, failed) = editor_tab(cx, &workspace, serde_json::json!({ "status": "loading", "filePath": missing }));
    assert!(matches!(failed.read_with(cx, |e, _| e.doc().clone()), EditorDoc::Error(_)));
    failed.update(cx, |e, cx| e.save_scroll_top(80., cx));
    cx.run_until_parked();
    assert!(tab_state(cx, &workspace, &tab).get("scrollTop").is_none());
}

#[gpui_kit::test]
fn links_open_in_the_browser_or_in_an_editor_tab_at_their_line(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("src/a.rs", "one\ntwo\nthree\n");
    repo.write("notes.md", "links\n");
    repo.commit_all("files");
    let (_store, workspace, browser) = open_browser(cx, &h, &repo, "notes.md");
    let a = format!("{}/src/a.rs", repo.root());
    let opened = |cx: &mut TestAppContext| -> Vec<(String, serde_json::Value)> {
        let mut opened: Vec<(String, serde_json::Value)> = editor_states(cx, &workspace)
            .into_iter()
            .map(|state| (state["filePath"].as_str().unwrap().to_owned(), state["scrollToLine"].clone()))
            .collect();
        opened.sort_by_key(|(_, line)| line.as_u64());
        opened
    };

    // From the Files viewer, a `file:` link: an Editor tab on its path (decoded and
    // normalized) at the line its `#L<n>` names.
    let link = format!("file://{}/src/../src/%61.rs#L3", repo.root());
    browser.update(cx, |b, cx| b.open_link(&link, cx));
    assert_eq!(opened(cx), [(a.clone(), serde_json::json!(3))]);
    // A web link: the browser, and no tab.
    browser.update(cx, |b, cx| b.open_link("https://example.com/docs#L3", cx));
    assert_eq!(cx.opened_url().as_deref(), Some("https://example.com/docs#L3"));
    assert_eq!(opened(cx).len(), 1);

    // From an Editor tab, one without a line: at the first.
    let notes = open_editor(cx, &workspace, &format!("{}/notes.md", repo.root()));
    notes.update(cx, |e, cx| e.open_link(&format!("file://{a}"), cx));
    let lines: Vec<serde_json::Value> = opened(cx).into_iter().filter(|(path, _)| *path == a).map(|(_, line)| line).collect();
    assert_eq!(lines, [serde_json::json!(1), serde_json::json!(3)]);
    // One vscode-uri cannot read opens nothing.
    notes.update(cx, |e, cx| e.open_link("file:////x", cx));
    assert_eq!(opened(cx).len(), 3);
}

/// Whether the probed word has a definition, once its search ends.
fn has_definition(cx: &mut TestAppContext, found: treeterm_model::definition::HasDefinition) -> bool {
    let (tx, rx) = futures::channel::oneshot::channel();
    cx.update(|cx| {
        cx.foreground_executor()
            .spawn(async move {
                match tx.send(found.await) {
                    Ok(()) | Err(_) => {} // Err: the test gave up
                }
            })
            .detach();
    });
    common::recv(cx, rx)
}

#[gpui_kit::test]
fn the_cmd_hover_underline_marks_words_with_a_definition(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("src/a.rs", "pub fn helper() {}\n");
    repo.write("src/b.rs", "fn main() {\n    helper();\n    missing();\n}\n");
    repo.write("notes.txt", "helper\n");
    repo.commit_all("files");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let editor = open_editor(cx, &workspace, &format!("{}/src/b.rs", repo.root()));
    let text = editor_text(cx, &editor);
    let probe = editor.read_with(cx, |e, _| e.definition_probe());

    let helper = text.find("helper").unwrap();
    let (word, found) = cx.update(|cx| probe.probe(&text, helper + 3, cx)).unwrap();
    assert_eq!(word, helper..helper + "helper".len());
    // Moving over the same word joins its search.
    let (_, again) = cx.update(|cx| probe.probe(&text, helper, cx)).unwrap();
    assert!(found.ptr_eq(&again));
    assert!(has_definition(cx, found));
    let missing = text.find("missing").unwrap();
    let (_, found) = cx.update(|cx| probe.probe(&text, missing, cx)).unwrap();
    assert!(!has_definition(cx, found));
    // Nothing to look up off a word, or in plain text.
    assert!(cx.update(|cx| probe.probe(&text, text.find('{').unwrap() - 1, cx)).is_none());
    let notes = open_editor(cx, &workspace, &format!("{}/notes.txt", repo.root()));
    let probe = notes.read_with(cx, |e, _| e.definition_probe());
    assert!(cx.update(|cx| probe.probe("helper\n", 1, cx)).is_none());

    // The Files viewer probes the file it shows.
    let state = serde_json::json!({ "selectedPath": format!("{}/src/b.rs", repo.root()), "expandedDirs": [] });
    let tab = workspace.update(cx, |w, cx| w.add_tab_with("filesystem", state, cx)).unwrap();
    let Some(TabContent::Files(browser)) = workspace.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not a files tab");
    };
    assert!(browser.read_with(cx, |b, _| b.definition_probe().is_none()), "nothing shown yet");
    wait_until(cx, &browser, |b| matches!(b.view(), FileView::Text { .. }));
    let probe = browser.read_with(cx, |b, _| b.definition_probe()).unwrap();
    let (_, found) = cx.update(|cx| probe.probe(&text, helper, cx)).unwrap();
    assert!(has_definition(cx, found));
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

/// The relative paths of resolved favourites, sorted; `None` until they are resolved.
fn favourite_list(favourites: &Favourites) -> Option<Vec<String>> {
    match favourites {
        Favourites::Ready(files) => {
            let mut paths: Vec<String> = files.iter().map(|f| f.relative_path.clone()).collect();
            paths.sort();
            Some(paths)
        }
        Favourites::Empty | Favourites::Loading | Favourites::Failed(_) => None,
    }
}

/// A child worktree inherits the favourites its ancestors list (`getWorkspaceFavouritePaths`):
/// its Files tab stars and lists them and its Review lists them, resolved in the child, and
/// both follow the ancestors as they change, join and leave. Only the child's own favourites
/// can be starred or unstarred there.
#[gpui_kit::test]
fn a_child_workspace_inherits_its_ancestors_favourites(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    for file in ["docs/a.md", "docs/b.md", "src/lib.rs", "README.md"] {
        repo.write(file, "text\n");
    }
    repo.commit_all("files");
    repo.git(&["branch", "feature"]);
    let child_dir = repo.worktree("feature");
    let store = open_store(cx, &h);
    let (parent_id, child_id) = (format!("ws-{}", uuid::Uuid::new_v4().simple()), format!("ws-{}", uuid::Uuid::new_v4().simple()));
    let body = |branch: &str, favourites: &[&str]| {
        serde_json::json!({
            "name": branch, "status": "active", "isGitRepo": true, "isWorktree": false, "gitBranch": branch,
            "appStates": {}, "metadata": {}, "favouritePaths": favourites, "createdAt": 1, "lastActivity": 1,
            "parentHash": ""
        })
    };
    // The child's own list holds only paths that normalize to nothing.
    let mut child_body = body("feature", &["", "/"]);
    child_body["parentId"] = serde_json::json!(parent_id);
    std::fs::write(h.daemon.workspaces_dir().join(format!("{child_id}.json")), child_body.to_string()).unwrap();
    std::fs::write(h.daemon.workspaces_dir().join(format!("{parent_id}.json")), body("main", &["docs", "./README.md"]).to_string()).unwrap();
    let child_ref = WorkspaceRef { id: child_id.clone(), path: child_dir.clone() };
    let parent_ref = WorkspaceRef { id: parent_id.clone(), path: repo.root() };
    let loaded = |s: &SessionStore, id: &str| matches!(s.entry(id), Some(WorkspaceEntry::Loaded(_)));

    // The child loads before its parent: no favourites, so no section in Files or Review.
    futures::executor::block_on(h.conn.update_session(vec![child_ref.clone()], None, "test-writer")).unwrap().unwrap();
    wait_until(cx, &store, |s| matches!(s.state(), SessionState::Live) && loaded(s, &child_id));
    let child = store.read_with(cx, |s, _| s.workspace_store(&child_id).unwrap().clone());
    let state = serde_json::json!({ "selectedPath": format!("{child_dir}/README.md"), "expandedDirs": [] });
    let tab = child.update(cx, |w, cx| w.add_tab_with("filesystem", state, cx)).unwrap();
    let Some(TabContent::Files(browser)) = child.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not a files tab");
    };
    let review = open_review(cx, &child);
    wait_review(cx, &review, "loaded", |r, _| !r.is_loading());
    assert_eq!(browser.read_with(cx, |b, _| b.favourites().clone()), Favourites::Empty);
    assert_eq!(review.read_with(cx, |r, _| r.favourites().clone()), Favourites::Empty);

    // The parent joins: the child's Files and Review list its favourites, found in the child.
    futures::executor::block_on(h.conn.update_session(vec![child_ref.clone(), parent_ref], None, "test-writer")).unwrap().unwrap();
    wait_until(cx, &store, |s| loaded(s, &parent_id));
    let parent = store.read_with(cx, |s, _| s.workspace_store(&parent_id).unwrap().clone());
    let inherited = vec!["README.md".to_owned(), "docs/a.md".to_owned(), "docs/b.md".to_owned()];
    wait_until(cx, &browser, |b| favourite_list(b.favourites()).as_ref() == Some(&inherited));
    wait_review(cx, &review, "inherited", |r, _| favourite_list(r.favourites()).as_ref() == Some(&inherited));
    assert_eq!(child.read_with(cx, |w, _| w.all_favourite_paths()), vec!["docs".to_owned(), "README.md".to_owned()]);
    let Favourites::Ready(files) = browser.read_with(cx, |b, _| b.favourites().clone()) else { unreachable!() };
    assert!(files.iter().all(|file| file.path.starts_with(&format!("{child_dir}/"))), "{files:?}");

    // What the parent lists is starred but cannot be unstarred, or starred again, here.
    let star = |cx: &mut TestAppContext, relative: &str| browser.read_with(cx, |b, cx| b.star(&format!("{child_dir}/{relative}"), cx));
    for covered in ["docs", "docs/a.md", "README.md"] {
        assert_eq!(star(cx, covered), Star::Covered, "{covered}");
    }
    assert_eq!(star(cx, "src"), Star::Off);
    browser.update(cx, |b, cx| b.toggle_favourite(&format!("{child_dir}/docs/a.md"), cx));
    browser.update(cx, |b, cx| b.toggle_favourite(&format!("{child_dir}/README.md"), cx));
    child.update(cx, |w, cx| w.add_favourite_path("docs/new.md", cx));
    assert_eq!(child.read_with(cx, |w, _| w.favourite_paths().to_vec()), vec!["".to_owned(), "/".to_owned()]);

    // The child's own favourites come first and are its to unstar.
    browser.update(cx, |b, cx| b.toggle_favourite(&format!("{child_dir}/src"), cx));
    assert_eq!(star(cx, "src"), Star::On);
    assert_eq!(child.read_with(cx, |w, _| w.all_favourite_paths()), vec!["src".to_owned(), "docs".to_owned(), "README.md".to_owned()]);
    let with_src = vec!["README.md".to_owned(), "docs/a.md".to_owned(), "docs/b.md".to_owned(), "src/lib.rs".to_owned()];
    wait_until(cx, &browser, |b| favourite_list(b.favourites()).as_ref() == Some(&with_src));
    wait_review(cx, &review, "own added", |r, _| favourite_list(r.favourites()).as_ref() == Some(&with_src));

    // The parent listing what the child already has changes nothing here: nothing resolves again.
    let resolved_again = std::rc::Rc::new(std::cell::Cell::new(false));
    let (files_flag, review_flag) = (resolved_again.clone(), resolved_again.clone());
    let _files_observe = cx.update(|cx| {
        cx.observe(&browser, move |b, cx| files_flag.set(files_flag.get() || matches!(b.read(cx).favourites(), Favourites::Loading)))
    });
    let _review_observe = cx.update(|cx| {
        cx.observe(&review, move |r, cx| review_flag.set(review_flag.get() || matches!(r.read(cx).favourites(), Favourites::Loading)))
    });
    parent.update(cx, |w, cx| w.add_favourite_path("src", cx));
    cx.run_until_parked();
    assert_eq!(parent.read_with(cx, |w, _| w.favourite_paths().to_vec()), vec!["docs".to_owned(), "./README.md".to_owned(), "src".to_owned()]);
    assert!(!resolved_again.get());
    assert_eq!(browser.read_with(cx, |b, _| favourite_list(b.favourites())), Some(with_src));

    // The parent unstars a folder: both lists resolve again without it.
    parent.update(cx, |w, cx| w.remove_favourite_path("docs", cx));
    let without_docs = vec!["README.md".to_owned(), "src/lib.rs".to_owned()];
    wait_until(cx, &browser, |b| favourite_list(b.favourites()).as_ref() == Some(&without_docs));
    wait_review(cx, &review, "parent's removed", |r, _| favourite_list(r.favourites()).as_ref() == Some(&without_docs));
    assert!(resolved_again.get());
    assert_eq!(star(cx, "docs/a.md"), Star::Off);

    // The parent leaves the session: the child keeps only its own.
    futures::executor::block_on(h.conn.update_session(vec![child_ref], None, "test-writer")).unwrap().unwrap();
    wait_until(cx, &store, |s| s.entry(&parent_id).is_none());
    let own = vec!["src/lib.rs".to_owned()];
    wait_until(cx, &browser, |b| favourite_list(b.favourites()).as_ref() == Some(&own));
    wait_review(cx, &review, "parent gone", |r, _| favourite_list(r.favourites()).as_ref() == Some(&own));
    assert_eq!(star(cx, "README.md"), Star::Off);
}

/// What the Files tab says about its comments, when a wait fails.
fn comment_state(b: &FileBrowser, cx: &gpui_kit::App) -> String {
    format!(
        "anchor {:?}\ndraft {:?}\nadding {:?}\nnotice {:?}\ncomments {:?}",
        b.anchor(),
        b.comment_draft(),
        b.adding_comment(),
        b.comment_notice(),
        b.file_comments(cx)
    )
}

fn wait_files(cx: &mut TestAppContext, browser: &Entity<FileBrowser>, what: &str, done: impl Fn(&FileBrowser, &gpui_kit::App) -> bool) {
    for _ in 0..1000 {
        cx.run_until_parked();
        if browser.read_with(cx, |b, cx| done(b, cx)) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("{what} not reached:\n{}", browser.read_with(cx, comment_state));
}

#[gpui_kit::test]
fn files_comments_are_made_on_a_line_shown_with_the_file_and_go_outdated(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("src/a.rs", "one\ntwo\nthree\n");
    repo.write("b.txt", "bee\n");
    repo.commit_all("files");
    let (_store, workspace, browser) = open_browser(cx, &h, &repo, "src/a.rs");
    let path = format!("{}/src/a.rs", repo.root());
    wait_files(cx, &browser, "HEAD read", |b, _| matches!(b.anchor(), Anchor::Head(_)));
    let Anchor::Head(head) = browser.read_with(cx, |b, _| b.anchor().clone()) else { unreachable!() };
    let committed = repo.git(&["rev-parse", "HEAD"]);
    assert_eq!(head, committed.trim());

    // A click on line 2's number starts a comment there; blank text is not saved.
    browser.update(cx, |b, cx| b.start_comment(2, cx));
    assert_eq!(browser.read_with(cx, |b, _| b.comment_draft().cloned()), Some(LineDraft { path: path.clone(), line_number: 2 }));
    browser.update(cx, |b, cx| b.submit_comment("  ", cx));
    assert!(browser.read_with(cx, |b, _| b.comment_draft().is_some()));
    browser.update(cx, |b, cx| b.submit_comment(" why two? ", cx));
    wait_files(cx, &browser, "comment saved", |b, _| b.comment_draft().is_none());
    assert_eq!(browser.read_with(cx, |b, _| b.adding_comment().clone()), GitOp::Idle);

    // Saved with the path the Review tab uses, on the working-tree side, tracking the line.
    let comments = browser.read_with(cx, |b, cx| b.file_comments(cx).unwrap());
    assert_eq!(comments.len(), 1);
    let comment = comments[0].clone();
    assert_eq!((comment.file_path.as_str(), comment.line_number, comment.text.as_str()), ("src/a.rs", 2, "why two?"));
    assert_eq!(comment.side, Side::Modified);
    assert_eq!(comment.commit, CommitAnchor::Head(head.clone()));
    assert_eq!(comment.line, LineAnchor::Blamed(head.clone()));
    assert_eq!((comment.freshness, comment.resolution), (Freshness::Current, Resolution::Open));

    // Comments Electron's Files tab wrote (absolute paths) show with the file; other files' do not.
    let mut stored: Vec<serde_json::Value> = serde_json::from_value(metadata(cx, &workspace, "reviewComments").unwrap()).unwrap();
    let electron = |id: &str, file: &str| {
        serde_json::json!({
            "id": id, "filePath": file, "lineNumber": 3, "text": format!("from {id}"), "commitHash": head,
            "lineCommitHash": null, "createdAt": 1, "isOutdated": false, "addressed": false, "side": "modified"
        })
    };
    stored.push(electron("absolute", &path));
    stored.push(electron("other", "b.txt"));
    let raw = serde_json::Value::Array(stored).to_string();
    workspace.update(cx, |w, cx| w.update_metadata("reviewComments", &raw, cx));
    let shown: Vec<String> = browser.read_with(cx, |b, cx| b.file_comments(cx).unwrap()).into_iter().map(|c| c.id).collect();
    assert_eq!(shown, vec![comment.id.clone(), "absolute".to_owned()]);

    // Showing another file drops the draft; that file shows its own comments.
    browser.update(cx, |b, cx| b.start_comment(1, cx));
    let other = format!("{}/b.txt", repo.root());
    browser.update(cx, |b, cx| b.open(&other, false, cx));
    assert!(browser.read_with(cx, |b, _| b.comment_draft().is_none()));
    wait_until(cx, &browser, |b| matches!(b.view(), FileView::Text { path, .. } if *path == other));
    let shown: Vec<String> = browser.read_with(cx, |b, cx| b.file_comments(cx).unwrap()).into_iter().map(|c| c.id).collect();
    assert_eq!(shown, vec!["other".to_owned()]);
    browser.update(cx, |b, cx| b.open(&path, false, cx));
    wait_until(cx, &browser, |b| matches!(b.view(), FileView::Text { path: shown, .. } if *shown == path));

    // Committing a change to the line moves its blame: a Files tab opened later marks it outdated.
    repo.write("src/a.rs", "one\nTWO\nthree\n");
    repo.commit_all("shout");
    let state = serde_json::json!({ "selectedPath": path, "expandedDirs": [] });
    let tab = workspace.update(cx, |w, cx| w.add_tab_with("filesystem", state, cx)).unwrap();
    let Some(TabContent::Files(later)) = workspace.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not a files tab");
    };
    wait_files(cx, &later, "outdated", |b, cx| {
        b.file_comments(cx).unwrap().iter().any(|c| c.id == comment.id && c.freshness == Freshness::Outdated)
    });
    // The untracked one went outdated with `HEAD`.
    let absolute = later.read_with(cx, |b, cx| b.file_comments(cx).unwrap()).into_iter().find(|c| c.id == "absolute").unwrap();
    assert_eq!(absolute.freshness, Freshness::Outdated);

    // Deleting removes it from the workspace.
    browser.update(cx, |b, cx| b.delete_comment(&comment.id, cx));
    assert_eq!(browser.read_with(cx, |b, _| b.comment_notice().clone()), Notice::Clear);
    let left: Vec<String> = browser.read_with(cx, |b, cx| b.file_comments(cx).unwrap()).into_iter().map(|c| c.id).collect();
    assert_eq!(left, vec!["absolute".to_owned()]);
}

#[gpui_kit::test]
fn a_repo_without_commits_takes_files_comments_without_a_commit(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::without_commits(&h);
    repo.write("a.txt", "one\ntwo\n");
    let (_store, workspace, browser) = open_browser(cx, &h, &repo, "a.txt");
    wait_files(cx, &browser, "HEAD unreadable", |b, _| matches!(b.anchor(), Anchor::Failed(_)));

    // Saved with no commit, its line untracked (Electron's `currentCommitHash` stays null).
    browser.update(cx, |b, cx| b.start_comment(2, cx));
    browser.update(cx, |b, cx| b.submit_comment("before the first commit", cx));
    wait_files(cx, &browser, "comment saved", |b, _| b.comment_draft().is_none());
    assert_eq!(browser.read_with(cx, |b, _| b.adding_comment().clone()), GitOp::Idle);
    let comments = browser.read_with(cx, |b, cx| b.file_comments(cx).unwrap());
    assert_eq!(comments.len(), 1);
    let comment = comments[0].clone();
    assert_eq!((&comment.commit, &comment.line), (&CommitAnchor::NotInRepo, &LineAnchor::Untracked));
    let stored = metadata(cx, &workspace, "reviewComments").unwrap();
    assert_eq!((&stored[0]["commitHash"], &stored[0]["lineCommitHash"]), (&serde_json::Value::Null, &serde_json::Value::Null));

    // After the first commit, the tab shown reads it; the comment does not go outdated.
    repo.commit_all("first");
    let head = repo.git(&["rev-parse", "HEAD"]).trim().to_owned();
    browser.update(cx, |b, cx| b.set_visible(true, cx));
    wait_files(cx, &browser, "HEAD read", |b, _| *b.anchor() == Anchor::Head(head.clone()));
    let comment = browser.read_with(cx, |b, cx| b.file_comments(cx).unwrap()).remove(0);
    assert_eq!(comment.freshness, Freshness::Current);
}

#[gpui_kit::test]
fn a_files_tab_shown_again_reads_head_again(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "one\ntwo\n");
    repo.commit_all("base");
    let first = repo.git(&["rev-parse", "HEAD"]).trim().to_owned();
    let (_store, _workspace, browser) = open_browser(cx, &h, &repo, "a.txt");
    wait_files(cx, &browser, "HEAD read", |b, _| *b.anchor() == Anchor::Head(first.clone()));
    let anchor = |cx: &mut TestAppContext| browser.read_with(cx, |b, _| b.anchor().clone());

    // Shown for the first time (it was made before): `HEAD` is read again.
    browser.update(cx, |b, cx| b.set_visible(true, cx));
    assert_eq!(anchor(cx), Anchor::Reading);
    wait_files(cx, &browser, "HEAD read again", |b, _| *b.anchor() == Anchor::Head(first.clone()));
    // Shown while shown: nothing is read.
    browser.update(cx, |b, cx| b.set_visible(true, cx));
    assert_eq!(anchor(cx), Anchor::Head(first.clone()));

    browser.update(cx, |b, cx| b.start_comment(2, cx));
    browser.update(cx, |b, cx| b.submit_comment("two?", cx));
    wait_files(cx, &browser, "comment saved", |b, _| b.comment_draft().is_none());
    let before = browser.read_with(cx, |b, cx| b.file_comments(cx).unwrap()).remove(0);
    assert_eq!(before.commit, CommitAnchor::Head(first.clone()));

    // A commit changes the line while the tab is hidden: nothing is read until it shows.
    browser.update(cx, |b, cx| b.set_visible(false, cx));
    repo.write("a.txt", "one\nTWO\n");
    repo.commit_all("shout");
    let second = repo.git(&["rev-parse", "HEAD"]).trim().to_owned();
    cx.run_until_parked();
    assert_eq!(anchor(cx), Anchor::Head(first.clone()));
    browser.update(cx, |b, cx| b.set_visible(true, cx));
    assert_eq!(anchor(cx), Anchor::Reading);
    wait_files(cx, &browser, "HEAD moved", |b, _| *b.anchor() == Anchor::Head(second.clone()));

    // The comment on the changed line went outdated; a new one is anchored to the new `HEAD`.
    let comments = browser.read_with(cx, |b, cx| b.file_comments(cx).unwrap());
    assert_eq!((comments[0].id.as_str(), comments[0].freshness), (before.id.as_str(), Freshness::Outdated));
    browser.update(cx, |b, cx| b.start_comment(1, cx));
    browser.update(cx, |b, cx| b.submit_comment("one?", cx));
    wait_files(cx, &browser, "second comment saved", |b, _| b.comment_draft().is_none());
    let after = browser.read_with(cx, |b, cx| b.file_comments(cx).unwrap()).remove(1);
    assert_eq!((after.text.as_str(), after.commit), ("one?", CommitAnchor::Head(second)));
}

#[gpui_kit::test]
fn files_comments_follow_the_workspace_becoming_a_repository(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    let head = repo.git(&["rev-parse", "HEAD"]).trim().to_owned();
    repo.write("a.txt", "one\n");
    let store = open_store(cx, &h);
    let id = format!("ws-{}", uuid::Uuid::new_v4().simple());
    let file = h.daemon.workspaces_dir().join(format!("{id}.json"));
    let tab = serde_json::json!({
        "applicationId": "filesystem", "title": "Files",
        "state": { "selectedPath": format!("{}/a.txt", repo.root()), "expandedDirs": [] }
    });
    // Each body differs (writers chain `parentHash`, so a body is never seen twice).
    let body = |is_git_repo: bool, last_activity: i64| {
        serde_json::json!({
            "name": "repo", "status": "active", "isGitRepo": is_git_repo, "isWorktree": false, "gitBranch": "main",
            "appStates": { "files": tab }, "activeTabId": "files", "metadata": {}, "createdAt": 1,
            "lastActivity": last_activity, "parentHash": ""
        })
        .to_string()
    };
    std::fs::write(&file, body(false, 1)).unwrap();
    let refs = vec![WorkspaceRef { id: id.clone(), path: repo.root() }];
    futures::executor::block_on(h.conn.update_session(refs, None, "test-writer")).unwrap().unwrap();
    wait_until(cx, &store, |s| matches!(s.state(), SessionState::Live) && matches!(s.entry(&id), Some(WorkspaceEntry::Loaded(_))));
    let workspace = store.read_with(cx, |s, _| s.workspace_store(&id).unwrap().clone());
    let Some(TabContent::Files(browser)) = workspace.read_with(cx, |w, _| w.tab_content("files")) else {
        panic!("not a files tab");
    };
    assert_eq!(browser.read_with(cx, |b, _| b.anchor().clone()), Anchor::NotInRepo);

    // Another window finds the workspace is a repository: `HEAD` is read.
    std::fs::write(&file, body(true, 2)).unwrap();
    wait_files(cx, &browser, "HEAD read", |b, _| *b.anchor() == Anchor::Head(head.clone()));
    // And that it is not: comments have no commit again.
    std::fs::write(&file, body(false, 3)).unwrap();
    wait_files(cx, &browser, "not a repository", |b, _| *b.anchor() == Anchor::NotInRepo);
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
    cx.new(|cx| SessionStore::new(conn, "client-a".into(), settings, None, cx.new(ConnectionTraffic::new), cx))
}

/// The repo as a workspace with no tabs (no terminal whose shell could start while a test's
/// git commands run; see [`serial`]).
fn open_repo_workspace(cx: &mut TestAppContext, h: &Harness, store: &Entity<SessionStore>, repo: &TempRepo) -> Entity<WorkspaceStore> {
    open_repo_workspace_with(cx, h, store, repo, serde_json::json!({}))
}

/// The repo as a workspace with these `appStates`.
fn open_repo_workspace_with(
    cx: &mut TestAppContext,
    h: &Harness,
    store: &Entity<SessionStore>,
    repo: &TempRepo,
    app_states: serde_json::Value,
) -> Entity<WorkspaceStore> {
    let id = format!("ws-{}", uuid::Uuid::new_v4().simple());
    let body = serde_json::json!({
        "name": "repo", "status": "active", "isGitRepo": true, "isWorktree": false, "gitBranch": "main",
        "appStates": app_states, "metadata": {}, "createdAt": 1, "lastActivity": 1, "parentHash": ""
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

    // "Next unviewed" walks the files as listed from the active file (none yet): it skips
    // viewed files, selects the one it finds and stops at the end.
    let order = ["a.txt".to_owned(), "b.txt".to_owned()];
    assert_eq!(review.update(cx, |r, cx| r.jump_unviewed(&order, Direction::Next, cx)), Some("b.txt".into()));
    assert_eq!(review.read_with(cx, |r, _| r.active_file().map(str::to_owned)), Some("b.txt".into()));
    assert_eq!(review.update(cx, |r, _| r.take_scroll_request()), Some("b.txt".into()));
    assert_eq!(review.update(cx, |r, cx| r.jump_unviewed(&order, Direction::Next, cx)), None);
    assert_eq!(review.update(cx, |r, cx| r.jump_unviewed(&order, Direction::Previous, cx)), None);
    assert_eq!(review.update(cx, |r, _| r.take_scroll_request()), None);
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
fn review_follows_the_diff_scroll_and_comes_back_to_the_file_last_clicked(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    for name in ["a.txt", "b.txt", "c.txt"] {
        repo.write(name, "one\n");
    }
    repo.commit_all("base");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    for name in ["a.txt", "b.txt", "c.txt"] {
        repo.write(name, "two\n");
    }
    repo.commit_all("change");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let state = serde_json::json!({ "viewMode": "committed", "baseBranchOverride": "main", "selectedFilePath": "b.txt" });
    let (tab, review) = open_review_with(cx, &workspace, state);
    wait_review(cx, &review, "committed", |r, _| matches!(r.content(), ReviewContent::Committed(_)));
    let active = |cx: &mut TestAppContext| review.read_with(cx, |r, _| r.active_file().map(str::to_owned));
    let band = |paths: &[&str]| paths.iter().map(|p| (*p).to_owned()).collect::<Vec<_>>();

    // The tab state's file is scrolled to by the first list shown, once.
    assert_eq!(review.update(cx, |r, _| r.take_scroll_request()), Some("b.txt".into()));
    assert_eq!(review.update(cx, |r, _| r.take_scroll_request()), None);

    // The last file in the diff's top band is the active one, whichever way the diff
    // scrolls: down, the file entering from below; up, still the lower one.
    assert_eq!(active(cx), None);
    review.update(cx, |r, cx| r.follow_scroll(&band(&["a.txt"]), cx));
    assert_eq!(active(cx), Some("a.txt".into()));
    review.update(cx, |r, cx| r.follow_scroll(&band(&["a.txt", "b.txt"]), cx));
    assert_eq!(active(cx), Some("b.txt".into()));
    review.update(cx, |r, cx| r.follow_scroll(&band(&["b.txt"]), cx));
    assert_eq!(active(cx), Some("b.txt".into()));
    review.update(cx, |r, cx| r.follow_scroll(&band(&["a.txt", "b.txt"]), cx));
    assert_eq!(active(cx), Some("b.txt".into()));
    review.update(cx, |r, cx| r.follow_scroll(&band(&["a.txt"]), cx));
    assert_eq!(active(cx), Some("a.txt".into()));
    // Next unviewed starts from it.
    let order = band(&["a.txt", "b.txt", "c.txt"]);
    assert_eq!(review.update(cx, |r, cx| r.jump_unviewed(&order, Direction::Next, cx)), Some("b.txt".into()));
    review.update(cx, |r, cx| r.follow_scroll(&band(&["a.txt", "b.txt"]), cx));
    assert_eq!(review.update(cx, |r, cx| r.jump_unviewed(&order, Direction::Next, cx)), Some("c.txt".into()));
    assert_eq!(review.update(cx, |r, _| r.take_scroll_request()), Some("c.txt".into()));

    // The same band reported again changes nothing; once other files are listed, the band
    // is watched afresh, and its last file is the active one.
    review.update(cx, |r, cx| r.follow_scroll(&band(&["a.txt", "b.txt"]), cx));
    assert_eq!(active(cx), Some("c.txt".into()));
    review.update(cx, |r, _| r.reobserve_scroll());
    review.update(cx, |r, cx| r.follow_scroll(&band(&["a.txt", "b.txt"]), cx));
    assert_eq!(active(cx), Some("b.txt".into()));
    // Another view's list has none until it reports its band.
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Uncommitted, cx));
    assert_eq!(active(cx), None);
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Committed, cx));

    // A click in the Committed tree is kept in the tab state: a Review opened from it comes
    // back to that file.
    review.update(cx, |r, cx| {
        r.select_file("c.txt", cx);
        r.remember_selected_file("c.txt", cx);
    });
    assert_eq!(active(cx), Some("c.txt".into()));
    let saved = tab_state(cx, &workspace, &tab);
    assert_eq!((saved["selectedFilePath"].as_str(), saved["viewMode"].as_str()), (Some("c.txt"), Some("committed")));
    let (_, again) = open_review_with(cx, &workspace, saved);
    assert_eq!(again.update(cx, |r, _| r.take_scroll_request()), Some("c.txt".into()));
    // `if (!scrollToFile) return`: an empty path is none.
    let (_, empty) = open_review_with(cx, &workspace, serde_json::json!({ "selectedFilePath": "" }));
    assert_eq!(empty.update(cx, |r, _| r.take_scroll_request()), None);
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
    assert!(!review.read_with(cx, |r, cx| r.header(cx).harness));
    review.update(cx, |r, cx| r.prompt_open_comments(cx));
    wait_until(cx, &review, |r| matches!(r.notice(), Notice::Error(e) if e.contains("No AI harness")));
    assert_eq!(review.read_with(cx, |r, cx| r.comments(cx).unwrap()[0].resolution), Resolution::Open);
    review.update(cx, |r, cx| r.dismiss_notice(cx));

    // With one, the prompt is typed into it, the tab is shown and the comment is addressed.
    let bot = workspace.update(cx, |w, cx| w.add_tab("aiharness-bot", cx)).unwrap();
    let review_tab = workspace.read_with(cx, |w, _| {
        w.file().app_states.iter().find(|(_, tab)| tab.application_id == "review").unwrap().0.clone()
    });
    workspace.update(cx, |w, cx| w.set_active_tab(&review_tab, cx));
    wait_until_app(cx, &workspace, |w, _| matches!(w.tab_content(&bot), Some(TabContent::Terminal(_))));
    assert!(review.read_with(cx, |r, cx| r.header(cx).harness));
    review.update(cx, |r, cx| r.prompt_open_comments(cx));
    wait_review(cx, &review, "comment addressed", |r, cx| r.comments(cx).unwrap()[0].resolution == Resolution::Addressed);
    assert_eq!(review.read_with(cx, |r, _| r.notice().clone()), Notice::Clear);
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

#[gpui_kit::test]
fn a_review_favourite_outside_the_diff_shows_in_a_viewer_that_takes_comments(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("notes.md", "one\ntwo\n");
    repo.write("a.txt", "a\n");
    repo.commit_all("base");
    repo.write("a.txt", "A\n");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    workspace.update(cx, |w, cx| {
        w.add_favourite_path("notes.md", cx);
        w.add_favourite_path("a.txt", cx);
    });
    let review = open_review(cx, &workspace);
    wait_review(cx, &review, "favourites", |r, _| matches!(r.favourites(), Favourites::Ready(files) if files.len() == 2));
    let Favourites::Ready(files) = review.read_with(cx, |r, _| r.favourites().clone()) else { unreachable!() };
    let notes = files.iter().find(|f| f.relative_path == "notes.md").unwrap().clone();

    // Nothing is selected until asked; then the file shows in an embedded viewer.
    assert!(review.read_with(cx, |r, _| r.favourite_selection().relative_path().is_none()));
    review.update(cx, |r, cx| r.select_favourite(notes.clone(), cx));
    let FavouriteSelection::Selected { viewer, .. } = review.read_with(cx, |r, _| r.favourite_selection().clone()) else {
        panic!("not selected");
    };
    assert_eq!(viewer.read_with(cx, |b, _| b.host()), Host::Embedded);
    wait_until(cx, &viewer, |b| matches!(b.view(), FileView::Text { path, .. } if *path == notes.path));
    // Selecting it again keeps the viewer.
    review.update(cx, |r, cx| r.select_favourite(notes.clone(), cx));
    let same = matches!(review.read_with(cx, |r, _| r.favourite_selection().clone()), FavouriteSelection::Selected { viewer: v, .. } if v == viewer);
    assert!(same);

    // A comment on it is saved with its relative path, as Electron's Review writes it.
    wait_files(cx, &viewer, "HEAD read", |b, _| matches!(b.anchor(), Anchor::Head(_)));
    viewer.update(cx, |b, cx| b.start_comment(2, cx));
    viewer.update(cx, |b, cx| b.submit_comment("second line", cx));
    wait_files(cx, &viewer, "comment saved", |b, _| b.comment_draft().is_none());
    let comments = review.read_with(cx, |r, cx| r.comments(cx).unwrap());
    assert_eq!(comments.len(), 1);
    assert_eq!((comments[0].file_path.as_str(), comments[0].line_number, comments[0].side), ("notes.md", 2, Side::Modified));

    // A repo search hit opens a Files tab at its line; the viewer keeps its file.
    let hit = treeterm_core::search::RepoHit { file_path: "a.txt".into(), line_number: 1, column: 1, preview: "A".into() };
    viewer.update(cx, |b, cx| b.open_repo_hit(&hit, cx));
    let files_tab = workspace.read_with(cx, |w, _| {
        w.file().app_states.values().find(|tab| tab.application_id == "filesystem").map(|tab| tab.state.clone())
    });
    let files_tab = files_tab.expect("a Files tab");
    assert_eq!(files_tab["selectedPath"], serde_json::json!(format!("{}/a.txt", repo.root())));
    assert_eq!(files_tab["scrollToLine"], serde_json::json!(1));
    assert!(viewer.read_with(cx, |b, _| matches!(b.view(), FileView::Text { path, .. } if *path == notes.path)));

    // Selecting anything in the diff gives the pane back to it.
    review.update(cx, |r, cx| r.show_diff(cx));
    assert!(review.read_with(cx, |r, _| r.favourite_selection().relative_path().is_none()));

    // When the favourites change, the selection goes.
    review.update(cx, |r, cx| r.select_favourite(notes.clone(), cx));
    assert_eq!(review.read_with(cx, |r, _| r.favourite_selection().relative_path().map(str::to_owned)), Some("notes.md".to_owned()));
    workspace.update(cx, |w, cx| w.remove_favourite_path("a.txt", cx));
    wait_review(cx, &review, "selection dropped", |r, _| r.favourite_selection().relative_path().is_none());
}

#[gpui_kit::test]
fn review_find_lists_the_diffs_matches_this_file_first_and_steps_through_them(cx: &mut TestAppContext) {
    use treeterm_core::search::{DiffStyle, UnchangedRegions};
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "one\nneedle two\nthree\n");
    repo.write("b.txt", "needle b\n");
    repo.write("c.txt", "a needle elsewhere\n");
    repo.commit_all("base");
    repo.write("a.txt", "one\nneedle TWO\nthree\n");
    repo.write("b.txt", "needle B\n");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let review = open_review(cx, &workspace);
    wait_review(cx, &review, "listed", |r, _| matches!(r.content(), ReviewContent::Uncommitted(u) if u.changes.files.len() == 2));
    read_all(cx, &review);
    let ReviewContent::Uncommitted(uncommitted) = review.read_with(cx, |r, _| r.content().clone()) else { unreachable!() };
    let patch = uncommitted.diffs.patch();
    let files = vec!["a.txt".to_owned(), "b.txt".to_owned()];
    let layout = DiffLayout { style: DiffStyle::Split, unchanged: UnchangedRegions::Collapsed };
    let matches = |review: &Entity<ReviewModel>, cx: &mut TestAppContext| {
        review.read_with(cx, |r, _| r.find_matches(&patch, &files, layout))
    };

    // Cmd+F opens an empty bar on the file being read.
    assert_eq!(review.read_with(cx, |r, _| r.find().clone()), DiffFind::Closed { query: String::new() });
    review.update(cx, |r, cx| r.open_find("b.txt", cx));
    let DiffFind::Open(opened) = review.read_with(cx, |r, _| r.find().clone()) else { panic!("not open") };
    assert_eq!((opened.query.as_str(), opened.file.as_str(), opened.active.is_none()), ("", "b.txt", true));
    assert!(matches(&review, cx).is_empty());

    // That file's matches come first, then the rest of the diff; both sides of a change.
    review.update(cx, |r, cx| r.set_find_query("needle", cx));
    let found = matches(&review, cx);
    assert_eq!(
        found.iter().map(|m| (m.path.as_str(), m.side, m.line)).collect::<Vec<_>>(),
        vec![
            ("b.txt", Side::Original, 1),
            ("b.txt", Side::Modified, 1),
            ("a.txt", Side::Original, 2),
            ("a.txt", Side::Modified, 2),
        ]
    );
    assert_eq!(found[3].preview, "needle TWO");
    assert_eq!(review.read_with(cx, |r, _| r.find_count_label(&found)), "4 found");

    // Enter and Shift+Enter walk them, wrapping; each choice is a new target to reveal.
    review.update(cx, |r, cx| r.step_find(&found, true, cx));
    assert_eq!(review.read_with(cx, |r, _| r.find_count_label(&found)), "1/4");
    review.update(cx, |r, cx| r.step_find(&found, false, cx));
    assert_eq!(review.read_with(cx, |r, _| r.find_count_label(&found)), "4/4");
    let target = |review: &Entity<ReviewModel>, cx: &mut TestAppContext| match review.read_with(cx, |r, _| r.find().clone()) {
        DiffFind::Open(state) => state.active.clone().expect("a match is chosen"),
        DiffFind::Closed { .. } => panic!("closed"),
    };
    let last = target(&review, cx);
    assert_eq!((last.path.as_str(), last.side, last.line), ("a.txt", Side::Modified, 2));
    review.update(cx, |r, cx| r.select_find_match(&found, 3, cx));
    assert!(target(&review, cx).seq > last.seq, "choosing it again moves there again");

    // The repo search lists other files only.
    cx.executor().advance_clock(treeterm_model::search_panel::REPO_SEARCH_DEBOUNCE);
    wait_review(cx, &review, "repo searched", |r, _| match r.find() {
        DiffFind::Open(state) => matches!(state.repo, RepoResults::Done { .. } | RepoResults::Failed { .. }),
        DiffFind::Closed { .. } => false,
    });
    let rows = review.read_with(cx, |r, _| r.find_rows(&found, &files));
    assert_eq!(rows[0], SearchRow::Section("In this file"));
    assert_eq!(rows[1], SearchRow::File("b.txt".into()));
    assert!(matches!(rows[2], SearchRow::InDiff { index: 0, side: Side::Original, line_number: 1, active: false, .. }));
    assert_eq!(rows[4], SearchRow::Section("In diff"));
    assert_eq!(rows[5], SearchRow::File("a.txt".into()));
    assert!(matches!(rows[7], SearchRow::InDiff { index: 3, active: true, .. }), "{rows:?}");
    assert_eq!(rows[8], SearchRow::Section("In repo"));
    assert_eq!(rows[9], SearchRow::File("c.txt".into()), "{rows:?}");
    let SearchRow::InRepo(hit) = rows[10].clone() else { panic!("{rows:?}") };
    assert_eq!(rows.len(), 12, "the diff's files are not listed again: {rows:?}");

    // A repo hit opens a Files tab at its line.
    review.update(cx, |r, cx| r.open_find_hit(&hit, cx));
    let files_tab = workspace.read_with(cx, |w, _| {
        w.file().app_states.values().find(|tab| tab.application_id == "filesystem").map(|tab| tab.state.clone())
    });
    let files_tab = files_tab.expect("a Files tab");
    assert_eq!(files_tab["selectedPath"], serde_json::json!(format!("{}/c.txt", repo.root())));
    assert_eq!(files_tab["scrollToLine"], serde_json::json!(1));

    // Closing keeps the query for the next Cmd+F, which focuses the box again.
    review.update(cx, |r, cx| r.close_find(cx));
    assert_eq!(review.read_with(cx, |r, _| r.find().clone()), DiffFind::Closed { query: "needle".to_owned() });
    assert!(matches(&review, cx).is_empty());
    review.update(cx, |r, cx| r.open_find("a.txt", cx));
    let DiffFind::Open(reopened) = review.read_with(cx, |r, _| r.find().clone()) else { panic!("not open") };
    assert_eq!((reopened.query.as_str(), reopened.file.as_str()), ("needle", "a.txt"));
    assert!(reopened.focus > opened.focus);
    assert_eq!(matches(&review, cx)[0].path, "a.txt", "the file being read comes first");

    // "Search" from the diff's menu starts over on the selection.
    review.update(cx, |r, cx| r.search_in_diff("TWO", "a.txt", cx));
    let found = matches(&review, cx);
    assert_eq!(found.iter().map(|m| (m.side, m.line)).collect::<Vec<_>>(), vec![(Side::Modified, 2)]);

    // Another view, another diff: the bar goes.
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Commits, cx));
    assert_eq!(review.read_with(cx, |r, _| r.find().clone()), DiffFind::Closed { query: String::new() });
}

#[gpui_kit::test]
fn review_find_opens_only_while_the_diff_list_is_on_screen(cx: &mut TestAppContext) {
    use treeterm_model::review::Loaded;
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("notes.md", "notes\n");
    let (workspace, review) = feature_review(cx, &h, &repo, "commits");
    // Cmd+F: whether the bar opens (closed again after).
    let opens = |cx: &mut TestAppContext| {
        review.update(cx, |r, cx| r.open_find("a.txt", cx));
        let open = matches!(review.read_with(cx, |r, _| r.find().clone()), DiffFind::Open(_));
        assert_eq!(open, review.read_with(cx, |r, _| r.diff_on_screen()));
        review.update(cx, |r, cx| r.close_find(cx));
        open
    };

    // Commits: not while it loads, nor before a commit is picked; then even while the
    // commit's diff loads (its list shows "0/0 viewed").
    assert!(matches!(review.read_with(cx, |r, _| r.content()), ReviewContent::Loading));
    assert!(!opens(cx));
    wait_review(cx, &review, "commits", |r, _| matches!(r.content(), ReviewContent::Commits(c) if c.len() == 1));
    assert!(!opens(cx));
    let picked = review.read_with(cx, |r, _| match r.content() {
        ReviewContent::Commits(commits) => commits[0].hash.clone(),
        other => panic!("{other:?}"),
    });
    review.update(cx, |r, cx| r.select_commit(&picked, cx));
    assert!(matches!(review.read_with(cx, |r, _| r.selected_commit().map(|s| s.diff.clone())), Some(Loaded::Loading)));
    assert!(opens(cx));

    // Committed and Uncommitted with changes.
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Committed, cx));
    wait_review(cx, &review, "committed", |r, _| matches!(r.content(), ReviewContent::Committed(c) if c.files.len() == 1));
    assert!(opens(cx));
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Uncommitted, cx));
    wait_review(cx, &review, "uncommitted", |r, _| matches!(r.content(), ReviewContent::Uncommitted(u) if u.changes.files.len() == 1));
    assert!(opens(cx));

    // A favourite's viewer in the diff's place: not until the diff is back.
    workspace.update(cx, |w, cx| w.add_favourite_path("notes.md", cx));
    wait_review(cx, &review, "favourites", |r, _| matches!(r.favourites(), Favourites::Ready(files) if files.len() == 1));
    let Favourites::Ready(files) = review.read_with(cx, |r, _| r.favourites().clone()) else { unreachable!() };
    // The bar goes with the list the viewer replaces: back to the diff, it opens empty.
    review.update(cx, |r, cx| r.open_find("a.txt", cx));
    review.update(cx, |r, cx| r.set_find_query("notes", cx));
    review.update(cx, |r, cx| r.select_favourite(files[0].clone(), cx));
    assert_eq!(review.read_with(cx, |r, _| r.find().clone()), DiffFind::Closed { query: String::new() });
    assert!(!opens(cx));
    review.update(cx, |r, cx| r.show_diff(cx));
    assert!(opens(cx));

    // No changes ("No committed changes to show"), and a diff that failed to load.
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Committed, cx));
    review.update(cx, |r, cx| r.set_base(Some("feature".into()), cx));
    wait_review(cx, &review, "no changes", |r, _| matches!(r.content(), ReviewContent::Committed(c) if c.files.is_empty()));
    assert!(!opens(cx));
    review.update(cx, |r, cx| r.set_base(Some("no-such-branch".into()), cx));
    wait_review(cx, &review, "failed", |r, _| matches!(r.content(), ReviewContent::Failed(_)));
    assert!(!opens(cx));
}

/// A Review tab with `state` (e.g. a base override) on the repo's workspace, and its tab id.
fn open_review_with(cx: &mut TestAppContext, workspace: &Entity<WorkspaceStore>, state: serde_json::Value) -> (String, Entity<ReviewModel>) {
    let tab = workspace.update(cx, |w, cx| w.add_tab_with("review", state, cx)).unwrap();
    let Some(TabContent::Review(review)) = workspace.read_with(cx, |w, _| w.tab_content(&tab)) else {
        panic!("not a review tab");
    };
    (tab, review)
}

fn tab_state(cx: &mut TestAppContext, workspace: &Entity<WorkspaceStore>, tab: &str) -> serde_json::Value {
    workspace.read_with(cx, |w, _| w.file().app_states[tab].state.clone())
}

#[gpui_kit::test]
fn review_counts_views_lists_commits_and_shows_a_commits_changes(cx: &mut TestAppContext) {
    use treeterm_model::review::{CommitDiff, Loaded, ReviewEvent, ReviewMode};
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "one\ntwo\n");
    repo.commit_all("base");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("a.txt", "one\nTWO\n");
    repo.write("dir/b.txt", "bee\n");
    repo.commit_all("first");
    repo.write("a.txt", "one  \nTWO\n");
    repo.commit_all("second");
    repo.write("c.txt", "uncommitted\n");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let (tab, review) =
        open_review_with(cx, &workspace, serde_json::json!({ "viewMode": "committed", "baseBranchOverride": "main" }));
    let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = events.clone();
    let _events = cx.update(|cx| {
        cx.subscribe(&review, move |_, event: &ReviewEvent, _| seen.borrow_mut().push(event.clone()))
    });

    // Every view's count arrives whatever is shown; the summary names both branches.
    wait_review(cx, &review, "counts", |r, _| r.overview().committed == 2 && r.overview().uncommitted == 1);
    wait_review(cx, &review, "committed", |r, _| matches!(r.content(), ReviewContent::Committed(_)));
    read_all(cx, &review);
    let ReviewContent::Committed(committed) = review.read_with(cx, |r, _| r.content().clone()) else { unreachable!() };
    assert_eq!((committed.base_branch.as_str(), committed.head_branch.as_str()), ("main", "feature"));
    assert_eq!((committed.total_additions, committed.total_deletions), (3, 2));
    // Patches carry whole files (here all of `a.txt`), so unchanged lines can be revealed.
    let patch = committed.diffs.patch();
    assert!(patch.contains("@@ -1,2 +1,2 @@\n-one\n-two\n+one  \n+TWO"), "{patch}");
    assert!(review.read_with(cx, |r, cx| r.is_base_overridden(cx)));

    // Ignoring whitespace, a line respaced at its end is unchanged.
    review.update(cx, |r, cx| r.set_ignore_whitespace(true, cx));
    wait_review(cx, &review, "ignoring whitespace", |r, _| {
        matches!(r.content(), ReviewContent::Committed(c) if c.diffs.is_complete() && !c.diffs.patch().contains("+one  "))
    });
    assert!(review.read_with(cx, |r, _| r.ignore_whitespace()));
    review.update(cx, |r, cx| r.set_ignore_whitespace(false, cx));

    // Commits: listed since the base, newest first; a picked one shows its files and patch,
    // and its files can be marked viewed together.
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Commits, cx));
    wait_review(cx, &review, "commits", |r, _| matches!(r.content(), ReviewContent::Commits(c) if c.len() == 2));
    assert_eq!((review.read_with(cx, |r, _| r.overview().commits), review.read_with(cx, |r, _| r.overview().more_commits)), (2, false));
    let ReviewContent::Commits(commits) = review.read_with(cx, |r, _| r.content().clone()) else { unreachable!() };
    assert_eq!(commits[0].message, "second");
    review.update(cx, |r, cx| r.select_commit(&commits[1].hash, cx));
    wait_review(cx, &review, "commit diff", |r, _| matches!(r.selected_commit().map(|s| &s.diff), Some(Loaded::Ready(_))));
    read_all(cx, &review);
    let Some(Loaded::Ready(CommitDiff { files, diffs })) = review.read_with(cx, |r, _| r.selected_commit().map(|s| s.diff.clone())) else {
        unreachable!()
    };
    let patch = diffs.patch();
    let mut paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    paths.sort();
    assert_eq!(paths, ["a.txt", "dir/b.txt"]);
    assert!(patch.contains("+++ b/dir/b.txt"), "{patch}");
    let all = vec!["a.txt".to_owned(), "dir/b.txt".to_owned()];
    review.update(cx, |r, cx| r.set_files_viewed(&all, true, cx));
    assert_eq!(review.read_with(cx, |r, cx| r.viewed_count(cx).unwrap()), (2, 2));
    review.update(cx, |r, cx| r.set_files_viewed(&all[..1], false, cx));
    assert_eq!(review.read_with(cx, |r, cx| r.viewed_count(cx).unwrap()), (1, 2));

    // Ignoring whitespace reads the picked commit's changes again, which stay shown
    // meanwhile (the diff keeps its place).
    let shown = |r: &ReviewModel, whitespace: bool| {
        matches!(r.selected_commit().map(|s| &s.diff), Some(Loaded::Ready(d)) if d.diffs.is_complete() && d.diffs.patch().contains("+one  ") == whitespace)
    };
    review.update(cx, |r, cx| r.select_commit(&commits[0].hash, cx));
    wait_review(cx, &review, "the second commit", |r, _| matches!(r.selected_commit().map(|s| &s.diff), Some(Loaded::Ready(_))));
    read_all(cx, &review);
    assert!(review.read_with(cx, |r, _| shown(r, true)));
    review.update(cx, |r, cx| r.set_ignore_whitespace(true, cx));
    assert!(review.read_with(cx, |r, _| shown(r, true)));
    wait_review(cx, &review, "the second commit ignoring whitespace", |r, _| shown(r, false));
    review.update(cx, |r, cx| r.set_ignore_whitespace(false, cx));
    wait_review(cx, &review, "the second commit with whitespace", |r, _| shown(r, true));

    // Resetting the base drops the override from the tab state (Electron's `undefined`); with
    // no parent there is then no base, and the Commits view lists the whole history.
    review.update(cx, |r, cx| r.set_base(None, cx));
    assert!(tab_state(cx, &workspace, &tab).get("baseBranchOverride").is_none(), "{}", tab_state(cx, &workspace, &tab));
    assert_eq!(review.read_with(cx, |r, _| r.base().clone()), treeterm_model::review::ReviewBase::None);
    wait_review(cx, &review, "all commits", |r, _| matches!(r.content(), ReviewContent::Commits(c) if c.len() == 4));
    review.update(cx, |r, cx| r.set_base(Some("main".into()), cx));
    assert_eq!(tab_state(cx, &workspace, &tab)["baseBranchOverride"], "main");

    // "Cancel" / "Close" closes the tab.
    review.update(cx, |r, cx| r.close(cx));
    cx.run_until_parked();
    assert!(events.borrow().contains(&ReviewEvent::CloseRequested));
    assert!(workspace.read_with(cx, |w, _| !w.file().app_states.contains_key(&tab)));
}

#[gpui_kit::test]
fn ignoring_whitespace_hides_a_change_at_a_lines_ends_but_not_within_it(cx: &mut TestAppContext) {
    use treeterm_model::review::{Loaded, ReviewMode};
    use treeterm_model::review_patch::FileDiff;
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "fn main() {\nlet x = 1;\nrun(x);\n}\n");
    repo.write("s.txt", "s\nx y\n");
    repo.write("w.txt", "w\nx y\n");
    repo.commit_all("base");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    // Re-indented, and respaced within a line (which `git diff -w` would hide too).
    repo.write("a.txt", "fn main() {\n    let x = 1;\nrun( x);\n}\n");
    repo.commit_all("feature");
    repo.write("s.txt", "  s\nx  y\n");
    repo.git(&["add", "s.txt"]);
    repo.write("w.txt", "w  \nx\ty\n");
    repo.write("u.txt", "  u\n");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let (_, review) =
        open_review_with(cx, &workspace, serde_json::json!({ "viewMode": "committed", "baseBranchOverride": "main" }));
    let section = |cx: &mut TestAppContext, path: &str| match file_diff(cx, &review, path) {
        Some(FileDiff::Loaded(section)) => section,
        other => panic!("{path}: {other:?}"),
    };
    wait_review(cx, &review, "committed", |r, _| matches!(r.content(), ReviewContent::Committed(_)));
    read_all(cx, &review);
    assert!(section(cx, "a.txt").contains("-let x = 1;\n-run(x);\n+    let x = 1;\n+run( x);\n"), "{}", section(cx, "a.txt"));

    // The re-indented line is unchanged, shown as it is now; the respaced one still changed.
    let ignored = "\n     let x = 1;\n-run(x);\n+run( x);\n";
    review.update(cx, |r, cx| r.set_ignore_whitespace(true, cx));
    wait_review(cx, &review, "committed, ignoring whitespace", |r, _| {
        matches!(r.content(), ReviewContent::Committed(c) if c.diffs.is_complete() && c.diffs.patch().contains(ignored))
    });
    // A file read again on its own reads the same.
    review.update(cx, |r, cx| r.refresh_file("a.txt", cx));
    wait_review(cx, &review, "a.txt read again", |r, _| !r.is_file_loading("a.txt"));
    assert!(section(cx, "a.txt").contains(ignored), "{}", section(cx, "a.txt"));

    // So are the staged and unstaged changes; an untracked file is all added.
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Uncommitted, cx));
    wait_review(cx, &review, "uncommitted", |r, _| matches!(r.content(), ReviewContent::Uncommitted(_)));
    read_all(cx, &review);
    assert!(section(cx, "s.txt").contains("@@ -1,2 +1,2 @@\n   s\n-x y\n+x  y\n"), "{}", section(cx, "s.txt"));
    assert!(section(cx, "w.txt").contains("@@ -1,2 +1,2 @@\n w  \n-x y\n+x\ty\n"), "{}", section(cx, "w.txt"));
    assert!(section(cx, "u.txt").contains("@@ -0,0 +1 @@\n+  u\n"), "{}", section(cx, "u.txt"));

    // And a commit's.
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Commits, cx));
    wait_review(cx, &review, "commits", |r, _| matches!(r.content(), ReviewContent::Commits(c) if c.len() == 1));
    let hash = review.read_with(cx, |r, _| match r.content() {
        ReviewContent::Commits(commits) => commits[0].hash.clone(),
        other => panic!("{other:?}"),
    });
    review.update(cx, |r, cx| r.select_commit(&hash, cx));
    wait_review(cx, &review, "the commit", |r, _| matches!(r.selected_commit().map(|s| &s.diff), Some(Loaded::Ready(_))));
    read_all(cx, &review);
    assert!(section(cx, "a.txt").contains(ignored), "{}", section(cx, "a.txt"));
}

#[gpui_kit::test]
fn selecting_a_commit_keeps_the_branchs_viewed_marks(cx: &mut TestAppContext) {
    use treeterm_model::review::Loaded;
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "one\ntwo\n");
    repo.commit_all("base");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("a.txt", "ONE\ntwo\n");
    repo.commit_all("first");
    repo.write("a.txt", "ONE\nTWO\n");
    repo.commit_all("second");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let (_, review) =
        open_review_with(cx, &workspace, serde_json::json!({ "viewMode": "committed", "baseBranchOverride": "main" }));
    wait_review(cx, &review, "committed", |r, _| matches!(r.content(), ReviewContent::Committed(_)));
    review.update(cx, |r, cx| r.toggle_viewed("a.txt", cx));
    let marked = metadata(cx, &workspace, "reviewViewedFiles");
    assert_eq!(marked, Some(serde_json::json!({ "a.txt": { "kind": "text", "additions": 2, "deletions": 2 } })));

    // The first commit changed the file differently from the whole branch: picking it, or
    // reloading while it is picked, keeps the branch's mark (`loadCommitDiff` does not
    // reconcile).
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Commits, cx));
    wait_review(cx, &review, "commits", |r, _| matches!(r.content(), ReviewContent::Commits(c) if c.len() == 2));
    let first = review.read_with(cx, |r, _| match r.content() {
        ReviewContent::Commits(commits) => commits[1].hash.clone(),
        other => panic!("{other:?}"),
    });
    review.update(cx, |r, cx| r.select_commit(&first, cx));
    wait_review(cx, &review, "commit diff", |r, _| matches!(r.selected_commit().map(|s| &s.diff), Some(Loaded::Ready(_))));
    assert_eq!(metadata(cx, &workspace, "reviewViewedFiles"), marked);
    assert_eq!(review.read_with(cx, |r, cx| r.viewed_count(cx).unwrap()), (1, 1));
    review.update(cx, |r, cx| r.refresh(cx));
    wait_review(cx, &review, "refreshed", |r, _| !r.is_refreshing());
    assert_eq!(metadata(cx, &workspace, "reviewViewedFiles"), marked);
}

/// A repo on `feature`, whose commit changed `a.txt` since `main`, with `u.txt` changed in
/// the working tree, and a Review of it comparing against `main`.
fn feature_review(cx: &mut TestAppContext, h: &Harness, repo: &TempRepo, view: &str) -> (Entity<WorkspaceStore>, Entity<ReviewModel>) {
    repo.write("a.txt", "one\n");
    repo.write("u.txt", "u\n");
    repo.commit_all("base");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("a.txt", "ONE\n");
    repo.commit_all("feature");
    repo.write("u.txt", "U\n");
    let store = open_store(cx, h);
    let workspace = open_repo_workspace(cx, h, &store, repo);
    let (_, review) =
        open_review_with(cx, &workspace, serde_json::json!({ "viewMode": view, "baseBranchOverride": "main" }));
    (workspace, review)
}

#[gpui_kit::test]
fn switching_review_views_shows_what_they_loaded_at_once(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    let (_workspace, review) = feature_review(cx, &h, &repo, "committed");
    assert!(matches!(review.read_with(cx, |r, _| r.content()), ReviewContent::Loading));
    // Both views load whatever is shown.
    wait_review(cx, &review, "both views", |r, _| {
        !r.is_loading() && r.overview().committed == 1 && r.overview().uncommitted == 1
    });
    assert!(matches!(review.read_with(cx, |r, _| r.content()), ReviewContent::Committed(_)));
    assert!(!review.read_with(cx, |r, _| r.is_refreshing()));
    review.update(cx, |r, cx| r.start_comment("a.txt", Side::Modified, 1, cx));

    // A switch shows the other view's data at once, reads nothing from git, and keeps the
    // comment being written (`commentInput`).
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Uncommitted, cx));
    assert!(matches!(review.read_with(cx, |r, _| r.content()), ReviewContent::Uncommitted(u) if u.changes.files.len() == 1));
    assert!(!review.read_with(cx, |r, _| r.is_loading()));
    assert!(review.read_with(cx, |r, _| r.draft().is_some()));
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Committed, cx));
    assert!(matches!(review.read_with(cx, |r, _| r.content()), ReviewContent::Committed(c) if c.files.len() == 1));
    assert!(!review.read_with(cx, |r, _| r.is_loading()));

    // Staging reloads the working tree only, keeping it on screen meanwhile.
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Uncommitted, cx));
    review.update(cx, |r, cx| r.stage(treeterm_model::review::StageTarget::All, cx));
    wait_review(cx, &review, "staged", |r, _| {
        matches!(r.content(), ReviewContent::Uncommitted(u) if u.has_staged()) && !r.is_loading()
    });
}

#[gpui_kit::test]
fn the_commits_list_loads_once_and_keeps_its_pages_and_selection(cx: &mut TestAppContext) {
    use treeterm_model::review::{Loaded, ReviewBase};
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "a\n");
    repo.commit_all("base");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    for index in 0..60 {
        repo.git(&["commit", "-q", "--allow-empty", "-m", &format!("c{index}")]);
    }
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let (_, review) =
        open_review_with(cx, &workspace, serde_json::json!({ "viewMode": "committed", "baseBranchOverride": "main" }));
    wait_review(cx, &review, "loaded", |r, _| !r.is_loading());
    // Not listed until shown.
    assert_eq!(review.read_with(cx, |r, _| r.overview().commits), 0);
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Commits, cx));
    assert!(matches!(review.read_with(cx, |r, _| r.content()), ReviewContent::Loading));
    wait_review(cx, &review, "a page", |r, _| matches!(r.content(), ReviewContent::Commits(c) if c.len() == 50));
    assert!(review.read_with(cx, |r, _| r.overview().more_commits));
    review.update(cx, |r, cx| r.load_more_commits(cx));
    wait_review(cx, &review, "two pages", |r, _| matches!(r.content(), ReviewContent::Commits(c) if c.len() == 60));
    assert!(!review.read_with(cx, |r, _| r.overview().more_commits));
    let listed = |r: &ReviewModel| match r.content() {
        ReviewContent::Commits(commits) => commits,
        other => panic!("{other:?}"),
    };
    let picked = review.read_with(cx, |r, _| listed(r)[55].hash.clone());
    review.update(cx, |r, cx| r.select_commit(&picked, cx));
    wait_review(cx, &review, "commit diff", |r, _| matches!(r.selected_commit().map(|s| &s.diff), Some(Loaded::Ready(_))));
    let selected = |r: &ReviewModel| r.selected_commit().map(|s| s.commit.hash.clone());

    // Back from another view: the same list at once, with the commit still picked.
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Committed, cx));
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Commits, cx));
    assert!(!review.read_with(cx, |r, _| r.is_loading()));
    assert_eq!(review.read_with(cx, |r, _| listed(r).len()), 60);
    assert_eq!(review.read_with(cx, |r, _| selected(r)), Some(picked.clone()));

    // "Refresh" reloads as many as are listed, the commit still picked.
    review.update(cx, |r, cx| r.refresh(cx));
    assert!(review.read_with(cx, |r, _| r.is_refreshing()));
    assert_eq!(review.read_with(cx, |r, _| listed(r).len()), 60, "kept on screen meanwhile");
    wait_review(cx, &review, "refreshed", |r, _| !r.is_refreshing());
    assert_eq!(review.read_with(cx, |r, _| listed(r).len()), 60);
    assert_eq!(review.read_with(cx, |r, _| selected(r)), Some(picked));

    // Another base lists other commits: the list starts over.
    review.update(cx, |r, cx| r.set_base(None, cx));
    assert_eq!(review.read_with(cx, |r, _| r.base().clone()), ReviewBase::None);
    assert!(matches!(review.read_with(cx, |r, _| r.content()), ReviewContent::Loading));
    assert_eq!(review.read_with(cx, |r, _| selected(r)), None);
    wait_review(cx, &review, "all commits", |r, _| matches!(r.content(), ReviewContent::Commits(c) if c.len() == 50));
}

#[gpui_kit::test]
fn the_diff_list_is_the_views_and_in_commits_the_picked_commits(cx: &mut TestAppContext) {
    use treeterm_model::review::{DiffList, Loaded};
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "one\n");
    repo.commit_all("base");
    repo.git(&["branch", "other"]);
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("a.txt", "two\n");
    repo.commit_all("first");
    repo.write("a.txt", "three\n");
    repo.commit_all("second");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let (_, review) =
        open_review_with(cx, &workspace, serde_json::json!({ "viewMode": "committed", "baseBranchOverride": "main" }));
    wait_review(cx, &review, "committed", |r, _| matches!(r.content(), ReviewContent::Committed(_)));
    let list = |cx: &mut TestAppContext| review.read_with(cx, |r, _| r.diff_list());
    assert_eq!(list(cx), Some(DiffList::Committed));

    // Commits: no list until a commit is picked, then the commit's.
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Commits, cx));
    wait_review(cx, &review, "commits", |r, _| matches!(r.content(), ReviewContent::Commits(c) if c.len() == 2));
    assert_eq!(list(cx), None);
    let picked = review.read_with(cx, |r, _| match r.content() {
        ReviewContent::Commits(commits) => commits[1].hash.clone(),
        other => panic!("{other:?}"),
    });
    review.update(cx, |r, cx| r.select_commit(&picked, cx));
    assert_eq!(list(cx), Some(DiffList::Commit(picked.clone())));
    wait_review(cx, &review, "commit diff", |r, _| matches!(r.selected_commit().map(|s| &s.diff), Some(Loaded::Ready(_))));

    // Committed keeps its list when another base drops the picked commit (the diff keeps
    // its place and collapsed files).
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Committed, cx));
    assert_eq!(list(cx), Some(DiffList::Committed));
    review.update(cx, |r, cx| r.set_base(Some("other".into()), cx));
    assert!(review.read_with(cx, |r, _| r.selected_commit().is_none()));
    assert_eq!(list(cx), Some(DiffList::Committed));
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Uncommitted, cx));
    assert_eq!(list(cx), Some(DiffList::Uncommitted));
    // And Commits shows none again.
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Commits, cx));
    assert_eq!(list(cx), None);
    wait_review(cx, &review, "commits since other", |r, _| matches!(r.content(), ReviewContent::Commits(c) if c.len() == 2));
}

#[gpui_kit::test]
fn an_empty_commits_list_shows_loading_while_it_reloads(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    // On `main` against `main`: no commits on the branch.
    let (_, review) =
        open_review_with(cx, &workspace, serde_json::json!({ "viewMode": "commits", "baseBranchOverride": "main" }));
    let empty = |r: &ReviewModel| matches!(r.content(), ReviewContent::Commits(c) if c.is_empty());
    wait_review(cx, &review, "no commits", |r, _| !r.is_loading() && empty(r));

    // Shown again while none are listed: "Loading commits..." until they are read again.
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Uncommitted, cx));
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Commits, cx));
    assert!(matches!(review.read_with(cx, |r, _| r.content()), ReviewContent::Loading));
    wait_review(cx, &review, "no commits again", |r, _| !r.is_loading() && empty(r));
    review.update(cx, |r, cx| r.refresh(cx));
    assert!(matches!(review.read_with(cx, |r, _| r.content()), ReviewContent::Loading));
    wait_review(cx, &review, "refreshed", |r, _| !r.is_refreshing() && empty(r));
}

#[gpui_kit::test]
fn viewed_marks_follow_both_views_whatever_is_shown(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    let (workspace, review) = feature_review(cx, &h, &repo, "committed");
    wait_review(cx, &review, "both views", |r, _| !r.is_loading() && r.overview().uncommitted == 1);
    review.update(cx, |r, cx| r.toggle_viewed("a.txt", cx));
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Uncommitted, cx));
    review.update(cx, |r, cx| r.toggle_viewed("u.txt", cx));
    let marks = |cx: &mut TestAppContext| {
        let mut marked: Vec<String> =
            metadata(cx, &workspace, "reviewViewedFiles").unwrap().as_object().unwrap().keys().cloned().collect();
        marked.sort();
        marked
    };
    assert_eq!(marks(cx), ["a.txt", "u.txt"]);

    // The branch changed `a.txt` again while the working tree is shown: its mark goes
    // (`loadDiff` reconciles whatever the view).
    repo.write("a.txt", "ONE\nTWO\n");
    repo.git(&["add", "a.txt"]);
    repo.git(&["commit", "-q", "-m", "more"]);
    review.update(cx, |r, cx| r.refresh(cx));
    wait_review(cx, &review, "refreshed", |r, _| !r.is_refreshing());
    assert_eq!(marks(cx), ["u.txt"]);

    // And the working tree's file while the branch is shown (`loadUncommittedChanges`).
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Committed, cx));
    repo.write("u.txt", "U\nV\n");
    review.update(cx, |r, cx| r.refresh(cx));
    wait_review(cx, &review, "refreshed", |r, _| !r.is_refreshing());
    assert_eq!(marks(cx), Vec::<String>::new());
}

#[gpui_kit::test]
fn only_refresh_spins_and_resolves_the_favourites_again(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("notes.md", "notes\n");
    repo.commit_all("base");
    repo.write("new.txt", "fresh\n");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    workspace.update(cx, |w, cx| w.add_favourite_path("notes.md", cx));
    let review = open_review(cx, &workspace);
    wait_review(cx, &review, "loaded", |r, _| !r.is_loading() && matches!(r.favourites(), Favourites::Ready(_)));
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(bool, bool)>::new()));
    let record = seen.clone();
    let _observe = cx.update(|cx| {
        cx.observe(&review, move |review, cx| {
            let review = review.read(cx);
            record.borrow_mut().push((review.is_refreshing(), matches!(review.favourites(), Favourites::Loading)));
        })
    });

    // Staging reloads the working tree, quietly.
    review.update(cx, |r, cx| r.stage(treeterm_model::review::StageTarget::All, cx));
    wait_review(cx, &review, "staged", |r, _| {
        matches!(r.content(), ReviewContent::Uncommitted(u) if u.has_staged()) && !r.is_loading()
    });
    assert!(!seen.borrow().is_empty());
    assert!(seen.take().iter().all(|states| *states == (false, false)));

    // "Refresh" spins until done and resolves the favourites again.
    review.update(cx, |r, cx| r.refresh(cx));
    assert!(review.read_with(cx, |r, _| r.is_refreshing()));
    wait_review(cx, &review, "refreshed", |r, _| !r.is_refreshing() && matches!(r.favourites(), Favourites::Ready(_)));
    assert!(seen.borrow().iter().any(|(_, loading)| *loading), "{:?}", seen.borrow());
}

#[gpui_kit::test]
fn a_review_shown_again_refreshes(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "a\n");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let listed = |r: &ReviewModel| match r.content() {
        ReviewContent::Uncommitted(u) => u.changes.files.len(),
        other => panic!("{other:?}"),
    };

    // First shown while its opening load runs: that load is fresh.
    let review = open_review(cx, &workspace);
    assert!(review.read_with(cx, |r, _| r.is_loading()));
    review.update(cx, |r, cx| r.set_visible(true, cx));
    assert!(!review.read_with(cx, |r, _| r.is_refreshing()));
    wait_review(cx, &review, "loaded", |r, _| !r.is_loading());
    assert_eq!(review.read_with(cx, |r, _| listed(r)), 1);
    // Shown again while shown: nothing.
    review.update(cx, |r, cx| r.set_visible(true, cx));
    assert!(!review.read_with(cx, |r, _| r.is_refreshing()));

    // First shown once it loaded (made when the session connected, shown later): it
    // refreshes.
    let later = open_review(cx, &workspace);
    wait_review(cx, &later, "loaded", |r, _| !r.is_loading());
    repo.write("c.txt", "c\n");
    later.update(cx, |r, cx| r.set_visible(true, cx));
    assert!(later.read_with(cx, |r, _| r.is_refreshing()));
    wait_review(cx, &later, "refreshed", |r, _| !r.is_refreshing());
    assert_eq!(later.read_with(cx, |r, _| listed(r)), 2);

    // Hidden, then shown again: it refreshes.
    review.update(cx, |r, cx| r.set_visible(false, cx));
    repo.write("b.txt", "b\n");
    assert!(!review.read_with(cx, |r, _| r.is_refreshing()));
    review.update(cx, |r, cx| r.set_visible(true, cx));
    assert!(review.read_with(cx, |r, _| r.is_refreshing()));
    wait_review(cx, &review, "refreshed", |r, _| !r.is_refreshing());
    assert_eq!(review.read_with(cx, |r, _| listed(r)), 3);
    review.update(cx, |r, cx| r.set_visible(true, cx));
    assert!(!review.read_with(cx, |r, _| r.is_refreshing()));
}

#[gpui_kit::test]
fn the_workspaces_own_branch_changing_reloads_the_review(cx: &mut TestAppContext) {
    use treeterm_model::review::Loaded;
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "one\n");
    repo.commit_all("base");
    repo.git(&["checkout", "-q", "-b", "other"]);
    repo.write("b.txt", "bee\n");
    repo.commit_all("on other");
    repo.git(&["checkout", "-q", "main"]);
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("a.txt", "ONE\n");
    repo.commit_all("on feature");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let (_, review) =
        open_review_with(cx, &workspace, serde_json::json!({ "viewMode": "commits", "baseBranchOverride": "main" }));
    let messages = |r: &ReviewModel| match r.content() {
        ReviewContent::Commits(commits) => commits.into_iter().map(|c| c.message).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    wait_review(cx, &review, "commits", |r, _| messages(r) == ["on feature"] && !r.is_loading());
    let commit = review.read_with(cx, |r, _| match r.content() {
        ReviewContent::Commits(commits) => commits[0].hash.clone(),
        other => panic!("{other:?}"),
    });
    review.update(cx, |r, cx| r.select_commit(&commit, cx));
    wait_review(cx, &review, "commit diff", |r, _| matches!(r.selected_commit().map(|s| &s.diff), Some(Loaded::Ready(_))));

    // Checked out elsewhere: the branch's commits and changes reload; the old pick goes.
    repo.git(&["checkout", "-q", "other"]);
    workspace.update(cx, |w, cx| w.set_git_branch("other", cx));
    assert!(review.read_with(cx, |r, _| r.selected_commit().is_none()));
    assert!(!review.read_with(cx, |r, _| r.is_refreshing()));
    wait_review(cx, &review, "the other branch", |r, _| messages(r) == ["on other"] && !r.is_loading());
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Committed, cx));
    let ReviewContent::Committed(committed) = review.read_with(cx, |r, _| r.content()) else { unreachable!() };
    assert_eq!(committed.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["b.txt"]);
}

#[gpui_kit::test]
fn a_checkout_the_git_info_refresh_finds_reloads_the_review(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "one\n");
    repo.commit_all("base");
    repo.git(&["checkout", "-q", "-b", "other"]);
    repo.write("b.txt", "bee\n");
    repo.commit_all("on other");
    repo.git(&["checkout", "-q", "main"]);
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("a.txt", "ONE\n");
    repo.commit_all("on feature");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let (_, review) =
        open_review_with(cx, &workspace, serde_json::json!({ "viewMode": "commits", "baseBranchOverride": "main" }));
    let messages = |r: &ReviewModel| match r.content() {
        ReviewContent::Commits(commits) => commits.into_iter().map(|c| c.message).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    wait_review(cx, &review, "commits", |r, _| messages(r) == ["on feature"] && !r.is_loading());
    // The git status refresh on load has recorded `feature` and finished.
    let id = workspace.read_with(cx, |w, _| w.workspace().id.clone());
    wait_until_app(cx, &store, |s, cx| {
        s.workspace_store(&id).is_some_and(|w| w.read(cx).file().git_branch.as_deref() == Some("feature"))
            && s.git_controller(&id).is_some_and(|c| !c.read(cx).is_refreshing())
    });

    // Checked out in the terminal; the AI going idle re-reads the git info.
    repo.git(&["checkout", "-q", "other"]);
    workspace.update(cx, |w, cx| w.refresh_git_info(cx));
    wait_review(cx, &review, "the other branch", |r, _| messages(r) == ["on other"] && !r.is_loading());
}

#[gpui_kit::test]
fn the_parents_branch_changing_keeps_the_review_and_moves_its_default_base(cx: &mut TestAppContext) {
    use treeterm_model::review::ReviewBase;
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "one\n");
    repo.commit_all("base");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("a.txt", "ONE\n");
    repo.commit_all("on feature");
    // `dev` already has the feature's commit.
    repo.git(&["branch", "dev"]);
    repo.git(&["checkout", "-q", "main"]);
    let child_dir = repo.worktree("feature");
    let store = open_store(cx, &h);
    // A parent on `main` (the repo) and a child on `feature` (a worktree of it).
    let (parent_id, child_id) = (format!("ws-{}", uuid::Uuid::new_v4().simple()), format!("ws-{}", uuid::Uuid::new_v4().simple()));
    let body = |branch: &str, parent: Option<&str>| {
        let mut body = serde_json::json!({
            "name": branch, "status": "active", "isGitRepo": true, "isWorktree": false, "gitBranch": branch,
            "appStates": {}, "metadata": {}, "createdAt": 1, "lastActivity": 1, "parentHash": ""
        });
        if let Some(parent) = parent {
            body["parentId"] = serde_json::json!(parent);
        }
        body.to_string()
    };
    std::fs::write(h.daemon.workspaces_dir().join(format!("{parent_id}.json")), body("main", None)).unwrap();
    std::fs::write(h.daemon.workspaces_dir().join(format!("{child_id}.json")), body("feature", Some(&parent_id))).unwrap();
    let refs = vec![
        WorkspaceRef { id: parent_id.clone(), path: repo.root() },
        WorkspaceRef { id: child_id.clone(), path: child_dir },
    ];
    futures::executor::block_on(h.conn.update_session(refs, None, "test-writer")).unwrap().unwrap();
    let loaded = |s: &SessionStore, id: &str| matches!(s.entry(id), Some(WorkspaceEntry::Loaded(_)));
    wait_until(cx, &store, |s| matches!(s.state(), SessionState::Live) && loaded(s, &parent_id) && loaded(s, &child_id));
    let parent = store.read_with(cx, |s, _| s.workspace_store(&parent_id).unwrap().clone());
    let child = store.read_with(cx, |s, _| s.workspace_store(&child_id).unwrap().clone());
    // The session tells children their parent's branch whenever a workspace changes.
    parent.update(cx, |_, cx| cx.notify());
    wait_until_app(cx, &child, |w, _| w.parent_branch() == Some("main"));
    let (follows_tab, follows) = open_review_with(cx, &child, serde_json::json!({ "viewMode": "committed" }));
    let (_, pinned) = open_review_with(cx, &child, serde_json::json!({ "viewMode": "committed", "baseBranchOverride": "main" }));
    for review in [&follows, &pinned] {
        wait_review(cx, review, "loaded", |r, _| !r.is_loading() && r.overview().committed == 1);
    }

    // The parent moves to `dev`: the tab following it compares against `dev` (nothing left
    // to review), as the same model; the tab with an override keeps comparing with `main`.
    repo.git(&["checkout", "-q", "dev"]);
    parent.update(cx, |w, cx| w.set_git_branch("dev", cx));
    wait_until_app(cx, &child, |w, _| w.parent_branch() == Some("dev"));
    assert_eq!(follows.read_with(cx, |r, _| r.base().clone()), ReviewBase::Branch("dev".into()));
    let same = child.read_with(cx, |w, _| match w.tab_content(&follows_tab) {
        Some(TabContent::Review(model)) => model.entity_id() == follows.entity_id(),
        _ => false,
    });
    assert!(same, "the Review tab keeps its model (and so its view)");
    wait_review(cx, &follows, "reloaded", |r, _| !r.is_loading() && r.overview().committed == 0);
    assert!(matches!(follows.read_with(cx, |r, _| r.content()), ReviewContent::Committed(c) if c.base_branch == "dev"));
    assert_eq!(pinned.read_with(cx, |r, _| r.base().clone()), ReviewBase::Branch("main".into()));
    // It only checks its conflicts against `dev`.
    wait_review(cx, &pinned, "checked", |r, _| !r.is_loading());
    assert_eq!(pinned.read_with(cx, |r, _| r.overview().committed), 1);
}

/// A parent workspace on `main` (the repo, checked out on `main` here) and its child on
/// `feature` (a worktree of it, with `tabs` as its `appStates`), and their refs. Only the
/// child joins the session: its Review tabs are made while its parent's branch is unknown,
/// as when the child's body loads first at start-up. The parent joins with [`add_parent`].
fn child_before_parent(
    cx: &mut TestAppContext,
    h: &Harness,
    repo: &TempRepo,
    tabs: serde_json::Value,
) -> (Entity<SessionStore>, Entity<WorkspaceStore>, Vec<WorkspaceRef>) {
    repo.git(&["checkout", "-q", "main"]);
    let child_dir = repo.worktree("feature");
    let store = open_store(cx, h);
    let (parent_id, child_id) = (format!("ws-{}", uuid::Uuid::new_v4().simple()), format!("ws-{}", uuid::Uuid::new_v4().simple()));
    let body = |branch: &str, tabs: serde_json::Value| {
        serde_json::json!({
            "name": branch, "status": "active", "isGitRepo": true, "isWorktree": false, "gitBranch": branch,
            "appStates": tabs, "metadata": {}, "createdAt": 1, "lastActivity": 1, "parentHash": ""
        })
    };
    let mut child_body = body("feature", tabs);
    child_body["parentId"] = serde_json::json!(parent_id);
    let parent_body = body("main", serde_json::json!({}));
    std::fs::write(h.daemon.workspaces_dir().join(format!("{parent_id}.json")), parent_body.to_string()).unwrap();
    std::fs::write(h.daemon.workspaces_dir().join(format!("{child_id}.json")), child_body.to_string()).unwrap();
    let child_ref = WorkspaceRef { id: child_id.clone(), path: child_dir };
    futures::executor::block_on(h.conn.update_session(vec![child_ref.clone()], None, "test-writer")).unwrap().unwrap();
    let loaded = |s: &SessionStore| matches!(s.entry(&child_id), Some(WorkspaceEntry::Loaded(_)));
    wait_until(cx, &store, |s| matches!(s.state(), SessionState::Live) && loaded(s));
    let child = store.read_with(cx, |s, _| s.workspace_store(&child_id).unwrap().clone());
    assert_eq!(child.read_with(cx, |w, _| w.parent_branch().map(str::to_owned)), None);
    (store, child, vec![child_ref, WorkspaceRef { id: parent_id, path: repo.root() }])
}

/// The parent from [`child_before_parent`] joins the session, and the child learns its branch.
fn add_parent(cx: &mut TestAppContext, h: &Harness, store: &Entity<SessionStore>, child: &Entity<WorkspaceStore>, refs: Vec<WorkspaceRef>) {
    let parent_id = refs[1].id.clone();
    futures::executor::block_on(h.conn.update_session(refs, None, "test-writer")).unwrap().unwrap();
    wait_until(cx, store, |s| matches!(s.entry(&parent_id), Some(WorkspaceEntry::Loaded(_))));
    let parent = store.read_with(cx, |s, _| s.workspace_store(&parent_id).unwrap().clone());
    // The session tells children their parent's branch whenever a workspace changes.
    parent.update(cx, |_, cx| cx.notify());
    wait_until_app(cx, child, |w, _| w.parent_branch() == Some("main"));
}

/// A child's Review tab on the tab state's id.
fn review_tab(cx: &mut TestAppContext, workspace: &Entity<WorkspaceStore>, tab: &str) -> Entity<ReviewModel> {
    match workspace.read_with(cx, |w, _| w.tab_content(tab)) {
        Some(TabContent::Review(review)) => review,
        _ => panic!("{tab} is not a review tab"),
    }
}

#[gpui_kit::test]
fn a_childs_review_made_before_its_parents_branch_is_known_shows_committed_once_it_is(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "one\n");
    repo.commit_all("base");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("a.txt", "ONE\n");
    repo.commit_all("on feature");
    // Restored with the child: one on the default view, one with Committed saved.
    let tab = |state: serde_json::Value| serde_json::json!({ "applicationId": "review", "title": "Review", "state": state });
    let tabs = serde_json::json!({ "default": tab(serde_json::json!({})), "saved": tab(serde_json::json!({ "viewMode": "committed" })) });
    let (store, child, refs) = child_before_parent(cx, &h, &repo, tabs);
    let (default, saved) = (review_tab(cx, &child, "default"), review_tab(cx, &child, "saved"));
    // Opened as well, and the Uncommitted standing in for Committed picked.
    let (picked_tab, picked) = open_review_with(cx, &child, serde_json::json!({ "viewMode": "committed" }));
    for review in [&default, &saved, &picked] {
        wait_review(cx, review, "loaded", |r, _| !r.is_loading());
        assert_eq!(review.read_with(cx, |r, _| r.mode()), ReviewMode::Uncommitted);
    }
    picked.update(cx, |r, cx| r.set_mode(ReviewMode::Uncommitted, cx));
    assert_eq!(tab_state(cx, &child, &picked_tab)["viewMode"], "uncommitted");

    // The parent's branch arrives: Committed comes back, on the same models (so the views
    // keep their layout), without rewriting the saved view.
    add_parent(cx, &h, &store, &child, refs);
    for (tab, review) in [("default", &default), ("saved", &saved)] {
        assert_eq!(review.read_with(cx, |r, _| r.mode()), ReviewMode::Committed);
        wait_review(cx, review, "compared", |r, _| !r.is_loading() && matches!(r.content(), ReviewContent::Committed(_)));
        assert_eq!(review_tab(cx, &child, tab).entity_id(), review.entity_id());
    }
    assert_eq!(tab_state(cx, &child, "default").get("viewMode"), None);
    assert_eq!(tab_state(cx, &child, "saved")["viewMode"], "committed");
    // The view picked stays.
    assert_eq!(picked.read_with(cx, |r, _| r.mode()), ReviewMode::Uncommitted);
    assert_eq!(review_tab(cx, &child, &picked_tab).entity_id(), picked.entity_id());
}

#[gpui_kit::test]
fn a_childs_review_with_its_own_base_checks_conflicts_once_the_parents_branch_is_known(cx: &mut TestAppContext) {
    use treeterm_model::review::{ConflictCheck, Conflicts, ReviewBase};
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "one\n");
    repo.commit_all("base");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("a.txt", "FEATURE\n");
    repo.commit_all("on feature");
    // The parent moved on, changing the same line.
    repo.git(&["checkout", "-q", "main"]);
    repo.write("a.txt", "MAIN\n");
    repo.commit_all("on main");
    repo.git(&["checkout", "-q", "feature"]);
    let state = serde_json::json!({ "baseBranchOverride": "main" });
    let tabs = serde_json::json!({ "pinned": { "applicationId": "review", "title": "Review", "state": state } });
    let (store, child, refs) = child_before_parent(cx, &h, &repo, tabs);
    let review = review_tab(cx, &child, "pinned");
    // No parent's branch: nothing to check against.
    wait_review(cx, &review, "loaded", |r, _| !r.is_loading() && r.overview().committed == 1);
    assert_eq!(review.read_with(cx, |r, _| r.conflicts().clone()), Conflicts::default());

    // Its base stays, but the conflicts are checked against the parent's branch now known.
    add_parent(cx, &h, &store, &child, refs);
    assert_eq!(review.read_with(cx, |r, _| r.base().clone()), ReviewBase::Branch("main".into()));
    wait_review(cx, &review, "checked", |r, _| !r.is_loading() && r.conflicts().parent_has_new_commits);
    let conflicts = review.read_with(cx, |r, _| r.conflicts().clone());
    assert_eq!(conflicts.check, ConflictCheck::Idle);
    assert_eq!(conflicts.last.conflicted_files, ["a.txt"]);
}

/// A failure to bring the comments up to date is the tab's notice. It, a comment that cannot
/// be added and a diff that cannot load are logged, as Electron's `console.error`s did.
#[gpui_kit::test]
fn review_state_that_cannot_load_shows_as_the_tabs_notice(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "a\n");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let review = open_review(cx, &workspace);
    wait_review(cx, &review, "loaded", |r, _| !r.is_loading() && matches!(r.head(), Head::Known(_)));
    assert_eq!(review.read_with(cx, |r, _| r.notice().clone()), Notice::Clear);

    // The comments cannot be brought up to date (`loadReviews` throws): shown and logged.
    let logs = treeterm_testkit::logs::capture();
    workspace.update(cx, |w, cx| w.update_metadata("reviewComments", "not json", cx));
    review.update(cx, |r, cx| r.refresh(cx));
    wait_review(cx, &review, "refreshed", |r, _| !r.is_refreshing());
    let notice = review.read_with(cx, |r, _| r.notice().clone());
    assert!(matches!(&notice, Notice::Error(e) if e.starts_with("Failed to load review state: ")), "{notice:?}");
    let logged = logs.line("Failed to load reviews: ").unwrap_or_else(|| panic!("{}", logs.text()));
    assert!(logged.contains("ERROR"), "{logged}");
    review.update(cx, |r, cx| r.dismiss_notice(cx));

    // Nor can a comment be added to them (`addReviewComment` throws): shown and logged.
    review.update(cx, |r, cx| r.start_comment("a.txt", Side::Modified, 1, cx));
    review.update(cx, |r, cx| r.submit_comment("why?", cx));
    wait_review(cx, &review, "refused", |r, _| matches!(r.adding(), GitOp::Failed(_)));
    let GitOp::Failed(shown) = review.read_with(cx, |r, _| r.adding().clone()) else { unreachable!() };
    let logged = logs.line("Failed to add comment: ").unwrap_or_else(|| panic!("{}", logs.text()));
    assert!(logged.contains("ERROR") && logged.contains(&shown), "{logged}");

    // A base branch that does not exist: the diff does not load (`loadDiff`'s catch).
    workspace.update(cx, |w, cx| w.update_metadata("reviewComments", "[]", cx));
    review.update(cx, |r, cx| r.set_base(Some("no-such-branch".into()), cx));
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Committed, cx));
    wait_review(cx, &review, "diff failed", |r, _| matches!(r.content(), ReviewContent::Failed(_)));
    let logged = logs.line("Error loading diff: ").unwrap_or_else(|| panic!("{}", logs.text()));
    assert!(logged.contains("ERROR"), "{logged}");
    // After git's own message, which may span lines.
    assert!(logs.text().contains("base=no-such-branch"), "{}", logs.text());
    drop(logs);

    // No commit to read `HEAD` from: no notice (Electron's `success: false`).
    repo.git(&["update-ref", "-d", "HEAD"]);
    review.update(cx, |r, cx| r.refresh(cx));
    wait_review(cx, &review, "refreshed", |r, _| !r.is_refreshing());
    assert!(matches!(review.read_with(cx, |r, _| r.head().clone()), Head::Failed(_)));
    assert_eq!(review.read_with(cx, |r, _| r.notice().clone()), Notice::Clear);
}

#[gpui_kit::test]
fn the_review_header_takes_the_child_actions_from_the_tabs_parent_id(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    // Electron's `hasParent`: the tab names a parent, here one that is not loaded, so its
    // branch is unknown; the child's actions still show.
    let (_, child) = open_review_with(cx, &workspace, serde_json::json!({ "parentWorkspaceId": "not-loaded" }));
    let (_, blank) = open_review_with(cx, &workspace, serde_json::json!({ "parentWorkspaceId": "" }));
    let (_, top) = open_review_with(cx, &workspace, serde_json::json!({}));
    for review in [&child, &blank, &top] {
        wait_review(cx, review, "loaded", |r, _| !r.is_loading());
    }
    let header = child.read_with(cx, |r, cx| r.header(cx));
    assert!(header.has_parent && header.parent_branch.is_none(), "{header:?}");
    assert!(!blank.read_with(cx, |r, cx| r.header(cx)).has_parent);
    assert!(!top.read_with(cx, |r, cx| r.header(cx)).has_parent);
}

#[gpui_kit::test]
fn the_review_header_shows_the_workspace_name_not_its_display_name(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let review = open_review(cx, &workspace);
    wait_review(cx, &review, "loaded", |r, _| !r.is_loading());
    let name = workspace.read_with(cx, |w, _| w.file().name.clone());
    assert!(!name.is_empty());
    // Electron's header shows `wsData.name`, also once the Settings tab renamed it.
    workspace.update(cx, |w, cx| w.set_display_name("Renamed", cx));
    assert_eq!(workspace.read_with(cx, |w, _| w.file().display_name().to_owned()), "Renamed");
    assert_eq!(review.read_with(cx, |r, cx| r.header(cx)).name, name);
}

/// The diffs of the list the review shows.
fn shown_diffs(review: &ReviewModel) -> treeterm_model::review_patch::FileDiffs {
    use treeterm_model::review::Loaded;
    match review.content() {
        ReviewContent::Committed(committed) => committed.diffs,
        ReviewContent::Uncommitted(uncommitted) => uncommitted.diffs,
        ReviewContent::Commits(_) => match review.selected_commit().map(|s| s.diff.clone()) {
            Some(Loaded::Ready(diff)) => diff.diffs,
            Some(Loaded::Loading | Loaded::Failed(_)) | None => Default::default(),
        },
        ReviewContent::Loading | ReviewContent::Failed(_) => Default::default(),
    }
}

/// Asks for the diff of every file of the list shown, as the view does for the files near its
/// viewport, and waits until each is read.
fn read_all(cx: &mut TestAppContext, review: &Entity<ReviewModel>) {
    let paths: Vec<String> = review.read_with(cx, |r, _| shown_diffs(r).iter().map(|(path, _)| path.to_owned()).collect());
    assert!(!paths.is_empty(), "no list shown");
    review.update(cx, |r, cx| r.request_files(&paths, cx));
    wait_review(cx, review, "every file read", |r, _| shown_diffs(r).is_complete());
}

/// `path`'s diff in the list shown.
fn file_diff(cx: &mut TestAppContext, review: &Entity<ReviewModel>, path: &str) -> Option<treeterm_model::review_patch::FileDiff> {
    review.read_with(cx, |r, _| shown_diffs(r).get(path).cloned())
}

fn set_mode(path: &std::path::Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[gpui_kit::test]
fn a_file_git_cannot_read_fails_in_its_own_section_while_the_others_show(cx: &mut TestAppContext) {
    use treeterm_model::review_patch::FileDiff;
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "one\n");
    repo.commit_all("base");
    repo.write("a.txt", "ONE\n");
    // Past 200 untracked files the list reads no sizes, so it lists one git cannot read.
    for index in 0..201 {
        repo.write(&format!("new/{index:03}.txt"), &format!("line {index}\n"));
    }
    let locked = repo.path.join("new/000.txt");
    set_mode(&locked, 0o000);
    if std::fs::read(&locked).is_ok() {
        eprintln!("skipped: the file is still readable (running as root)");
        set_mode(&locked, 0o644);
        return;
    }
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let review = open_review(cx, &workspace);
    wait_review(cx, &review, "listed", |r, _| matches!(r.content(), ReviewContent::Uncommitted(_)));

    // The list shows first, no file's diff asked for; then each file asked for shows its diff
    // or its error.
    let listed = review.read_with(cx, |r, _| shown_diffs(r));
    assert_eq!(listed.iter().count(), 202);
    assert!(listed.iter().all(|(_, diff)| *diff == FileDiff::NotLoaded));
    read_all(cx, &review);
    set_mode(&locked, 0o644);
    let failed = file_diff(cx, &review, "new/000.txt");
    assert!(matches!(&failed, Some(FileDiff::Failed(error)) if error.contains("Permission denied")), "{failed:?}");
    assert!(matches!(file_diff(cx, &review, "new/001.txt"), Some(FileDiff::Loaded(section)) if section.contains("+line 1\n")));
    assert!(matches!(file_diff(cx, &review, "a.txt"), Some(FileDiff::Loaded(section)) if section.contains("+ONE")));
}

#[gpui_kit::test]
fn a_files_refresh_reads_that_file_alone(cx: &mut TestAppContext) {
    use treeterm_model::review_patch::FileDiff;
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "one\n");
    repo.write("b.txt", "bee\n");
    repo.write("c.txt", "a file moved\nwith its lines\n");
    repo.commit_all("base");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("a.txt", "ONE\n");
    repo.write("b.txt", "BEE\n");
    repo.git(&["mv", "c.txt", "d.txt"]);
    repo.commit_all("feature");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let (_, review) =
        open_review_with(cx, &workspace, serde_json::json!({ "viewMode": "committed", "baseBranchOverride": "main" }));
    wait_review(cx, &review, "listed", |r, _| matches!(r.content(), ReviewContent::Committed(_)));
    read_all(cx, &review);
    let before = review.read_with(cx, |r, _| shown_diffs(r));
    let listed = |cx: &mut TestAppContext| match review.read_with(cx, |r, _| r.content()) {
        ReviewContent::Committed(committed) => committed.files,
        other => panic!("{other:?}"),
    };
    let files = listed(cx);

    // The branch moves on in both files: the refreshed one reads its diff alone, loading
    // meanwhile; the other, and the list, stay as they were read.
    repo.write("a.txt", "ONE\nmore\n");
    repo.write("b.txt", "BEE again\n");
    repo.commit_all("more");
    review.update(cx, |r, cx| r.refresh_file("a.txt", cx));
    assert!(review.read_with(cx, |r, _| r.is_file_loading("a.txt") && !r.is_file_loading("b.txt")));
    assert_eq!(file_diff(cx, &review, "a.txt"), Some(FileDiff::Loading));
    wait_review(cx, &review, "a.txt read", |r, _| !r.is_file_loading("a.txt"));
    let a = file_diff(cx, &review, "a.txt");
    assert!(matches!(&a, Some(FileDiff::Loaded(section)) if section.contains("+more")), "{a:?}");
    assert_eq!(file_diff(cx, &review, "b.txt").as_ref(), before.get("b.txt"));
    assert_eq!(listed(cx), files);
    assert!(!review.read_with(cx, |r, _| r.is_refreshing()));

    // A renamed file is read with the path it came from: still a rename.
    review.update(cx, |r, cx| r.refresh_file("d.txt", cx));
    wait_review(cx, &review, "d.txt read", |r, _| !r.is_file_loading("d.txt"));
    assert_eq!(file_diff(cx, &review, "d.txt").as_ref(), before.get("d.txt"));
    assert!(matches!(before.get("d.txt"), Some(FileDiff::Loaded(section)) if section.contains("rename from c.txt")));

    // A whole reload started after a file's own read reads that file too.
    repo.write("a.txt", "ONE\nthird\n");
    repo.commit_all("third");
    review.update(cx, |r, cx| {
        r.refresh_file("a.txt", cx);
        r.refresh(cx);
    });
    assert!(review.read_with(cx, |r, _| r.is_file_loading("a.txt")));
    wait_review(cx, &review, "reloaded", |r, _| !r.is_refreshing() && !r.is_file_loading("a.txt"));
    assert!(matches!(file_diff(cx, &review, "a.txt"), Some(FileDiff::Loaded(section)) if section.contains("+third")));
    assert!(matches!(file_diff(cx, &review, "b.txt"), Some(FileDiff::Loaded(section)) if section.contains("+BEE again")));

    // Not listed: nothing to read.
    review.update(cx, |r, cx| r.refresh_file("nope.txt", cx));
    assert!(!review.read_with(cx, |r, _| r.is_file_loading("nope.txt")));
}

#[gpui_kit::test]
fn a_files_refresh_reads_its_staged_unstaged_untracked_or_commit_diff(cx: &mut TestAppContext) {
    use treeterm_model::review::{Loaded, ReviewMode};
    use treeterm_model::review_patch::FileDiff;
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "a\n");
    repo.write("b.txt", "b\n");
    repo.commit_all("base");
    repo.write("a.txt", "A\n");
    repo.git(&["add", "a.txt"]);
    repo.write("b.txt", "B\n");
    repo.write("u.txt", "u\n");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let review = open_review(cx, &workspace);
    wait_review(cx, &review, "listed", |r, _| matches!(r.content(), ReviewContent::Uncommitted(_)));
    let refreshed = |cx: &mut TestAppContext, path: &str| {
        review.update(cx, |r, cx| r.refresh_file(path, cx));
        assert!(review.read_with(cx, |r, _| r.is_file_loading(path)));
        wait_review(cx, &review, "read again", |r, _| !r.is_file_loading(path));
        file_diff(cx, &review, path)
    };
    let loaded = |diff: Option<FileDiff>, text: &str| matches!(&diff, Some(FileDiff::Loaded(section)) if section.contains(text));

    // Staged: the index's change, not the working tree's.
    repo.write("a.txt", "A2\n");
    repo.git(&["add", "a.txt"]);
    repo.write("a.txt", "A3\n");
    assert!(loaded(refreshed(cx, "a.txt"), "+A2"));
    // Unstaged, and untracked.
    repo.write("b.txt", "B2\n");
    assert!(loaded(refreshed(cx, "b.txt"), "+B2"));
    repo.write("u.txt", "u2\n");
    assert!(loaded(refreshed(cx, "u.txt"), "+u2"));
    // One git cannot read fails alone.
    let others = (file_diff(cx, &review, "a.txt"), file_diff(cx, &review, "b.txt"));
    let locked = repo.path.join("u.txt");
    set_mode(&locked, 0o000);
    if std::fs::read(&locked).is_err() {
        let failed = refreshed(cx, "u.txt");
        set_mode(&locked, 0o644);
        assert!(matches!(&failed, Some(FileDiff::Failed(error)) if error.contains("Permission denied")), "{failed:?}");
        assert_eq!((file_diff(cx, &review, "a.txt"), file_diff(cx, &review, "b.txt")), others);
    }
    set_mode(&locked, 0o644);

    // A commit's file, in Commits.
    repo.commit_all("more");
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Commits, cx));
    wait_review(cx, &review, "commits", |r, _| matches!(r.content(), ReviewContent::Commits(c) if !c.is_empty()));
    let hash = review.read_with(cx, |r, _| match r.content() {
        ReviewContent::Commits(commits) => commits[0].hash.clone(),
        other => panic!("{other:?}"),
    });
    review.update(cx, |r, cx| r.select_commit(&hash, cx));
    wait_review(cx, &review, "commit listed", |r, _| matches!(r.selected_commit().map(|s| &s.diff), Some(Loaded::Ready(_))));
    read_all(cx, &review);
    let shown = file_diff(cx, &review, "b.txt");
    assert!(loaded(shown.clone(), "+B2"), "{shown:?}");
    assert_eq!(refreshed(cx, "b.txt"), shown);
}

#[gpui_kit::test]
fn a_files_diff_is_read_when_asked_for_and_a_reload_reads_again_those_asked_for(cx: &mut TestAppContext) {
    use treeterm_model::review_patch::FileDiff;
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "a\n");
    repo.write("b.txt", "b\n");
    repo.write("c.txt", "a file moved\nwith its lines\n");
    repo.commit_all("base");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.write("a.txt", "A\n");
    repo.write("b.txt", "B\n");
    repo.git(&["mv", "c.txt", "d.txt"]);
    repo.commit_all("feature");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let (_, review) =
        open_review_with(cx, &workspace, serde_json::json!({ "viewMode": "committed", "baseBranchOverride": "main" }));
    wait_review(cx, &review, "listed", |r, _| matches!(r.content(), ReviewContent::Committed(_)));
    wait_review(cx, &review, "loaded", |r, _| !r.is_loading());
    let loaded = |diff: Option<FileDiff>, text: &str| matches!(&diff, Some(FileDiff::Loaded(section)) if section.contains(text));

    // Listed, no file's diff is read yet ("Scroll to load diff").
    let listed = review.read_with(cx, |r, _| shown_diffs(r));
    assert_eq!(
        listed.iter().collect::<Vec<_>>(),
        [("a.txt", &FileDiff::NotLoaded), ("b.txt", &FileDiff::NotLoaded), ("d.txt", &FileDiff::NotLoaded)]
    );

    // Near the viewport: read on its own, "Loading diff..." meanwhile; the others stay as
    // they are, and a path not listed is not read.
    review.update(cx, |r, cx| r.request_files(&["b.txt".to_owned(), "nope.txt".to_owned()], cx));
    assert_eq!(file_diff(cx, &review, "b.txt"), Some(FileDiff::Loading));
    assert!(review.read_with(cx, |r, _| r.is_file_loading("b.txt") && !r.is_file_loading("nope.txt")));
    wait_review(cx, &review, "b.txt read", |r, _| !r.is_file_loading("b.txt"));
    let b = file_diff(cx, &review, "b.txt");
    assert!(loaded(b.clone(), "+B"), "{b:?}");
    assert_eq!(file_diff(cx, &review, "a.txt"), Some(FileDiff::NotLoaded));
    // Asked for again once read: it is not read again.
    review.update(cx, |r, cx| r.request_files(&["b.txt".to_owned()], cx));
    assert_eq!(file_diff(cx, &review, "b.txt"), b);
    // A renamed file is read with the path it came from: a rename, not an added file.
    review.update(cx, |r, cx| r.request_files(&["d.txt".to_owned()], cx));
    wait_review(cx, &review, "d.txt read", |r, _| !r.is_file_loading("d.txt"));
    let d = file_diff(cx, &review, "d.txt");
    assert!(loaded(d.clone(), "rename from c.txt"), "{d:?}");

    // A reload reads again the files asked for, which show what they showed until it has; the
    // others are still not read.
    repo.write("a.txt", "A2\n");
    repo.write("b.txt", "B2\n");
    repo.commit_all("more");
    review.update(cx, |r, cx| r.refresh(cx));
    assert_eq!(file_diff(cx, &review, "b.txt"), b);
    assert!(!review.read_with(cx, |r, _| r.is_file_loading("b.txt")));
    wait_review(cx, &review, "reloaded", |r, _| !r.is_refreshing());
    assert!(loaded(file_diff(cx, &review, "b.txt"), "+B2"));
    assert_eq!(file_diff(cx, &review, "d.txt"), d);
    assert_eq!(file_diff(cx, &review, "a.txt"), Some(FileDiff::NotLoaded));

    // A file asked for while a reload runs is read on its own, and the reload keeps that read.
    repo.write("a.txt", "A3\n");
    repo.commit_all("third");
    review.update(cx, |r, cx| {
        r.refresh(cx);
        r.request_files(&["a.txt".to_owned()], cx);
    });
    assert!(review.read_with(cx, |r, _| r.is_file_loading("a.txt")));
    wait_review(cx, &review, "reloaded", |r, _| !r.is_refreshing() && !r.is_file_loading("a.txt"));
    assert!(loaded(file_diff(cx, &review, "a.txt"), "+A3"));

    // A file the reload lists for the first time is not read.
    repo.write("e.txt", "new\n");
    repo.commit_all("e");
    review.update(cx, |r, cx| r.refresh(cx));
    wait_review(cx, &review, "e.txt listed", |r, _| !r.is_refreshing() && shown_diffs(r).get("e.txt").is_some());
    assert_eq!(file_diff(cx, &review, "e.txt"), Some(FileDiff::NotLoaded));
    assert!(loaded(file_diff(cx, &review, "b.txt"), "+B2"));
}

#[gpui_kit::test]
fn an_uncommitted_reload_reads_again_the_staged_unstaged_and_untracked_files_asked_for(cx: &mut TestAppContext) {
    use treeterm_model::review_patch::FileDiff;
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    for name in ["a.txt", "b.txt", "c.txt"] {
        repo.write(name, "x\n");
    }
    repo.write("r.txt", "a file moved\nwith its lines\n");
    repo.commit_all("base");
    repo.write("a.txt", "A\n");
    repo.git(&["add", "a.txt"]);
    repo.git(&["mv", "r.txt", "s.txt"]);
    repo.write("b.txt", "B\n");
    repo.write("c.txt", "C\n");
    repo.write("u.txt", "u\n");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let review = open_review(cx, &workspace);
    wait_review(cx, &review, "listed", |r, _| matches!(r.content(), ReviewContent::Uncommitted(u) if u.changes.files.len() == 5));
    let loaded = |diff: Option<FileDiff>, text: &str| matches!(&diff, Some(FileDiff::Loaded(section)) if section.contains(text));
    let asked: Vec<String> = ["a.txt", "s.txt", "b.txt", "u.txt"].map(str::to_owned).to_vec();
    review.update(cx, |r, cx| r.request_files(&asked, cx));
    wait_review(cx, &review, "read", |r, _| asked.iter().all(|path| !r.is_file_loading(path)));
    // A staged rename is read with the path it came from.
    let s = file_diff(cx, &review, "s.txt");
    assert!(loaded(s.clone(), "rename from r.txt"), "{s:?}");

    // Each changes; a reload reads again those asked for, each its own way: staged (the
    // index's change), unstaged, untracked. c.txt is still not read.
    repo.write("a.txt", "A2\n");
    repo.git(&["add", "a.txt"]);
    repo.write("a.txt", "A3\n");
    repo.write("b.txt", "B2\n");
    repo.write("c.txt", "C2\n");
    repo.write("u.txt", "u2\n");
    review.update(cx, |r, cx| r.refresh(cx));
    wait_review(cx, &review, "reloaded", |r, _| !r.is_refreshing());
    assert!(loaded(file_diff(cx, &review, "a.txt"), "+A2"));
    assert!(loaded(file_diff(cx, &review, "b.txt"), "+B2"));
    assert!(loaded(file_diff(cx, &review, "u.txt"), "+u2"));
    assert_eq!(file_diff(cx, &review, "s.txt"), s);
    assert_eq!(file_diff(cx, &review, "c.txt"), Some(FileDiff::NotLoaded));
}

/// Git reads `path`'s two sides slowly (a textconv sleeping a second for each) in any patch that
/// shows it; listing the files (numstat, name-status, stat) stays quick.
fn slow_to_read(repo: &TempRepo, path: &str) {
    let info = repo.path.join(".git/info");
    std::fs::create_dir_all(&info).unwrap();
    std::fs::write(info.join("attributes"), format!("{path} diff=slow\n")).unwrap();
    repo.git(&["config", "diff.slow.textconv", "sleep 1; cat"]);
}

/// Lets a reload just started list its files (its re-read of a file [`slow_to_read`] then runs
/// for two seconds).
fn let_it_list(cx: &mut TestAppContext) {
    for _ in 0..30 {
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[gpui_kit::test]
fn a_file_read_on_its_own_while_a_reload_reads_others_keeps_that_read(cx: &mut TestAppContext) {
    use treeterm_model::review_patch::FileDiff;
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    let names = ["a.txt", "b.txt", "slow.txt"];
    for name in names {
        repo.write(name, "x\n");
    }
    repo.commit_all("base");
    repo.git(&["checkout", "-q", "-b", "feature"]);
    for name in names {
        repo.write(name, &format!("{name}\n"));
    }
    repo.commit_all("feature");
    slow_to_read(&repo, "slow.txt");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let (_, review) =
        open_review_with(cx, &workspace, serde_json::json!({ "viewMode": "committed", "baseBranchOverride": "main" }));
    wait_review(cx, &review, "listed", |r, _| matches!(r.content(), ReviewContent::Committed(_)));
    wait_review(cx, &review, "loaded", |r, _| !r.is_loading());
    review.update(cx, |r, cx| r.request_files(&["slow.txt".to_owned()], cx));
    wait_review(cx, &review, "slow.txt read", |r, _| !r.is_file_loading("slow.txt"));
    let loaded = |diff: &Option<FileDiff>, text: &str| matches!(diff, Some(FileDiff::Loaded(section)) if section.contains(text));
    assert!(loaded(&file_diff(cx, &review, "slow.txt"), "+slow.txt"));

    // A reload reads slow.txt again, slowly. a.txt, asked for as it starts, and b.txt, asked
    // for once it has listed the files, are read on their own meanwhile, and first.
    review.update(cx, |r, cx| {
        r.refresh(cx);
        r.request_files(&["a.txt".to_owned()], cx);
    });
    let_it_list(cx);
    review.update(cx, |r, cx| r.request_files(&["b.txt".to_owned()], cx));
    wait_review(cx, &review, "a.txt and b.txt read", |r, _| !r.is_file_loading("a.txt") && !r.is_file_loading("b.txt"));
    assert!(review.read_with(cx, |r, _| r.is_refreshing()), "the reload ended first");
    let (a, b) = (file_diff(cx, &review, "a.txt"), file_diff(cx, &review, "b.txt"));
    assert!(loaded(&a, "+a.txt") && loaded(&b, "+b.txt"), "{a:?} {b:?}");

    // Once it ends, they keep what they read (not "Scroll to load diff", nor read again).
    wait_review(cx, &review, "reloaded", |r, _| !r.is_refreshing());
    assert_eq!((file_diff(cx, &review, "a.txt"), file_diff(cx, &review, "b.txt")), (a, b));
    assert!(review.read_with(cx, |r, _| !r.is_file_loading("a.txt") && !r.is_file_loading("b.txt")));
    assert!(loaded(&file_diff(cx, &review, "slow.txt"), "+slow.txt"));
}

#[gpui_kit::test]
fn a_file_read_on_its_own_during_a_reload_that_lists_it_another_way_is_read_again(cx: &mut TestAppContext) {
    use treeterm_model::review_patch::FileDiff;
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("a.txt", "a\n");
    repo.write("slow.txt", "x\n");
    repo.commit_all("base");
    repo.write("a.txt", "A\n");
    repo.write("slow.txt", "X\n");
    slow_to_read(&repo, "slow.txt");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let review = open_review(cx, &workspace);
    wait_review(cx, &review, "listed", |r, _| matches!(r.content(), ReviewContent::Uncommitted(u) if u.changes.files.len() == 2));
    wait_review(cx, &review, "loaded", |r, _| !r.is_loading());
    review.update(cx, |r, cx| r.request_files(&["slow.txt".to_owned()], cx));
    wait_review(cx, &review, "slow.txt read", |r, _| !r.is_file_loading("slow.txt"));

    // a.txt is staged, then asked for while a reload (reading slow.txt again, slowly) runs: it
    // is read the way the list shown has it, unstaged, which now shows nothing. The reload
    // lists it staged: it is read again that way.
    repo.git(&["add", "a.txt"]);
    review.update(cx, |r, cx| r.refresh(cx));
    let_it_list(cx);
    review.update(cx, |r, cx| r.request_files(&["a.txt".to_owned()], cx));
    wait_review(cx, &review, "reloaded", |r, _| !r.is_refreshing() && !r.is_file_loading("a.txt"));
    let a = file_diff(cx, &review, "a.txt");
    assert!(matches!(&a, Some(FileDiff::Loaded(section)) if section.contains("+A")), "{a:?}");
}

#[gpui_kit::test]
fn a_commits_rename_reads_as_a_deletion_and_an_addition_also_read_together(cx: &mut TestAppContext) {
    use treeterm_model::review::{Loaded, ReviewMode};
    use treeterm_model::review_patch::FileDiff;
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    repo.write("c.txt", "a file moved\nwith its lines\n");
    repo.write("w.txt", "x\n");
    repo.commit_all("base");
    repo.git(&["mv", "c.txt", "d.txt"]);
    repo.write("w.txt", "x \n");
    repo.commit_all("move");
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let review = open_review(cx, &workspace);
    review.update(cx, |r, cx| r.set_mode(ReviewMode::Commits, cx));
    wait_review(cx, &review, "commits", |r, _| matches!(r.content(), ReviewContent::Commits(c) if !c.is_empty()));
    let hash = review.read_with(cx, |r, _| match r.content() {
        ReviewContent::Commits(commits) => commits[0].hash.clone(),
        other => panic!("{other:?}"),
    });
    review.update(cx, |r, cx| r.select_commit(&hash, cx));
    wait_review(cx, &review, "commit listed", |r, _| matches!(r.selected_commit().map(|s| &s.diff), Some(Loaded::Ready(_))));
    let deleted = |diff: &Option<FileDiff>| {
        matches!(diff, Some(FileDiff::Loaded(section)) if section.contains("deleted file mode") && section.contains("-a file moved"))
    };
    let added = |diff: &Option<FileDiff>| {
        matches!(diff, Some(FileDiff::Loaded(section)) if section.contains("new file mode") && section.contains("+a file moved"))
    };

    // The commit lists its old path deleted and its new one added (`diff-tree`), and each
    // reads so on its own, as in Electron.
    read_all(cx, &review);
    let (c, d) = (file_diff(cx, &review, "c.txt"), file_diff(cx, &review, "d.txt"));
    assert!(deleted(&c) && added(&d), "{c:?} {d:?}");

    // Read again together (a reload reads every file asked for in one command; here ignoring
    // whitespace, which makes w.txt's respaced line unchanged): still a deletion and an
    // addition, not one rename.
    review.update(cx, |r, cx| r.set_ignore_whitespace(true, cx));
    wait_review(cx, &review, "read again ignoring whitespace", |r, _| {
        matches!(shown_diffs(r).get("w.txt"), Some(FileDiff::Loaded(section)) if !section.contains("\n-x\n"))
    });
    let (c, d) = (file_diff(cx, &review, "c.txt"), file_diff(cx, &review, "d.txt"));
    assert!(deleted(&c) && added(&d), "{c:?} {d:?}");
}

#[gpui_kit::test]
fn find_reads_every_file_a_few_at_a_time_and_counts_once_all_are_read(cx: &mut TestAppContext) {
    use treeterm_core::search::{DiffStyle, UnchangedRegions};
    use treeterm_model::review::MAX_SEARCH_LOADS;
    use treeterm_model::review_patch::FileDiff;
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let repo = TempRepo::new(&h);
    let names: Vec<String> = (0..6).map(|index| format!("f{index}.txt")).collect();
    for name in &names {
        repo.write(name, &format!("{name} line\n"));
    }
    repo.commit_all("base");
    // Three files change a line with the needle, the other three another line.
    for (index, name) in names.iter().enumerate() {
        let line = if index % 2 == 0 { format!("needle {index}") } else { format!("other {index}") };
        repo.write(name, &format!("{line}\n"));
    }
    let store = open_store(cx, &h);
    let workspace = open_repo_workspace(cx, &h, &store, &repo);
    let review = open_review(cx, &workspace);
    wait_review(cx, &review, "listed", |r, _| matches!(r.content(), ReviewContent::Uncommitted(u) if u.changes.files.len() == 6));
    wait_review(cx, &review, "loaded", |r, _| !r.is_loading());
    let layout = DiffLayout { style: DiffStyle::Split, unchanged: UnchangedRegions::Collapsed };
    let (files, layout_files) = (names.clone(), names.clone());
    // The count and the most files being read, at each change.
    let label = move |r: &ReviewModel| {
        let matches = r.find_matches(&shown_diffs(r).patch(), &layout_files, layout);
        (r.find_count_label(&matches), r.find_rows(&matches, &layout_files))
    };
    let seen = std::rc::Rc::new(std::cell::RefCell::new((Vec::new(), 0)));
    let record = seen.clone();
    let observe_label = label.clone();
    let _observe = cx.update(|cx| {
        cx.observe(&review, move |review, cx| {
            let review = review.read(cx);
            let reading = shown_diffs(review).iter().filter(|(_, diff)| **diff == FileDiff::Loading).count();
            let mut seen = record.borrow_mut();
            seen.0.push(observe_label(review).0);
            seen.1 = seen.1.max(reading);
        })
    });

    // A query: every file is read, a few at a time; until they are, the count says more may
    // come and an empty section does not say "No matches".
    review.update(cx, |r, cx| {
        r.open_find("f1.txt", cx);
        r.set_find_query("needle", cx);
    });
    let (count, rows) = review.read_with(cx, |r, _| label(r));
    assert_eq!(count, "Searching…");
    assert!(review.read_with(cx, |r, _| r.find_pending()));
    assert!(!rows.contains(&SearchRow::Note("No matches".to_owned())), "{rows:?}");
    wait_review(cx, &review, "every file read", |r, _| shown_diffs(r).is_complete());
    let (labels, most) = seen.borrow().clone();
    assert!((1..=MAX_SEARCH_LOADS).contains(&most), "{most} read at once");
    assert!(labels.iter().any(|label| label.ends_with(" found…")), "{labels:?}");
    assert!(!review.read_with(cx, |r, _| r.find_pending()));
    assert_eq!(review.read_with(cx, |r, _| label(r).0), "3 found");

    // Once every file is read, a section with none says so.
    review.update(cx, |r, cx| r.set_find_query("nowhere", cx));
    let (count, rows) = review.read_with(cx, |r, _| label(r));
    assert_eq!(count, "0 found");
    assert_eq!(rows.iter().filter(|row| **row == SearchRow::Note("No matches".to_owned())).count(), 2, "{rows:?}");

    // Closed, or without a query, find reads nothing more.
    let (_, review) = open_review_with(cx, &workspace, serde_json::json!({ "viewMode": "uncommitted" }));
    wait_review(cx, &review, "listed", |r, _| matches!(r.content(), ReviewContent::Uncommitted(u) if u.changes.files.len() == 6));
    review.update(cx, |r, cx| {
        r.open_find("f0.txt", cx);
        r.set_find_query("needle", cx);
        r.set_find_query("", cx);
    });
    let reading = review.read_with(cx, |r, _| shown_diffs(r).iter().filter(|(_, diff)| **diff == FileDiff::Loading).count());
    assert_eq!(reading, MAX_SEARCH_LOADS);
    wait_review(cx, &review, "those read", |r, _| files.iter().all(|path| !r.is_file_loading(path)));
    assert_eq!(
        review.read_with(cx, |r, _| shown_diffs(r).iter().filter(|(_, diff)| **diff == FileDiff::NotLoaded).count()),
        files.len() - MAX_SEARCH_LOADS
    );
    assert!(!review.read_with(cx, |r, _| r.find_pending()));
}
