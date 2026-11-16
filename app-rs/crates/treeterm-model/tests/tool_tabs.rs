//! The tool tabs (Settings, Comments, TTYs, Chat, System Prompt Debugger, Activity History,
//! GitHub, Upload) against a real in-process daemon.

use std::sync::Arc;

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use serde_json::json;
use treeterm_core::activity::{ActivityState, TransitionDetail};
use treeterm_core::llm::{FakeLlm, LlmError};
use treeterm_core::review::Resolution;
use treeterm_core::settings::{ClassifierProvider, Settings};
use treeterm_model::analyzer::LlmService;
use treeterm_model::run_actions::{Detection, RunActions};
use treeterm_model::session_store::{SessionState, SessionStore, WorkspaceEntry};
use treeterm_model::settings_store::SettingsStore;
use treeterm_model::tool_tabs::analyzer_history::HistorySource;
use treeterm_model::tool_tabs::chat::{ChatRole, MISSING_API_KEY};
use treeterm_model::tool_tabs::comments::PushState;
use treeterm_model::tool_tabs::prompt_debugger::{DebugMode, DebugOutput, PromptDebuggerEvent};
use treeterm_model::tool_tabs::ssh_upload::UploadTarget;
use treeterm_model::tool_tabs::tty_list::{ProcessList, referenced_pty_ids};
use treeterm_model::traffic::ConnectionTraffic;
use treeterm_model::workspace_store::{TabContent, WorkspaceStore};
use treeterm_proto::treeterm::WorkspaceRef;

mod common;
use common::{Harness, harness, wait_until, wait_until_app};

/// One test at a time: PTY shells forked by one test must not hold another's pipes.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn llm_settings() -> Settings {
    let mut settings = Settings::default();
    settings.llm.api_key = "test-key".into();
    settings.llm.base_url = "http://localhost".into();
    settings.llm.model = "chat-model".into();
    settings.terminal_analyzer.provider = ClassifierProvider::ChatCompletions;
    settings.terminal_analyzer.model = "classifier-model".into();
    settings.terminal_analyzer.title_model = "title-model".into();
    settings
}

fn open_store(cx: &mut TestAppContext, h: &Harness, settings: Settings, llm: Option<LlmService>) -> Entity<SessionStore> {
    open_window(cx, h, "client-a", settings, llm)
}

/// The session as window `client` sees it.
fn open_window(
    cx: &mut TestAppContext,
    h: &Harness,
    client: &str,
    settings: Settings,
    llm: Option<LlmService>,
) -> Entity<SessionStore> {
    let (conn, client) = (h.conn.clone(), client.to_owned());
    let settings = cx.new(|_| SettingsStore::in_memory(settings));
    cx.new(|cx| SessionStore::new(conn, client, settings, llm, cx.new(ConnectionTraffic::new), cx))
}

/// A directory under the daemon's root as a loaded workspace with no tabs; its id and store.
fn open_workspace(
    cx: &mut TestAppContext,
    h: &Harness,
    store: &Entity<SessionStore>,
    metadata: serde_json::Value,
) -> (String, std::path::PathBuf, Entity<WorkspaceStore>) {
    let id = format!("ws-{}", uuid::Uuid::new_v4().simple());
    let dir = h.daemon.root().join(&id);
    std::fs::create_dir_all(&dir).unwrap();
    let body = json!({
        "name": "tools", "status": "active", "isGitRepo": false, "isWorktree": false,
        "appStates": {}, "metadata": metadata, "createdAt": 1, "lastActivity": 1, "parentHash": ""
    });
    std::fs::write(h.daemon.workspaces_dir().join(format!("{id}.json")), body.to_string()).unwrap();
    let refs = vec![WorkspaceRef { id: id.clone(), path: dir.to_string_lossy().into_owned() }];
    futures::executor::block_on(h.conn.update_session(refs, None, "test-writer")).unwrap().unwrap();
    wait_until(cx, store, |s| matches!(s.state(), SessionState::Live) && matches!(s.entry(&id), Some(WorkspaceEntry::Loaded(_))));
    let workspace = store.read_with(cx, |s, _| s.workspace_store(&id).unwrap().clone());
    (id, dir, workspace)
}

fn file_on_disk(h: &Harness, id: &str) -> serde_json::Value {
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
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("disk condition not reached");
}

fn content(cx: &mut TestAppContext, workspace: &Entity<WorkspaceStore>, tab: &str) -> TabContent {
    workspace.read_with(cx, |w, _| w.tab_content(tab)).expect("the tab exists")
}

