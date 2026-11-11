//! The Activity History tab (`AnalyzerHistory.tsx`): a tab's activity transitions and, for
//! an AI-harness tab, its analyzer's LLM calls. Tab state `{ sourceTabId }` names the tab.
//!
//! Unlike Electron's snapshot (refreshed by a button), the lists are live: the model
//! follows the workspace (transitions) and the source tab's analyzer (calls).

use std::time::{Duration, SystemTime};

use gpui_kit::{App, Context, Entity, EntityId, Subscription, WeakEntity};
use serde_json::Value;
use treeterm_core::activity::{Transition, TransitionDetail};
use treeterm_core::analyzer::{HistoryEntry, HistoryKind, HistoryRecord, ResponseStatus};

use crate::analyzer::AnalyzerModel;
use crate::workspace_store::WorkspaceStore;

/// Which tab's history is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistorySource {
    Tab(String),
    /// The tab state carries no `sourceTabId` ("Invalid state: missing sourceTabId").
    Missing,
}

pub struct AnalyzerHistory {
    workspace: WeakEntity<WorkspaceStore>,
    source: HistorySource,
    /// The source tab's analyzer, observed while it exists.
    analyzer: Option<(EntityId, Subscription)>,
    _workspace: Subscription,
}

impl AnalyzerHistory {
    /// `analyzer`: the source tab's analyzer, if it already runs.
    pub fn new(
        state: &Value,
        workspace: &Entity<WorkspaceStore>,
        analyzer: Option<Entity<AnalyzerModel>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let source = match state.get("sourceTabId").and_then(Value::as_str) {
            Some(tab) => HistorySource::Tab(tab.to_owned()),
            None => HistorySource::Missing,
        };
        let observe = cx.observe(workspace, |history, workspace, cx| {
            let analyzer = match &history.source {
                HistorySource::Tab(tab) => workspace.read(cx).analyzer(tab),
                HistorySource::Missing => None,
            };
            history.follow(analyzer, cx);
            cx.notify();
        });
        let mut history = Self { workspace: workspace.downgrade(), source, analyzer: None, _workspace: observe };
        history.follow(analyzer, cx);
        history
    }

    pub fn source(&self) -> &HistorySource {
        &self.source
    }

    /// Observe `analyzer` (the source tab's, which a re-attached terminal replaces).
    fn follow(&mut self, analyzer: Option<Entity<AnalyzerModel>>, cx: &mut Context<Self>) {
        match (analyzer, &self.analyzer) {
            (Some(analyzer), Some((followed, _))) if analyzer.entity_id() == *followed => {}
            (Some(analyzer), Some(_) | None) => {
                let observe = cx.observe(&analyzer, |_, _, cx| cx.notify());
                self.analyzer = Some((analyzer.entity_id(), observe));
            }
            (None, _) => self.analyzer = None,
        }
    }

    fn source_tab(&self) -> Option<&str> {
        match &self.source {
            HistorySource::Tab(tab) => Some(tab),
            HistorySource::Missing => None,
        }
    }

    /// The source tab's transitions, newest first.
    pub fn transitions(&self, cx: &App) -> Vec<Transition> {
        let (Some(tab), Some(workspace)) = (self.source_tab(), self.workspace.upgrade()) else {
            return Vec::new();
        };
        workspace.read(cx).activity_states().transitions(tab).iter().rev().cloned().collect()
    }

    /// The analyzer's calls, newest first; `None` when the source tab is not analyzed.
    pub fn calls(&self, cx: &App) -> Option<Vec<HistoryEntry>> {
        let tab = self.source_tab()?;
        let analyzer = self.workspace.upgrade()?.read(cx).analyzer(tab)?;
        Some(analyzer.read(cx).analyzer().history().rev().cloned().collect())
    }

    /// "Debug": open the system prompt debugger on `buffer`.
    pub fn debug(&self, buffer: &str, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let opened = workspace.update(cx, |w, cx| w.open_prompt_debugger(buffer, cx));
        if opened.is_none() {
            tracing::error!("the system prompt debugger is not registered");
        }
    }
}

/// `TRANSITION_KIND_LABELS`.
pub fn transition_kind(detail: &TransitionDetail) -> &'static str {
    match detail {
        TransitionDetail::ViewportChanged { .. } => "viewport changed",
        TransitionDetail::ViewportIdle { .. } => "viewport idle",
        TransitionDetail::Classification { .. } => "classification",
        TransitionDetail::Debugger { .. } => "debugger",
    }
}

