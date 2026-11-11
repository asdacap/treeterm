//! The AI-harness terminal analyzer as a sans-IO state machine. Port of
//! `store/createAnalyzerStore.ts`.
//!
//! The analyzer classifies what an AI harness tab is doing whenever its viewport goes quiet,
//! auto-approves safe permission prompts, and suggests a workspace title, description and
//! branch name after the first command. It does no I/O and owns no timers or terminal: the
//! owner (a GPUI model) feeds it inputs and carries out the [`Effect`]s each input returns,
//! in order.
//!
//! # Driving it
//!
//! - The tab's terminal runs the viewport idle detector (`treeterm_term::idle`) with
//!   [`idle_timeout`] for the timeout. Feed its `Activity` to [`Analyzer::on_activity`] and
//!   its `Idle` to [`Analyzer::on_idle`]; [`Effect::RequestIdleEdge`] asks that detector for
//!   an edge (`request_idle_edge`), whose output is fed back the same way.
//! - Run [`Effect::Classify`] with [`crate::llm::Llm::analyze_terminal`] and pass the outcome
//!   (or why the call itself failed) to [`Analyzer::on_classified`]; likewise
//!   [`Effect::GenerateTitle`] → `generate_title` → [`Analyzer::on_title`] and
//!   [`Effect::RenameBranch`] → the git rename → [`Analyzer::on_branch_renamed`]. Results
//!   for requests the analyzer no longer awaits are ignored, so the owner may drop (cancel)
//!   requests freely, e.g. on [`Analyzer::stop`].
//! - Keep a timer for [`Analyzer::deadline`] and call [`Analyzer::on_timer`] when it fires.
//! - Pass every settings change to [`Analyzer::set_settings`], typed input to
//!   [`Analyzer::on_user_input`], and the PTY's exit to [`Analyzer::on_exit`].
//!
//! Inputs that look at the screen take the current viewport text
//! (`treeterm_term::view::snapshot_viewport`): a response is applied only if the screen it
//! classified is still the one showing.

#[cfg(test)]
mod tests;

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant, SystemTime};

use crate::activity::{ActivityState, TransitionDetail};
use crate::classification::{
    Classification, ClassificationInput, ClassifierSettings, Provider, classification_identity,
};
use crate::llm::{AnalyzeOutcome, TitleOutcome, TitleSettings, TitleSuggestion};
use crate::settings::{ClassifierCriteria, LlmSettings, Settings, TerminalAnalyzerSettings};

/// Delay after the user presses Enter before capturing the screen for the title model:
/// gives the command time to run and render, so the suggestion reflects what happened.
pub const TITLE_GENERATION_DELAY: Duration = Duration::from_millis(1000);

/// Analyzer and title calls kept for debugging, oldest dropped first.
pub const MAX_HISTORY: usize = 1000;

/// Classifications remembered per analyzer run (most recently used kept).
const CACHE_SIZE: usize = 10;

/// Idle debounce for a tab: longer while its workspace already carries an unread marker.
pub fn idle_timeout(settings: &TerminalAnalyzerSettings, unread: bool) -> Duration {
    if unread {
        settings.idle_debounce_unread
    } else {
        settings.idle_debounce
    }
}

/// The time an input happened: monotonic for durations, wall clock for history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Now {
    pub instant: Instant,
    pub wall: SystemTime,
}

impl Now {
    pub fn current() -> Self {
        Self {
            instant: Instant::now(),
            wall: SystemTime::now(),
        }
    }
}

/// Correlates an effect's result with the analyzer's request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifyRequest {
    pub id: RequestId,
    pub input: ClassificationInput,
    pub settings: ClassifierSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleRequest {
    pub id: RequestId,
    pub buffer: String,
    pub settings: TitleSettings,
}

/// A manual, user-triggered re-label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refresh {
    /// Overwrite the display name and description.
    TitleAndDescription,
    /// Rename the git branch and mark it user-defined.
    BranchName,
}

