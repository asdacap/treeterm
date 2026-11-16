//! The AI harness's auto-approve switch, against a real in-process daemon.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use serde_json::{Value, json};
use treeterm_core::llm::FakeLlm;
use treeterm_core::pty::CreatePty;
use treeterm_core::settings::{ClassifierProvider, Settings};
use treeterm_model::analyzer::LlmService;
use treeterm_model::session_store::{SessionState, SessionStore, WorkspaceEntry};
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::workspace_store::WorkspaceStore;
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
    cx.new(|cx| SessionStore::new(conn, "client-a".into(), settings, llm, cx))
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
