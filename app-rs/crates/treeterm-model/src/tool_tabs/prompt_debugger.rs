//! The System Prompt Debugger tab (`SystemPromptDebugger.tsx`): run the terminal analyzer
//! or the title generator on a buffer with an edited prompt and model, see the raw result
//! and its timing, and save the prompt to the settings. Tab state `{ bufferText? }` seeds
//! the buffer, and a later non-empty `bufferText` replaces it. A test drives the tab's own
//! activity state (`Debugger` transitions).

use std::time::Duration;

use gpui_kit::{App, Context, Entity, EventEmitter, Task, WeakEntity};
use serde_json::Value;
use treeterm_core::activity::{ActivityState, TransitionDetail};
use treeterm_core::classification::{ClassificationInput, ClassifierSettings, Provider};
use treeterm_core::llm::TitleSettings;
use treeterm_core::settings::{ClassifierProvider, ReasoningEffort, Settings};

use crate::analyzer::LlmService;
use crate::settings_store::SettingsStore;
use crate::workspace_store::WorkspaceStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugMode {
    /// "Terminal Analyzer".
    Analyzer,
    /// "Title Generator".
    Title,
}

impl DebugMode {
    pub fn label(self) -> &'static str {
        match self {
            DebugMode::Analyzer => "Terminal Analyzer",
            DebugMode::Title => "Title Generator",
        }
    }
}

/// A request that came back: how long it took, and its result as JSON or why it failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugRun {
    pub duration: Duration,
    pub outcome: Result<String, String>,
}

/// What the pane shows under Test (Electron's `result`, `error` and `duration`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DebugOutput {
    /// The last request that came back since the output was cleared.
    pub run: Option<DebugRun>,
    /// A test refused before sending ("Model must be configured."): it takes the error's
    /// place and leaves the last run's result and duration on screen.
    pub refused: Option<String>,
}

impl DebugOutput {
    /// The red error line.
    pub fn error(&self) -> Option<&str> {
        match (&self.refused, &self.run) {
            (Some(refused), _) => Some(refused),
            (None, Some(DebugRun { outcome: Err(error), .. })) => Some(error),
            (None, Some(DebugRun { outcome: Ok(_), .. }) | None) => None,
        }
    }

    /// The green result block.
    pub fn result(&self) -> Option<&str> {
        match &self.run {
            Some(DebugRun { outcome: Ok(result), .. }) => Some(result),
            Some(DebugRun { outcome: Err(_), .. }) | None => None,
        }
    }

