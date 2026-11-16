//! One AI-harness tab's analyzer: drives `treeterm_core::analyzer::Analyzer` (the sans-IO
//! port of `createAnalyzerStore.ts`) from the tab's terminal, runs the LLM calls it asks
//! for, and reports workspace changes as events for the workspace store to apply.
//!
//! The analyzer reads the tab's own terminal (always alive in the workspace store), so
//! unlike Electron it needs no second, headless attachment. Its auto-approve is written by
//! the workspace store through a background writer ([`crate::pty_writer`]), which the
//! daemon never gates on focus, as Electron's analyzer writes through its own.
//!
//! Auto-approve starts off with every analyzer and is never saved, as in Electron.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use futures::channel::oneshot;
use gpui_kit::{App, Context, Entity, EventEmitter, Subscription, Task, WeakEntity};
use treeterm_core::activity::{ActivityState, TransitionDetail};
use treeterm_core::analyzer::{Analyzer, Effect, Now, Refresh, RequestId, WorkspaceFacts};
use treeterm_core::git::GitApi;
use treeterm_core::llm::{AnalyzerCache, HttpLlm, Llm, LlmApi, LlmError, RetryPolicy};
use treeterm_core::rt::Rt;
use treeterm_core::settings::Settings;

use crate::terminal::{TerminalEvent, TerminalModel};
use crate::workspace_store::WorkspaceStore;

/// The LLM client and the runtime its requests run on, shared by every analyzer.
#[derive(Clone)]
pub struct LlmService {
    llm: Llm,
    rt: Rt,
}

impl LlmService {
    pub fn new(api: Arc<dyn LlmApi>, rt: Rt) -> Self {
        Self {
            llm: Llm::new(api, AnalyzerCache::default()),
            rt,
        }
    }

    /// The real HTTP client (OpenAI-compatible endpoints from the settings).
    pub fn http(rt: Rt) -> Result<Self, LlmError> {
        let api = {
            let _entered = rt.enter();
            HttpLlm::new(RetryPolicy::default())?
        };
        Ok(Self::new(Arc::new(api), rt))
    }

    pub fn llm(&self) -> &Llm {
        &self.llm
    }

    /// The runtime the requests run on.
    pub fn rt(&self) -> &Rt {
        &self.rt
    }
}