/// What the owner must do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Start a classification; report it to [`Analyzer::on_classified`].
    Classify(Box<ClassifyRequest>),
    /// Start a title suggestion; report it to [`Analyzer::on_title`].
    GenerateTitle(TitleRequest),
    /// Publish the tab's activity state (`setActivityTabState`).
    SetActivity {
        state: ActivityState,
        detail: TransitionDetail,
    },
    /// Write to the tab's PTY (auto-approve).
    Write(&'static str),
    /// Ask the tab's idle detector for an idle edge and feed what it reports.
    RequestIdleEdge,
    /// Set a workspace metadata key.
    UpdateMetadata {
        key: &'static str,
        value: String,
        reason: &'static str,
    },
    /// Rename the workspace's git branch; report it to [`Analyzer::on_branch_renamed`].
    RenameBranch {
        id: RequestId,
        from: String,
        to: String,
    },
    /// Refresh the workspace's git info and status (the program may have changed files).
    RefreshGit,
    /// A manual refresh finished; failures carry a message fit to show the user.
    RefreshFinished {
        refresh: Refresh,
        result: Result<(), String>,
    },
}

/// What the analyzer needs to know about the tab's workspace, read when it is needed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceFacts {
    /// A child worktree (only those are labelled automatically, or have a branch to rename).
    pub has_parent: bool,
    pub has_display_name: bool,
    pub has_description: bool,
    /// The user (or an earlier suggestion) named the branch; no automatic rename.
    pub branch_is_user_defined: bool,
    /// The checked-out branch, if any.
    pub git_branch: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryKind {
    Analyzer,
    Title,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseStatus {
    /// `cached`: an analyzer answer from the shared cache.
    Ok { cached: bool },
    /// The provider reported an error.
    Error(String),
    /// A classification for a screen or configuration that was no longer current.
    Discarded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryRecord {
    /// The call returned; `response` is its result as JSON.
    Response {
        response: String,
        status: ResponseStatus,
        system_prompt: String,
        duration: Duration,
    },
    /// The call itself failed: nothing answered.
    CallFailed { error: String },
}

/// One analyzer or title call, for the debug view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub timestamp: SystemTime,
    pub kind: HistoryKind,
    pub model: String,
    pub buffer_text: String,
    pub record: HistoryRecord,
}

impl HistoryEntry {
    /// The TS entry's `error`: `[discarded]` for a discarded response.
    pub fn error(&self) -> Option<&str> {
        match &self.record {
            HistoryRecord::Response {
                status: ResponseStatus::Ok { .. },
                ..
            } => None,
            HistoryRecord::Response {
                status: ResponseStatus::Error(error),
                ..
            }
            | HistoryRecord::CallFailed { error } => Some(error),
            HistoryRecord::Response {
                status: ResponseStatus::Discarded,
                ..
            } => Some("[discarded]"),
        }
    }

    /// The TS entry's `response`: empty when the call failed.
    pub fn response(&self) -> &str {
        match &self.record {
            HistoryRecord::Response { response, .. } => response,
            HistoryRecord::CallFailed { .. } => "",
        }
    }
}

/// Classification settings whose change makes an answer stale and the screen worth
/// classifying again. Credentials are not part of it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Configuration {
    provider: Provider,
    model: String,
    base_url: String,
    system_prompt: String,
    criteria: ClassifierCriteria,
    safe_paths: Vec<String>,
}

#[derive(Debug)]
struct InFlight {
    id: RequestId,
    /// `classification_identity` of the request (the cache key).
    identity: String,
    /// The screen that was classified.
    buffer: String,
    configuration: Configuration,
    model: String,
    started: Instant,
    /// An idle edge arrived meanwhile: analyze again once this one is done.
    reanalyze: bool,
}

#[derive(Debug)]
enum Request {
    Idle,
    InFlight(Box<InFlight>),
}

