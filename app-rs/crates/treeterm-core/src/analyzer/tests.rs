//! Port of `createAnalyzerStore.test.ts`.
//!
//! [`Harness`] is a driver like the GPUI model's: the real viewport idle detector, a fake
//! clock (the TS fake timers) and scripted LLM answers. Screens are set directly instead of
//! parsed from escape sequences (terminal emulation is `treeterm-term`'s). Auto answers are
//! delivered when the harness flushes, like a resolved promise on the next microtask:
//! [`Harness::advance`] mirrors `vi.advanceTimersByTime`, [`Harness::advance_async`]
//! `vi.advanceTimersByTimeAsync`.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use treeterm_term::idle::{IdleDetector, IdleOutput};

use super::*;
use crate::llm::{Analyzed, AnalyzerCache, FakeLlm, HttpResponse, Llm};
use crate::settings::{ClassifierProvider, ReasoningEffort};

const MS: Duration = Duration::from_millis(1);

fn settings() -> Settings {
    let mut settings = Settings::default();
    settings.llm.api_key = "test-key".to_owned();
    settings.llm.base_url = "http://localhost".to_owned();
    let analyzer = &mut settings.terminal_analyzer;
    analyzer.provider = ClassifierProvider::ChatCompletions;
    analyzer.title_model = "test-model".to_owned();
    analyzer.model = "test-model".to_owned();
    analyzer.system_prompt = "test prompt".to_owned();
    analyzer.title_system_prompt = "title prompt".to_owned();
    analyzer.reasoning_effort = ReasoningEffort::Low;
    analyzer.safe_paths = vec!["/tmp".to_owned()];
    analyzer.buffer_lines = 10;
    analyzer.idle_debounce = 500 * MS;
    analyzer.idle_debounce_unread = 15000 * MS;
    analyzer.criteria = ClassifierCriteria::default();
    settings
}

fn workspace() -> WorkspaceFacts {
    WorkspaceFacts {
        has_parent: true,
        has_display_name: false,
        has_description: false,
        branch_is_user_defined: false,
        git_branch: Some("old-branch".to_owned()),
    }
}

fn classified(state: ActivityState, reason: &str) -> Result<AnalyzeOutcome, String> {
    Ok(AnalyzeOutcome {
        result: Ok(Analyzed {
            classification: Classification {
                state,
                reason: reason.to_owned(),
            },
            cached: false,
        }),
        system_prompt: "system prompt".to_owned(),
    })
}

fn provider_error(error: &str) -> Result<AnalyzeOutcome, String> {
    Ok(AnalyzeOutcome {
        result: Err(error.to_owned()),
        system_prompt: "system prompt".to_owned(),
    })
}

fn titled(title: &str, description: &str, branch_name: &str) -> Result<TitleOutcome, String> {
    Ok(TitleOutcome {
        result: Ok(TitleSuggestion {
            title: title.to_owned(),
            description: description.to_owned(),
            branch_name: branch_name.to_owned(),
        }),
        system_prompt: "title prompt".to_owned(),
    })
}

type ClassifyResponder = Box<dyn FnMut(&ClassifyRequest) -> Option<Result<AnalyzeOutcome, String>>>;
type TitleResponder = Box<dyn FnMut(&TitleRequest) -> Result<TitleOutcome, String>>;
type RenameResponder = Box<dyn FnMut(&str, &str) -> Result<(), String>>;

enum Ready {
    Classified(RequestId, Result<AnalyzeOutcome, String>),
    Title(RequestId, Result<TitleOutcome, String>),
    Renamed(RequestId, Result<(), String>),
}

struct Harness {
    analyzer: Analyzer,
    detector: IdleDetector,
    start: Instant,
    now: Instant,
    screen: String,
    settings: Settings,
    unread: bool,
    idle_disabled: bool,
    workspace: WorkspaceFacts,
    /// `None`: the request stays pending until [`Self::resolve`].
    classify: ClassifyResponder,
    title: TitleResponder,
    rename: RenameResponder,
    ready: VecDeque<Ready>,
    classify_calls: Vec<ClassifyRequest>,
    title_calls: Vec<TitleRequest>,
    activity: Vec<(ActivityState, TransitionDetail)>,
    writes: Vec<&'static str>,
    metadata: Vec<(&'static str, String, &'static str)>,
    renames: Vec<(String, String)>,
    git_refreshes: usize,
    refreshes: Vec<(Refresh, Result<(), String>)>,
}

impl Harness {
    /// `makeDeps()` + `createAnalyzerStore` + `start()`: an empty screen.
    fn new() -> Self {
        Self::with_settings(settings())
    }

    fn with_settings(settings: Settings) -> Self {
        let start = Instant::now();
        let timeout = idle_timeout(&settings.terminal_analyzer, false);
        let mut analyzer = Analyzer::new("/test", &settings);
        analyzer.start();
        Self {
            analyzer,
            detector: IdleDetector::new(String::new(), timeout, start),
            start,
            now: start,
            screen: String::new(),
            settings,
            unread: false,
            idle_disabled: false,
            workspace: workspace(),
            classify: Box::new(|_| Some(classified(ActivityState::Idle, "prompt visible"))),
            title: Box::new(|_| titled("Test Title", "Test Description", "test-title")),
            rename: Box::new(|_, _| Ok(())),
            ready: VecDeque::new(),
            classify_calls: Vec::new(),
            title_calls: Vec::new(),
            activity: Vec::new(),
            writes: Vec::new(),
            metadata: Vec::new(),
            renames: Vec::new(),
            git_refreshes: 0,
            refreshes: Vec::new(),
        }
    }

    /// Every classification stays pending until resolved by hand.
    fn manual(mut self) -> Self {
        self.classify = Box::new(|_| None);
        self
    }

    /// Only the first classification stays pending; later ones answer `then`.
    fn first_manual(mut self, then: Result<AnalyzeOutcome, String>) -> Self {
        let mut calls = 0;
        self.classify = Box::new(move |_| {
            calls += 1;
            (calls > 1).then(|| then.clone())
        });
        self
    }

    fn answering(mut self, response: Result<AnalyzeOutcome, String>) -> Self {
        self.classify = Box::new(move |_| Some(response.clone()));
        self
    }

    fn now(&self) -> Now {
        Now {
            instant: self.now,
            wall: SystemTime::UNIX_EPOCH + (self.now - self.start),
        }
    }

    /// The viewport changed (`processSnapshot` after a parsed write).
    fn emit(&mut self, screen: &str) {
        screen.clone_into(&mut self.screen);
        let timeout = idle_timeout(&self.settings.terminal_analyzer, self.unread);
        let outputs = self
            .detector
            .process_snapshot(screen.to_owned(), self.now, timeout);
        self.feed(outputs);
    }

