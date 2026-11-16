//! Writes into a running AI harness (prompts, the workspace description and auto-approve)
//! and the auto-approve switch, against a real in-process daemon.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{AppContext as _, Entity, Task, TestAppContext};
use serde_json::{Value, json};
use treeterm_core::llm::FakeLlm;
use treeterm_core::pty::CreatePty;
use treeterm_core::review::{REVIEW_COMMENTS_KEY, Resolution, ReviewComments};
use treeterm_core::settings::{ClassifierProvider, Settings, TerminalSettings};
use treeterm_model::analyzer::LlmService;
use treeterm_model::harness_prompts::{self, HarnessPrompt};
use treeterm_model::session_store::{SessionState, SessionStore, WorkspaceEntry};
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::terminal::{Overlay, ScrollbarVisibility, SelectionCopy, TerminalModel, TerminalOptions};
use treeterm_model::traffic::ConnectionTraffic;
use treeterm_model::workspace_store::{TabContent, WorkspaceStore};
use treeterm_proto::treeterm::WorkspaceRef;
use treeterm_term::fit::WidthLimit;
use treeterm_term::palette::DEFAULT_BACKGROUND;

mod common;
use common::{Harness, harness, wait_until, wait_until_app};

/// One test at a time: PTY shells forked by one test must not hold another's pipes.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Settings with one AI harness, `aiharness-bot`, running `command`.
fn bot_settings(command: &str) -> Settings {
    treeterm_core::settings::from_value(json!({
        "aiHarness": { "instances": [{
            "id": "bot", "name": "Bot", "icon": "B", "command": command,
            "isDefault": false, "enableSandbox": false, "allowNetwork": true,
            "backgroundColor": "#1a1a24", "disableScrollbar": false, "keepOnExit": true
        }]}
    }))
    .unwrap()
}

/// An LLM whose classifier calls a screen safe to approve once it shows `marker`.
fn safe_once_shown(settings: &mut Settings, h: &Harness, marker: &'static str) -> LlmService {
    settings.llm.api_key = "test-key".into();
    settings.llm.base_url = "http://localhost".into();
    settings.terminal_analyzer.provider = ClassifierProvider::ChatCompletions;
    settings.terminal_analyzer.model = "test-model".into();
    settings.terminal_analyzer.idle_debounce = Duration::from_millis(300);
    let fake = FakeLlm::default();
    fake.on_complete_chat(move |messages, _| {
        Ok(if messages.iter().any(|m| m.content.contains(marker)) {
            r#"{"state":"safe_permission_requested","reason":"only edits the worktree"}"#.into()
        } else {
            r#"{"state":"idle","reason":"waiting"}"#.into()
        })
    });
    LlmService::new(Arc::new(fake), h.rt.clone())
}

fn open_store(
    cx: &mut TestAppContext,
    h: &Harness,
    settings: Settings,
    llm: Option<LlmService>,
) -> Entity<SessionStore> {
    let conn = h.conn.clone();
    let settings = cx.new(|_| SettingsStore::in_memory(settings));
    cx.new(|cx| {
        SessionStore::new(
            conn,
            "client-a".into(),
            settings,
            llm,
            cx.new(ConnectionTraffic::new),
            cx,
        )
    })
}