/// State of one start/stop cycle; reset by stopping.
#[derive(Debug)]
struct Run {
    request: Request,
    /// Identity → result, most recently used last.
    cache: VecDeque<(String, Classification)>,
    last_configuration: Configuration,
    model_error_shown: bool,
}

impl Run {
    fn cached(&mut self, identity: &str) -> Option<Classification> {
        let index = self.cache.iter().position(|(entry, _)| entry == identity)?;
        let hit = self.cache.remove(index)?;
        let result = hit.1.clone();
        self.cache.push_back(hit);
        Some(result)
    }

    fn remember(&mut self, identity: String, result: Classification) {
        self.cache.push_back((identity, result));
        if self.cache.len() > CACHE_SIZE {
            self.cache.pop_front();
        }
    }

    /// The in-flight request `id`, if that is what the run awaits.
    fn take(&mut self, id: RequestId) -> Option<InFlight> {
        match std::mem::replace(&mut self.request, Request::Idle) {
            Request::InFlight(request) if request.id == id => Some(*request),
            other => {
                self.request = other;
                None
            }
        }
    }
}

#[derive(Debug)]
enum Lifecycle {
    Stopped,
    Running(Box<Run>),
}

/// Automatic labelling, at most once per run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TitleCapture {
    /// Waiting for the first Enter.
    Waiting,
    /// Enter was pressed; capture the screen at this time.
    Scheduled(Instant),
    Taken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TitlePurpose {
    Automatic,
    Refresh(Refresh),
}

#[derive(Debug)]
struct TitleJob {
    purpose: TitlePurpose,
    model: String,
    buffer: String,
    started: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenamePurpose {
    Automatic,
    Refresh,
}

/// One AI-harness tab's analyzer (`createAnalyzerStore`).
#[derive(Debug)]
pub struct Analyzer {
    cwd: String,
    llm: LlmSettings,
    settings: TerminalAnalyzerSettings,
    ai_state: ActivityState,
    reason: String,
    analyzing: bool,
    auto_approve: bool,
    lifecycle: Lifecycle,
    title: TitleCapture,
    titles: HashMap<RequestId, TitleJob>,
    renames: HashMap<RequestId, RenamePurpose>,
    history: VecDeque<HistoryEntry>,
    next_id: u64,
}

/// The screen as the analyzer reads it: nothing when blank.
fn non_blank(viewport: &str) -> Option<&str> {
    (!viewport.trim().is_empty()).then_some(viewport)
}

/// Lowercase kebab-case: `^[a-z0-9]+(-[a-z0-9]+)*$`.
fn is_valid_branch_name(name: &str) -> bool {
    name.split('-').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    })
}

impl Analyzer {
    /// Stopped, idle, auto-approve off.
    pub fn new(cwd: impl Into<String>, settings: &Settings) -> Self {
        Self {
            cwd: cwd.into(),
            llm: settings.llm.clone(),
            settings: settings.terminal_analyzer.clone(),
            ai_state: ActivityState::Idle,
            reason: String::new(),
            analyzing: false,
            auto_approve: false,
            lifecycle: Lifecycle::Stopped,
            title: TitleCapture::Waiting,
            titles: HashMap::new(),
            renames: HashMap::new(),
            history: VecDeque::new(),
            next_id: 0,
        }
    }

    pub fn ai_state(&self) -> ActivityState {
        self.ai_state
    }

    /// Why the current state was chosen (the classifier's reason or error).
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// A classification is under way.
    pub fn analyzing(&self) -> bool {
        self.analyzing
    }

    pub fn auto_approve(&self) -> bool {
        self.auto_approve
    }

    pub fn is_running(&self) -> bool {
        matches!(self.lifecycle, Lifecycle::Running(_))
    }

    /// Analyzer and title calls, oldest first.
    pub fn history(&self) -> impl DoubleEndedIterator<Item = &HistoryEntry> + ExactSizeIterator {
        self.history.iter()
    }