#[gpui_kit::test]
fn the_settings_tab_opens_once_and_edits_the_workspace_in_electrons_format(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h, Settings::default(), None);
    let (id, _dir, workspace) = open_workspace(cx, &h, &store, json!({}));

    let tab = workspace.update(cx, |w, cx| w.open_or_focus_tab("workspace-settings", cx)).unwrap();
    assert!(matches!(content(cx, &workspace, &tab), TabContent::WorkspaceSettings));
    let other = workspace.update(cx, |w, cx| w.add_tab("filesystem", cx)).unwrap();
    assert_eq!(workspace.read_with(cx, |w, _| w.file().active_tab_id.clone()), Some(other));
    // Settings again focuses the same tab.
    assert_eq!(workspace.update(cx, |w, cx| w.open_or_focus_tab("workspace-settings", cx)), Some(tab.clone()));
    assert_eq!(workspace.read_with(cx, |w, _| w.file().active_tab_id.clone()), Some(tab.clone()));

    workspace.update(cx, |w, cx| {
        w.set_display_name("  Renamed  ", cx);
        w.set_display_name("   ", cx); // blank names are ignored
        w.set_description(" what it is for ", cx);
        w.set_default_application("filesystem", cx);
    });
    wait_for_disk(cx, || file_on_disk(&h, &id)["settings"]["defaultApplicationId"] == "filesystem");
    let disk = file_on_disk(&h, &id);
    assert_eq!(disk["metadata"]["displayName"], "Renamed");
    assert_eq!(disk["metadata"]["description"], "what it is for");
    assert_eq!(disk["appStates"][&tab]["applicationId"], "workspace-settings");
    assert_eq!(disk["appStates"][&tab]["state"], json!({}));

    let raw: serde_json::Value =
        serde_json::from_str(&workspace.read_with(cx, |w, _| w.workspace_json()).unwrap()).unwrap();
    assert_eq!(raw["id"], id.as_str());
    assert_eq!(raw["settings"]["defaultApplicationId"], "filesystem");
}

/// "Mark as Favourite" and the favourite's "Unmark as Favourite" (`toggleFavourite`): marked,
/// the metadata says `isFavourite: "true"`; unmarked, the key is gone, as Electron leaves it.
#[gpui_kit::test]
fn a_workspace_is_marked_and_unmarked_as_favourite_in_electrons_format(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h, Settings::default(), None);
    let (id, _dir, workspace) = open_workspace(cx, &h, &store, json!({}));
    assert!(!workspace.read_with(cx, |w, _| w.is_favourite()));

    workspace.update(cx, |w, cx| w.toggle_favourite(cx));
    assert!(workspace.read_with(cx, |w, _| w.is_favourite()));
    wait_for_disk(cx, || file_on_disk(&h, &id)["metadata"]["isFavourite"] == "true");

    workspace.update(cx, |w, cx| w.toggle_favourite(cx));
    assert!(!workspace.read_with(cx, |w, _| w.is_favourite()));
    wait_for_disk(cx, || file_on_disk(&h, &id)["metadata"].get("isFavourite").is_none());
}