/// Workspace changes the analyzer asks for; the workspace store applies them.
#[derive(Debug, Clone, PartialEq)]
pub enum AnalyzerEvent {
    SetActivity { state: ActivityState, detail: TransitionDetail },
    UpdateMetadata { key: &'static str, value: String },
    /// The branch was renamed (refresh: record the new name).
    BranchRenamed(String),
    /// The program may have changed files: re-read git info.
    RefreshGit,
    /// Write to the tab's PTY (auto-approve's Enter).
    Write(&'static str),
    /// A manual refresh finished: [`AnalyzerModel::is_refreshing`] changed.
    RefreshFinished,
}

impl EventEmitter<AnalyzerEvent> for AnalyzerModel {}

pub struct AnalyzerModel {
    analyzer: Analyzer,
    llm: LlmService,
    git: GitApi,
    /// Where the branch lives (`gitRootPath`, else the workspace path).
    repo_path: String,
    terminal: Entity<TerminalModel>,
    workspace: WeakEntity<WorkspaceStore>,
    tab_id: String,
    /// Manual refreshes under way, and who waits for each outcome.
    refreshes: HashMap<RequestId, Waiter>,
    jobs: HashMap<RequestId, Task<()>>,
    timer: Option<Task<()>>,
    _terminal_events: Subscription,
}

/// One manual refresh's outcome, awaited by whoever started it (`TitleRefreshResult`).
struct Waiter {
    refresh: Refresh,
    outcome: oneshot::Sender<Result<(), String>>,
}

fn now(cx: &App) -> Now {
    Now {
        instant: cx.background_executor().now(),
        wall: SystemTime::now(),
    }
}

impl AnalyzerModel {
    /// Starts analyzing right away (the terminal is attached), with auto-approve off.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        settings: &Settings,
        cwd: String,
        repo_path: String,
        llm: LlmService,
        git: GitApi,
        terminal: Entity<TerminalModel>,
        workspace: WeakEntity<WorkspaceStore>,
        tab_id: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut analyzer = Analyzer::new(cwd, settings);
        analyzer.start();
        let events = cx.subscribe(&terminal, |model, _, event: &TerminalEvent, cx| model.on_terminal(event, cx));
        Self {
            analyzer,
            llm,
            git,
            repo_path,
            terminal,
            workspace,
            tab_id,
            refreshes: HashMap::new(),
            jobs: HashMap::new(),
            timer: None,
            _terminal_events: events,
        }
    }

    pub fn analyzer(&self) -> &Analyzer {
        &self.analyzer
    }

    pub fn tab_id(&self) -> &str {
        &self.tab_id
    }

    /// A manual refresh of this kind is under way.
    pub fn is_refreshing(&self, refresh: Refresh) -> bool {
        self.refreshes.values().any(|waiter| waiter.refresh == refresh)
    }

    /// What the labeller and the System Prompt Debugger read (`getBufferText`): the screen
    /// while the analyzer runs, nothing once it stopped (the PTY exited) or while blank.
    pub fn buffer_text(&self, cx: &App) -> String {
        if !self.analyzer.is_running() {
            return String::new();
        }
        let viewport = self.terminal.read(cx).viewport();
        if viewport.trim().is_empty() { String::new() } else { viewport }
    }

    pub fn set_auto_approve(&mut self, value: bool, cx: &mut Context<Self>) {
        self.analyzer.set_auto_approve(value);
        cx.notify();
    }

    pub fn set_settings(&mut self, settings: &Settings, cx: &mut Context<Self>) {
        let effects = self.analyzer.set_settings(settings);
        self.apply(effects, cx);
    }

    /// Re-label the workspace from what is on screen now: its display name and description,
    /// or its branch. The outcome arrives on the receiver; failures carry a message fit to
    /// show the user. It is dropped unsent if the analyzer stops or goes first.
    pub fn refresh(&mut self, refresh: Refresh, cx: &mut Context<Self>) -> oneshot::Receiver<Result<(), String>> {
        let buffer = self.buffer_text(cx);
        let (id, effects) = match refresh {
            Refresh::TitleAndDescription => self.analyzer.refresh_title_and_description(&buffer, now(cx)),
            Refresh::BranchName => self.analyzer.refresh_branch_name(&buffer, now(cx)),
        };
        let (outcome, receiver) = oneshot::channel();
        // Before the effects: a refresh that cannot start finishes among them.
        self.refreshes.insert(id, Waiter { refresh, outcome });
        self.apply(effects, cx);
        receiver
    }

    /// Stop for good (the tab closed or left the session). Refreshes under way are dropped.
    pub fn stop(&mut self) {
        self.analyzer.stop();
        self.jobs.clear();
        self.refreshes.clear();
        self.timer = None;
    }

    /// What the analyzer needs to know about the workspace. Read only from event handlers
    /// and task completions, never while the workspace store is being updated.
    fn facts(&self, cx: &App) -> (WorkspaceFacts, bool) {
        let Some(workspace) = self.workspace.upgrade() else {
            return (WorkspaceFacts::default(), false);
        };
        let store = workspace.read(cx);
        let file = store.file();
        let meta = |key: &str| file.metadata.get(key).is_some_and(|v| !v.is_empty());
        let facts = WorkspaceFacts {
            has_parent: file.parent_id.as_deref().is_some_and(|p| !p.is_empty()),
            has_display_name: meta("displayName"),
            has_description: meta("description"),
            branch_is_user_defined: file.metadata.get("branchIsUserDefined").map(String::as_str) == Some("true"),
            git_branch: file.git_branch.clone(),
        };
        (facts, store.tab_flag(&self.tab_id, "idleDetectorDisabled"))
    }

    fn on_terminal(&mut self, event: &TerminalEvent, cx: &mut Context<Self>) {
        let effects = match event {
            TerminalEvent::Activity(viewport) => {
                let (_, idle_detector_disabled) = self.facts(cx);
                self.analyzer.on_activity(viewport, idle_detector_disabled)
            }
            TerminalEvent::Idle(edge) => self.analyzer.on_idle(&edge.snapshot, now(cx)),
            TerminalEvent::Input(data) => {
                self.analyzer.on_user_input(data, cx.background_executor().now());
                Vec::new()
            }
            TerminalEvent::Exited(_) => self.analyzer.on_exit(),
            TerminalEvent::CloseRequested { .. } | TerminalEvent::Clipboard(_) | TerminalEvent::Bell => Vec::new(),
        };
        self.apply(effects, cx);
    }

    fn apply(&mut self, effects: Vec<Effect>, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                Effect::Classify(request) => {
                    let id = request.id;
                    let run = self.llm.rt.run(self.llm.llm.analyze_terminal(request.input, request.settings));
                    self.jobs.insert(
                        id,
                        cx.spawn(async move |this, cx| {
                            let outcome = run.await.map_err(|lost| lost.to_string());
                            match this.update(cx, |model, cx| {
                                model.jobs.remove(&id);
                                let viewport = model.terminal.read(cx).viewport();
                                let effects = model.analyzer.on_classified(id, outcome, &viewport, now(cx));
                                model.apply(effects, cx);
                            }) {
                                Ok(()) | Err(_) => {} // Err: tab closed
                            }
                        }),
                    );
                }
                Effect::GenerateTitle(request) => {
                    let id = request.id;
                    let run = self.llm.rt.run(self.llm.llm.generate_title(request.buffer, request.settings));
                    self.jobs.insert(
                        id,
                        cx.spawn(async move |this, cx| {
                            let outcome = run.await.map_err(|lost| lost.to_string());
                            match this.update(cx, |model, cx| {
                                model.jobs.remove(&id);
                                let (facts, _) = model.facts(cx);
                                let effects = model.analyzer.on_title(id, outcome, now(cx), &facts);
                                model.apply(effects, cx);
                            }) {
                                Ok(()) | Err(_) => {}
                            }
                        }),
                    );
                }
                Effect::SetActivity { state, detail } => cx.emit(AnalyzerEvent::SetActivity { state, detail }),
                Effect::Write(text) => cx.emit(AnalyzerEvent::Write(text)),
                Effect::RequestIdleEdge => self.terminal.update(cx, |t, cx| t.request_idle_edge(cx)),
                Effect::UpdateMetadata { key, value, reason } => {
                    tracing::debug!(tab = %self.tab_id, key, reason, "analyzer updates metadata");
                    cx.emit(AnalyzerEvent::UpdateMetadata { key, value });
                }
                Effect::RenameBranch { id, from, to } => {
                    let (git, repo) = (self.git.clone(), self.repo_path.clone());
                    self.jobs.insert(
                        id,
                        cx.spawn(async move |this, cx| {
                            let result = git.rename_branch(&repo, &from, &to).await.map_err(|e| e.to_string());
                            match this.update(cx, |model, cx| {
                                model.jobs.remove(&id);
                                if result.is_ok() {
                                    cx.emit(AnalyzerEvent::BranchRenamed(to));
                                }
                                let effects = model.analyzer.on_branch_renamed(id, result);
                                model.apply(effects, cx);
                            }) {
                                Ok(()) | Err(_) => {}
                            }
                        }),
                    );
                }
                Effect::RefreshGit => cx.emit(AnalyzerEvent::RefreshGit),
                Effect::RefreshFinished { id, refresh, result } => {
                    if let Some(Waiter { outcome, .. }) = self.refreshes.remove(&id)
                        && let Err(unread) = outcome.send(result)
                    {
                        tracing::warn!(tab = %self.tab_id, ?refresh, ?unread, "nobody awaited the refresh's outcome");
                    }
                    cx.emit(AnalyzerEvent::RefreshFinished);
                }
            }
        }
        self.schedule_timer(cx);
        cx.notify();
    }

    /// The analyzer's only timer: the delayed title capture after the first Enter.
    fn schedule_timer(&mut self, cx: &mut Context<Self>) {
        let Some(deadline) = self.analyzer.deadline() else {
            self.timer = None;
            return;
        };
        let executor = cx.background_executor().clone();
        let wait = deadline.saturating_duration_since(executor.now());
        self.timer = Some(cx.spawn(async move |this, cx| {
            executor.timer(wait).await;
            match this.update(cx, |model, cx| {
                model.timer = None;
                let viewport = model.terminal.read(cx).viewport();
                let (facts, _) = model.facts(cx);
                let effects = model.analyzer.on_timer(now(cx), &viewport, &facts);
                model.apply(effects, cx);
            }) {
                Ok(()) | Err(_) => {}
            }
        }));
    }
}