    /// When the owner must call [`Self::on_timer`].
    pub fn deadline(&self) -> Option<Instant> {
        match self.title {
            TitleCapture::Scheduled(at) => Some(at),
            TitleCapture::Waiting | TitleCapture::Taken => None,
        }
    }

    /// Begin analyzing the attached terminal. Does nothing while running.
    pub fn start(&mut self) {
        if let Lifecycle::Stopped = self.lifecycle {
            self.lifecycle = Lifecycle::Running(Box::new(Run {
                request: Request::Idle,
                cache: VecDeque::new(),
                last_configuration: self.configuration(),
                model_error_shown: false,
            }));
        }
    }

    /// Stop analyzing: the pending classification, the remembered classifications and the
    /// scheduled title capture are dropped. History, state and auto-approve stay.
    pub fn stop(&mut self) {
        self.lifecycle = Lifecycle::Stopped;
        self.title = TitleCapture::Waiting;
    }

    pub fn set_auto_approve(&mut self, value: bool) {
        self.auto_approve = value;
    }

    /// New settings. A changed classification configuration classifies the screen again
    /// (through an idle edge, so a running burst still debounces it).
    pub fn set_settings(&mut self, settings: &Settings) -> Vec<Effect> {
        let before = self.configuration();
        self.llm = settings.llm.clone();
        self.settings = settings.terminal_analyzer.clone();
        if self.is_running() && self.configuration() != before {
            vec![Effect::RequestIdleEdge]
        } else {
            Vec::new()
        }
    }

    /// The idle detector saw a burst of output begin.
    pub fn on_activity(&mut self, viewport: &str, idle_detector_disabled: bool) -> Vec<Effect> {
        let mut out = Vec::new();
        // The per-tab opt-out: output churn no longer marks the tab Working; idle
        // classification still runs.
        if self.is_running() && !idle_detector_disabled {
            self.update_ai_state(ActivityState::Working, None, viewport, &mut out);
        }
        out
    }

    /// The idle detector saw the viewport go quiet: classify it.
    pub fn on_idle(&mut self, viewport: &str, now: Now) -> Vec<Effect> {
        let mut out = Vec::new();
        self.analyze(viewport, now, &mut out);
        out
    }

    /// The PTY exited: stop and report the program completed.
    pub fn on_exit(&mut self) -> Vec<Effect> {
        let mut out = Vec::new();
        if !self.is_running() {
            return out;
        }
        self.stop();
        self.analyzing = false;
        // The terminal is gone: the transition carries no screen.
        self.update_ai_state(ActivityState::Completed, Some(""), "", &mut out);
        out
    }

