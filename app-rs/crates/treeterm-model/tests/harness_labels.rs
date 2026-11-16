//! The LLM re-label of a workspace through its AI harness (Refresh Title & Description,
//! Refresh Branch Name): which harness tab is read, what it reads, the outcome and the
//! in-flight state, against a real in-process daemon.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{AppContext as _, Entity, Task, TestAppContext};
use serde_json::{Value, json};
use treeterm_core::analyzer::Refresh;
use treeterm_core::llm::FakeLlm;
use treeterm_core::pty::CreatePty;
use treeterm_core::settings::Settings;
use treeterm_model::analyzer::LlmService;
use treeterm_model::session_store::{SessionState, SessionStore, WorkspaceEntry};
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::terminal::TerminalModel;
use treeterm_model::traffic::ConnectionTraffic;
use treeterm_model::workspace_store::{
    NO_AI_HARNESS_TAB, REFRESH_ANALYZER_GONE, TabContent, WorkspaceStore,
};
use treeterm_proto::treeterm::WorkspaceRef;

mod common;
use common::{Harness, harness, wait_until, wait_until_app};

/// One test at a time: PTY shells forked by one test must not hold another's pipes.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

const TITLE_MODEL: &str = "title-model";

/// Settings with one AI harness, `aiharness-bot`, running `command` and kept on exit; the
/// title model set and the classifier off.
fn bot_settings(command: &str, title_model: &str) -> Settings {
    let mut settings = treeterm_core::settings::from_value(json!({
        "aiHarness": { "instances": [{
            "id": "bot", "name": "Bot", "icon": "B", "command": command,
            "isDefault": false, "enableSandbox": false, "allowNetwork": true,
            "backgroundColor": "#1a1a24", "disableScrollbar": false, "keepOnExit": true
        }]}
    }))
    .unwrap();
    settings.llm.api_key = "test-key".into();
    settings.llm.base_url = "http://localhost".into();
    settings.terminal_analyzer.model = String::new();
    settings.terminal_analyzer.title_model = title_model.into();
    settings
}

/// An LLM whose title model answers `title` as JSON (and whose classifier, if asked, idle).
fn titling(h: &Harness, title: Value) -> (FakeLlm, LlmService) {
    let fake = FakeLlm::default();
    fake.on_complete_chat(move |_, settings| {
        Ok(if settings.model == TITLE_MODEL {
            title.to_string()
        } else {
            r#"{"state":"idle","reason":"waiting"}"#.into()
        })
    });
    let llm = LlmService::new(Arc::new(fake.clone()), h.rt.clone());
    (fake, llm)
}

fn suggestion(branch_name: &str) -> Value {
    json!({ "title": "New Title", "description": "New description", "branchName": branch_name })
}

fn open_store(
    cx: &mut TestAppContext,
    h: &Harness,
    settings: Settings,
    llm: LlmService,
) -> Entity<SessionStore> {
    let conn = h.conn.clone();
    let settings = cx.new(|_| SettingsStore::in_memory(settings));
    cx.new(|cx| {
        SessionStore::new(
            conn,
            "client-a".into(),
            settings,
            Some(llm),
            cx.new(ConnectionTraffic::new),
            cx,
        )
    })
}

/// A new directory under the daemon's root for a workspace: its id and path.
fn workspace_dir(h: &Harness) -> (String, PathBuf) {
    let id = format!("ws-{}", uuid::Uuid::new_v4().simple());
    let dir = h.daemon.root().join(&id);
    std::fs::create_dir_all(&dir).unwrap();
    (id, dir)
}

/// A directory under the daemon's root as a loaded workspace with these tabs.
fn open_workspace(
    cx: &mut TestAppContext,
    h: &Harness,
    store: &Entity<SessionStore>,
    app_states: Value,
) -> Entity<WorkspaceStore> {
    let (id, dir) = workspace_dir(h);
    let body = json!({
        "name": "labels", "status": "active", "isGitRepo": false, "isWorktree": false,
        "appStates": app_states, "metadata": {}, "createdAt": 1, "lastActivity": 1, "parentHash": ""
    });
    load_workspace(cx, h, store, &id, &dir, body)
}