    /// The `{n}ms` next to Test.
    pub fn duration(&self) -> Option<Duration> {
        self.run.as_ref().map(|run| run.duration)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PromptDebuggerEvent {
    /// Drive this tab's activity state.
    SetActivity { state: ActivityState, detail: TransitionDetail },
    /// The tab state's `bufferText` replaced the buffer.
    BufferReplaced(String),
}

impl EventEmitter<PromptDebuggerEvent> for PromptDebugger {}

pub struct PromptDebugger {
    llm: Option<LlmService>,
    /// Where the endpoint and classifier settings are read from (they change live).
    workspace: WeakEntity<WorkspaceStore>,
    mode: DebugMode,
    analyzer_prompt: String,
    title_prompt: String,
    buffer: String,
    /// The tab state's `bufferText` as last seen: only a change replaces the buffer.
    seeded: String,
    classifier_model: String,
    title_model: String,
    reasoning: ReasoningEffort,
    /// A request is out (Test is disabled); it lands even after a mode switch.
    running: bool,
    output: DebugOutput,
    _run: Option<Task<()>>,
}

impl PromptDebugger {
    /// The prompts and models start from `settings`; the buffer from the tab state.
    pub fn new(llm: Option<LlmService>, settings: &Settings, state: &Value, workspace: WeakEntity<WorkspaceStore>) -> Self {
        let analyzer = &settings.terminal_analyzer;
        let seeded = buffer_text(state).to_owned();
        Self {
            llm,
            workspace,
            mode: DebugMode::Analyzer,
            analyzer_prompt: analyzer.system_prompt.clone(),
            title_prompt: analyzer.title_system_prompt.clone(),
            buffer: seeded.clone(),
            seeded,
            classifier_model: analyzer.model.clone(),
            title_model: analyzer.title_model.clone(),
            reasoning: analyzer.reasoning_effort,
            running: false,
            output: DebugOutput::default(),
            _run: None,
        }
    }

    /// The tab's state changed (another window, Electron): a new, non-empty `bufferText`
    /// replaces the buffer, edits included; the same one again leaves the edits alone.
    pub fn follow_state(&mut self, state: &Value, cx: &mut Context<Self>) {
        let text = buffer_text(state);
        if text == self.seeded {
            return;
        }
        self.seeded = text.to_owned();
        if text.is_empty() {
            return;
        }
        self.buffer = text.to_owned();
        cx.emit(PromptDebuggerEvent::BufferReplaced(self.buffer.clone()));
        cx.notify();
    }

    pub fn mode(&self) -> DebugMode {
        self.mode
    }

    /// Switching clears the output; a running test keeps running and shows its result
    /// when it comes back.
    pub fn set_mode(&mut self, mode: DebugMode, cx: &mut Context<Self>) {
        self.mode = mode;
        self.output = DebugOutput::default();
        cx.notify();
    }

    /// A test is out: Test reads "Testing..." and is disabled.
    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn output(&self) -> &DebugOutput {
        &self.output
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    pub fn set_buffer(&mut self, text: &str) {
        self.buffer = text.to_owned();
    }

    pub fn reasoning(&self) -> ReasoningEffort {
        self.reasoning
    }

    pub fn set_reasoning(&mut self, reasoning: ReasoningEffort, cx: &mut Context<Self>) {
        self.reasoning = reasoning;
        cx.notify();
    }

    /// The model of the current mode.
    pub fn model(&self) -> &str {
        self.model_for(self.mode)
    }

    /// The model `mode` runs with (the classifier's, or the title model).
    pub fn model_for(&self, mode: DebugMode) -> &str {
        match mode {
            DebugMode::Analyzer => &self.classifier_model,
            DebugMode::Title => &self.title_model,
        }
    }

    pub fn set_model(&mut self, mode: DebugMode, model: &str) {
        match mode {
            DebugMode::Analyzer => self.classifier_model = model.to_owned(),
            DebugMode::Title => self.title_model = model.to_owned(),
        }
    }

    /// The prompt of `mode`.
    pub fn prompt(&self, mode: DebugMode) -> &str {
        match mode {
            DebugMode::Analyzer => &self.analyzer_prompt,
            DebugMode::Title => &self.title_prompt,
        }
    }

    pub fn set_prompt(&mut self, mode: DebugMode, prompt: &str) {
        match mode {
            DebugMode::Analyzer => self.analyzer_prompt = prompt.to_owned(),
            DebugMode::Title => self.title_prompt = prompt.to_owned(),
        }
    }

    fn settings(&self, cx: &App) -> Option<std::rc::Rc<Settings>> {
        Some(self.workspace.upgrade()?.read(cx).settings().clone())
    }

    /// Who answers: titles always come from a chat model; the analyzer uses the configured
    /// provider (reasoning is not sent to the classifier).
    pub fn provider(&self, cx: &App) -> ClassifierProvider {
        match (self.mode, self.settings(cx)) {
            (DebugMode::Title, _) | (DebugMode::Analyzer, None) => ClassifierProvider::ChatCompletions,
            (DebugMode::Analyzer, Some(settings)) => settings.terminal_analyzer.provider,
        }
    }

    /// "Update": save the current mode's prompt to the settings.
    pub fn save_prompt(&self, settings: &Entity<SettingsStore>, cx: &mut Context<Self>) {
        let (mode, prompt) = (self.mode, self.prompt(self.mode).to_owned());
        settings.update(cx, |store, cx| {
            store.update(
                move |s| match mode {
                    DebugMode::Analyzer => s.terminal_analyzer.system_prompt = prompt,
                    DebugMode::Title => s.terminal_analyzer.title_system_prompt = prompt,
                },
                cx,
            );
        });
    }

    fn set_activity(&self, state: ActivityState, cx: &mut Context<Self>) {
        let detail = TransitionDetail::Debugger { snapshot: self.buffer.clone() };
        cx.emit(PromptDebuggerEvent::SetActivity { state, detail });
    }

    /// The request came back after `duration`.
    fn finish(&mut self, duration: Duration, outcome: Result<String, String>, cx: &mut Context<Self>) {
        self.running = false;
        self.output = DebugOutput { run: Some(DebugRun { duration, outcome }), refused: None };
        cx.notify();
    }

    /// Nothing is sent; the last run stays on screen under `message`.
    fn refuse(&mut self, message: &str, cx: &mut Context<Self>) {
        self.output.refused = Some(message.to_owned());
        cx.notify();
    }

    /// "Test": run the current mode's request on the buffer.
    pub fn test(&mut self, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        if self.model().is_empty() {
            return self.refuse("Model must be configured.", cx);
        }
        let (Some(settings), Some(llm)) = (self.settings(cx), self.llm.clone()) else {
            return self.refuse("No LLM client is available in this session", cx);
        };
        self.running = true;
        self.output = DebugOutput::default();
        self.set_activity(ActivityState::Working, cx);
        let started = cx.background_executor().now();
        let finished = move |cx: &App| cx.background_executor().now().saturating_duration_since(started);
        let analyzer = &settings.terminal_analyzer;
        self._run = Some(match self.mode {
            DebugMode::Analyzer => {
                llm.llm().clear_analyzer_cache();
                let input = ClassificationInput {
                    buffer: self.buffer.clone(),
                    cwd: String::new(),
                    safe_paths: analyzer.safe_paths.clone(),
                    system_prompt: self.analyzer_prompt.clone(),
                    criteria: analyzer.criteria.clone(),
                };
                let classifier = ClassifierSettings {
                    base_url: settings.llm.base_url.clone(),
                    api_key: settings.llm.api_key.clone(),
                    model: self.classifier_model.clone(),
                    provider: match self.provider(cx) {
                        ClassifierProvider::Classifier => Provider::Classifier,
                        ClassifierProvider::ChatCompletions => Provider::ChatCompletions { reasoning_effort: self.reasoning },
                    },
                };
                let request = llm.rt().run(llm.llm().analyze_terminal(input, classifier));
                cx.spawn(async move |this, cx| {
                    let outcome = request.await;
                    match this.update(cx, |model, cx| {
                        let duration = finished(cx);
                        match outcome {
                            Ok(outcome) => match &outcome.result {
                                Ok(analyzed) => {
                                    model.set_activity(analyzed.classification.state, cx);
                                    model.finish(duration, Ok(outcome.to_json()), cx);
                                }
                                Err(error) => {
                                    model.set_activity(ActivityState::Error, cx);
                                    model.finish(duration, Err(error.clone()), cx);
                                }
                            },
                            Err(lost) => {
                                model.set_activity(ActivityState::Error, cx);
                                model.finish(duration, Err(lost.to_string()), cx);
                            }
                        }
                    }) {
                        Ok(()) | Err(_) => {} // Err: tab closed
                    }
                })
            }
            DebugMode::Title => {
                let title = TitleSettings {
                    base_url: settings.llm.base_url.clone(),
                    api_key: settings.llm.api_key.clone(),
                    model: self.title_model.clone(),
                    title_system_prompt: self.title_prompt.clone(),
                    reasoning_effort: self.reasoning,
                };
                let request = llm.rt().run(llm.llm().generate_title(self.buffer.clone(), title));
                cx.spawn(async move |this, cx| {
                    let outcome = request.await;
                    match this.update(cx, |model, cx| {
                        let duration = finished(cx);
                        match outcome {
                            Ok(outcome) => match &outcome.result {
                                Ok(_) => {
                                    model.set_activity(ActivityState::Completed, cx);
                                    model.finish(duration, Ok(outcome.to_json()), cx);
                                }
                                Err(error) => {
                                    model.set_activity(ActivityState::Error, cx);
                                    model.finish(duration, Err(error.clone()), cx);
                                }
                            },
                            Err(lost) => {
                                model.set_activity(ActivityState::Error, cx);
                                model.finish(duration, Err(lost.to_string()), cx);
                            }
                        }
                    }) {
                        Ok(()) | Err(_) => {}
                    }
                })
            }
        });
        cx.notify();
    }
}

/// The tab state's `bufferText` (empty when unset).
fn buffer_text(state: &Value) -> &str {
    state.get("bufferText").and_then(Value::as_str).unwrap_or_default()
}

/// The reasoning choices, in order.
pub use super::chat::REASONING_CHOICES;