    /// Result of [`Effect::Classify`]: the facade's outcome, or why the call itself failed.
    pub fn on_classified(
        &mut self,
        id: RequestId,
        response: Result<AnalyzeOutcome, String>,
        viewport: &str,
        now: Now,
    ) -> Vec<Effect> {
        let mut out = Vec::new();
        let configuration = self.configuration();
        let Lifecycle::Running(run) = &mut self.lifecycle else {
            return out;
        };
        let Some(request) = run.take(id) else {
            return out;
        };
        let InFlight {
            identity,
            buffer,
            configuration: requested,
            model,
            started,
            reanalyze,
            ..
        } = request;
        let duration = now.instant.saturating_duration_since(started);
        let configuration_changed = configuration != requested;
        let outcome = match response {
            Ok(outcome) => outcome,
            Err(error) => {
                if configuration_changed {
                    self.analyze(viewport, now, &mut out);
                    return out;
                }
                self.analyzing = false;
                self.update_ai_state(ActivityState::Error, Some(&error), viewport, &mut out);
                self.record(
                    now,
                    HistoryKind::Analyzer,
                    model,
                    buffer,
                    HistoryRecord::CallFailed { error },
                );
                if reanalyze {
                    self.analyze(viewport, now, &mut out);
                }
                return out;
            }
        };
        // Compare the screen itself, not a change counter: a blip that ends back on the
        // requested buffer fires no idle edge to re-analyze, so its response must still apply.
        if non_blank(viewport) != Some(buffer.as_str()) || configuration_changed {
            let reanalyze = reanalyze || configuration_changed;
            // Still a valid answer for the screen it was asked about: if the screen returns
            // there, the next analysis reuses it instead of asking again.
            if let Ok(analyzed) = &outcome.result {
                run.remember(identity, analyzed.classification.clone());
            }
            // The screen moved while we waited, and the burst it is in may end back on the
            // requested screen — a blip that fires no edge, leaving this classification
            // unapplied for good. Make that burst's quiet edge re-check regardless.
            if !reanalyze {
                out.push(Effect::RequestIdleEdge);
            }
            let record = HistoryRecord::Response {
                response: outcome.to_json(),
                status: ResponseStatus::Discarded,
                system_prompt: outcome.system_prompt,
                duration,
            };
            self.record(now, HistoryKind::Analyzer, model, buffer, record);
            self.analyzing = false;
            if reanalyze {
                self.analyze(viewport, now, &mut out);
            }
            return out;
        }
        let response = outcome.to_json();
        let status = match outcome.result {
            Ok(analyzed) => {
                run.remember(identity, analyzed.classification.clone());
                self.analyzing = false;
                let Classification { state, reason } = analyzed.classification;
                self.update_ai_state(state, Some(&reason), viewport, &mut out);
                ResponseStatus::Ok {
                    cached: analyzed.cached,
                }
            }
            Err(error) => {
                self.analyzing = false;
                self.update_ai_state(ActivityState::Error, Some(&error), viewport, &mut out);
                ResponseStatus::Error(error)
            }
        };
        let record = HistoryRecord::Response {
            response,
            status,
            system_prompt: outcome.system_prompt,
            duration,
        };
        self.record(now, HistoryKind::Analyzer, model, buffer, record);
        if reanalyze {
            self.analyze(viewport, now, &mut out);
        }
        out
    }

    /// Typed input: the first Enter schedules the automatic title capture.
    pub fn on_user_input(&mut self, data: &str, now: Instant) {
        if self.title == TitleCapture::Waiting && data.contains('\r') {
            self.title = TitleCapture::Scheduled(now + TITLE_GENERATION_DELAY);
        }
    }

    /// The timer for [`Self::deadline`] fired. Early or stale calls do nothing.
    pub fn on_timer(
        &mut self,
        now: Now,
        viewport: &str,
        workspace: &WorkspaceFacts,
    ) -> Vec<Effect> {
        let mut out = Vec::new();
        let TitleCapture::Scheduled(at) = self.title else {
            return out;
        };
        if now.instant < at {
            return out;
        }
        self.title = TitleCapture::Taken;
        // Automatic labelling only fills in what is missing, on child worktrees.
        if !workspace.has_parent || (workspace.has_display_name && workspace.has_description) {
            return out;
        }
        if non_blank(viewport).is_none() {
            self.title = TitleCapture::Waiting;
            return out;
        }
        match self.request_title(viewport, now, TitlePurpose::Automatic) {
            Ok(effect) => out.push(effect),
            Err(error) => tracing::warn!("analyzer: automatic title generation failed: {error}"),
        }
        out
    }

    /// Ask for a new display name and description, overwriting the current ones.
    pub fn refresh_title_and_description(&mut self, viewport: &str, now: Now) -> Vec<Effect> {
        self.refresh(Refresh::TitleAndDescription, viewport, now)
    }

    /// Ask for a branch name, rename the branch to it and mark it user-defined.
    pub fn refresh_branch_name(&mut self, viewport: &str, now: Now) -> Vec<Effect> {
        self.refresh(Refresh::BranchName, viewport, now)
    }