    fn feed(&mut self, outputs: Vec<IdleOutput>) {
        for output in outputs {
            let effects = match output {
                IdleOutput::Activity(snapshot) => {
                    self.analyzer.on_activity(&snapshot, self.idle_disabled)
                }
                IdleOutput::Idle(edge) => self.analyzer.on_idle(&edge.snapshot, self.now()),
            };
            self.apply(effects);
        }
    }

    fn apply(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Classify(request) => {
                    if let Some(response) = (self.classify)(&request) {
                        self.ready
                            .push_back(Ready::Classified(request.id, response));
                    }
                    self.classify_calls.push(*request);
                }
                Effect::GenerateTitle(request) => {
                    self.ready
                        .push_back(Ready::Title(request.id, (self.title)(&request)));
                    self.title_calls.push(request);
                }
                Effect::SetActivity { state, detail } => self.activity.push((state, detail)),
                Effect::Write(data) => self.writes.push(data),
                Effect::RequestIdleEdge => {
                    let outputs = self.detector.request_idle_edge();
                    self.feed(outputs);
                }
                Effect::UpdateMetadata { key, value, reason } => {
                    self.metadata.push((key, value, reason))
                }
                Effect::RenameBranch { id, from, to } => {
                    self.ready
                        .push_back(Ready::Renamed(id, (self.rename)(&from, &to)));
                    self.renames.push((from, to));
                }
                Effect::RefreshGit => self.git_refreshes += 1,
                Effect::RefreshFinished { refresh, result } => {
                    self.refreshes.push((refresh, result))
                }
            }
        }
    }

    /// Deliver answered requests (and what they start) until none are left.
    fn flush(&mut self) {
        while let Some(ready) = self.ready.pop_front() {
            let screen = self.screen.clone();
            let effects = match ready {
                Ready::Classified(id, response) => {
                    self.analyzer
                        .on_classified(id, response, &screen, self.now())
                }
                Ready::Title(id, response) => {
                    self.analyzer
                        .on_title(id, response, self.now(), &self.workspace.clone())
                }
                Ready::Renamed(id, result) => self.analyzer.on_branch_renamed(id, result),
            };
            self.apply(effects);
        }
    }

    fn run_timers(&mut self, by: Duration, flush: bool) {
        if flush {
            self.flush();
        }
        let target = self.now + by;
        loop {
            let detector = self.detector.deadline().filter(|at| *at <= target);
            let analyzer = self.analyzer.deadline().filter(|at| *at <= target);
            match (detector, analyzer) {
                (Some(d), a) if a.is_none_or(|a| d <= a) => {
                    self.now = d;
                    let outputs = self.detector.on_timer(d);
                    self.feed(outputs);
                }
                (_, Some(a)) => {
                    self.now = a;
                    let screen = self.screen.clone();
                    let effects =
                        self.analyzer
                            .on_timer(self.now(), &screen, &self.workspace.clone());
                    self.apply(effects);
                }
                (None, None) => break,
                (Some(_), None) => unreachable!("handled by the first arm"),
            }
            if flush {
                self.flush();
            }
        }
        self.now = target;
    }

    fn advance(&mut self, ms: u64) {
        self.run_timers(Duration::from_millis(ms), false);
    }

    fn advance_async(&mut self, ms: u64) {
        self.run_timers(Duration::from_millis(ms), true);
        self.flush();
    }

    /// Answer the `index`th classification request.
    fn resolve(&mut self, index: usize, response: Result<AnalyzeOutcome, String>) {
        let id = self.classify_calls[index].id;
        let screen = self.screen.clone();
        let effects = self
            .analyzer
            .on_classified(id, response, &screen, self.now());
        self.apply(effects);
        self.flush();
    }

    fn user_input(&mut self, data: &str) {
        self.analyzer.on_user_input(data, self.now);
    }

    fn set_settings(&mut self, settings: Settings) {
        let effects = self.analyzer.set_settings(&settings);
        self.settings = settings;
        self.apply(effects);
    }

    fn exit(&mut self) {
        let effects = self.analyzer.on_exit();
        self.apply(effects);
    }

    fn refresh_title_and_description(&mut self) {
        let screen = self.screen.clone();
        let effects = self
            .analyzer
            .refresh_title_and_description(&screen, self.now());
        self.apply(effects);
        self.flush();
    }

    fn refresh_branch_name(&mut self) {
        let screen = self.screen.clone();
        let effects = self.analyzer.refresh_branch_name(&screen, self.now());
        self.apply(effects);
        self.flush();
    }

    /// Runs the silent first burst after start (the scrollback replay) through to its
    /// classification so the detector knows a resting state and later bursts publish Working.
    fn settle(&mut self) {
        self.emit("replay");
        self.advance_async(1000);
        assert!(
            !self
                .activity
                .iter()
                .any(|(state, _)| *state == ActivityState::Working)
        );
        self.activity.clear();
        self.classify_calls.clear();
    }

    fn states(&self) -> Vec<ActivityState> {
        self.activity.iter().map(|(state, _)| *state).collect()
    }

    fn history(&self) -> Vec<HistoryEntry> {
        self.analyzer.history().cloned().collect()
    }

    fn metadata_keys(&self) -> Vec<&'static str> {
        self.metadata.iter().map(|(key, _, _)| *key).collect()
    }

    fn has_metadata(&self, key: &str, value: &str, reason: &str) -> bool {
        self.metadata
            .iter()
            .any(|(k, v, r)| *k == key && v == value && *r == reason)
    }
}

fn classification(snapshot: &str, reason: &str) -> TransitionDetail {
    TransitionDetail::Classification {
        snapshot: snapshot.to_owned(),
        reason: reason.to_owned(),
    }
}

#[test]
fn creates_store_with_initial_state() {
    let analyzer = Analyzer::new("/test", &settings());
    assert_eq!(analyzer.ai_state(), ActivityState::Idle);
    assert!(!analyzer.analyzing());
    assert_eq!(analyzer.reason(), "");
    assert!(!analyzer.auto_approve());
    assert!(!analyzer.is_running());
}

#[test]
fn set_auto_approve_updates_auto_approve_state() {
    let mut analyzer = Analyzer::new("/test", &settings());
    analyzer.set_auto_approve(true);
    assert!(analyzer.auto_approve());
    analyzer.set_auto_approve(false);
    assert!(!analyzer.auto_approve());
}

// `getBufferText returns null when not started`, `stop resets terminal reference`, `reads the
// alternate buffer while a full-screen app holds the screen`, `waits for asynchronous parsing
// before scheduling classification`, `feeds the daemon-replayed scrollback into the headless
// terminal on start` and the TTY attach/dispose cases: the analyzer has no terminal or TTY of
// its own; the owner passes the viewport (`snapshot_viewport`, tested in `treeterm-term`) and
// owns the PTY attachment.