/// A directory under the daemon's root as a loaded workspace with these tabs; its id and
/// store.
fn open_workspace(
    cx: &mut TestAppContext,
    h: &Harness,
    store: &Entity<SessionStore>,
    app_states: Value,
    active: Option<&str>,
    metadata: Value,
) -> (String, Entity<WorkspaceStore>) {
    let id = format!("ws-{}", uuid::Uuid::new_v4().simple());
    let dir = h.daemon.root().join(&id);
    std::fs::create_dir_all(&dir).unwrap();
    let mut body = json!({
        "name": "harness", "status": "active", "isGitRepo": false, "isWorktree": false,
        "appStates": app_states, "metadata": metadata, "createdAt": 1, "lastActivity": 1, "parentHash": ""
    });
    if let Some(active) = active {
        body["activeTabId"] = active.into();
    }
    std::fs::write(
        h.daemon.workspaces_dir().join(format!("{id}.json")),
        body.to_string(),
    )
    .unwrap();
    let refs = vec![WorkspaceRef {
        id: id.clone(),
        path: dir.to_string_lossy().into_owned(),
    }];
    futures::executor::block_on(h.conn.update_session(refs, None, "test-writer"))
        .unwrap()
        .unwrap();
    wait_until(cx, store, |s| {
        matches!(s.state(), SessionState::Live)
            && matches!(s.entry(&id), Some(WorkspaceEntry::Loaded(_)))
    });
    let workspace = store.read_with(cx, |s, _| s.workspace_store(&id).unwrap().clone());
    (id, workspace)
}

fn sandbox() -> Value {
    json!({ "enabled": false, "allowNetwork": true, "allowedPaths": [] })
}