#[gpui_kit::test]
fn the_comments_tab_lists_comments_with_their_code_and_acts_on_them(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h, Settings::default(), None);
    let comment = |id: &str, line: u32, addressed: bool| {
        json!({
            "id": id, "filePath": "src/lib.rs", "lineNumber": line, "text": format!("fix {id}"),
            "commitHash": "abc", "lineCommitHash": null, "createdAt": 1_700_000_000_000_i64,
            "isOutdated": false, "addressed": addressed, "side": "modified"
        })
    };
    let comments = json!([comment("c1", 5, false), comment("c2", 1, true)]).to_string();
    let (id, dir, workspace) = open_workspace(cx, &h, &store, json!({ "reviewComments": comments }));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let text = (1..=10).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n");
    std::fs::write(dir.join("src/lib.rs"), text).unwrap();

    let tab = workspace.update(cx, |w, cx| w.add_tab("comments", cx)).unwrap();
    let TabContent::Comments(model) = content(cx, &workspace, &tab) else { panic!("not a comments tab") };
    let listed = model.read_with(cx, |m, cx| m.comments(cx)).unwrap();
    assert_eq!(listed.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["c1", "c2"]);
    // The code around each comment is read through the daemon.
    wait_until(cx, &model, |m| m.code_context(&listed[0]).is_some());
    let around = model.read_with(cx, |m, _| m.code_context(&listed[0])).unwrap();
    assert_eq!(around.iter().map(|l| l.number).collect::<Vec<_>>(), vec![2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(around.iter().find(|l| l.target).map(|l| l.text.as_str()), Some("line 5"));
    assert!(model.read_with(cx, |m, cx| m.prompt(cx)).contains("fix c1"));
    assert!(!model.read_with(cx, |m, cx| m.prompt(cx)).contains("fix c2"), "only unprompted comments");

    // Prompting needs an AI harness in the workspace.
    model.update(cx, |m, cx| m.prompt_comment("c1", cx));
    wait_until(cx, &model, |m| m.error().is_some());
    assert_eq!(model.read_with(cx, |m, _| m.error().map(str::to_owned)).as_deref(), Some("No AI harness is running in this workspace"));
    // Pushing needs the session's GitHub client.
    model.update(cx, |m, cx| m.push_to_github(None, cx));
    assert!(matches!(model.read_with(cx, |m, _| m.push_state().clone()), PushState::Done(text) if text.starts_with("Push failed")));

    model.update(cx, |m, cx| m.set_filter(Resolution::Addressed, cx));
    assert_eq!(model.read_with(cx, |m, _| m.filter()), Resolution::Addressed);
    model.update(cx, |m, cx| m.delete("c2", cx));
    wait_for_disk(cx, || {
        let disk = file_on_disk(&h, &id);
        let raw = disk["metadata"]["reviewComments"].as_str().unwrap_or_default().to_owned();
        raw.contains("c1") && !raw.contains("c2")
    });
    assert_eq!(model.read_with(cx, |m, cx| m.comments(cx)).unwrap().len(), 1);

    // "Go to file" opens the file browser on the commented line.
    let first = model.read_with(cx, |m, cx| m.comments(cx)).unwrap().remove(0);
    model.update(cx, |m, cx| m.go_to_file(&first, cx));
    let files_tab = workspace.read_with(cx, |w, _| w.file().active_tab_id.clone()).unwrap();
    let state = workspace.read_with(cx, |w, _| w.file().app_states[&files_tab].clone());
    assert_eq!(state.application_id, "filesystem");
    assert_eq!(state.state["selectedPath"], format!("{}/src/lib.rs", dir.display()));
    assert_eq!(state.state["scrollToLine"], 5);
}

#[gpui_kit::test]
fn the_tty_tab_lists_this_workspaces_ptys_and_opens_and_kills_them(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h, Settings::default(), None);
    let (_id, _dir, workspace) = open_workspace(cx, &h, &store, json!({}));
    let terminal_tab = workspace.update(cx, |w, cx| w.add_tab("terminal", cx)).unwrap();
    wait_until_app(cx, &workspace, |w, _| matches!(w.tab_content(&terminal_tab), Some(TabContent::Terminal(_))));
    let pty = workspace.read_with(cx, |w, _| w.file().app_states[&terminal_tab].state["ptyId"].as_str().unwrap().to_owned());
    // A PTY of this workspace that no tab references.
    let path = workspace.read_with(cx, |w, _| w.workspace().path.clone());
    let create = treeterm_core::pty::CreatePty { cwd: path, ..Default::default() };
    let orphan = futures::executor::block_on(h.conn.create_pty(create)).unwrap().unwrap();

    let tab = workspace.update(cx, |w, cx| w.add_tab("tty-list", cx)).unwrap();
    let TabContent::TtyList(model) = content(cx, &workspace, &tab) else { panic!("not a tty tab") };
    wait_until(cx, &model, |m| matches!(m.list(), ProcessList::Loaded(_)));
    let referenced = store.read_with(cx, referenced_pty_ids);
    assert!(referenced.contains(&pty));
    let rows = model.read_with(cx, |m, _| m.rows(&referenced));
    let flags: Vec<(String, bool)> = rows.iter().map(|r| (r.info.id.clone(), r.orphan)).collect();
    assert!(flags.contains(&(pty.clone(), false)), "{flags:?}");
    assert!(flags.contains(&(orphan.clone(), true)), "{flags:?}");

    // "Open" attaches a new terminal tab to the PTY.
    model.update(cx, |m, cx| m.open(&orphan, cx));
    let opened = workspace.read_with(cx, |w, _| w.file().active_tab_id.clone()).unwrap();
    let state = workspace.read_with(cx, |w, _| w.file().app_states[&opened].clone());
    assert_eq!((state.application_id.as_str(), state.state["ptyId"].as_str()), ("terminal", Some(orphan.as_str())));
    workspace.update(cx, |w, cx| w.remove_tab(&opened, cx)); // kills it

    model.update(cx, |m, cx| m.kill(&pty, cx));
    wait_until(cx, &model, |m| match m.list() {
        ProcessList::Loaded(sessions) => !sessions.iter().any(|s| s.id == pty),
        ProcessList::Loading | ProcessList::Failed(_) => false,
    });
    assert_eq!(model.read_with(cx, |m, _| m.kill_error().map(str::to_owned)), None);
}

