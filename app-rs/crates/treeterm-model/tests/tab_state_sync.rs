//! Open tabs follow the tab state another window (or Electron) writes, as Electron's
//! components read it live: the Files tab's selection, expanded directories and
//! `scrollToLine`. Two windows on one workspace of a real in-process daemon.

use std::time::Duration;

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use serde_json::{Value, json};
use treeterm_core::wire::stable_json::sha256_hex;
use treeterm_model::file_browser::{FileBrowser, FileView, Listing};
use treeterm_model::session_store::{SessionState, SessionStore, WorkspaceEntry};
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::traffic::ConnectionTraffic;
use treeterm_model::workspace_store::{TabContent, WorkspaceStore};
use treeterm_proto::treeterm::WorkspaceRef;

mod common;
use common::{Harness, harness, wait_until};

const ID: &str = "ws";

/// One test at a time: PTY shells forked by one test must not hold another's pipes.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Window A and window B on one workspace (`ID`), in a directory holding some files.
struct Windows {
    dir: String,
    a: Entity<WorkspaceStore>,
    b: Entity<WorkspaceStore>,
    _sessions: [Entity<SessionStore>; 2],
    h: Harness,
}

fn open_windows(cx: &mut TestAppContext, files: &[(&str, &str)]) -> Windows {
    let h = harness();
    let dir = h.daemon.root().join(ID);
    for (path, text) in files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    let body = json!({
        "name": ID, "status": "active", "isGitRepo": false, "isWorktree": false,
        "appStates": {}, "metadata": {}, "createdAt": 1, "lastActivity": 1, "parentHash": ""
    });
    std::fs::write(
        h.daemon.workspaces_dir().join(format!("{ID}.json")),
        body.to_string(),
    )
    .unwrap();
    let dir = dir.to_string_lossy().into_owned();
    let refs = vec![WorkspaceRef {
        id: ID.into(),
        path: dir.clone(),
    }];
    futures::executor::block_on(h.conn.update_session(refs, None, "test-writer"))
        .unwrap()
        .unwrap();
    let mut open = |client: &str| {
        let (conn, client) = (h.conn.clone(), client.to_owned());
        let settings = cx.new(|_| SettingsStore::in_memory(Default::default()));
        let session = cx.new(|cx| {
            SessionStore::new(
                conn,
                client,
                settings,
                None,
                cx.new(ConnectionTraffic::new),
                cx,
            )
        });
        let github = std::sync::Arc::new(treeterm_core::github::UnavailableGitHub::new(
            "not in tests".to_owned(),
        ));
        session.update(cx, |s, _| s.set_github_api(github));
        session
    };
    let sessions = [open("client-a"), open("client-b")];
    for session in &sessions {
        wait_until(cx, session, |s| {
            matches!(s.state(), SessionState::Live)
                && matches!(s.entry(ID), Some(WorkspaceEntry::Loaded(_)))
        });
    }
    let [a, b] = sessions
        .each_ref()
        .map(|s| s.read_with(cx, |s, _| s.workspace_store(ID).unwrap().clone()));
    Windows {
        dir,
        a,
        b,
        _sessions: sessions,
        h,
    }
}

impl Windows {
    fn path(&self, relative: &str) -> String {
        format!("{}/{relative}", self.dir)
    }

    /// The workspace body on disk, as written.
    fn raw(&self) -> String {
        std::fs::read_to_string(self.h.daemon.workspaces_dir().join(format!("{ID}.json"))).unwrap()
    }

    fn disk(&self) -> Value {
        serde_json::from_str(&self.raw()).unwrap()
    }

    /// The tab's state on disk.
    fn disk_state(&self, tab: &str) -> Value {
        self.disk()["appStates"][tab]["state"].clone()
    }