    /// Result of [`Effect::GenerateTitle`]: the facade's outcome, or why the call failed.
    pub fn on_title(
        &mut self,
        id: RequestId,
        response: Result<TitleOutcome, String>,
        now: Now,
        workspace: &WorkspaceFacts,
    ) -> Vec<Effect> {
        let Some(job) = self.titles.remove(&id) else {
            return Vec::new();
        };
        let TitleJob {
            purpose,
            model,
            buffer,
            started,
        } = job;
        let result = match response {
            Ok(outcome) => {
                let record = HistoryRecord::Response {
                    response: outcome.to_json(),
                    status: match &outcome.result {
                        Ok(_) => ResponseStatus::Ok { cached: false },
                        Err(error) => ResponseStatus::Error(error.clone()),
                    },
                    system_prompt: outcome.system_prompt,
                    duration: now.instant.saturating_duration_since(started),
                };
                self.record(now, HistoryKind::Title, model, buffer, record);
                match outcome.result {
                    Ok(suggestion) if !suggestion.title.is_empty() => Ok(suggestion),
                    Ok(_) => Err("LLM returned no title".to_owned()),
                    Err(error) => Err(error),
                }
            }
            Err(error) => {
                self.record(
                    now,
                    HistoryKind::Title,
                    model,
                    buffer,
                    HistoryRecord::CallFailed {
                        error: error.clone(),
                    },
                );
                Err(error)
            }
        };
        match purpose {
            TitlePurpose::Automatic => self.apply_automatic_title(result, workspace),
            TitlePurpose::Refresh(Refresh::TitleAndDescription) => {
                let suggestion = match result {
                    Ok(suggestion) => suggestion,
                    Err(error) => return vec![finished(Refresh::TitleAndDescription, Err(error))],
                };
                let mut out = vec![metadata(
                    "displayName",
                    suggestion.title,
                    "manualRefreshTitle",
                )];
                if !suggestion.description.is_empty() {
                    out.push(metadata(
                        "description",
                        suggestion.description,
                        "manualRefreshDescription",
                    ));
                    out.push(metadata(
                        "descriptionPrompted",
                        "true".to_owned(),
                        "manualRefreshDescriptionPrompted",
                    ));
                }
                out.push(finished(Refresh::TitleAndDescription, Ok(())));
                out
            }
            TitlePurpose::Refresh(Refresh::BranchName) => {
                let rename =
                    result.and_then(|suggestion| branch_rename(&suggestion.branch_name, workspace));
                match rename {
                    Ok((from, to)) => vec![self.rename(RenamePurpose::Refresh, from, to)],
                    Err(error) => vec![finished(Refresh::BranchName, Err(error))],
                }
            }
        }
    }

    /// Result of [`Effect::RenameBranch`].
    pub fn on_branch_renamed(&mut self, id: RequestId, result: Result<(), String>) -> Vec<Effect> {
        match (self.renames.remove(&id), result) {
            (None, _) | (Some(RenamePurpose::Automatic), Ok(())) => Vec::new(),
            (Some(RenamePurpose::Automatic), Err(error)) => {
                tracing::warn!("analyzer: automatic branch rename failed: {error}");
                Vec::new()
            }
            (Some(RenamePurpose::Refresh), Ok(())) => vec![
                metadata(
                    "branchIsUserDefined",
                    "true".to_owned(),
                    "manualRefreshBranch",
                ),
                finished(Refresh::BranchName, Ok(())),
            ],
            (Some(RenamePurpose::Refresh), Err(error)) => {
                vec![finished(Refresh::BranchName, Err(error))]
            }
        }
    }

    // --- internals ------------------------------------------------------------------------

    fn configuration(&self) -> Configuration {
        let classifier = ClassifierSettings::from_settings(&self.llm, &self.settings);
        Configuration {
            provider: classifier.provider,
            model: classifier.model,
            base_url: classifier.base_url,
            system_prompt: self.settings.system_prompt.clone(),
            criteria: self.settings.criteria.clone(),
            safe_paths: self.settings.safe_paths.clone(),
        }
    }

