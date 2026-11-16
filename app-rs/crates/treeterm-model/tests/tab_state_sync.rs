//! Open tabs follow the tab state another window (or Electron) writes, as Electron's
//! components read it live: the Files tab's selection, expanded directories and
//! `scrollToLine`, the Editor tab's text, unsaved mark and view mode, and the ports Web App
//! and Browser tabs saved. Two windows on one workspace of a real in-process daemon.

use std::time::Duration;

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use serde_json::{Value, json};
use treeterm_core::wire::stable_json::sha256_hex;
use treeterm_model::browser_page::{BrowserPageModel, BrowserPhase};
use treeterm_model::file_browser::{FileBrowser, FileView, Listing};
use treeterm_model::session_store::{SessionState, SessionStore, WorkspaceEntry};
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::text_editor::{EditorDoc, TextEditor};
use treeterm_model::traffic::ConnectionTraffic;
use treeterm_model::web_app::WebAppModel;
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
    open_windows_with(cx, files, Default::default())
}

/// [`open_windows`] with these settings in both windows.
fn open_windows_with(
    cx: &mut TestAppContext,
    files: &[(&str, &str)],
    settings: treeterm_core::settings::Settings,
) -> Windows {
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
        let settings = cx.new(|_| SettingsStore::in_memory(settings.clone()));
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

fn editor_of(
    cx: &mut TestAppContext,
    workspace: &Entity<WorkspaceStore>,
    tab: &str,
) -> Entity<TextEditor> {
    match workspace.read_with(cx, |w, _| w.tab_content(tab)) {
        Some(TabContent::Editor(editor)) => editor,
        _ => panic!("not an Editor tab"),
    }
}

fn web_app_of(
    cx: &mut TestAppContext,
    workspace: &Entity<WorkspaceStore>,
    tab: &str,
) -> Entity<WebAppModel> {
    match workspace.read_with(cx, |w, _| w.tab_content(tab)) {
        Some(TabContent::WebApp { app, .. }) => app,
        _ => panic!("not a Web App tab"),
    }
}

fn page_of(
    cx: &mut TestAppContext,
    workspace: &Entity<WorkspaceStore>,
    tab: &str,
) -> Entity<BrowserPageModel> {
    match workspace.read_with(cx, |w, _| w.tab_content(tab)) {
        Some(TabContent::Browser { page, .. }) => page,
        _ => panic!("not a Browser tab"),
    }
}

fn shows(browser: &FileBrowser, path: &str) -> bool {
    matches!(browser.view(), FileView::Text { path: shown, .. } if shown == path)
}

fn current(editor: &TextEditor) -> Option<&str> {
    match editor.doc() {
        EditorDoc::Ready { current, .. } => Some(current),
        EditorDoc::Loading | EditorDoc::Error(_) => None,
    }
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

#[gpui_kit::test]
fn the_editor_follows_another_windows_unsaved_edits_and_saves_them(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let w = open_windows(cx, &[("notes.md", "# One\n")]);
    let notes = w.path("notes.md");
    let tab = w.add_tab(
        cx,
        "editor",
        json!({ "status": "loading", "filePath": notes }),
    );
    let (mine, theirs) = (editor_of(cx, &w.a, &tab), editor_of(cx, &w.b, &tab));
    wait_until(cx, &mine, |e| current(e) == Some("# One\n"));
    wait_until(cx, &theirs, |e| current(e) == Some("# One\n"));
    w.quiet(cx);
    assert!(
        mine.read_with(cx, |e, _| e.is_preview()),
        "markdown opens on its preview"
    );
    let revision = mine.read_with(cx, |e, _| e.external_revision());

    // The other window switches to the editor.
    theirs.update(cx, |e, cx| e.toggle_view_mode(cx));
    wait_until(cx, &mine, |e| !e.is_preview());

    // It types (Electron's `handleEditorChange` writes the text and the unsaved mark).
    w.quiet(cx);
    let before = sha256_hex(&w.raw());
    w.b_writes(cx, &tab, |state| {
        state["currentContent"] = json!("# Two\n");
        state["isDirty"] = json!(true);
    });
    wait_until(cx, &mine, |e| current(e) == Some("# Two\n"));
    assert!(mine.read_with(cx, |e, _| e.is_dirty()));
    assert!(mine.read_with(cx, |e, _| e.external_revision()) > revision);
    // Taking it writes nothing back: the last body on disk is still the other window's,
    // written over the one before.
    w.quiet(cx);
    assert_eq!(w.disk()["parentHash"], json!(before));

    // Saved here, the file has the other window's text, and both windows are clean.
    mine.update(cx, |e, cx| e.save(cx));
    wait_until(cx, &mine, |e| !e.is_dirty());
    assert_eq!(std::fs::read_to_string(&notes).unwrap(), "# Two\n");
    wait_until(cx, &theirs, |e| {
        current(e) == Some("# Two\n") && !e.is_dirty()
    });
    w.wait_for_disk(cx, |disk| {
        disk["appStates"][&tab]["state"]["isDirty"] == json!(false)
    });

    // Typed in the other window's editor: here too, the tab marked unsaved.
    theirs.update(cx, |e, cx| e.set_content("# Three\n".to_owned(), cx));
    wait_until(cx, &mine, |e| {
        current(e) == Some("# Three\n") && e.is_dirty()
    });
    assert_eq!(
        w.a.read_with(cx, |w, _| w.file().app_states[&tab].title.clone()),
        "notes.md \u{2022}"
    );
}

#[gpui_kit::test]
fn an_editor_edit_reaches_the_tab_state_at_once(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let w = open_windows(cx, &[("a.txt", "a\n")]);
    let tab = w.add_tab(
        cx,
        "editor",
        json!({ "status": "loading", "filePath": w.path("a.txt") }),
    );
    let mine = editor_of(cx, &w.a, &tab);
    wait_until(cx, &mine, |e| current(e) == Some("a\n"));
    w.quiet(cx);

    // An edit that keeps the file dirty goes into the body with no pause, and to disk with
    // no clock advanced.
    mine.update(cx, |e, cx| e.set_content("ab\n".to_owned(), cx));
    mine.update(cx, |e, cx| e.set_content("abc\n".to_owned(), cx));
    let state =
        w.a.read_with(cx, |w, _| w.file().app_states[&tab].state.clone());
    assert_eq!(
        (state["currentContent"].clone(), state["isDirty"].clone()),
        (json!("abc\n"), json!(true))
    );
    w.wait_for_disk(cx, |disk| {
        disk["appStates"][&tab]["state"]["currentContent"] == json!("abc\n")
    });
    let theirs = editor_of(cx, &w.b, &tab);
    wait_until(cx, &theirs, |e| current(e) == Some("abc\n"));
}

#[gpui_kit::test]
fn a_stale_copy_of_the_editors_own_text_is_not_taken(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let w = open_windows(cx, &[("a.txt", "base\n")]);
    let tab = w.add_tab(
        cx,
        "editor",
        json!({ "status": "loading", "filePath": w.path("a.txt") }),
    );
    let mine = editor_of(cx, &w.a, &tab);
    wait_until(cx, &mine, |e| current(e) == Some("base\n"));
    mine.update(cx, |e, cx| e.set_content("a".to_owned(), cx));
    mine.update(cx, |e, cx| e.set_content("ab".to_owned(), cx));
    w.wait_for_disk(cx, |disk| {
        disk["appStates"][&tab]["state"]["currentContent"] == json!("ab")
    });
    w.quiet(cx);

    // Another writer brings back the text this window wrote just before its last (it won the
    // race to save the workspace): the text typed since stays, and is written again.
    w.b_writes(cx, &tab, |state| state["currentContent"] = json!("a"));
    wait_until(cx, &w.b, |w| {
        w.file().app_states[&tab].state["currentContent"] == json!("ab")
    });
    w.quiet(cx);
    assert_eq!(
        mine.read_with(cx, |e, _| current(e).map(str::to_owned)),
        Some("ab".to_owned())
    );
    assert_eq!(w.disk_state(&tab)["currentContent"], json!("ab"));

    // The other window's own edits are taken, also one going back to a text this window
    // wrote moments ago: once another writer's text is taken, this window's are history.
    let theirs = editor_of(cx, &w.b, &tab);
    wait_until(cx, &theirs, |e| current(e) == Some("ab"));
    theirs.update(cx, |e, cx| e.set_content("abX".to_owned(), cx));
    wait_until(cx, &mine, |e| current(e) == Some("abX"));
    theirs.update(cx, |e, cx| e.set_content("a".to_owned(), cx));
    wait_until(cx, &mine, |e| current(e) == Some("a"));
    w.quiet(cx);
    assert_eq!(w.disk_state(&tab)["currentContent"], json!("a"));

    // Long after this window wrote over a text, that text from another writer is an edit.
    mine.update(cx, |e, cx| e.set_content("ay".to_owned(), cx));
    mine.update(cx, |e, cx| e.set_content("ayz".to_owned(), cx));
    w.wait_for_disk(cx, |disk| {
        disk["appStates"][&tab]["state"]["currentContent"] == json!("ayz")
    });
    w.quiet(cx);
    cx.executor().advance_clock(Duration::from_secs(6));
    w.b_writes(cx, &tab, |state| state["currentContent"] = json!("ay"));
    wait_until(cx, &mine, |e| current(e) == Some("ay"));
    w.quiet(cx);
    assert_eq!(w.disk_state(&tab)["currentContent"], json!("ay"));
}

#[gpui_kit::test]
fn web_app_and_browser_tabs_take_the_ports_another_window_saves(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let settings = treeterm_core::settings::from_value(json!({
        "webApp": { "instances": [{
            "id": "py", "name": "Py", "icon": "P", "command": "sleep 300",
            "isDefault": false, "keepOnExit": false
        }]},
        "browser": { "instances": [{
            "id": "docs", "name": "Docs", "icon": "B", "url": "http://localhost:3000/",
            "isDefault": false
        }]}
    }))
    .unwrap();
    let w = open_windows_with(cx, &[], settings);
    // A server on port 1, where nothing answers: its start-up waits for it.
    let web = w.add_tab(
        cx,
        "webapp-py",
        json!({ "port": { "status": "assigned", "port": 1 } }),
    );
    let page = w.add_tab(cx, "browser-docs", json!({}));
    let (app, browser) = (web_app_of(cx, &w.a, &web), page_of(cx, &w.a, &page));
    let ports = |cx: &mut TestAppContext| {
        (
            app.read_with(cx, |a, _| (a.port(), a.local_port())),
            browser.read_with(cx, |p, _| p.local_port()),
        )
    };
    assert_eq!(ports(cx), ((Some(1), None), None));
    let phases = |cx: &mut TestAppContext| {
        (
            app.read_with(cx, |a, _| a.phase().clone()),
            browser.read_with(cx, |p, _| p.phase().clone()),
        )
    };
    let before_phases = phases(cx);
    assert_eq!(
        before_phases.1,
        BrowserPhase::Ready {
            home: "http://localhost:3000/".into()
        }
    );
    let before = sha256_hex(&w.raw());

    // The other window saves its ports: the next start (and the next forward) takes them,
    // the running one goes on as it was, and taking them writes nothing.
    let assigned = |port: u16| json!({ "status": "assigned", "port": port });
    w.b_writes(cx, &web, |state| {
        state["port"] = assigned(2);
        state["localPort"] = assigned(23456);
    });
    wait_until(cx, &app, |a| a.local_port() == Some(23456));
    w.quiet(cx);
    assert_eq!(w.disk()["parentHash"], json!(before));
    w.b_writes(cx, &page, |state| state["localPort"] = assigned(23457));
    wait_until(cx, &browser, |p| p.local_port() == Some(23457));
    assert_eq!(ports(cx), ((Some(2), Some(23456)), Some(23457)));
    assert_eq!(phases(cx), before_phases);

    // Ports it sets back to unassigned are forgotten too.
    let unassigned = json!({ "status": "unassigned" });
    w.b_writes(cx, &web, |state| state["localPort"] = unassigned.clone());
    w.b_writes(cx, &page, |state| state["localPort"] = unassigned.clone());
    wait_until(cx, &browser, |p| p.local_port().is_none());
    assert_eq!(ports(cx), ((Some(2), None), None));
    assert_eq!(phases(cx), before_phases);
}