/// Workspace metadata holding one open review comment, `c1`.
fn open_comment() -> Value {
    let comment = json!({
        "id": "c1", "filePath": "src/lib.rs", "lineNumber": 5, "text": "fix c1",
        "commitHash": "abc", "lineCommitHash": null, "createdAt": 1_700_000_000_000_i64,
        "isOutdated": false, "addressed": false, "side": "modified"
    });
    json!({ REVIEW_COMMENTS_KEY: json!([comment]).to_string() })
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

/// Another terminal attachment of `pty_id` (another window, Electron), once its replay is
/// shown.
fn viewer(cx: &mut TestAppContext, h: &Harness, pty_id: &str) -> Entity<TerminalModel> {
    let options = TerminalOptions {
        pty_id: pty_id.to_owned(),
        background: DEFAULT_BACKGROUND,
        settings: TerminalSettings::default(),
        keep_on_exit: true,
        width_limit: WidthLimit::Capped(160),
        scrollbar: ScrollbarVisibility::Shown,
        selection_copy: SelectionCopy::OnCopy,
        idle_timeout: None,
        traffic: treeterm_core::traffic::ByteCounter::default(),
    };
    let conn = h.conn.clone();
    let model = cx.new(|cx| TerminalModel::new(conn, options, cx));
    wait_until(cx, &model, |m| {
        *m.overlay() == Overlay::Hidden && !m.screen_text().trim().is_empty()
    });
    model
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

fn file_on_disk(h: &Harness, id: &str) -> Value {
    let bytes = std::fs::read(h.daemon.workspaces_dir().join(format!("{id}.json"))).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// Wait for a condition on the workspace file (written from tokio threads).
fn wait_for_disk(cx: &mut TestAppContext, done: impl Fn() -> bool) {
    for _ in 0..1000 {
        cx.run_until_parked();
        if done() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("disk condition not reached");
}

fn active_tab(cx: &mut TestAppContext, workspace: &Entity<WorkspaceStore>) -> Option<String> {
    workspace.read_with(cx, |w, _| w.file().active_tab_id.clone())
}

fn prompt(
    cx: &mut TestAppContext,
    workspace: &Entity<WorkspaceStore>,
    text: &str,
) -> Result<(), String> {
    let task = workspace.update(cx, |w, cx| w.prompt_harness(text, cx));
    finish(cx, task)
}

#[gpui_kit::test]
fn a_prompt_lands_raw_while_another_terminal_holds_the_input(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    // Asks for bracketed paste, then shows every byte it gets (ESC as ^[).
    let store = open_store(
        cx,
        &h,
        bot_settings(r#"sh -c 'printf "\033[?2004h"; echo ready-$((40+2)); exec cat -v'"#),
        None,
    );
    let (_, workspace) = open_workspace(cx, &h, &store, json!({}), None, json!({}));
    let bot = workspace
        .update(cx, |w, cx| w.add_tab("aiharness-bot", cx))
        .unwrap();
    let terminal = terminal_of(cx, &workspace, &bot);
    wait_until(cx, &terminal, |t| t.screen_text().contains("ready-42"));
    let other = workspace
        .update(cx, |w, cx| w.add_tab("comments", cx))
        .unwrap();
    assert_eq!(active_tab(cx, &workspace), Some(other));

    // The tab's terminal held the input; its keystroke landing proves it.
    terminal.update(cx, |t, cx| {
        t.set_focused(true, cx);
        t.write_input(b"held-by-tab\r", cx);
    });
    wait_until(cx, &terminal, |t| t.screen_text().contains("held-by-tab"));
    // Another terminal of the PTY claims its input; its own keystroke landing proves it.
    let pty = terminal.read_with(cx, |t, _| t.pty_id().to_owned());
    let claimer = viewer(cx, &h, &pty);
    claimer.update(cx, |t, cx| {
        t.set_focused(true, cx);
        t.write_input(b"claimed-by-viewer\r", cx);
    });
    wait_until(cx, &terminal, |t| {
        t.screen_text().contains("claimed-by-viewer")
    });
    // The tab's own terminal attachment is now gated by the daemon: it kept focus, so no
    // key press claimed the input back.
    terminal.update(cx, |t, cx| t.write_input(b"dropped-by-daemon", cx));

    let result = prompt(cx, &workspace, "hello\nworld");
    assert_eq!(result, Ok(()));
    wait_until(cx, &terminal, |t| t.screen_text().contains("world"));
    let screen = terminal.read_with(cx, |t, _| t.screen_text());
    assert!(screen.contains("hello"), "{screen}");
    assert!(
        !screen.contains("[200~"),
        "written raw, not bracketed: {screen}"
    );
    assert!(!screen.contains("dropped-by-daemon"), "{screen}");
    assert_eq!(active_tab(cx, &workspace), Some(bot));
}

#[gpui_kit::test]
fn a_prompt_that_cannot_be_written_shows_nothing_and_marks_nothing(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h, Settings::default(), None);
    let tabs = json!({
        "other": { "applicationId": "comments", "title": "Comments", "state": {} },
        "dead": { "applicationId": "aiharness-bot", "title": "Bot", "state": { "ptyId": "no-such-pty", "sandbox": sandbox() } },
    });
    let (id, workspace) = open_workspace(cx, &h, &store, tabs, Some("other"), open_comment());
    assert!(workspace.read_with(cx, |w, _| w.has_running_harness()));

    // Written twice (the retry through a fresh writer), failing both times.
    let task =
        cx.update(|cx| harness_prompts::send(&HarnessPrompt::ReviewComments, &store, &id, cx));
    let error = finish(cx, task).unwrap_err();
    assert!(
        error.starts_with("Failed to write to the AI harness"),
        "{error}"
    );
    let raw = workspace.read_with(cx, |w, _| {
        w.file().metadata.get(REVIEW_COMMENTS_KEY).cloned()
    });
    let comments = ReviewComments::parse(raw.as_deref()).unwrap().comments();
    assert_eq!(comments[0].resolution, Resolution::Open);
    assert_eq!(active_tab(cx, &workspace).as_deref(), Some("other"));
}

#[gpui_kit::test]
fn a_harness_tab_is_found_by_its_id_and_state_even_without_its_instance(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let store = open_store(cx, &h, Settings::default(), None);
    // `aiharness-gone` is not in the settings, and its state lacks a sandbox.
    let tabs = json!({ "gone": { "applicationId": "aiharness-gone", "title": "Gone", "state": { "ptyId": pty } } });
    let (_, workspace) = open_workspace(cx, &h, &store, tabs, Some("gone"), json!({}));
    assert!(
        !workspace.read_with(cx, |w, _| w.has_running_harness()),
        "not a harness state without a sandbox"
    );
    let result = prompt(cx, &workspace, "echo nope");
    assert_eq!(
        result,
        Err("No AI harness is running in this workspace".to_owned())
    );

    let set_state = |cx: &mut TestAppContext, state: Value| {
        workspace.update(cx, |w, cx| w.update_tab_state("gone", |s| *s = state, cx));
    };
    set_state(cx, json!({ "ptyId": null, "sandbox": sandbox() }));
    assert!(
        !workspace.read_with(cx, |w, _| w.has_running_harness()),
        "no PTY yet"
    );
    set_state(cx, json!({ "ptyId": pty, "sandbox": sandbox() }));
    assert!(workspace.read_with(cx, |w, _| w.has_running_harness()));

    let shell = viewer(cx, &h, &pty);
    let result = prompt(cx, &workspace, "echo prompted-$((40+2))");
    assert_eq!(result, Ok(()));
    wait_until(cx, &shell, |t| t.screen_text().contains("prompted-42"));
}

#[gpui_kit::test]
fn the_comments_tab_marks_a_comment_prompted_once_it_landed(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let store = open_store(cx, &h, Settings::default(), None);
    let tabs = json!({
        "list": { "applicationId": "comments", "title": "Comments", "state": {} },
        "gone": { "applicationId": "aiharness-gone", "title": "Gone", "state": { "ptyId": pty, "sandbox": sandbox() } },
    });
    let (_, workspace) = open_workspace(cx, &h, &store, tabs, Some("list"), open_comment());
    let Some(TabContent::Comments(model)) = workspace.read_with(cx, |w, _| w.tab_content("list"))
    else {
        panic!("not a comments tab");
    };
    let shell = viewer(cx, &h, &pty);

    model.update(cx, |m, cx| m.prompt_comment("c1", cx));
    wait_until_app(cx, &model, |m, cx| {
        m.comments(cx).unwrap()[0].resolution == Resolution::Addressed
    });
    assert_eq!(
        model.read_with(cx, |m, _| m.error().map(str::to_owned)),
        None
    );
    assert_eq!(active_tab(cx, &workspace).as_deref(), Some("gone"));
    wait_until(cx, &shell, |t| t.screen_text().contains("fix c1"));
}

fn description_prompt(
    cx: &mut TestAppContext,
    workspace: &Entity<WorkspaceStore>,
) -> Option<String> {
    workspace.read_with(cx, |w, _| w.description_prompt().map(str::to_owned))
}

fn description_prompted(
    cx: &mut TestAppContext,
    workspace: &Entity<WorkspaceStore>,
) -> Option<String> {
    workspace.read_with(cx, |w, _| {
        w.file().metadata.get("descriptionPrompted").cloned()
    })
}

/// A harness tab, `bot`, whose instance left the settings (so nothing is started for it),
/// on `pty_id`, and a Comments tab, `list`.
fn bot_and_list(pty_id: Value) -> Value {
    json!({
        "bot": { "applicationId": "aiharness-gone", "title": "Gone", "state": { "ptyId": pty_id, "sandbox": sandbox() } },
        "list": { "applicationId": "comments", "title": "Comments", "state": {} },
    })
}

#[gpui_kit::test]
fn the_description_is_offered_while_a_harness_tab_is_active_until_prompted(
    cx: &mut TestAppContext,
) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h, Settings::default(), None);
    let metadata = json!({ "description": "Fix the login bug" });
    let (_, workspace) = open_workspace(
        cx,
        &h,
        &store,
        bot_and_list(Value::Null),
        Some("bot"),
        metadata,
    );
    let offered = Some("Fix the login bug".to_owned());
    assert_eq!(description_prompt(cx, &workspace), offered);

    workspace.update(cx, |w, cx| w.set_active_tab("list", cx));
    assert_eq!(
        description_prompt(cx, &workspace),
        None,
        "not on a harness tab"
    );
    // Nothing is offered there, so nothing is sent or marked.
    let task = workspace.update(cx, |w, cx| w.prompt_description(cx));
    assert_eq!(finish(cx, task), Ok(()));
    assert_eq!(description_prompted(cx, &workspace), None);
    workspace.update(cx, |w, cx| w.set_active_tab("bot", cx));
    assert_eq!(description_prompt(cx, &workspace), offered);

    let set = |cx: &mut TestAppContext, key: &str, value: &str| {
        workspace.update(cx, |w, cx| w.update_metadata(key, value, cx));
    };
    set(cx, "descriptionPrompted", "true");
    assert_eq!(description_prompt(cx, &workspace), None, "already prompted");
    set(cx, "descriptionPrompted", "");
    assert_eq!(
        description_prompt(cx, &workspace),
        offered,
        "empty counts as unset"
    );
    set(cx, "description", "");
    assert_eq!(description_prompt(cx, &workspace), None, "no description");
}

#[gpui_kit::test]
fn without_an_active_tab_the_first_tab_decides(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h, Settings::default(), None);
    let metadata = json!({ "description": "Fix the login bug" });
    let (_, harness_first) = open_workspace(
        cx,
        &h,
        &store,
        bot_and_list(Value::Null),
        None,
        metadata.clone(),
    );
    assert_eq!(
        description_prompt(cx, &harness_first),
        Some("Fix the login bug".to_owned())
    );

    let list_first = json!({
        "list": { "applicationId": "comments", "title": "Comments", "state": {} },
        "bot": { "applicationId": "aiharness-gone", "title": "Gone", "state": { "ptyId": null, "sandbox": sandbox() } },
    });
    let (_, list_first) = open_workspace(cx, &h, &store, list_first, None, metadata);
    assert_eq!(description_prompt(cx, &list_first), None);
}

#[gpui_kit::test]
fn skipping_the_description_marks_it_prompted_on_disk(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h, Settings::default(), None);
    let metadata = json!({ "description": "Fix the login bug" });
    let (id, workspace) = open_workspace(
        cx,
        &h,
        &store,
        bot_and_list(Value::Null),
        Some("bot"),
        metadata,
    );

    workspace.update(cx, |w, cx| w.dismiss_description_prompt(cx));
    assert_eq!(description_prompt(cx, &workspace), None);
    wait_for_disk(cx, || {
        file_on_disk(&h, &id)["metadata"]["descriptionPrompted"] == "true"
    });
    assert_eq!(
        file_on_disk(&h, &id)["metadata"]["description"],
        "Fix the login bug"
    );
}