    fn next_id(&mut self) -> RequestId {
        self.next_id += 1;
        RequestId(self.next_id)
    }

    fn analyze(&mut self, viewport: &str, now: Now, out: &mut Vec<Effect>) {
        let configuration = self.configuration();
        let Lifecycle::Running(run) = &mut self.lifecycle else {
            return;
        };
        let Some(buffer) = non_blank(viewport) else {
            self.analyzing = false;
            return self.update_ai_state(ActivityState::Idle, Some(""), viewport, out);
        };
        // One request at a time: the next edge's work waits for this one.
        if let Request::InFlight(request) = &mut run.request {
            request.reanalyze = true;
            return;
        }
        if configuration != run.last_configuration {
            run.last_configuration = configuration.clone();
            run.model_error_shown = false;
        }
        let model = self.settings.model.clone();
        if model.is_empty() {
            if !run.model_error_shown {
                run.model_error_shown = true;
                self.analyzing = false;
                self.update_ai_state(
                    ActivityState::Error,
                    Some("Terminal analyzer model not configured"),
                    viewport,
                    out,
                );
            }
            return;
        }
        let settings = ClassifierSettings::from_settings(&self.llm, &self.settings);
        let input =
            ClassificationInput::from_settings(buffer.to_owned(), self.cwd.clone(), &self.settings);
        let identity = match classification_identity(&input, &settings) {
            Ok(identity) => identity,
            Err(error) => {
                let error = error.to_string();
                self.analyzing = false;
                self.update_ai_state(ActivityState::Error, Some(&error), viewport, out);
                let record = HistoryRecord::CallFailed { error };
                return self.record(now, HistoryKind::Analyzer, model, buffer.to_owned(), record);
            }
        };
        if let Some(Classification { state, reason }) = run.cached(&identity) {
            self.analyzing = false;
            return self.update_ai_state(state, Some(&reason), viewport, out);
        }
        self.next_id += 1;
        let id = RequestId(self.next_id);
        run.request = Request::InFlight(Box::new(InFlight {
            id,
            identity,
            buffer: buffer.to_owned(),
            configuration,
            model,
            started: now.instant,
            reanalyze: false,
        }));
        self.analyzing = true;
        out.push(Effect::Classify(Box::new(ClassifyRequest {
            id,
            input,
            settings,
        })));
    }

    /// `reason`: `None` keeps the current one (an activity edge carries none).
    fn update_ai_state(
        &mut self,
        state: ActivityState,
        reason: Option<&str>,
        viewport: &str,
        out: &mut Vec<Effect>,
    ) {
        let previous = std::mem::replace(&mut self.ai_state, state);
        if let Some(reason) = reason {
            reason.clone_into(&mut self.reason);
        }
        if previous != state {
            if state == ActivityState::SafePermissionRequested && self.auto_approve {
                out.push(Effect::Write("\r"));
            }
            if matches!(state, ActivityState::Idle | ActivityState::Completed) {
                out.push(Effect::RefreshGit);
            }
        }
        out.push(Effect::SetActivity {
            state,
            detail: TransitionDetail::Classification {
                snapshot: non_blank(viewport).unwrap_or_default().to_owned(),
                reason: reason.unwrap_or_default().to_owned(),
            },
        });
    }

    fn record(
        &mut self,
        now: Now,
        kind: HistoryKind,
        model: String,
        buffer_text: String,
        record: HistoryRecord,
    ) {
        self.history.push_back(HistoryEntry {
            timestamp: now.wall,
            kind,
            model,
            buffer_text,
            record,
        });
        if self.history.len() > MAX_HISTORY {
            self.history.pop_front();
        }
    }