/// A child worktree without tabs, on branch `old-branch` of its own repository (its parent
/// is not in the session), loaded: its store and path.
fn open_child_worktree(
    cx: &mut TestAppContext,
    h: &Harness,
    store: &Entity<SessionStore>,
) -> (Entity<WorkspaceStore>, PathBuf) {
    let (id, dir) = workspace_dir(h);
    git(&dir, &["init", "-q", "-b", "old-branch"]);
    git(
        &dir,
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
    let body = json!({
        "name": "labels", "status": "active", "isGitRepo": true, "isWorktree": true,
        "parentId": "ws-parent", "gitBranch": "old-branch", "gitRootPath": dir,
        "appStates": {}, "metadata": {}, "createdAt": 1, "lastActivity": 1, "parentHash": ""
    });
    (load_workspace(cx, h, store, &id, &dir, body), dir)
}

/// Add workspace `id` at `dir`, as `body` describes it, to the session; wait until it loaded.
fn load_workspace(
    cx: &mut TestAppContext,
    h: &Harness,
    store: &Entity<SessionStore>,
    id: &str,
    dir: &Path,
    body: Value,
) -> Entity<WorkspaceStore> {
    std::fs::write(
        h.daemon.workspaces_dir().join(format!("{id}.json")),
        body.to_string(),
    )
    .unwrap();
    let refs = vec![WorkspaceRef {
        id: id.to_owned(),
        path: dir.to_string_lossy().into_owned(),
    }];
    futures::executor::block_on(h.conn.update_session(refs, None, "test-writer"))
        .unwrap()
        .unwrap();
    wait_until(cx, store, |s| {
        matches!(s.state(), SessionState::Live)
            && matches!(s.entry(id), Some(WorkspaceEntry::Loaded(_)))
    });
    store.read_with(cx, |s, _| s.workspace_store(id).unwrap().clone())
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

/// A login shell with an empty config dir, so the user's dotfiles stay out of the test.
fn spawn_shell(h: &Harness) -> String {
    let mut env = HashMap::new();
    env.insert(
        "ZDOTDIR".to_owned(),
        h.daemon.root().to_string_lossy().into_owned(),
    );
    env.insert("PS1".to_owned(), "$ ".to_owned());
    futures::executor::block_on(h.conn.create_pty(CreatePty {
        cwd: h.daemon.root().to_string_lossy().into_owned(),
        env,
        size: Some((80, 24)),
        ..CreatePty::default()
    }))
    .unwrap()
    .unwrap()
}

fn terminal_of(
    cx: &mut TestAppContext,
    workspace: &Entity<WorkspaceStore>,
    tab: &str,
) -> Entity<TerminalModel> {
    wait_until_app(cx, workspace, |w, _| {
        matches!(w.tab_content(tab), Some(TabContent::Terminal(_)))
    });
    match workspace.read_with(cx, |w, _| w.tab_content(tab)) {
        Some(TabContent::Terminal(terminal)) => terminal,
        _ => unreachable!(),
    }
}

/// A harness tab whose analyzer runs, once its screen shows `marker`.
fn add_harness(
    cx: &mut TestAppContext,
    workspace: &Entity<WorkspaceStore>,
    marker: &str,
) -> (String, Entity<TerminalModel>) {
    let tab = workspace
        .update(cx, |w, cx| w.add_tab("aiharness-bot", cx))
        .unwrap();
    let terminal = terminal_of(cx, workspace, &tab);
    wait_until_app(cx, workspace, |w, _| w.analyzer(&tab).is_some());
    wait_until(cx, &terminal, |t| t.screen_text().contains(marker));
    (tab, terminal)
}

/// Type `text` into a terminal and wait until it shows `shown`.
fn type_into(cx: &mut TestAppContext, terminal: &Entity<TerminalModel>, text: &str, shown: &str) {
    terminal.update(cx, |t, cx| {
        t.set_focused(true, cx);
        t.write_input(text.as_bytes(), cx);
    });
    wait_until(cx, terminal, |t| t.screen_text().contains(shown));
}

/// Run `task` to completion (its I/O arrives from tokio threads in real time).
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

fn refresh(
    cx: &mut TestAppContext,
    workspace: &Entity<WorkspaceStore>,
    refresh: Refresh,
) -> Result<(), String> {
    let task = workspace.update(cx, |w, cx| w.refresh_labels(refresh, cx));
    finish(cx, task)
}

fn is_refreshing(
    cx: &mut TestAppContext,
    workspace: &Entity<WorkspaceStore>,
    refresh: Refresh,
) -> bool {
    workspace.read_with(cx, |w, cx| w.is_refreshing(refresh, cx))
}

fn metadata(
    cx: &mut TestAppContext,
    workspace: &Entity<WorkspaceStore>,
    key: &str,
) -> Option<String> {
    workspace.read_with(cx, |w, _| w.file().metadata.get(key).cloned())
}

/// What the title model was shown, call by call.
fn screens_read(fake: &FakeLlm) -> Vec<String> {
    fake.chat_calls()
        .into_iter()
        .filter(|(_, settings)| settings.model == TITLE_MODEL)
        .map(|(messages, _)| messages.last().unwrap().content.clone())
        .collect()
}

/// The System Prompt Debugger opened from `tab`'s activity badge: its `bufferText`.
fn debugger_buffer(
    cx: &mut TestAppContext,
    workspace: &Entity<WorkspaceStore>,
    tab: &str,
) -> Value {
    let debugger = workspace
        .update(cx, |w, cx| w.open_prompt_debugger_for_tab(tab, cx))
        .unwrap();
    workspace.read_with(cx, |w, _| {
        w.file().app_states[&debugger].state["bufferText"].clone()
    })
}

#[gpui_kit::test]
fn without_an_ai_harness_tab_a_refresh_fails_with_electrons_message(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let (fake, llm) = titling(&h, suggestion("new-branch"));
    let store = open_store(cx, &h, bot_settings("cat", TITLE_MODEL), llm);
    let tabs = json!({ "plain": { "applicationId": "terminal", "title": "Terminal", "state": { "ptyId": pty } } });
    let workspace = open_workspace(cx, &h, &store, tabs);
    terminal_of(cx, &workspace, "plain");

    for kind in [Refresh::TitleAndDescription, Refresh::BranchName] {
        assert_eq!(
            refresh(cx, &workspace, kind),
            Err(NO_AI_HARNESS_TAB.to_owned())
        );
        assert!(!is_refreshing(cx, &workspace, kind));
    }
    assert_eq!(
        NO_AI_HARNESS_TAB,
        "No AI Harness tab open — the labeller has no terminal to read"
    );
    assert!(fake.chat_calls().is_empty());
}

#[gpui_kit::test]
fn a_refresh_without_a_title_model_fails_at_once(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let (fake, llm) = titling(&h, suggestion("new-branch"));
    let settings = bot_settings(r#"sh -c 'echo ready-$((40+2)); exec cat'"#, "");
    let store = open_store(cx, &h, settings, llm);
    let workspace = open_workspace(cx, &h, &store, json!({}));
    add_harness(cx, &workspace, "ready-42");

    let task = workspace.update(cx, |w, cx| {
        w.refresh_labels(Refresh::TitleAndDescription, cx)
    });
    assert!(!is_refreshing(cx, &workspace, Refresh::TitleAndDescription));
    assert_eq!(
        finish(cx, task),
        Err("Title model not configured".to_owned())
    );
    assert!(fake.chat_calls().is_empty());
}

#[gpui_kit::test]
fn a_title_refresh_is_under_way_until_it_relabels_the_workspace(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let (fake, llm) = titling(&h, suggestion("new-branch"));
    let settings = bot_settings(r#"sh -c 'echo ready-$((40+2)); exec cat'"#, TITLE_MODEL);
    let store = open_store(cx, &h, settings, llm);
    let workspace = open_workspace(cx, &h, &store, json!({}));
    add_harness(cx, &workspace, "ready-42");
    // What the workspace's observers (the header's button) see whenever it changes.
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let _observer = cx.update(|cx| {
        cx.observe(&workspace, move |w, cx| {
            sink.borrow_mut()
                .push(w.read(cx).is_refreshing(Refresh::TitleAndDescription, cx));
        })
    });

    let task = workspace.update(cx, |w, cx| {
        w.refresh_labels(Refresh::TitleAndDescription, cx)
    });
    assert!(is_refreshing(cx, &workspace, Refresh::TitleAndDescription));
    assert!(!is_refreshing(cx, &workspace, Refresh::BranchName));
    assert_eq!(seen.borrow().first(), Some(&true));

    assert_eq!(finish(cx, task), Ok(()));
    assert!(!is_refreshing(cx, &workspace, Refresh::TitleAndDescription));
    assert_eq!(seen.borrow().last(), Some(&false));
    assert_eq!(
        metadata(cx, &workspace, "displayName").as_deref(),
        Some("New Title")
    );
    assert_eq!(
        metadata(cx, &workspace, "description").as_deref(),
        Some("New description")
    );
    assert_eq!(
        metadata(cx, &workspace, "descriptionPrompted").as_deref(),
        Some("true")
    );
    assert!(screens_read(&fake)[0].contains("ready-42"));
}

#[gpui_kit::test]
fn the_active_ai_harness_tab_is_read_first(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let (fake, llm) = titling(&h, suggestion("new-branch"));
    let settings = bot_settings(r#"sh -c 'echo ready-$((40+2)); exec cat'"#, TITLE_MODEL);
    let store = open_store(cx, &h, settings, llm);
    let workspace = open_workspace(cx, &h, &store, json!({}));
    let (first, first_terminal) = add_harness(cx, &workspace, "ready-42");
    type_into(cx, &first_terminal, "first-screen\r", "first-screen");
    let (_, second_terminal) = add_harness(cx, &workspace, "ready-42");
    type_into(cx, &second_terminal, "second-screen\r", "second-screen");

    // The second tab is active (added last): it is read, not the first in tab order.
    assert_eq!(
        refresh(cx, &workspace, Refresh::TitleAndDescription),
        Ok(())
    );
    // The first again once it is active.
    workspace.update(cx, |w, cx| w.set_active_tab(&first, cx));
    assert_eq!(
        refresh(cx, &workspace, Refresh::TitleAndDescription),
        Ok(())
    );
    // A tab without an analyzer active: the first harness in tab order.
    workspace
        .update(cx, |w, cx| w.add_tab("comments", cx))
        .unwrap();
    assert_eq!(
        refresh(cx, &workspace, Refresh::TitleAndDescription),
        Ok(())
    );

    let screens = screens_read(&fake);
    assert_eq!(screens.len(), 3);
    assert!(screens[0].contains("second-screen") && !screens[0].contains("first-screen"));
    assert!(screens[1].contains("first-screen") && !screens[1].contains("second-screen"));
    assert!(screens[2].contains("first-screen") && !screens[2].contains("second-screen"));
}

#[gpui_kit::test]
fn an_exited_harness_gives_the_labeller_and_the_debugger_nothing(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let (fake, llm) = titling(&h, suggestion("new-branch"));
    let settings = bot_settings(r#"sh -c 'echo bye-$((40+2))'"#, TITLE_MODEL);
    let store = open_store(cx, &h, settings, llm);
    let workspace = open_workspace(cx, &h, &store, json!({}));
    let (tab, terminal) = add_harness(cx, &workspace, "bye-42");
    // keepOnExit: the tab stays with the final screen, its analyzer stopped.
    wait_until_app(cx, &workspace, |w, cx| {
        w.analyzer(&tab)
            .is_some_and(|a| !a.read(cx).analyzer().is_running())
    });
    assert!(terminal.read_with(cx, |t, _| t.viewport().contains("bye-42")));

    for kind in [Refresh::TitleAndDescription, Refresh::BranchName] {
        assert_eq!(
            refresh(cx, &workspace, kind),
            Err("Terminal is empty — nothing for the labeller to read".to_owned())
        );
    }
    assert!(fake.chat_calls().is_empty());
    assert_eq!(debugger_buffer(cx, &workspace, &tab), json!(""));
}

#[gpui_kit::test]
fn the_debugger_gets_nothing_from_a_blank_harness_and_a_plain_terminals_screen(
    cx: &mut TestAppContext,
) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let (_, llm) = titling(&h, suggestion("new-branch"));
    let settings = bot_settings(
        r#"sh -c 'echo started-$((40+2)); read go; clear; exec sleep 60'"#,
        TITLE_MODEL,
    );
    let store = open_store(cx, &h, settings, llm);
    let tabs = json!({ "plain": { "applicationId": "terminal", "title": "Terminal", "state": { "ptyId": pty } } });
    let workspace = open_workspace(cx, &h, &store, tabs);
    let (tab, terminal) = add_harness(cx, &workspace, "started-42");
    terminal.update(cx, |t, cx| {
        t.set_focused(true, cx);
        t.write_input(b"\r", cx);
    });
    wait_until(cx, &terminal, |t| t.screen_text().trim().is_empty());
    assert!(terminal.read_with(cx, |t, _| !t.viewport().is_empty()));
    assert_eq!(debugger_buffer(cx, &workspace, &tab), json!(""));

    let plain = terminal_of(cx, &workspace, "plain");
    wait_until(cx, &plain, |t| !t.screen_text().trim().is_empty());
    let (screen, debugger) = workspace.update(cx, |w, cx| {
        let screen = plain.read(cx).viewport();
        (screen, w.open_prompt_debugger_for_tab("plain", cx).unwrap())
    });
    let buffer = workspace.read_with(cx, |w, _| {
        w.file().app_states[&debugger].state["bufferText"].clone()
    });
    assert_eq!(buffer, json!(screen));
}

#[gpui_kit::test]
fn a_branch_refresh_reports_an_invalid_suggestion(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let (_, llm) = titling(&h, suggestion("Bad Name"));
    let settings = bot_settings(r#"sh -c 'echo ready-$((40+2)); exec cat'"#, TITLE_MODEL);
    let store = open_store(cx, &h, settings, llm);
    let workspace = open_workspace(cx, &h, &store, json!({}));
    add_harness(cx, &workspace, "ready-42");

    let task = workspace.update(cx, |w, cx| w.refresh_labels(Refresh::BranchName, cx));
    assert!(is_refreshing(cx, &workspace, Refresh::BranchName));
    assert!(!is_refreshing(cx, &workspace, Refresh::TitleAndDescription));
    assert_eq!(
        finish(cx, task),
        Err("LLM suggested an invalid branch name: \"Bad Name\"".to_owned())
    );
    assert!(!is_refreshing(cx, &workspace, Refresh::BranchName));
    assert_eq!(metadata(cx, &workspace, "branchIsUserDefined"), None);
}

#[gpui_kit::test]
fn a_branch_refresh_is_under_way_until_the_branch_is_renamed(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let (fake, llm) = titling(&h, suggestion("new-branch"));
    let settings = bot_settings(r#"sh -c 'echo ready-$((40+2)); exec cat'"#, TITLE_MODEL);
    let store = open_store(cx, &h, settings, llm);
    let (workspace, dir) = open_child_worktree(cx, &h, &store);
    let (tab, _) = add_harness(cx, &workspace, "ready-42");
    let analyzer = workspace.read_with(cx, |w, _| w.analyzer(&tab).unwrap());
    // What the branch badge reads (Renaming..., else the branch) whenever the workspace or
    // its analyzer changes.
    let seen = Rc::new(RefCell::new(Vec::new()));
    let record = {
        let (seen, workspace) = (seen.clone(), workspace.clone());
        move |cx: &mut gpui_kit::App| {
            let w = workspace.read(cx);
            let badge = (
                w.is_refreshing(Refresh::BranchName, cx),
                w.file().git_branch.clone(),
            );
            seen.borrow_mut().push(badge);
        }
    };
    let _observers = cx.update(|cx| {
        let on_analyzer = record.clone();
        [
            cx.observe(&workspace, move |_, cx| record(cx)),
            cx.observe(&analyzer, move |_, cx| on_analyzer(cx)),
        ]
    });
    seen.borrow_mut().clear();

    let task = workspace.update(cx, |w, cx| w.refresh_labels(Refresh::BranchName, cx));
    assert!(is_refreshing(cx, &workspace, Refresh::BranchName));
    assert!(!is_refreshing(cx, &workspace, Refresh::TitleAndDescription));
    assert_eq!(finish(cx, task), Ok(()));
    assert!(!is_refreshing(cx, &workspace, Refresh::BranchName));

    let old = Some("old-branch".to_owned());
    let new = Some("new-branch".to_owned());
    let seen = seen.borrow();
    assert_eq!(seen.first(), Some(&(true, old.clone())));
    // Under way through the LLM call and the git rename: it ends with the new branch.
    assert!(!seen.contains(&(false, old)), "{seen:?}");
    assert_eq!(seen.last(), Some(&(false, new)));
    assert_eq!(
        git(&dir, &["branch", "--show-current"]).trim(),
        "new-branch"
    );
    assert_eq!(
        metadata(cx, &workspace, "branchIsUserDefined").as_deref(),
        Some("true")
    );
    assert_eq!(screens_read(&fake).len(), 1);
}

#[gpui_kit::test]
fn a_refresh_whose_harness_tab_closes_fails(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let (_, llm) = titling(&h, suggestion("new-branch"));
    let settings = bot_settings(r#"sh -c 'echo ready-$((40+2)); exec cat'"#, TITLE_MODEL);
    let store = open_store(cx, &h, settings, llm);
    let workspace = open_workspace(cx, &h, &store, json!({}));
    let (tab, _) = add_harness(cx, &workspace, "ready-42");

    let task = workspace.update(cx, |w, cx| {
        w.refresh_labels(Refresh::TitleAndDescription, cx)
    });
    workspace.update(cx, |w, cx| w.remove_tab(&tab, cx));
    assert_eq!(finish(cx, task), Err(REFRESH_ANALYZER_GONE.to_owned()));
    assert!(!is_refreshing(cx, &workspace, Refresh::TitleAndDescription));
    assert_eq!(metadata(cx, &workspace, "displayName"), None);
}