#[gpui_kit::test]
fn prompting_the_description_types_it_into_the_harness_and_marks_it_prompted(
    cx: &mut TestAppContext,
) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let store = open_store(cx, &h, Settings::default(), None);
    let metadata = json!({ "description": "echo described-$((40+2))" });
    let (id, workspace) = open_workspace(
        cx,
        &h,
        &store,
        bot_and_list(json!(pty)),
        Some("bot"),
        metadata,
    );
    let shell = viewer(cx, &h, &pty);

    let task = workspace.update(cx, |w, cx| w.prompt_description(cx));
    // Marked at once, before the write lands.
    assert_eq!(
        description_prompted(cx, &workspace).as_deref(),
        Some("true")
    );
    assert_eq!(description_prompt(cx, &workspace), None);
    assert_eq!(finish(cx, task), Ok(()));
    wait_until(cx, &shell, |t| t.screen_text().contains("described-42"));
    wait_for_disk(cx, || {
        file_on_disk(&h, &id)["metadata"]["descriptionPrompted"] == "true"
    });
}

#[gpui_kit::test]
fn prompting_the_description_without_a_running_harness_still_marks_it_prompted(
    cx: &mut TestAppContext,
) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h, Settings::default(), None);
    let metadata = json!({ "description": "Fix the login bug" });
    let (_, workspace) = open_workspace(
        cx,
        &h,
        &store,
        bot_and_list(Value::Null),
        Some("bot"),
        metadata,
    );
    assert_eq!(
        description_prompt(cx, &workspace),
        Some("Fix the login bug".to_owned())
    );

    let task = workspace.update(cx, |w, cx| w.prompt_description(cx));
    assert_eq!(
        finish(cx, task),
        Err("No AI harness is running in this workspace".to_owned())
    );
    assert_eq!(
        description_prompted(cx, &workspace).as_deref(),
        Some("true")
    );
}