    fn refresh(&mut self, refresh: Refresh, viewport: &str, now: Now) -> Vec<Effect> {
        match self.request_title(viewport, now, TitlePurpose::Refresh(refresh)) {
            Ok(effect) => vec![effect],
            Err(error) => vec![finished(refresh, Err(error))],
        }
    }

    fn request_title(
        &mut self,
        viewport: &str,
        now: Now,
        purpose: TitlePurpose,
    ) -> Result<Effect, String> {
        if self.settings.title_model.is_empty() {
            return Err("Title model not configured".to_owned());
        }
        let Some(buffer) = non_blank(viewport) else {
            return Err("Terminal is empty — nothing for the labeller to read".to_owned());
        };
        let id = self.next_id();
        let settings = TitleSettings {
            base_url: self.llm.base_url.clone(),
            api_key: self.llm.api_key.clone(),
            model: self.settings.title_model.clone(),
            title_system_prompt: self.settings.title_system_prompt.clone(),
            reasoning_effort: self.settings.reasoning_effort,
        };
        let job = TitleJob {
            purpose,
            model: settings.model.clone(),
            buffer: buffer.to_owned(),
            started: now.instant,
        };
        self.titles.insert(id, job);
        Ok(Effect::GenerateTitle(TitleRequest {
            id,
            buffer: buffer.to_owned(),
            settings,
        }))
    }

    /// Fill in what is missing; rename the branch once per workspace unless user-defined.
    fn apply_automatic_title(
        &mut self,
        result: Result<TitleSuggestion, String>,
        workspace: &WorkspaceFacts,
    ) -> Vec<Effect> {
        let suggestion = match result {
            Ok(suggestion) => suggestion,
            Err(error) => {
                tracing::warn!("analyzer: automatic title generation failed: {error}");
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        if !workspace.has_display_name {
            out.push(metadata(
                "displayName",
                suggestion.title,
                "analyzerSetDisplayName",
            ));
        }
        if !workspace.has_description && !suggestion.description.is_empty() {
            out.push(metadata(
                "description",
                suggestion.description,
                "analyzerSetDescription",
            ));
            out.push(metadata(
                "descriptionPrompted",
                "true".to_owned(),
                "analyzerSetDescriptionPrompted",
            ));
        }
        if !workspace.branch_is_user_defined {
            // One shot per workspace, persisted: otherwise every new tab or restart (while
            // the workspace still lacks a description) renamed the branch again, including
            // branches the user named or checked out in the meantime.
            out.push(metadata(
                "branchIsUserDefined",
                "true".to_owned(),
                "analyzerSetBranch",
            ));
            match branch_rename(&suggestion.branch_name, workspace) {
                Ok((from, to)) => out.push(self.rename(RenamePurpose::Automatic, from, to)),
                Err(error) => tracing::warn!("analyzer: automatic branch rename skipped: {error}"),
            }
        }
        out
    }

    fn rename(&mut self, purpose: RenamePurpose, from: String, to: String) -> Effect {
        let id = self.next_id();
        self.renames.insert(id, purpose);
        Effect::RenameBranch { id, from, to }
    }
}

fn metadata(key: &'static str, value: String, reason: &'static str) -> Effect {
    Effect::UpdateMetadata { key, value, reason }
}

fn finished(refresh: Refresh, result: Result<(), String>) -> Effect {
    Effect::RefreshFinished { refresh, result }
}

/// `(current branch, suggested name)` when the suggestion is valid and the workspace is a
/// worktree with a current branch.
fn branch_rename(
    branch_name: &str,
    workspace: &WorkspaceFacts,
) -> Result<(String, String), String> {
    if !is_valid_branch_name(branch_name) {
        return Err(format!(
            "LLM suggested an invalid branch name: \"{branch_name}\""
        ));
    }
    match &workspace.git_branch {
        Some(current) if !current.is_empty() && workspace.has_parent => {
            Ok((current.clone(), branch_name.to_owned()))
        }
        Some(_) | None => Err("Workspace has no branch to rename".to_owned()),
    }
}