#[test]
fn start_opens_tty_stream_and_starts_polling_when_settings_are_configured() {
    let mut h = Harness::new();
    h.settle();
    h.emit("$ echo hello\nhello\n$ ");
    h.advance(500);
    assert_eq!(h.analyzer.ai_state(), ActivityState::Working);
}

#[test]
fn start_polls_but_analyze_shows_error_once_when_model_is_missing() {
    let mut s = settings();
    s.llm.api_key = String::new();
    s.llm.base_url = String::new();
    s.terminal_analyzer.model = String::new();
    s.terminal_analyzer.system_prompt = String::new();
    s.terminal_analyzer.reasoning_effort = ReasoningEffort::Off;
    s.terminal_analyzer.safe_paths = Vec::new();
    let mut h = Harness::with_settings(s);
    h.emit("$ hello");
    h.advance_async(0);
    // The first burst after start is the replay: no Working, only the idle debounce schedules analyze.
    assert_eq!(h.analyzer.ai_state(), ActivityState::Idle);
    assert!(h.activity.is_empty());
    h.advance_async(500);
    assert_eq!(h.analyzer.ai_state(), ActivityState::Error);
    assert_eq!(
        h.analyzer.reason(),
        "Terminal analyzer model not configured"
    );

    let count = h.activity.len();
    h.emit("$ more data");
    h.advance_async(1000);
    // Only the Working state from the detector.
    assert_eq!(h.activity.len(), count + 1);
    assert!(h.classify_calls.is_empty());
}

#[test]
fn analyzes_with_empty_api_key_valid_for_self_hosted_like_ollama() {
    let mut s = settings();
    s.llm.api_key = String::new();
    s.llm.base_url = "http://localhost:11434/v1".to_owned();
    s.terminal_analyzer.model = "llama3".to_owned();
    let mut h = Harness::with_settings(s);
    h.emit("$ echo hello\nhello\n$ ");
    h.advance(1000);
    h.advance_async(0);
    assert_eq!(h.classify_calls.len(), 1);
    assert_eq!(h.classify_calls[0].settings.api_key, "");
    assert_eq!(h.analyzer.ai_state(), ActivityState::Idle);
}

#[test]
fn analyze_calls_llm_analyze_terminal_and_updates_state() {
    let mut h = Harness::new();
    h.emit("$ echo hello\nhello\n$ ");
    h.advance(500);
    h.advance(500);
    h.advance_async(0);
    assert_eq!(h.classify_calls.len(), 1);
    let request = &h.classify_calls[0];
    assert_eq!(request.input.buffer, "$ echo hello\nhello\n$ ");
    assert_eq!(request.input.cwd, "/test");
    assert_eq!(
        request.settings.provider,
        Provider::ChatCompletions {
            reasoning_effort: ReasoningEffort::Low
        }
    );
    assert_eq!(h.analyzer.ai_state(), ActivityState::Idle);
    assert_eq!(h.analyzer.reason(), "prompt visible");
    assert!(!h.analyzer.analyzing());
    assert_eq!(
        h.activity.last(),
        Some(&(
            ActivityState::Idle,
            classification("$ echo hello\nhello\n$ ", "prompt visible")
        ))
    );
}

#[test]
fn analyze_handles_error_result() {
    let mut h = Harness::new().answering(provider_error("API error"));
    h.emit("$ echo hello\nhello\n$ ");
    h.advance(1000);
    h.advance_async(0);
    assert_eq!(h.analyzer.ai_state(), ActivityState::Error);
    assert_eq!(h.analyzer.reason(), "API error");
    assert!(!h.analyzer.analyzing());
}

#[test]
fn analyze_handles_llm_call_failure() {
    let mut h = Harness::new().answering(Err("Network error".to_owned()));
    h.emit("$ echo hello\nhello\n$ ");
    h.advance(1000);
    h.advance_async(0);
    assert_eq!(h.analyzer.ai_state(), ActivityState::Error);
    assert_eq!(h.analyzer.reason(), "Network error");
}

#[test]
fn applies_a_response_discarded_during_a_blip_once_the_screen_returns_to_what_was_classified() {
    let mut h = Harness::new().manual();
    h.emit("$ done");
    h.advance_async(1000);
    assert_eq!(h.classify_calls.len(), 1);

    // A spinner frame is on screen when the response lands, so it is discarded...
    h.emit("spinner");
    h.advance_async(50);
    h.resolve(0, classified(ActivityState::Idle, "prompt visible"));
    h.advance_async(50);
    assert_eq!(h.history()[0].error(), Some("[discarded]"));

    // ...and the screen goes back to the classified one: a blip, which fires no idle edge.
    h.emit("$ done");
    h.advance_async(1000);
    assert_eq!(h.analyzer.reason(), "prompt visible");
    // Reused from the cache, not asked again.
    assert_eq!(h.classify_calls.len(), 1);
}

#[test]
fn queues_pending_analyze_when_request_is_in_flight_and_drains_after_completion() {
    let mut h = Harness::new().manual();
    h.emit("$ echo hello\nhello\n$ ");
    h.advance(1000);
    assert_eq!(h.classify_calls.len(), 1);

    // The buffer changes while the request is in flight: queued, not sent.
    h.emit("$ npm test\nPASS all tests\n$ ");
    h.advance(1000);
    assert_eq!(h.classify_calls.len(), 1);

    // The first response is stale (the screen changed) and drains the queued work.
    h.resolve(0, classified(ActivityState::Idle, "prompt visible"));
    assert_eq!(h.history()[0].error(), Some("[discarded]"));
    assert_eq!(h.analyzer.ai_state(), ActivityState::Working);
    assert_eq!(h.classify_calls.len(), 2);
    assert_eq!(
        h.classify_calls[1].input.buffer,
        "$ npm test\nPASS all tests\n$ "
    );

    h.resolve(1, classified(ActivityState::Idle, "tests passed"));
    assert_eq!(h.analyzer.reason(), "tests passed");
}

#[test]
fn accepts_an_in_flight_classification_despite_continuous_identical_viewport_repaints() {
    let mut h = Harness::new().manual();
    h.emit("$ echo hello\nhello\n$ ");
    h.advance(1000);
    assert_eq!(h.classify_calls.len(), 1);
    for _ in 0..10 {
        h.emit("$ echo hello\nhello\n$ ");
        h.advance_async(100);
    }
    assert_eq!(h.classify_calls.len(), 1);

    h.resolve(0, classified(ActivityState::Idle, "prompt visible"));
    assert_eq!(h.analyzer.ai_state(), ActivityState::Idle);
    assert_eq!(h.analyzer.reason(), "prompt visible");
    assert_eq!(h.history()[0].error(), None);

    h.emit("$ echo hello\nhello\n$ ");
    h.advance_async(1000);
    assert_eq!(h.analyzer.ai_state(), ActivityState::Idle);
    assert_eq!(h.classify_calls.len(), 1);
}