#[gpui_kit::test]
fn auto_approve_starts_off_and_is_never_saved(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let pty = spawn_shell(&h);
    let mut settings = bot_settings("cat");
    let llm = safe_once_shown(&mut settings, &h, "never shown");
    let store = open_store(cx, &h, settings, Some(llm));
    // Saved by an earlier native build that persisted the switch.
    let state =
        json!({ "ptyId": pty, "sandbox": sandbox(), "autoApprove": true, "keepOnExit": true });
    let tabs =
        json!({ "bot": { "applicationId": "aiharness-bot", "title": "Bot", "state": state } });
    let (_, workspace) = open_workspace(cx, &h, &store, tabs, Some("bot"), json!({}));
    wait_until_app(cx, &workspace, |w, _| w.analyzer("bot").is_some());
    let analyzer = workspace.read_with(cx, |w, _| w.analyzer("bot")).unwrap();
    assert!(!analyzer.read_with(cx, |a, _| a.analyzer().auto_approve()));

    workspace.update(cx, |w, cx| w.set_auto_approve("bot", true, cx));
    assert!(analyzer.read_with(cx, |a, _| a.analyzer().auto_approve()));
    workspace.update(cx, |w, cx| w.set_auto_approve("bot", false, cx));
    assert!(!analyzer.read_with(cx, |a, _| a.analyzer().auto_approve()));
    // The tab's state still holds what it was loaded with.
    let saved = workspace.read_with(cx, |w, _| {
        w.file().app_states["bot"].state["autoApprove"].clone()
    });
    assert_eq!(saved, json!(true));
}