    /// Wait until nobody writes the body any more (it stays the same for a while).
    fn quiet(&self, cx: &mut TestAppContext) {
        let (mut last, mut unchanged) = (self.raw(), 0);
        for _ in 0..1000 {
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(10));
            let now = self.raw();
            if now == last {
                unchanged += 1;
                if unchanged == 30 {
                    return;
                }
            } else {
                (last, unchanged) = (now, 0);
            }
        }
        panic!("the body kept changing");
    }

    fn wait_for_disk(&self, cx: &mut TestAppContext, done: impl Fn(&Value) -> bool) {
        for _ in 0..1000 {
            cx.run_until_parked();
            if done(&self.disk()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("disk condition not reached: {}", self.raw());
    }

    /// A opens a tab of `application` with `state`; once B has it too and every write
    /// landed, the tab id.
    fn add_tab(&self, cx: &mut TestAppContext, application: &str, state: Value) -> String {
        let tab = self
            .a
            .update(cx, |w, cx| w.add_tab_with(application, state, cx))
            .unwrap();
        wait_until(cx, &self.b, |w| w.file().app_states.contains_key(&tab));
        self.quiet(cx);
        tab
    }

    /// B writes the tab's state, as its components (or Electron's) do, once it has the body on
    /// disk: a write over an older one would lose the race to it, and be dropped.
    fn b_writes(&self, cx: &mut TestAppContext, tab: &str, update: impl FnOnce(&mut Value)) {
        self.quiet(cx);
        let on_disk = self.disk_state(tab);
        wait_until(cx, &self.b, |w| w.file().app_states[tab].state == on_disk);
        self.b
            .update(cx, |w, cx| w.update_tab_state(tab, update, cx));
    }
}

fn files_of(
    cx: &mut TestAppContext,
    workspace: &Entity<WorkspaceStore>,
    tab: &str,
) -> Entity<FileBrowser> {
    match workspace.read_with(cx, |w, _| w.tab_content(tab)) {
        Some(TabContent::Files(browser)) => browser,
        _ => panic!("not a Files tab"),
    }
}

fn shows(browser: &FileBrowser, path: &str) -> bool {
    matches!(browser.view(), FileView::Text { path: shown, .. } if shown == path)
}

#[gpui_kit::test]
fn the_files_tab_follows_another_windows_selection_and_expanded_directories(
    cx: &mut TestAppContext,
) {
    let _serial = serial();
    cx.executor().allow_parking();
    let w = open_windows(
        cx,
        &[
            ("src/a.txt", "alpha\n"),
            ("src/b.txt", "beta\n"),
            ("lib/c.txt", "gamma\n"),
            ("top.txt", "top\n"),
        ],
    );
    let (src, lib, a_txt, top) = (
        w.path("src"),
        w.path("lib"),
        w.path("src/a.txt"),
        w.path("top.txt"),
    );
    let tab = w.add_tab(
        cx,
        "filesystem",
        json!({ "selectedPath": top, "expandedDirs": [] }),
    );
    let mine = files_of(cx, &w.a, &tab);
    wait_until(cx, &mine, |b| shows(b, &top));
    mine.update(cx, |b, cx| b.start_comment(1, cx));
    assert!(mine.read_with(cx, |b, _| b.comment_draft().is_some()));
    let before = sha256_hex(&w.raw());

    // The other window expands src and picks a file in it.
    w.b_writes(cx, &tab, |state| {
        state["selectedPath"] = json!(a_txt);
        state["expandedDirs"] = json!([src]);
    });
    wait_until(cx, &mine, |b| {
        shows(b, &a_txt) && matches!(b.listing(&src), Some(Listing::Loaded(_)))
    });
    let rows: Vec<(String, usize, bool)> = mine.read_with(cx, |b, _| {
        b.rows()
            .into_iter()
            .map(|row| (row.name, row.depth, row.expanded))
            .collect()
    });
    let row = |name: &str, depth, expanded| (name.to_owned(), depth, expanded);
    assert_eq!(
        rows,
        [
            row("lib", 0, false),
            row("src", 0, true),
            row("a.txt", 1, false),
            row("b.txt", 1, false),
            row("top.txt", 0, false)
        ]
    );
    assert_eq!(
        mine.read_with(cx, |b, _| b.selected().map(str::to_owned)),
        Some(a_txt.clone())
    );
    assert_eq!(
        mine.read_with(cx, |b, _| b.comment_draft().cloned()),
        None,
        "the comment went with its file"
    );

    // Taking it writes nothing: the last body on disk is still the other window's, written
    // over the one before.
    w.quiet(cx);
    assert_eq!(w.disk()["parentHash"], json!(before));
    // Nor does opening the shown file again ("Open anyway"): the state already says it.
    let settled = w.raw();
    mine.update(cx, |b, cx| b.open(&a_txt, true, cx));
    w.quiet(cx);
    assert_eq!(w.raw(), settled);

    // This window's next change starts from the other window's: both directories expanded.
    mine.update(cx, |b, cx| b.toggle_dir(&lib, cx));
    w.wait_for_disk(cx, |disk| {
        disk["appStates"][&tab]["state"]["expandedDirs"] == json!([src, lib])
    });
    assert_eq!(w.disk_state(&tab)["selectedPath"], json!(a_txt));

    // A directory the other window closes closes here.
    w.b_writes(cx, &tab, |state| state["expandedDirs"] = json!([lib]));
    wait_until(cx, &mine, |b| {
        b.rows().iter().all(|row| row.name != "a.txt")
    });
    let theirs = files_of(cx, &w.b, &tab);
    assert!(
        theirs.read_with(cx, |b, _| shows(b, &a_txt)),
        "the other window shows what it picked"
    );
}

#[gpui_kit::test]
fn a_scroll_to_line_from_another_window_is_revealed_and_cleared(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let lines = "one\ntwo\nthree\nfour\nfive\n";
    let w = open_windows(cx, &[("a.txt", lines), ("b.txt", lines)]);
    let (a_txt, b_txt) = (w.path("a.txt"), w.path("b.txt"));
    // Opened at a line: shown there.
    let tab = w.add_tab(
        cx,
        "filesystem",
        json!({ "selectedPath": a_txt, "expandedDirs": [], "scrollToLine": 2 }),
    );
    let mine = files_of(cx, &w.a, &tab);
    wait_until(cx, &mine, |b| shows(b, &a_txt));
    let opened = mine.read_with(cx, |b, _| b.reveal().cloned()).unwrap();
    assert_eq!(
        (opened.path.as_str(), opened.line_number),
        (a_txt.as_str(), 2)
    );
    let seq =
        |cx: &mut TestAppContext| mine.read_with(cx, |b, _| b.reveal().map(|reveal| reveal.seq));

    // A body that brings nothing new does not move the viewer back there.
    w.b_writes(cx, &tab, |state| state["scrollTop"] = json!(10));
    w.wait_for_disk(cx, |disk| {
        disk["appStates"][&tab]["state"]["scrollTop"] == json!(10)
    });
    wait_until(cx, &w.a, |w| {
        w.file().app_states[&tab].state["scrollTop"] == json!(10)
    });
    assert_eq!(seq(cx), Some(opened.seq));

    // The other window opens a search hit (`handleOpenLocation`): its file, centred on the
    // line, which is then used up.
    w.b_writes(cx, &tab, |state| {
        state["selectedPath"] = json!(b_txt);
        state["scrollToLine"] = json!(4);
    });
    wait_until(cx, &mine, |b| shows(b, &b_txt));
    let reveal = mine.read_with(cx, |b, _| b.reveal().cloned()).unwrap();
    assert_eq!(
        (
            reveal.path.as_str(),
            reveal.line_number,
            reveal.column,
            reveal.len
        ),
        (b_txt.as_str(), 4, 1, 0)
    );
    assert!(reveal.seq > opened.seq);
    let line_four = lines.find("four").unwrap();
    assert_eq!(
        mine.read_with(cx, |b, _| b.reveal_range()),
        Some((reveal.seq, line_four..line_four))
    );
    w.wait_for_disk(cx, |disk| {
        disk["appStates"][&tab]["state"]
            .get("scrollToLine")
            .is_none()
    });
    w.quiet(cx);
    assert_eq!(w.disk_state(&tab)["selectedPath"], json!(b_txt));

    // The same line asked for again is revealed again.
    w.b_writes(cx, &tab, |state| state["scrollToLine"] = json!(4));
    wait_until(cx, &mine, |b| {
        b.reveal().is_some_and(|again| again.seq > reveal.seq)
    });
    w.wait_for_disk(cx, |disk| {
        disk["appStates"][&tab]["state"]
            .get("scrollToLine")
            .is_none()
    });
}