#[gpui_kit::test]
fn the_chat_tab_streams_replies_and_saves_the_conversation(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let fake = FakeLlm::default();
    fake.on_stream_chat(|messages, _| {
        let last = messages.last().map(|m| m.content.clone()).unwrap_or_default();
        vec![Ok("Hello".into()), Ok(format!(", you said {last}"))]
    });
    let llm = LlmService::new(Arc::new(fake.clone()), h.rt.clone());
    let store = open_store(cx, &h, llm_settings(), Some(llm));
    let (id, _dir, workspace) = open_workspace(cx, &h, &store, json!({}));
    let tab = workspace.update(cx, |w, cx| w.add_tab("chat", cx)).unwrap();
    let TabContent::Chat(chat) = content(cx, &workspace, &tab) else { panic!("not a chat tab") };

    assert!(!chat.update(cx, |c, cx| c.send("   ", cx)), "blank input is not sent");
    assert!(chat.update(cx, |c, cx| c.send("hi", cx)));
    assert!(chat.read_with(cx, |c, _| c.is_streaming()));
    assert!(!chat.update(cx, |c, cx| c.send("again", cx)), "one reply at a time");
    wait_until(cx, &chat, |c| !c.is_streaming());
    let messages = chat.read_with(cx, |c, _| c.messages().to_vec());
    assert_eq!(messages.iter().map(|m| m.role).collect::<Vec<_>>(), vec![ChatRole::User, ChatRole::Assistant]);
    assert_eq!(messages[1].content, "Hello, you said hi");
    // The request carried the endpoint, model and history (not the empty reply).
    let FakeCallStream { history, model } = last_stream_call(&fake);
    assert_eq!((history, model.as_str()), (vec!["hi".to_owned()], "chat-model"));
    wait_for_disk(cx, || file_on_disk(&h, &id)["appStates"][&tab]["state"]["messages"][1]["content"] == "Hello, you said hi");
    assert_eq!(file_on_disk(&h, &id)["appStates"][&tab]["state"]["messages"][0]["role"], "user");

    // A failing stream shows its error and keeps what arrived.
    fake.on_stream_chat(|_, _| vec![Ok("partial".into()), Err(LlmError::Timeout)]);
    assert!(chat.update(cx, |c, cx| c.send("more", cx)));
    wait_until(cx, &chat, |c| !c.is_streaming());
    assert_eq!(chat.read_with(cx, |c, _| c.error().map(str::to_owned)).as_deref(), Some("Request timed out."));
    assert_eq!(chat.read_with(cx, |c, _| c.messages()[3].content.clone()), "partial");

    // Cancelling stops the reply where it is.
    assert!(chat.update(cx, |c, cx| c.send("stop", cx)));
    chat.update(cx, |c, cx| c.cancel(cx));
    cx.run_until_parked();
    assert!(!chat.read_with(cx, |c, _| c.is_streaming()));
    assert_eq!(chat.read_with(cx, |c, _| c.messages().len()), 6);

    // A chat reopened from the saved state continues the conversation.
    let reopened = workspace.update(cx, |w, cx| w.add_tab_with("chat", json!({ "messages": [
        { "id": "1", "role": "user", "content": "earlier" }
    ]}), cx)).unwrap();
    let TabContent::Chat(again) = content(cx, &workspace, &reopened) else { panic!("not a chat tab") };
    assert_eq!(again.read_with(cx, |c, _| c.messages()[0].content.clone()), "earlier");
}

struct FakeCallStream {
    history: Vec<String>,
    model: String,
}

fn last_stream_call(fake: &FakeLlm) -> FakeCallStream {
    let call = fake
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            treeterm_core::llm::FakeCall::StreamChat { messages, settings } => Some((messages, settings)),
            treeterm_core::llm::FakeCall::CompleteChat { .. } | treeterm_core::llm::FakeCall::Fetch(_) => None,
        })
        .next_back()
        .expect("a streamed chat");
    FakeCallStream { history: call.0.into_iter().map(|m| m.content).collect(), model: call.1.model }
}

#[gpui_kit::test]
fn chat_without_an_api_key_says_where_to_set_it(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let llm = LlmService::new(Arc::new(FakeLlm::default()), h.rt.clone());
    let store = open_store(cx, &h, Settings::default(), Some(llm));
    let (_id, _dir, workspace) = open_workspace(cx, &h, &store, json!({}));
    let tab = workspace.update(cx, |w, cx| w.add_tab("chat", cx)).unwrap();
    let TabContent::Chat(chat) = content(cx, &workspace, &tab) else { panic!("not a chat tab") };
    assert!(!chat.update(cx, |c, cx| c.send("hi", cx)));
    assert_eq!(chat.read_with(cx, |c, _| c.error().map(str::to_owned)).as_deref(), Some(MISSING_API_KEY));
    assert!(chat.read_with(cx, |c, _| c.messages().is_empty()));
}