#[gpui_kit::test]
fn auto_approve_lands_while_another_terminal_holds_the_input(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    // Waits for the other terminal's line, asks, and reports the answer it got.
    let mut settings = bot_settings(
        r#"sh -c 'read claim; echo "Allow write to config-$((40+2)).toml? (y/n)"; read answer; echo approved-$((40+2)); sleep 60'"#,
    );
    let llm = safe_once_shown(&mut settings, &h, "config-42.toml");
    let store = open_store(cx, &h, settings, Some(llm));
    let (_, workspace) = open_workspace(cx, &h, &store, json!({}), None, json!({}));
    let bot = workspace
        .update(cx, |w, cx| w.add_tab("aiharness-bot", cx))
        .unwrap();
    let terminal = terminal_of(cx, &workspace, &bot);
    wait_until_app(cx, &workspace, |w, _| w.analyzer(&bot).is_some());
    workspace.update(cx, |w, cx| w.set_auto_approve(&bot, true, cx));
    // The command has started once its line is echoed.
    wait_until(cx, &terminal, |t| t.screen_text().contains("exec sh -c"));

    let pty = terminal.read_with(cx, |t, _| t.pty_id().to_owned());
    let claimer = viewer(cx, &h, &pty);
    claimer.update(cx, |t, cx| {
        t.set_focused(true, cx);
        t.write_input(b"claimed\r", cx);
    });
    wait_until(cx, &terminal, |t| {
        t.screen_text().contains("config-42.toml")
    });

    // Each idle edge (fake clock) gets the screen classified; the safe question is approved.
    for _ in 0..1000 {
        std::thread::sleep(Duration::from_millis(20));
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        if terminal.read_with(cx, |t, _| t.screen_text().contains("approved-42")) {
            return;
        }
    }
    let screen = terminal.read_with(cx, |t, _| t.screen_text());
    panic!("the safe question was never approved; screen:\n{screen}");
}