/// `transitionExtra`: what follows the kind pill.
pub fn transition_extra(detail: &TransitionDetail) -> String {
    match detail {
        TransitionDetail::ViewportIdle { idle_timeout, .. } => format!("after {}ms", idle_timeout.as_millis()),
        TransitionDetail::Classification { reason, .. } => reason.clone(),
        TransitionDetail::ViewportChanged { .. } | TransitionDetail::Debugger { .. } => String::new(),
    }
}

/// `bufferPreview`: the first three lines, then `...`.
pub fn buffer_preview(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let preview = lines.iter().take(3).copied().collect::<Vec<_>>().join("\n");
    if lines.len() > 3 { format!("{preview}\n...") } else { preview }
}

/// The collapsed response: its first 80 characters, then `...`.
pub fn response_preview(text: &str) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(80).collect();
    if chars.next().is_some() { format!("{head}...") } else { head }
}

/// The kind pill's text and colour (`KIND_COLORS`).
pub fn kind_pill(kind: HistoryKind) -> (&'static str, u32) {
    match kind {
        HistoryKind::Analyzer => ("analyzer", 0x1a5276),
        HistoryKind::Title => ("title", 0x6a0dad),
    }
}

/// An entry's pills and extras, as Electron derives them from its fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSummary {
    pub cached: bool,
    /// `durationMs`; absent when the call itself failed.
    pub duration: Option<Duration>,
    pub error: Option<String>,
    pub response: String,
    pub system_prompt: Option<String>,
}

pub fn summarize(entry: &HistoryEntry) -> CallSummary {
    let (cached, duration, system_prompt) = match &entry.record {
        HistoryRecord::Response { status, system_prompt, duration, .. } => {
            let cached = match status {
                ResponseStatus::Ok { cached } => *cached,
                ResponseStatus::Error(_) | ResponseStatus::Discarded => false,
            };
            (cached, Some(*duration), (!system_prompt.is_empty()).then(|| system_prompt.clone()))
        }
        HistoryRecord::CallFailed { .. } => (false, None, None),
    };
    CallSummary {
        cached,
        duration,
        error: entry.error().map(str::to_owned),
        response: entry.response().to_owned(),
        system_prompt,
    }
}

/// A transition's or call's clock time (`formatTime`).
pub fn clock(at: SystemTime) -> String {
    treeterm_core::clock::time_of_day(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_cut_long_buffers_and_responses() {
        assert_eq!(buffer_preview("a\nb\nc"), "a\nb\nc");
        assert_eq!(buffer_preview("a\nb\nc\nd"), "a\nb\nc\n...");
        assert_eq!(response_preview("short"), "short");
        let long = "x".repeat(81);
        assert_eq!(response_preview(&long), format!("{}...", "x".repeat(80)));
        assert_eq!(response_preview(&"é".repeat(80)), "é".repeat(80));
    }

    #[test]
    fn transitions_are_labelled_with_their_cause() {
        let idle = TransitionDetail::ViewportIdle { snapshot: String::new(), idle_timeout: Duration::from_millis(1500) };
        assert_eq!(transition_kind(&idle), "viewport idle");
        assert_eq!(transition_extra(&idle), "after 1500ms");
        let classified = TransitionDetail::Classification { snapshot: String::new(), reason: "asks".into() };
        assert_eq!(transition_extra(&classified), "asks");
        assert_eq!(transition_extra(&TransitionDetail::Debugger { snapshot: String::new() }), "");
    }

    #[test]
    fn calls_summarize_their_record() {
        let entry = |record| HistoryEntry {
            timestamp: SystemTime::UNIX_EPOCH,
            kind: HistoryKind::Analyzer,
            model: "m".into(),
            buffer_text: "buf".into(),
            record,
        };
        let ok = summarize(&entry(HistoryRecord::Response {
            response: "{}".into(),
            status: ResponseStatus::Ok { cached: true },
            system_prompt: "prompt".into(),
            duration: Duration::from_millis(12),
        }));
        assert_eq!(
            ok,
            CallSummary {
                cached: true,
                duration: Some(Duration::from_millis(12)),
                error: None,
                response: "{}".into(),
                system_prompt: Some("prompt".into()),
            }
        );
        let failed = summarize(&entry(HistoryRecord::CallFailed { error: "boom".into() }));
        assert_eq!(failed.error.as_deref(), Some("boom"));
        assert_eq!((failed.duration, failed.cached, failed.response.as_str()), (None, false, ""));
        assert_eq!(kind_pill(HistoryKind::Title).0, "title");
    }
}