#[test]
fn ignores_empty_data_after_classification() {
    let mut h = Harness::new();
    h.emit("$ echo hello\nhello\n$ ");
    h.advance(1000);
    h.advance_async(0);
    assert_eq!(h.classify_calls.len(), 1);
    // Empty data does not change the viewport.
    h.emit("$ echo hello\nhello\n$ ");
    h.advance(1000);
    h.advance_async(0);
    assert_eq!(h.classify_calls.len(), 1);
}

#[test]
fn classifies_changed_viewport_text_after_resize_but_ignores_identical_resize() {
    let mut h = Harness::new();
    h.emit("abcdefghij");
    h.advance_async(1000);
    assert_eq!(h.classify_calls.len(), 1);

    // Resized to 5 columns: the viewport text changed.
    let resized = format!("abcde{}", "\n".repeat(23));
    h.emit(&resized);
    h.advance_async(1000);
    assert_eq!(h.classify_calls.len(), 2);
    assert_eq!(h.classify_calls[1].input.buffer, resized);

    h.emit(&resized);
    h.advance_async(1000);
    assert_eq!(h.classify_calls.len(), 2);
    assert_eq!(h.analyzer.ai_state(), ActivityState::Idle);
}

#[test]
fn ignores_a_response_from_before_stop_and_restart() {
    // (`ignores old stream events and parsed-write callbacks after stop and restart`: the
    // stream is the owner's; what reaches the analyzer is the old cycle's answer.)
    let mut h = Harness::new().manual();
    h.emit("old screen");
    h.advance_async(1000);
    assert_eq!(h.classify_calls.len(), 1);
    h.analyzer.stop();
    h.analyzer.start();
    h.activity.clear();
    h.resolve(0, classified(ActivityState::SafePermissionRequested, "old"));
    assert!(h.activity.is_empty());
    assert_eq!(h.analyzer.ai_state(), ActivityState::Idle);
    assert!(h.history().is_empty());
}

#[test]
fn returns_to_idle_when_the_viewport_is_cleared_while_classification_is_pending() {
    let mut h = Harness::new().manual();
    h.emit("working");
    h.advance_async(1000);
    assert!(h.analyzer.analyzing());

    h.emit("");
    h.advance_async(1000);
    assert_eq!(h.analyzer.ai_state(), ActivityState::Idle);
    assert!(!h.analyzer.analyzing());
    assert_eq!(h.classify_calls.len(), 1);

    h.resolve(0, classified(ActivityState::Working, "old output"));
    assert_eq!(h.analyzer.ai_state(), ActivityState::Idle);
    assert_eq!(h.history()[0].error(), Some("[discarded]"));
}

#[test]
fn marks_an_exited_pty_completed_and_ignores_its_pending_classification() {
    let mut h = Harness::new().manual();
    h.emit("working");
    h.advance_async(1000);
    assert!(h.analyzer.analyzing());

    h.exit();
    assert_eq!(h.analyzer.ai_state(), ActivityState::Completed);
    assert!(!h.analyzer.analyzing());
    assert_eq!(
        h.activity.last(),
        Some(&(ActivityState::Completed, classification("", "")))
    );
    h.resolve(0, classified(ActivityState::Working, "old output"));
    h.advance_async(1000);
    assert_eq!(h.analyzer.ai_state(), ActivityState::Completed);
    assert_eq!(h.classify_calls.len(), 1);
}

#[test]
fn does_not_restart_polling_after_an_already_exited_pty_is_replayed_during_attach() {
    let mut h = Harness::new();
    h.exit();
    h.advance_async(1000);
    assert_eq!(h.analyzer.ai_state(), ActivityState::Completed);
    assert!(!h.analyzer.is_running());
    assert_eq!(h.analyzer.deadline(), None);
    assert!(h.classify_calls.is_empty());
}

#[test]
fn dedup_lru_cache_retains_previous_buffers_across_changes() {
    let mut h = Harness::new();
    let mut count = 0;
    h.classify = Box::new(move |_| {
        count += 1;
        Some(classified(ActivityState::Idle, &format!("call-{count}")))
    });
    h.emit("$ echo hello\nhello\n$ ");
    h.advance(1000);
    h.advance_async(0);
    assert_eq!(h.classify_calls.len(), 1);

    h.emit("$ npm test\nPASS\n$ ");
    h.advance(1000);
    h.advance_async(0);
    assert_eq!(h.classify_calls.len(), 2);

    // Back to the first screen: the remembered answer, no third call.
    h.emit("$ echo hello\nhello\n$ ");
    h.advance(1000);
    h.advance_async(0);
    assert_eq!(h.classify_calls.len(), 2);
    assert_eq!(h.analyzer.reason(), "call-1");
}

#[test]
fn forgets_the_least_recently_used_classification_past_ten() {
    let mut h = Harness::new();
    for i in 0..11 {
        h.emit(&format!("screen {i}"));
        h.advance_async(1000);
    }
    assert_eq!(h.classify_calls.len(), 11);
    h.emit("screen 0");
    h.advance_async(1000);
    assert_eq!(h.classify_calls.len(), 12);
    h.emit("screen 5");
    h.advance_async(1000);
    assert_eq!(h.classify_calls.len(), 12);
}

// === onUserInput ===

fn titled_harness() -> Harness {
    let mut h = Harness::new();
    h.emit("$ ");
    h.advance_async(0);
    h
}

#[test]
fn triggers_title_generation_on_first_enter_key() {
    let mut h = titled_harness();
    h.user_input("hello\r");
    // The screen is not captured until the delay elapses.
    h.advance_async(999);
    assert!(h.title_calls.is_empty());
    h.advance_async(1);
    assert_eq!(h.title_calls.len(), 1);
    let request = &h.title_calls[0];
    assert_eq!(request.buffer, "$ ");
    assert_eq!(
        request.settings,
        TitleSettings {
            base_url: "http://localhost".to_owned(),
            api_key: "test-key".to_owned(),
            model: "test-model".to_owned(),
            title_system_prompt: "title prompt".to_owned(),
            reasoning_effort: ReasoningEffort::Low,
        }
    );
    assert!(h.has_metadata("displayName", "Test Title", "analyzerSetDisplayName"));
    assert!(h.has_metadata("description", "Test Description", "analyzerSetDescription"));
    assert!(h.has_metadata(
        "descriptionPrompted",
        "true",
        "analyzerSetDescriptionPrompted"
    ));
}

#[test]
fn does_not_trigger_title_generation_on_non_enter_input() {
    let mut h = titled_harness();
    h.user_input("hello");
    h.advance_async(2000);
    assert!(h.title_calls.is_empty());
}

#[test]
fn does_not_trigger_title_generation_twice() {
    let mut h = titled_harness();
    h.user_input("\r");
    h.advance_async(1000);
    assert_eq!(h.title_calls.len(), 1);
    h.user_input("\r");
    h.advance_async(1000);
    assert_eq!(h.title_calls.len(), 1);
}