#[gpui_kit::test]
fn the_prompt_debugger_runs_both_prompts_and_its_history_shows_the_runs(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let fake = FakeLlm::default();
    fake.on_complete_chat(|messages, settings| match settings.model.as_str() {
        "title-model" => Ok(r#"{"title":"T","description":"D","branchName":"b"}"#.to_owned()),
        "failing-model" => Err(LlmError::Timeout),
        _ if messages.iter().any(|m| m.content.contains("Allow?")) => {
            Ok(r#"{"state":"permission_request","reason":"asks"}"#.to_owned())
        }
        _ => Ok(r#"{"state":"idle","reason":"nothing"}"#.to_owned()),
    });
    let llm = LlmService::new(Arc::new(fake.clone()), h.rt.clone());
    let settings = llm_settings();
    let store = open_store(cx, &h, settings.clone(), Some(llm));
    let (_id, _dir, workspace) = open_workspace(cx, &h, &store, json!({}));
    let tab = workspace.update(cx, |w, cx| w.open_prompt_debugger("Allow? (y/n)", cx)).unwrap();
    let TabContent::PromptDebugger(debugger) = content(cx, &workspace, &tab) else { panic!("not a debugger tab") };
    let read = |cx: &mut TestAppContext| debugger.read_with(cx, |d, _| (d.buffer().to_owned(), d.model().to_owned()));
    assert_eq!(read(cx), ("Allow? (y/n)".to_owned(), "classifier-model".to_owned()));
    assert_eq!(debugger.read_with(cx, |d, _| d.prompt(DebugMode::Analyzer).to_owned()), settings.terminal_analyzer.system_prompt);
    let output = |cx: &mut TestAppContext| debugger.read_with(cx, |d, _| d.output().clone());
    let activity = |cx: &mut TestAppContext| workspace.read_with(cx, |w, _| w.activity(&tab));

    debugger.update(cx, |d, cx| d.test(cx));
    assert!(debugger.read_with(cx, |d, _| d.is_running()));
    assert_eq!(activity(cx), ActivityState::Working);
    wait_until(cx, &debugger, |d| !d.is_running());
    let analyzed = output(cx);
    assert!(analyzed.duration().is_some());
    assert_eq!(analyzed.error(), None);
    let result: serde_json::Value = serde_json::from_str(analyzed.result().unwrap()).unwrap();
    assert_eq!((result["state"].as_str(), result["reason"].as_str()), (Some("permission_request"), Some("asks")));
    assert_eq!(activity(cx), ActivityState::PermissionRequest);

    // The title generator, with its own model; switching clears the output.
    debugger.update(cx, |d, cx| d.set_mode(DebugMode::Title, cx));
    assert_eq!(output(cx), DebugOutput::default());
    assert_eq!(debugger.read_with(cx, |d, _| d.model().to_owned()), "title-model");
    debugger.update(cx, |d, cx| d.test(cx));
    wait_until(cx, &debugger, |d| !d.is_running());
    let titled = output(cx);
    assert_eq!(serde_json::from_str::<serde_json::Value>(titled.result().unwrap()).unwrap()["branchName"], "b");
    assert_eq!(activity(cx), ActivityState::Completed);

    // Without a model nothing is sent: the error shows above the last result and its time.
    let calls = fake.calls().len();
    debugger.update(cx, |d, _| d.set_model(DebugMode::Title, ""));
    debugger.update(cx, |d, cx| d.test(cx));
    let refused = output(cx);
    assert_eq!(refused.error(), Some("Model must be configured."));
    assert_eq!((refused.result(), refused.duration()), (titled.result(), titled.duration()));
    assert!(!debugger.read_with(cx, |d, _| d.is_running()));
    assert_eq!(fake.calls().len(), calls);
    assert_eq!(activity(cx), ActivityState::Completed, "a refused test drives no activity");

    // A failed request shows why, with its time; a refusal then replaces only the error.
    debugger.update(cx, |d, _| d.set_model(DebugMode::Title, "failing-model"));
    debugger.update(cx, |d, cx| d.test(cx));
    assert_eq!(output(cx), DebugOutput::default(), "a new test clears the output");
    wait_until(cx, &debugger, |d| !d.is_running());
    let failed = output(cx);
    assert!(failed.error().is_some_and(|error| !error.is_empty()));
    assert_eq!(failed.result(), None);
    assert!(failed.duration().is_some());
    assert_eq!(activity(cx), ActivityState::Error);
    debugger.update(cx, |d, _| d.set_model(DebugMode::Title, ""));
    debugger.update(cx, |d, cx| d.test(cx));
    let refused = output(cx);
    assert_eq!((refused.error(), refused.result()), (Some("Model must be configured."), None));
    assert_eq!(refused.duration(), failed.duration());

    // A mode switch while a test runs clears the output but not the test: Test stays
    // disabled, and the old mode's result and final activity land when it comes back.
    debugger.update(cx, |d, cx| {
        d.set_mode(DebugMode::Analyzer, cx);
        d.test(cx);
        d.set_mode(DebugMode::Title, cx);
        d.test(cx); // disabled: neither sent nor refused
    });
    assert!(debugger.read_with(cx, |d, _| d.is_running()));
    assert_eq!(output(cx), DebugOutput::default());
    assert_eq!(activity(cx), ActivityState::Working);
    wait_until(cx, &debugger, |d| !d.is_running());
    let landed = output(cx);
    assert!(landed.duration().is_some());
    assert_eq!(serde_json::from_str::<serde_json::Value>(landed.result().unwrap()).unwrap()["state"], "permission_request");
    assert_eq!(debugger.read_with(cx, |d, _| d.mode()), DebugMode::Title);
    assert_eq!(activity(cx), ActivityState::PermissionRequest);

    // The debugger tab's history shows the transitions its runs drove.
    let history_tab = workspace.update(cx, |w, cx| w.open_activity_history(&tab, cx)).unwrap();
    let TabContent::AnalyzerHistory(history) = content(cx, &workspace, &history_tab) else { panic!("not a history tab") };
    assert_eq!(history.read_with(cx, |h, _| h.source().clone()), HistorySource::Tab(tab.clone()));
    let transitions = history.read_with(cx, |h, cx| h.transitions(cx));
    let targets: Vec<ActivityState> = transitions.iter().map(|t| t.to).collect();
    assert_eq!(
        targets,
        vec![
            ActivityState::PermissionRequest,
            ActivityState::Working,
            ActivityState::Error,
            ActivityState::Working,
            ActivityState::Completed,
            ActivityState::Working,
            ActivityState::PermissionRequest,
            ActivityState::Working
        ],
        "newest first"
    );
    assert!(matches!(&transitions[0].detail, TransitionDetail::Debugger { snapshot } if snapshot == "Allow? (y/n)"));
    assert!(history.read_with(cx, |h, cx| h.calls(cx)).is_none(), "the debugger has no analyzer");

    // "Debug" on an entry opens another debugger on its buffer.
    history.update(cx, |h, cx| h.debug("other buffer", cx));
    let opened = workspace.read_with(cx, |w, _| w.file().active_tab_id.clone()).unwrap();
    assert_eq!(workspace.read_with(cx, |w, _| w.file().app_states[&opened].state["bufferText"].clone()), "other buffer");

    // A history tab without a source says so.
    let broken = workspace.update(cx, |w, cx| w.add_tab("analyzer-history", cx)).unwrap();
    let TabContent::AnalyzerHistory(broken) = content(cx, &workspace, &broken) else { panic!("not a history tab") };
    assert_eq!(broken.read_with(cx, |h, _| h.source().clone()), HistorySource::Missing);
}

#[gpui_kit::test]
fn a_workspace_flashes_on_entering_an_attention_state_unless_its_unread_marker_was_showing(cx: &mut TestAppContext) {
    use treeterm_core::attention_flash::{FLASH_PERIOD, FLASH_REPEATS};
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let fake = FakeLlm::default();
    fake.on_complete_chat(|_, _| Ok(r#"{"state":"permission_request","reason":"asks"}"#.to_owned()));
    let llm = LlmService::new(Arc::new(fake.clone()), h.rt.clone());
    let store = open_store(cx, &h, llm_settings(), Some(llm));
    let (_id, _dir, workspace) = open_workspace(cx, &h, &store, json!({}));
    // The debugger drives its tab's activity: Working while it asks, then the answer.
    let tab = workspace.update(cx, |w, cx| w.open_prompt_debugger("Allow? (y/n)", cx)).unwrap();
    let TabContent::PromptDebugger(debugger) = content(cx, &workspace, &tab) else { panic!("not a debugger tab") };
    let flash = |cx: &mut TestAppContext| {
        let now = cx.executor().now();
        workspace.read_with(cx, |w, _| w.attention_flash(now)).map(|(state, _)| state)
    };
    let run = |cx: &mut TestAppContext| {
        debugger.update(cx, |d, cx| d.test(cx));
        assert_eq!(flash(cx), None, "working does not flash");
        wait_until(cx, &debugger, |d| !d.is_running());
        assert_eq!(workspace.read_with(cx, |w, _| w.activity(&tab)), ActivityState::PermissionRequest);
    };
    assert_eq!(flash(cx), None);

    // Not shown: finishing marks it unread, and its rows flash in the state's colour.
    run(cx);
    assert!(workspace.read_with(cx, |w, _| w.has_unread_attention()));
    assert_eq!(flash(cx), Some(ActivityState::PermissionRequest));
    cx.executor().advance_clock(FLASH_PERIOD * FLASH_REPEATS);
    assert_eq!(flash(cx), None, "three pulses, then over");

    // Again while the marker shows: quiet, like the ding.
    run(cx);
    assert_eq!(flash(cx), None);

    // Shown, it is marked read; the next attention state flashes, though no marker is set.
    store.update(cx, |s, cx| s.set_shown(true, cx));
    wait_until(cx, &workspace, |w| !w.has_unread_attention());
    run(cx);
    assert!(!workspace.read_with(cx, |w, _| w.has_unread_attention()));
    assert_eq!(flash(cx), Some(ActivityState::PermissionRequest));
}

#[gpui_kit::test]
fn the_prompt_debugger_without_an_llm_client_sends_nothing(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h, llm_settings(), None);
    let (_id, _dir, workspace) = open_workspace(cx, &h, &store, json!({}));
    let tab = workspace.update(cx, |w, cx| w.open_prompt_debugger("$ ls", cx)).unwrap();
    let TabContent::PromptDebugger(debugger) = content(cx, &workspace, &tab) else { panic!("not a debugger tab") };
    // Like a missing model: only the error, and no activity.
    debugger.update(cx, |d, cx| d.test(cx));
    let output = debugger.read_with(cx, |d, _| d.output().clone());
    assert_eq!(output, DebugOutput { run: None, refused: Some("No LLM client is available in this session".into()) });
    assert!(!debugger.read_with(cx, |d, _| d.is_running()));
    assert_eq!(workspace.read_with(cx, |w, _| w.activity(&tab)), ActivityState::Idle);
}

#[gpui_kit::test]
fn the_prompt_debugger_buffer_follows_its_tab_state_from_another_window(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h, llm_settings(), None);
    let (id, _dir, workspace) = open_workspace(cx, &h, &store, json!({}));
    let other = open_window(cx, &h, "client-b", llm_settings(), None);
    wait_until(cx, &other, |s| matches!(s.entry(&id), Some(WorkspaceEntry::Loaded(_))));
    let theirs = other.read_with(cx, |s, _| s.workspace_store(&id).unwrap().clone());
    let tab = workspace.update(cx, |w, cx| w.open_prompt_debugger("first", cx)).unwrap();
    let TabContent::PromptDebugger(debugger) = content(cx, &workspace, &tab) else { panic!("not a debugger tab") };
    let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let _events = cx.update(|cx| {
        let events = events.clone();
        cx.subscribe(&debugger, move |_, event: &PromptDebuggerEvent, _| events.borrow_mut().push(event.clone()))
    });
    wait_until(cx, &theirs, |w| w.file().app_states.contains_key(&tab));
    let write = |cx: &mut TestAppContext, key: &str, value: serde_json::Value| {
        theirs.update(cx, |w, cx| w.update_tab_state(&tab, |state| state[key] = value, cx));
    };
    let arrived = |cx: &mut TestAppContext, key: &str, value: serde_json::Value| {
        wait_until(cx, &workspace, |w| w.file().app_states[&tab].state[key] == value);
    };
    let buffer = |cx: &mut TestAppContext| debugger.read_with(cx, |d, _| d.buffer().to_owned());

    // The other window (or Electron) points the tab at another buffer.
    write(cx, "bufferText", json!("second"));
    wait_until(cx, &debugger, |d| d.buffer() == "second");
    assert_eq!(*events.borrow(), vec![PromptDebuggerEvent::BufferReplaced("second".into())]);

    // Edits survive a body whose bufferText did not change, and an empty one.
    debugger.update(cx, |d, _| d.set_buffer("mine"));
    write(cx, "unrelated", json!(1));
    arrived(cx, "unrelated", json!(1));
    assert_eq!(buffer(cx), "mine");
    write(cx, "bufferText", json!(""));
    arrived(cx, "bufferText", json!(""));
    assert_eq!(buffer(cx), "mine");
    assert_eq!(events.borrow().len(), 1);
    // The same text again after a change is new again.
    write(cx, "bufferText", json!("second"));
    wait_until(cx, &debugger, |d| d.buffer() == "second");
    assert_eq!(events.borrow().len(), 2);
}

#[gpui_kit::test]
fn github_and_upload_tabs_read_the_session(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h, Settings::default(), None);
    let (id, _dir, workspace) = open_workspace(cx, &h, &store, json!({}));

    let github = workspace.update(cx, |w, cx| w.add_tab("github", cx)).unwrap();
    assert!(matches!(content(cx, &workspace, &github), TabContent::GitHub));
    // The tab shows the PR the session's git controller loads for this workspace.
    assert!(store.read_with(cx, |s, _| s.git_controller(&id).is_some()));

    // Upload is for ssh sessions; on this machine it says so and never uploads.
    assert!(!workspace.read_with(cx, |w, _| w.is_remote()));
    let upload = workspace.update(cx, |w, cx| w.add_tab("ssh-upload", cx)).unwrap();
    let TabContent::SshUpload(model) = content(cx, &workspace, &upload) else { panic!("not an upload tab") };
    assert_eq!(model.read_with(cx, |m, cx| m.target(cx)), UploadTarget::Local);
    model.update(cx, |m, cx| m.set_local_path("/tmp/file".into(), cx));
    assert!(!model.read_with(cx, |m, cx| m.can_upload(cx)));
}

/// The Run menu's model for `workspace`, once detection has finished; and what it found.
fn detected_run_actions(
    cx: &mut TestAppContext,
    h: &Harness,
    workspace: &Entity<WorkspaceStore>,
) -> (Entity<RunActions>, Vec<treeterm_core::run_actions::RunAction>) {
    let conn = h.conn.clone();
    let model = cx.new(|cx| RunActions::new(&conn, workspace, cx));
    wait_until(cx, &model, |m| matches!(m.detection(), Detection::Done(_)));
    let Detection::Done(actions) = model.read_with(cx, |m, _| m.detection().clone()) else { unreachable!() };
    (model, actions)
}

/// The workspace's terminal tabs: (title, ptyId, keepOnExit), by title.
fn terminal_tabs(cx: &mut TestAppContext, workspace: &Entity<WorkspaceStore>) -> Vec<(String, String, serde_json::Value)> {
    let mut tabs: Vec<_> = workspace.read_with(cx, |w, _| {
        w.file()
            .app_states
            .values()
            .filter(|tab| tab.application_id == "terminal")
            .map(|tab| (tab.title.clone(), tab.state["ptyId"].as_str().unwrap().to_owned(), tab.state["keepOnExit"].clone()))
            .collect()
    });
    tabs.sort_by(|a, b| a.0.cmp(&b.0));
    tabs
}

#[gpui_kit::test]
fn two_runs_started_back_to_back_both_open_tabs(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h, Settings::default(), None);
    let (_id, dir, workspace) = open_workspace(cx, &h, &store, json!({}));
    std::fs::write(dir.join("package.json"), r#"{"scripts": {"a": "echo a", "b": "echo b"}}"#).unwrap();
    let (model, actions) = detected_run_actions(cx, &h, &workspace);
    let ids: Vec<String> = actions.iter().map(|a| a.id()).collect();
    assert_eq!(ids, ["npm:a", "npm:b"]);

    // The second starts before the first has its PTY (`handleRun` is a fresh call per click).
    model.update(cx, |m, cx| {
        m.run(&actions[0], cx);
        m.run(&actions[1], cx);
    });
    assert_eq!(model.read_with(cx, |m, _| m.running()), 2);
    wait_until(cx, &model, |m| m.running() == 0);

    let tabs = terminal_tabs(cx, &workspace);
    let titles: Vec<&str> = tabs.iter().map(|(title, _, _)| title.as_str()).collect();
    assert_eq!(titles, ["npm:a", "npm:b"]);
    assert_ne!(tabs[0].1, tabs[1].1, "each run has its own PTY");
    assert!(tabs.iter().all(|(_, _, keep)| *keep == json!(true)));
    assert_eq!(model.read_with(cx, |m, _| m.error().map(str::to_owned)), None);
}

#[gpui_kit::test]
fn a_run_that_cannot_start_says_why_until_a_later_run_starts(cx: &mut TestAppContext) {
    let _serial = serial();
    cx.executor().allow_parking();
    let h = harness();
    let store = open_store(cx, &h, Settings::default(), None);
    let (_id, dir, workspace) = open_workspace(cx, &h, &store, json!({}));
    std::fs::write(dir.join("package.json"), r#"{"scripts": {"a": "echo a"}}"#).unwrap();
    std::fs::create_dir_all(dir.join(".vscode")).unwrap();
    std::fs::write(dir.join(".vscode/tasks.json"), r#"{"tasks": [{"label": "build", "command": "echo build"}]}"#).unwrap();
    let (model, actions) = detected_run_actions(cx, &h, &workspace);
    let ids: Vec<String> = actions.iter().map(|a| a.id()).collect();
    assert_eq!(ids, ["npm:a", "vscode-task:build"]);

    // The task's command is read when it runs; the file has gone since detection.
    std::fs::remove_file(dir.join(".vscode/tasks.json")).unwrap();
    model.update(cx, |m, cx| m.run(&actions[1], cx));
    wait_until(cx, &model, |m| m.running() == 0);
    assert_eq!(
        model.read_with(cx, |m, _| m.error().map(str::to_owned)).as_deref(),
        Some("Failed to run vscode-task:build: tasks.json not found")
    );
    assert!(terminal_tabs(cx, &workspace).is_empty());

    model.update(cx, |m, cx| m.run(&actions[0], cx));
    wait_until(cx, &model, |m| m.running() == 0);
    assert_eq!(model.read_with(cx, |m, _| m.error().map(str::to_owned)), None);
    let titles: Vec<String> = terminal_tabs(cx, &workspace).into_iter().map(|(title, _, _)| title).collect();
    assert_eq!(titles, ["npm:a"]);
}
