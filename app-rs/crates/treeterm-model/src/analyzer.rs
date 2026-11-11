//! One AI-harness tab's analyzer: drives `treeterm_core::analyzer::Analyzer` (the sans-IO
//! port of `createAnalyzerStore.ts`) from the tab's terminal, runs the LLM calls it asks
//! for, and reports workspace changes as events for the workspace store to apply.
//!
//! The analyzer reads the tab's own terminal (always alive in the workspace store), so
//! unlike Electron it needs no second, headless attachment.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

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
    last_refresh: Option<(Refresh, Result<(), String>)>,
    jobs: HashMap<RequestId, Task<()>>,
    timer: Option<Task<()>>,
    _terminal_events: Subscription,
}

fn now(cx: &App) -> Now {
    Now {
        instant: cx.background_executor().now(),
        wall: SystemTime::now(),
    }
}

impl AnalyzerModel {
    /// Starts analyzing right away (the terminal is attached).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        settings: &Settings,
        cwd: String,
        repo_path: String,
        auto_approve: bool,
        llm: LlmService,
        git: GitApi,
        terminal: Entity<TerminalModel>,
        workspace: WeakEntity<WorkspaceStore>,
        tab_id: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut analyzer = Analyzer::new(cwd, settings);
        analyzer.set_auto_approve(auto_approve);
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
            last_refresh: None,
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

    /// The last manual refresh's outcome, for the status bar.
    pub fn last_refresh(&self) -> Option<&(Refresh, Result<(), String>)> {
        self.last_refresh.as_ref()
    }

    pub fn set_auto_approve(&mut self, value: bool, cx: &mut Context<Self>) {
        self.analyzer.set_auto_approve(value);
        cx.notify();
    }

    pub fn set_settings(&mut self, settings: &Settings, cx: &mut Context<Self>) {
        let effects = self.analyzer.set_settings(settings);
        self.apply(effects, cx);
    }

    /// Re-label the workspace from what is on screen now (display name and description).
    pub fn refresh_title_and_description(&mut self, cx: &mut Context<Self>) {
        let viewport = self.terminal.read(cx).viewport();
        let effects = self.analyzer.refresh_title_and_description(&viewport, now(cx));
        self.apply(effects, cx);
    }

    /// Rename the branch from what is on screen now.
    pub fn refresh_branch_name(&mut self, cx: &mut Context<Self>) {
        let viewport = self.terminal.read(cx).viewport();
        let effects = self.analyzer.refresh_branch_name(&viewport, now(cx));
        self.apply(effects, cx);
    }

    /// Stop for good (the tab closed or left the session).
    pub fn stop(&mut self) {
        self.analyzer.stop();
        self.jobs.clear();
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
                Effect::Write(text) => self.terminal.read(cx).send_unfocused(text.as_bytes()),
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
                Effect::RefreshFinished { refresh, result } => self.last_refresh = Some((refresh, result)),
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