#[test]
fn cancels_the_pending_title_capture_when_stopped_before_the_delay_elapses() {
    let mut h = titled_harness();
    h.user_input("hello\r");
    h.advance_async(500);
    h.analyzer.stop();
    h.advance_async(1000);
    assert!(h.title_calls.is_empty());
}

#[test]
fn does_not_generate_title_when_display_name_and_description_already_exist() {
    let mut h = titled_harness();
    h.workspace.has_display_name = true;
    h.workspace.has_description = true;
    h.user_input("\r");
    h.advance_async(1000);
    assert!(h.title_calls.is_empty());
}

#[test]
fn generates_description_even_when_display_name_already_exists() {
    let mut h = titled_harness();
    h.workspace.has_display_name = true;
    h.user_input("\r");
    h.advance_async(1000);
    assert_eq!(h.title_calls.len(), 1);
    assert!(h.has_metadata("description", "Test Description", "analyzerSetDescription"));
    assert!(!h.metadata_keys().contains(&"displayName"));
}

#[test]
fn retries_the_capture_on_the_next_enter_while_the_screen_is_blank() {
    let mut h = Harness::new();
    h.user_input("\r");
    h.advance_async(1000);
    assert!(h.title_calls.is_empty());
    h.emit("$ ls");
    h.user_input("\r");
    h.advance_async(1000);
    assert_eq!(h.title_calls.len(), 1);
}

// === manual refresh ===

fn started() -> Harness {
    let mut h = Harness::new();
    h.emit("$ echo hi\nhi\n$ ");
    h
}

#[test]
fn refresh_title_and_description_overwrites_display_name_and_description_even_when_already_set() {
    let mut h = started();
    h.workspace.has_display_name = true;
    h.workspace.has_description = true;
    h.refresh_title_and_description();
    assert_eq!(h.refreshes, vec![(Refresh::TitleAndDescription, Ok(()))]);
    assert_eq!(h.title_calls.len(), 1);
    assert!(h.has_metadata("displayName", "Test Title", "manualRefreshTitle"));
    assert!(h.has_metadata(
        "description",
        "Test Description",
        "manualRefreshDescription"
    ));
    assert!(h.has_metadata(
        "descriptionPrompted",
        "true",
        "manualRefreshDescriptionPrompted"
    ));
}

#[test]
fn refresh_title_and_description_reports_failure_when_there_is_no_buffer() {
    let mut h = Harness::new();
    h.refresh_title_and_description();
    let expected = Err("Terminal is empty — nothing for the labeller to read".to_owned());
    assert_eq!(h.refreshes, vec![(Refresh::TitleAndDescription, expected)]);
    assert!(h.title_calls.is_empty());
    assert!(h.metadata.is_empty());
}

#[test]
fn refresh_title_and_description_reports_failure_when_no_model_is_configured() {
    let mut s = settings();
    s.terminal_analyzer.title_model = String::new();
    s.terminal_analyzer.model = String::new();
    let mut h = Harness::with_settings(s);
    h.refresh_title_and_description();
    assert_eq!(
        h.refreshes,
        vec![(
            Refresh::TitleAndDescription,
            Err("Title model not configured".to_owned())
        )]
    );
    assert!(h.title_calls.is_empty());
}

#[test]
fn refresh_title_and_description_surfaces_the_llm_error_message_and_writes_no_metadata() {
    let mut h = started();
    h.title = Box::new(|_| {
        Ok(TitleOutcome {
            result: Err("rate limited".to_owned()),
            system_prompt: "title prompt".to_owned(),
        })
    });
    h.refresh_title_and_description();
    assert_eq!(
        h.refreshes,
        vec![(Refresh::TitleAndDescription, Err("rate limited".to_owned()))]
    );
    assert!(h.metadata.is_empty());
}

#[test]
fn refresh_title_and_description_surfaces_a_thrown_llm_error() {
    let mut h = started();
    h.title = Box::new(|_| Err("network down".to_owned()));
    h.refresh_title_and_description();
    assert_eq!(
        h.refreshes,
        vec![(Refresh::TitleAndDescription, Err("network down".to_owned()))]
    );
}

#[test]
fn refresh_title_and_description_reports_an_empty_title() {
    let mut h = started();
    h.title = Box::new(|_| titled("", "d", "b"));
    h.refresh_title_and_description();
    assert_eq!(
        h.refreshes,
        vec![(
            Refresh::TitleAndDescription,
            Err("LLM returned no title".to_owned())
        )]
    );
    assert!(h.metadata.is_empty());
}

#[test]
fn refresh_branch_name_renames_the_branch_and_marks_it_user_defined_even_when_already_user_defined()
{
    let mut h = started();
    h.workspace.branch_is_user_defined = true;
    h.refresh_branch_name();
    assert_eq!(h.refreshes, vec![(Refresh::BranchName, Ok(()))]);
    assert_eq!(
        h.renames,
        vec![("old-branch".to_owned(), "test-title".to_owned())]
    );
    assert!(h.has_metadata("branchIsUserDefined", "true", "manualRefreshBranch"));
    // A branch-only refresh does not touch the display name or description.
    assert!(!h.metadata_keys().contains(&"displayName"));
}

#[test]
fn refresh_branch_name_does_not_rename_when_the_suggested_branch_name_is_invalid() {
    let mut h = started();
    h.title = Box::new(|_| titled("Test Title", "d", "Invalid Name!"));
    h.refresh_branch_name();
    let expected = Err("LLM suggested an invalid branch name: \"Invalid Name!\"".to_owned());
    assert_eq!(h.refreshes, vec![(Refresh::BranchName, expected)]);
    assert!(h.renames.is_empty());
    assert!(h.metadata.is_empty());
}

#[test]
fn refresh_branch_name_does_not_rename_when_the_llm_omits_the_branch_name() {
    let mut h = started();
    h.title = Box::new(|_| titled("Test Title", "d", ""));
    h.refresh_branch_name();
    assert!(matches!(
        h.refreshes.as_slice(),
        [(Refresh::BranchName, Err(_))]
    ));
    assert!(h.renames.is_empty());
}

#[test]
fn refresh_branch_name_surfaces_a_rename_failure() {
    let mut h = started();
    h.rename = Box::new(|_, _| Err("branch already exists".to_owned()));
    h.refresh_branch_name();
    assert_eq!(
        h.refreshes,
        vec![(Refresh::BranchName, Err("branch already exists".to_owned()))]
    );
    assert!(h.metadata.is_empty());
}

#[test]
fn refresh_branch_name_reports_failure_when_the_workspace_has_no_branch() {
    let mut h = started();
    h.workspace.git_branch = None;
    h.refresh_branch_name();
    assert_eq!(
        h.refreshes,
        vec![(
            Refresh::BranchName,
            Err("Workspace has no branch to rename".to_owned())
        )]
    );
    assert!(h.renames.is_empty());
}

