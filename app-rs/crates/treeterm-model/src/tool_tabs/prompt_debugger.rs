//! The System Prompt Debugger tab (`SystemPromptDebugger.tsx`): run the terminal analyzer
//! or the title generator on a buffer with an edited prompt and model, see the raw result
//! and its timing, and save the prompt to the settings. Tab state `{ bufferText? }` seeds
//! the buffer. A test drives the tab's own activity state (`Debugger` transitions).

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

/// The last test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DebugRun {
    Idle,
    Running,
    /// The result as JSON, or why it failed; `duration` once a request was made.
    Finished { duration: Option<Duration>, outcome: Result<String, String> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum PromptDebuggerEvent {
    /// Drive this tab's activity state.
    SetActivity { state: ActivityState, detail: TransitionDetail },
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
    classifier_model: String,
    title_model: String,
    reasoning: ReasoningEffort,
    run: DebugRun,
    _run: Option<Task<()>>,
}

impl PromptDebugger {
    /// The prompts and models start from `settings`; the buffer from the tab state.
    pub fn new(llm: Option<LlmService>, settings: &Settings, state: &Value, workspace: WeakEntity<WorkspaceStore>) -> Self {
        let analyzer = &settings.terminal_analyzer;
        Self {
            llm,
            workspace,
            mode: DebugMode::Analyzer,
            analyzer_prompt: analyzer.system_prompt.clone(),
            title_prompt: analyzer.title_system_prompt.clone(),
            buffer: state.get("bufferText").and_then(Value::as_str).unwrap_or_default().to_owned(),
            classifier_model: analyzer.model.clone(),
            title_model: analyzer.title_model.clone(),
            reasoning: analyzer.reasoning_effort,
            run: DebugRun::Idle,
            _run: None,
        }
    }

    pub fn mode(&self) -> DebugMode {
        self.mode
    }

    /// Switching clears the last result.
    pub fn set_mode(&mut self, mode: DebugMode, cx: &mut Context<Self>) {
        self.mode = mode;
        self.run = DebugRun::Idle;
        self._run = None;
        cx.notify();
    }

    pub fn run(&self) -> &DebugRun {
        &self.run
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
                |s| match mode {
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

    fn finish(&mut self, duration: Option<Duration>, outcome: Result<String, String>, cx: &mut Context<Self>) {
        self.run = DebugRun::Finished { duration, outcome };
        cx.notify();
    }

    /// "Test": run the current mode's request on the buffer.
    pub fn test(&mut self, cx: &mut Context<Self>) {
        if self.run == DebugRun::Running {
            return;
        }
        if self.model().is_empty() {
            return self.finish(None, Err("Model must be configured.".to_owned()), cx);
        }
        let (Some(settings), Some(llm)) = (self.settings(cx), self.llm.clone()) else {
            return self.finish(None, Err("No LLM client is available in this session".to_owned()), cx);
        };
        self.run = DebugRun::Running;
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
                        let duration = Some(finished(cx));
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
                        let duration = Some(finished(cx));
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

/// The reasoning choices, in order.
pub use super::chat::REASONING_CHOICES;