// === auto-approve ===

fn safe_permission(auto_approve: bool, state: ActivityState) -> Harness {
    let mut h = Harness::new().answering(classified(state, "asks"));
    h.analyzer.set_auto_approve(auto_approve);
    h.emit("Allow edit? (y/n)");
    h.advance_async(1000);
    h
}

#[test]
fn auto_approves_safe_permission_requests_via_own_tty() {
    let h = safe_permission(true, ActivityState::SafePermissionRequested);
    assert_eq!(h.writes, vec!["\r"]);
}

#[test]
fn does_not_auto_approve_when_auto_approve_is_false() {
    let h = safe_permission(false, ActivityState::SafePermissionRequested);
    assert!(h.writes.is_empty());
}

#[test]
fn does_not_auto_approve_for_non_safe_states() {
    let h = safe_permission(true, ActivityState::PermissionRequest);
    assert!(h.writes.is_empty());
}

#[test]
fn approves_once_per_change_into_the_safe_state() {
    let mut h = safe_permission(false, ActivityState::SafePermissionRequested);
    // Turning it on while already asking approves nothing, and neither does classifying the
    // same prompt again (here after a settings change): only a state change does...
    h.analyzer.set_auto_approve(true);
    let mut s = settings();
    s.terminal_analyzer.safe_paths.push("/other".to_owned());
    h.set_settings(s);
    h.advance_async(1000);
    assert_eq!(h.classify_calls.len(), 2);
    assert!(h.writes.is_empty());
    // ...so the next safe request (after output marked the tab Working) is approved.
    h.emit("Allow write? (y/n)");
    h.advance_async(1000);
    assert_eq!(h.states()[h.states().len() - 2], ActivityState::Working);
    assert_eq!(h.writes, vec!["\r"]);
}

// === activity state sync ===

#[test]
fn activity_state_sync_calls_set_activity_tab_state_during_analysis() {
    let mut h = Harness::new();
    h.settle();
    h.emit("$ echo hello\nhello\n$ ");
    h.advance(500);
    // The detector edge carries no reason, only the screen.
    assert_eq!(
        h.activity[0],
        (
            ActivityState::Working,
            classification("$ echo hello\nhello\n$ ", "")
        )
    );
    h.advance(500);
    h.advance_async(0);
    assert_eq!(
        h.activity.last(),
        Some(&(
            ActivityState::Idle,
            classification("$ echo hello\nhello\n$ ", "prompt visible")
        ))
    );
}

#[test]
fn does_not_publish_working_for_the_replay_after_start_only_its_classification() {
    let mut h = Harness::new();
    h.emit("$ replayed scrollback\n$ ");
    h.advance_async(0);
    h.advance(500);
    h.advance_async(0);
    assert_eq!(h.states(), vec![ActivityState::Idle]);

    h.emit("$ npm test\n");
    h.advance(500);
    assert_eq!(h.states().last(), Some(&ActivityState::Working));
}

#[test]
fn publishes_working_when_the_process_was_already_producing_output_at_start() {
    let mut h = Harness::new();
    h.emit("$ npm test\n");
    // A spinner keeps the viewport changing; a replay would have gone quiet by now.
    for t in (0..500).step_by(100) {
        h.advance_async(100);
        h.emit(&format!("spinner {t}"));
    }
    h.advance_async(0);
    assert_eq!(h.states(), vec![ActivityState::Working]);
    assert!(h.classify_calls.is_empty());

    h.advance(500);
    h.advance_async(0);
    assert_eq!(h.classify_calls.len(), 1);
    assert_eq!(h.states().last(), Some(&ActivityState::Idle));
}

#[test]
fn skips_the_working_edge_but_still_classifies_on_idle_while_the_idle_detector_is_disabled() {
    let mut h = Harness::new();
    h.idle_disabled = true;
    h.settle();
    h.emit("$ echo hello\nhello\n$ ");
    h.advance(500);
    assert!(!h.states().contains(&ActivityState::Working));
    h.advance_async(500);
    assert_eq!(h.classify_calls.len(), 1);
    assert_eq!(h.states().last(), Some(&ActivityState::Idle));
}

// === history logging ===

fn classified_once(response: Result<AnalyzeOutcome, String>) -> Harness {
    let mut h = Harness::new().answering(response);
    h.emit("$ echo hello\nhello\n$ ");
    h.advance(1000);
    h.advance_async(0);
    h
}

#[test]
fn logs_successful_analysis_to_history_with_response() {
    let response = classified(ActivityState::Idle, "prompt visible");
    let h = classified_once(response.clone());
    let history = h.history();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].kind, HistoryKind::Analyzer);
    assert_eq!(history[0].error(), None);
    assert_eq!(history[0].model, "test-model");
    assert_eq!(history[0].buffer_text, "$ echo hello\nhello\n$ ");
    assert_eq!(history[0].response(), response.unwrap().to_json());
    assert!(matches!(
        &history[0].record,
        HistoryRecord::Response { status: ResponseStatus::Ok { cached: false }, system_prompt, .. } if system_prompt == "system prompt"
    ));
}

#[test]
fn logs_error_result_to_history_with_response() {
    let h = classified_once(provider_error("API error"));
    let history = h.history();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].kind, HistoryKind::Analyzer);
    assert_eq!(history[0].error(), Some("API error"));
    assert_eq!(
        history[0].response(),
        r#"{"error":"API error","systemPrompt":"system prompt"}"#
    );
}

#[test]
fn logs_exception_to_history() {
    let h = classified_once(Err("Network error".to_owned()));
    let history = h.history();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].kind, HistoryKind::Analyzer);
    assert_eq!(history[0].error(), Some("Network error"));
    assert_eq!(history[0].response(), "");
}

#[test]
fn logs_title_generation_to_history() {
    let mut h = titled_harness();
    h.user_input("hello\r");
    h.advance_async(1000);
    assert_eq!(h.title_calls.len(), 1);
    let history = h.history();
    let entry = history
        .iter()
        .find(|entry| entry.kind == HistoryKind::Title)
        .unwrap();
    assert_eq!(entry.error(), None);
    assert_eq!(entry.model, "test-model");
    assert_eq!(
        entry.response(),
        r#"{"title":"Test Title","description":"Test Description","branchName":"test-title","systemPrompt":"title prompt"}"#
    );
}

#[test]
fn skips_branch_rename_when_branch_is_user_defined() {
    let mut h = titled_harness();
    h.workspace.branch_is_user_defined = true;
    h.user_input("hello\r");
    h.advance_async(1000);
    assert_eq!(h.title_calls.len(), 1);
    assert!(h.has_metadata("displayName", "Test Title", "analyzerSetDisplayName"));
    assert!(h.renames.is_empty());
    assert!(!h.metadata_keys().contains(&"branchIsUserDefined"));
}

#[test]
fn marks_the_branch_as_defined_when_auto_renaming_so_later_analyzers_leave_it_alone() {
    let mut h = titled_harness();
    h.user_input("hello\r");
    h.advance_async(1000);
    assert!(h.has_metadata("branchIsUserDefined", "true", "analyzerSetBranch"));
    assert_eq!(
        h.renames,
        vec![("old-branch".to_owned(), "test-title".to_owned())]
    );
}

#[test]
fn marks_the_branch_as_defined_even_when_the_automatic_rename_is_skipped_or_fails() {
    let mut h = titled_harness();
    h.title = Box::new(|_| titled("Test Title", "d", "Not Valid"));
    h.user_input("\r");
    h.advance_async(1000);
    assert!(h.has_metadata("branchIsUserDefined", "true", "analyzerSetBranch"));
    assert!(h.renames.is_empty());

    let mut failing = titled_harness();
    failing.rename = Box::new(|_, _| Err("exists".to_owned()));
    failing.user_input("\r");
    failing.advance_async(1000);
    assert_eq!(failing.renames.len(), 1);
    assert!(failing.refreshes.is_empty());
}

#[test]
fn skips_title_description_and_branch_rename_when_workspace_has_no_parent() {
    let mut h = titled_harness();
    h.workspace.has_parent = false;
    h.user_input("hello\r");
    h.advance_async(1000);
    assert!(h.title_calls.is_empty());
    assert!(h.metadata.is_empty());
    assert!(h.renames.is_empty());
}

#[test]
fn logs_title_generation_failure_to_history() {
    let mut h = titled_harness();
    h.title = Box::new(|_| Err("Title API error".to_owned()));
    h.user_input("hello\r");
    h.advance_async(1000);
    assert_eq!(h.title_calls.len(), 1);
    let history = h.history();
    let entry = history
        .iter()
        .find(|entry| entry.kind == HistoryKind::Title && entry.error().is_some())
        .unwrap();
    assert_eq!(entry.error(), Some("Title API error"));
    assert_eq!(entry.response(), "");
    assert!(h.metadata.is_empty());
}

// === git refresh on AI state change ===

fn after_working(response: Result<AnalyzeOutcome, String>) -> Harness {
    let mut h = Harness::new();
    h.settle();
    h.git_refreshes = 0;
    h.classify = Box::new(move |_| Some(response.clone()));
    h.emit("$ work");
    h.advance(500);
    assert_eq!(h.analyzer.ai_state(), ActivityState::Working);
    h.advance_async(0);
    h
}

#[test]
fn triggers_refresh_git_info_and_refresh_git_when_state_becomes_idle() {
    assert_eq!(
        after_working(classified(ActivityState::Idle, "")).git_refreshes,
        1
    );
}

#[test]
fn triggers_refresh_git_info_and_refresh_git_when_state_becomes_completed() {
    assert_eq!(
        after_working(classified(ActivityState::Completed, "")).git_refreshes,
        1
    );
}

#[test]
fn does_not_trigger_git_refresh_when_state_becomes_error() {
    assert_eq!(after_working(provider_error("boom")).git_refreshes, 0);
}

#[test]
fn does_not_trigger_git_refresh_when_state_becomes_user_input_required() {
    assert_eq!(
        after_working(classified(ActivityState::UserInputRequired, "")).git_refreshes,
        0
    );
}

#[test]
fn does_not_trigger_git_refresh_when_state_becomes_working() {
    let mut h = Harness::new().manual();
    h.settle();
    h.git_refreshes = 0;
    h.emit("$ work");
    h.advance_async(500);
    assert_eq!(h.analyzer.ai_state(), ActivityState::Working);
    assert_eq!(h.git_refreshes, 0);
}

#[test]
fn stops_polling_on_tty_exit() {
    let mut h = Harness::new();
    h.exit();
    // Output after the exit is not analyzed.
    h.emit("some data");
    h.advance(1000);
    h.advance_async(0);
    assert!(h.classify_calls.is_empty());
}

#[test]
fn reclassifies_an_unchanged_buffer_when_provider_safe_paths_or_criteria_change_without_changing_the_title_model()
 {
    let mut h = Harness::new();
    let mut s = settings();
    h.emit("$ ");
    h.advance_async(1000);
    assert_eq!(h.classify_calls.len(), 1);
    assert_eq!(
        h.classify_calls[0].settings.provider,
        Provider::ChatCompletions {
            reasoning_effort: ReasoningEffort::Low
        }
    );
    assert_eq!(h.classify_calls[0].settings.model, "test-model");
    assert_eq!(h.classify_calls[0].input.cwd, "/test");

    s.terminal_analyzer.provider = ClassifierProvider::Classifier;
    h.set_settings(s.clone());
    h.advance_async(1000);
    assert_eq!(h.classify_calls.len(), 2);
    // Only the chat provider carries a reasoning effort.
    assert_eq!(h.classify_calls[1].settings.provider, Provider::Classifier);
    assert_eq!(h.classify_calls[1].settings.model, "test-model");

    s.terminal_analyzer.safe_paths = vec!["/different".to_owned()];
    h.set_settings(s.clone());
    h.advance_async(1000);
    assert_eq!(h.classify_calls.len(), 3);

    s.terminal_analyzer.criteria.idle = "Edited idle".to_owned();
    h.set_settings(s.clone());
    h.advance_async(1000);
    assert_eq!(h.classify_calls.len(), 4);
    assert_eq!(h.classify_calls[3].input.criteria.idle, "Edited idle");

    h.refresh_title_and_description();
    assert_eq!(h.title_calls[0].settings.model, "test-model");
    let last = h.history().pop().unwrap();
    assert_eq!(
        (last.kind, last.model.as_str()),
        (HistoryKind::Title, "test-model")
    );
}

#[test]
fn ignores_settings_changes_that_do_not_affect_classification() {
    let mut h = Harness::new();
    h.emit("$ ");
    h.advance_async(1000);
    let mut s = settings();
    // Credentials, the title model and unrelated settings are not part of the configuration.
    s.llm.api_key = "rotated".to_owned();
    s.terminal_analyzer.title_model = "other".to_owned();
    s.terminal.font_size += 1.0;
    h.set_settings(s);
    h.advance_async(1000);
    assert_eq!(h.classify_calls.len(), 1);
}

#[test]
fn waits_for_the_unread_idle_debounce_while_the_workspace_carries_an_unread_marker() {
    let mut h = Harness::new();
    h.settle();
    h.unread = true;
    let settled_state = h.analyzer.ai_state();
    h.emit("$ ");
    h.advance_async(14999);
    assert_eq!(h.analyzer.ai_state(), settled_state);
    assert!(h.classify_calls.is_empty());
    h.advance_async(1);
    assert_eq!(h.classify_calls.len(), 1);

    // Once acknowledged, the next burst uses the short debounce again.
    h.unread = false;
    h.emit("$ ls");
    h.advance_async(500);
    assert_eq!(h.classify_calls.len(), 2);
}

#[test]
fn neither_marks_working_nor_reclassifies_for_a_blip_that_restores_the_screen() {
    let mut h = Harness::new();
    h.settle();
    h.emit("replayx");
    h.advance_async(100);
    h.emit("replay");
    h.advance_async(5000);
    assert!(h.activity.is_empty());
    assert!(h.classify_calls.is_empty());
}

#[test]
fn applies_an_in_flight_response_when_a_blip_restores_the_requested_screen_before_it_lands() {
    let mut h = Harness::new().manual();
    h.emit("Continue?");
    h.advance_async(1000);
    assert!(h.analyzer.analyzing());
    h.emit("Continue?x");
    h.advance_async(100);
    h.emit("Continue?");
    h.advance_async(0);
    h.resolve(
        0,
        classified(ActivityState::UserInputRequired, "asks to continue"),
    );
    h.advance_async(5000);
    assert_eq!(h.classify_calls.len(), 1);
    assert_eq!(h.analyzer.ai_state(), ActivityState::UserInputRequired);
    assert!(!h.analyzer.analyzing());
}

#[test]
fn discards_a_safe_permission_response_after_a_provider_switch_before_the_next_poll() {
    let then = classified(
        ActivityState::PermissionRequest,
        "Classifier decision: permission_request",
    );
    let mut h = Harness::new().first_manual(then);
    h.analyzer.set_auto_approve(true);
    h.emit("Allow mutation?");
    h.advance_async(1000);
    assert!(h.analyzer.analyzing());

    let mut s = settings();
    s.terminal_analyzer.provider = ClassifierProvider::Classifier;
    h.set_settings(s);
    h.resolve(
        0,
        classified(ActivityState::SafePermissionRequested, "stale"),
    );
    assert_eq!(h.classify_calls.len(), 2);
    assert_eq!(h.analyzer.ai_state(), ActivityState::PermissionRequest);
    assert_eq!(
        h.analyzer.reason(),
        "Classifier decision: permission_request"
    );
    assert!(!h.analyzer.analyzing());
    assert!(h.writes.is_empty());
    assert!(!h.states().contains(&ActivityState::SafePermissionRequested));
    assert_eq!(h.history()[0].error(), Some("[discarded]"));
}

#[test]
fn real_jev_adapter_handles_choices_without_auto_approval() {
    for choice in [
        "malformed",
        "safe_permission_requested_typo",
        "permission_request",
    ] {
        let mut s = settings();
        s.terminal_analyzer.provider = ClassifierProvider::Classifier;
        let fake = FakeLlm::default();
        let body = format!(
            r#"{{"answers":{{"activity_state":{{"type":"choice","choice":"{choice}"}}}}}}"#
        );
        fake.on_fetch(move |_| {
            Ok(HttpResponse {
                status: 200,
                status_text: "OK".to_owned(),
                body: body.clone(),
            })
        });
        let llm = Llm::new(Arc::new(fake.clone()), AnalyzerCache::default());
        let mut h = Harness::with_settings(s);
        h.classify = Box::new(move |request| {
            Some(Ok(futures::executor::block_on(llm.analyze_terminal(
                request.input.clone(),
                request.settings.clone(),
            ))))
        });
        h.analyzer.set_auto_approve(true);
        h.emit("Allow action?");
        h.advance_async(1000);
        assert_eq!(fake.fetch_calls().len(), 1, "{choice}");
        let expected = if choice == "permission_request" {
            ActivityState::PermissionRequest
        } else {
            ActivityState::Error
        };
        assert_eq!(h.analyzer.ai_state(), expected, "{choice}");
        if choice == "permission_request" {
            assert_eq!(
                h.analyzer.reason(),
                "Classifier decision: permission_request"
            );
        }
        assert_ne!(h.analyzer.reason(), "", "{choice}");
        assert!(!h.analyzer.analyzing());
        assert!(h.writes.is_empty());
    }
}

// --- Rust additions ---

#[test]
fn reports_an_invalid_classifier_endpoint_as_an_error_without_a_request() {
    let mut s = settings();
    s.terminal_analyzer.provider = ClassifierProvider::Classifier;
    s.llm.base_url = "not a url".to_owned();
    let mut h = Harness::with_settings(s);
    h.emit("$ ");
    h.advance_async(1000);
    assert!(h.classify_calls.is_empty());
    assert_eq!(h.analyzer.ai_state(), ActivityState::Error);
    assert!(h.analyzer.reason().starts_with("Invalid URL"));
    assert!(matches!(
        h.history()[0].record,
        HistoryRecord::CallFailed { .. }
    ));
}

#[test]
fn reanalyzes_after_a_failed_call_when_an_edge_arrived_meanwhile() {
    let mut h = Harness::new().manual();
    h.emit("first");
    h.advance_async(1000);
    h.emit("second");
    h.advance_async(1000);
    assert_eq!(h.classify_calls.len(), 1);
    h.resolve(0, Err("Network error".to_owned()));
    assert_eq!(h.analyzer.ai_state(), ActivityState::Error);
    assert_eq!(h.classify_calls.len(), 2);
    assert_eq!(h.classify_calls[1].input.buffer, "second");
}

#[test]
fn ignores_edges_and_results_while_stopped() {
    let mut analyzer = Analyzer::new("/test", &settings());
    assert!(analyzer.on_activity("x", false).is_empty());
    assert!(analyzer.on_idle("x", Now::current()).is_empty());
    assert!(analyzer.on_exit().is_empty());
    assert!(analyzer.set_settings(&settings()).is_empty());
    analyzer.start();
    let effects = analyzer.on_idle("x", Now::current());
    let Some(Effect::Classify(request)) = effects.first().cloned() else {
        panic!("{effects:?}")
    };
    analyzer.stop();
    let late = analyzer.on_classified(
        request.id,
        classified(ActivityState::Idle, ""),
        "x",
        Now::current(),
    );
    assert!(late.is_empty());
    assert!(analyzer.history().next().is_none());
}

#[test]
fn computes_the_idle_debounce_from_the_unread_state() {
    let s = settings().terminal_analyzer;
    assert_eq!(idle_timeout(&s, false), 500 * MS);
    assert_eq!(idle_timeout(&s, true), 15000 * MS);
}

#[test]
fn validates_branch_names_as_lowercase_kebab_case() {
    for valid in ["fix-build", "a", "v2-api-3"] {
        assert!(is_valid_branch_name(valid), "{valid}");
    }
    for invalid in ["", "-a", "a-", "a--b", "Fix", "a_b", "a b", "ü"] {
        assert!(!is_valid_branch_name(invalid), "{invalid}");
    }
}
