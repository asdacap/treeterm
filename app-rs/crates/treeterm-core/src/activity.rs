//! What each tab's program is doing, and why it changed. Port of `store/activityState.ts`
//! (the Zustand store becomes a plain value; the GPUI model owns one and notifies).

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::settings::ClassifiedState;

/// Port of `ActivityState` (`src/shared/types.ts`). The viewport idle detector produces
/// `Working`/`Idle`; the analyzer the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityState {
    #[default]
    Idle,
    Working,
    UserInputRequired,
    PermissionRequest,
    SafePermissionRequested,
    Completed,
    /// The program in the terminal reported a failure of its own: service overloaded, API
    /// error, crash. The user should retry.
    ApplicationError,
    /// The classifier itself failed; the terminal's real state is unknown.
    Error,
}

impl ActivityState {
    pub const ALL: [ActivityState; 8] = [
        ActivityState::Idle,
        ActivityState::Working,
        ActivityState::UserInputRequired,
        ActivityState::PermissionRequest,
        ActivityState::SafePermissionRequested,
        ActivityState::Completed,
        ActivityState::ApplicationError,
        ActivityState::Error,
    ];

    /// The wire value.
    pub fn as_str(self) -> &'static str {
        match self {
            ActivityState::Idle => "idle",
            ActivityState::Working => "working",
            ActivityState::UserInputRequired => "user_input_required",
            ActivityState::PermissionRequest => "permission_request",
            ActivityState::SafePermissionRequested => "safe_permission_requested",
            ActivityState::Completed => "completed",
            ActivityState::ApplicationError => "application_error",
            ActivityState::Error => "error",
        }
    }
}

impl From<ClassifiedState> for ActivityState {
    fn from(state: ClassifiedState) -> Self {
        match state {
            ClassifiedState::Working => ActivityState::Working,
            ClassifiedState::SafePermissionRequested => ActivityState::SafePermissionRequested,
            ClassifiedState::PermissionRequest => ActivityState::PermissionRequest,
            ClassifiedState::Completed => ActivityState::Completed,
            ClassifiedState::UserInputRequired => ActivityState::UserInputRequired,
            ClassifiedState::ApplicationError => ActivityState::ApplicationError,
            ClassifiedState::Idle => ActivityState::Idle,
        }
    }
}

/// Why a tab changed state. Every kind carries the viewport that was on screen at that moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionDetail {
    /// Idle detector saw the viewport change after a quiet period.
    ViewportChanged { snapshot: String },
    /// Idle detector saw no viewport change for the armed timeout.
    ViewportIdle {
        snapshot: String,
        idle_timeout: Duration,
    },
    /// AI harness analyzer classified the buffer (or reset on its own).
    Classification { snapshot: String, reason: String },
    /// System prompt debugger drove the state by hand.
    Debugger { snapshot: String },
}

impl TransitionDetail {
    pub fn snapshot(&self) -> &str {
        match self {
            TransitionDetail::ViewportChanged { snapshot }
            | TransitionDetail::ViewportIdle { snapshot, .. }
            | TransitionDetail::Classification { snapshot, .. }
            | TransitionDetail::Debugger { snapshot } => snapshot,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub timestamp: SystemTime,
    pub from: ActivityState,
    pub to: ActivityState,
    pub detail: TransitionDetail,
}

/// Transitions kept per tab, oldest dropped first.
pub const MAX_TRANSITIONS: usize = 1000;

/// Every tab's state and transition log (`useActivityStateStore`).
#[derive(Debug, Clone, Default)]
pub struct ActivityStates {
    states: HashMap<String, ActivityState>,
    transitions: HashMap<String, Vec<Transition>>,
}

impl ActivityStates {
    /// A tab without a state has not reported one yet (or was removed).
    pub fn get(&self, tab_id: &str) -> Option<ActivityState> {
        self.states.get(tab_id).copied()
    }

    /// Whether the tab has a state at all (`Object.hasOwn(states, tabId)`).
    pub fn contains(&self, tab_id: &str) -> bool {
        self.states.contains_key(tab_id)
    }

    /// The tab's transition log, oldest first.
    pub fn transitions(&self, tab_id: &str) -> &[Transition] {
        self.transitions
            .get(tab_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// Update a tab's state and log the transition (same-state changes are logged too).
    pub fn set_tab_state(
        &mut self,
        tab_id: &str,
        state: ActivityState,
        detail: TransitionDetail,
        timestamp: SystemTime,
    ) {
        let from = self
            .states
            .insert(tab_id.to_owned(), state)
            .unwrap_or_default();
        let log = self.transitions.entry(tab_id.to_owned()).or_default();
        log.push(Transition {
            timestamp,
            from,
            to: state,
            detail,
        });
        let excess = log.len().saturating_sub(MAX_TRANSITIONS);
        log.drain(..excess);
    }

    /// Forget a closed tab.
    pub fn remove_tab_state(&mut self, tab_id: &str) {
        self.states.remove(tab_id);
        self.transitions.remove(tab_id);
    }

    /// The workspace's consolidated state over `tab_ids` (tabs without a state count as idle).
    pub fn workspace_state<'a>(&self, tab_ids: impl IntoIterator<Item = &'a str>) -> ActivityState {
        consolidate(tab_ids.into_iter().filter_map(|id| self.get(id)))
    }
}

/// Priority: working > permission_request > safe_permission_requested > user_input_required >
/// application_error > error > completed > idle.
pub fn consolidate(states: impl IntoIterator<Item = ActivityState>) -> ActivityState {
    let rank = |state: ActivityState| match state {
        ActivityState::Working => 7,
        ActivityState::PermissionRequest => 6,
        ActivityState::SafePermissionRequested => 5,
        ActivityState::UserInputRequired => 4,
        ActivityState::ApplicationError => 3,
        ActivityState::Error => 2,
        ActivityState::Completed => 1,
        ActivityState::Idle => 0,
    };
    states
        .into_iter()
        .max_by_key(|state| rank(*state))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport() -> TransitionDetail {
        TransitionDetail::ViewportChanged {
            snapshot: "$ ls".to_owned(),
        }
    }

    fn set(store: &mut ActivityStates, tab: &str, state: ActivityState) {
        store.set_tab_state(tab, state, viewport(), SystemTime::UNIX_EPOCH);
    }

    // --- setTabState ---

    #[test]
    fn sets_state_for_a_tab() {
        let mut store = ActivityStates::default();
        set(&mut store, "tab1", ActivityState::Working);
        assert_eq!(store.get("tab1"), Some(ActivityState::Working));
    }

    #[test]
    fn updates_state_for_existing_tab() {
        let mut store = ActivityStates::default();
        set(&mut store, "tab1", ActivityState::Idle);
        set(&mut store, "tab1", ActivityState::Working);
        assert_eq!(store.get("tab1"), Some(ActivityState::Working));
    }

    #[test]
    fn maintains_states_for_multiple_tabs() {
        let mut store = ActivityStates::default();
        set(&mut store, "tab1", ActivityState::Working);
        set(&mut store, "tab2", ActivityState::Idle);
        set(&mut store, "tab3", ActivityState::UserInputRequired);
        assert_eq!(store.states.len(), 3);
        assert_eq!(store.get("tab1"), Some(ActivityState::Working));
        assert_eq!(store.get("tab2"), Some(ActivityState::Idle));
        assert_eq!(store.get("tab3"), Some(ActivityState::UserInputRequired));
    }

    // --- transitions ---

    #[test]
    fn logs_the_first_transition_from_idle_with_the_detail_and_a_timestamp() {
        let mut store = ActivityStates::default();
        let at = SystemTime::UNIX_EPOCH + Duration::from_millis(1_700_000_000_000);
        store.set_tab_state("tab1", ActivityState::Working, viewport(), at);
        assert_eq!(
            store.transitions("tab1"),
            [Transition {
                timestamp: at,
                from: ActivityState::Idle,
                to: ActivityState::Working,
                detail: viewport()
            }]
        );
    }

    #[test]
    fn chains_from_the_previous_state_and_records_same_state_changes() {
        let idle = TransitionDetail::ViewportIdle {
            snapshot: "done".to_owned(),
            idle_timeout: Duration::from_millis(2000),
        };
        let reclassified = TransitionDetail::Classification {
            snapshot: "done".to_owned(),
            reason: "prompt".to_owned(),
        };
        let mut store = ActivityStates::default();
        set(&mut store, "tab1", ActivityState::Working);
        store.set_tab_state(
            "tab1",
            ActivityState::Idle,
            idle.clone(),
            SystemTime::UNIX_EPOCH,
        );
        store.set_tab_state(
            "tab1",
            ActivityState::Idle,
            reclassified.clone(),
            SystemTime::UNIX_EPOCH,
        );
        let log = store.transitions("tab1");
        assert_eq!(
            log.iter().map(|t| (t.from, t.to)).collect::<Vec<_>>(),
            vec![
                (ActivityState::Idle, ActivityState::Working),
                (ActivityState::Working, ActivityState::Idle),
                (ActivityState::Idle, ActivityState::Idle),
            ]
        );
        assert_eq!(log[1].detail, idle);
        assert_eq!(log[2].detail, reclassified);
    }

    #[test]
    fn keeps_each_tabs_log_separate() {
        let mut store = ActivityStates::default();
        set(&mut store, "tab1", ActivityState::Working);
        set(&mut store, "tab2", ActivityState::Working);
        assert_eq!(store.transitions("tab1").len(), 1);
        assert_eq!(store.transitions("tab2").len(), 1);
    }

    #[test]
    fn drops_the_oldest_entries_past_the_cap() {
        let mut store = ActivityStates::default();
        for i in 0..MAX_TRANSITIONS + 2 {
            let detail = TransitionDetail::ViewportChanged {
                snapshot: i.to_string(),
            };
            store.set_tab_state(
                "tab1",
                ActivityState::Working,
                detail,
                SystemTime::UNIX_EPOCH,
            );
        }
        let log = store.transitions("tab1");
        assert_eq!(log.len(), MAX_TRANSITIONS);
        assert_eq!(log[0].detail.snapshot(), "2");
        assert_eq!(
            log[log.len() - 1].detail.snapshot(),
            (MAX_TRANSITIONS + 1).to_string()
        );
    }

    // `updates the state and the log in one notification`: one `&mut` call updates both; the
    // owning GPUI model notifies once per call.

    // --- removeTabState ---

    #[test]
    fn removes_state_and_transitions_for_a_tab() {
        let mut store = ActivityStates::default();
        set(&mut store, "tab1", ActivityState::Working);
        store.remove_tab_state("tab1");
        assert_eq!(store.get("tab1"), None);
        assert_eq!(store.transitions("tab1").len(), 0);
    }

    #[test]
    fn does_nothing_when_removing_non_existent_tab() {
        let mut store = ActivityStates::default();
        set(&mut store, "tab1", ActivityState::Working);
        store.remove_tab_state("tab2");
        assert_eq!(store.get("tab1"), Some(ActivityState::Working));
        assert_eq!(store.states.len(), 1);
    }

    // --- getWorkspaceState ---

    fn workspace(tabs: &[(&str, ActivityState)], ids: &[&str]) -> ActivityState {
        let mut store = ActivityStates::default();
        for (tab, state) in tabs {
            set(&mut store, tab, *state);
        }
        store.workspace_state(ids.iter().copied())
    }

    #[test]
    fn returns_idle_when_no_tabs() {
        assert_eq!(workspace(&[], &[]), ActivityState::Idle);
    }

    #[test]
    fn returns_idle_when_all_tabs_are_idle() {
        let tabs = [("tab1", ActivityState::Idle), ("tab2", ActivityState::Idle)];
        assert_eq!(workspace(&tabs, &["tab1", "tab2"]), ActivityState::Idle);
    }

    #[test]
    fn returns_working_when_any_tab_is_working() {
        let tabs = [
            ("tab1", ActivityState::Idle),
            ("tab2", ActivityState::Working),
        ];
        assert_eq!(workspace(&tabs, &["tab1", "tab2"]), ActivityState::Working);
    }

    #[test]
    fn returns_user_input_required_when_no_working_but_some_waiting() {
        let tabs = [
            ("tab1", ActivityState::Idle),
            ("tab2", ActivityState::UserInputRequired),
        ];
        assert_eq!(
            workspace(&tabs, &["tab1", "tab2"]),
            ActivityState::UserInputRequired
        );
    }

    #[test]
    fn returns_working_over_user_input_required() {
        let tabs = [
            ("tab1", ActivityState::Working),
            ("tab2", ActivityState::UserInputRequired),
        ];
        assert_eq!(workspace(&tabs, &["tab1", "tab2"]), ActivityState::Working);
    }

    #[test]
    fn returns_permission_request_over_safe_permission_requested() {
        let tabs = [
            ("tab1", ActivityState::SafePermissionRequested),
            ("tab2", ActivityState::PermissionRequest),
        ];
        assert_eq!(
            workspace(&tabs, &["tab1", "tab2"]),
            ActivityState::PermissionRequest
        );
    }

    #[test]
    fn returns_safe_permission_requested_over_user_input_required() {
        let tabs = [
            ("tab1", ActivityState::UserInputRequired),
            ("tab2", ActivityState::SafePermissionRequested),
        ];
        assert_eq!(
            workspace(&tabs, &["tab1", "tab2"]),
            ActivityState::SafePermissionRequested
        );
    }

    #[test]
    fn returns_user_input_required_over_application_error() {
        let tabs = [
            ("tab1", ActivityState::ApplicationError),
            ("tab2", ActivityState::UserInputRequired),
        ];
        assert_eq!(
            workspace(&tabs, &["tab1", "tab2"]),
            ActivityState::UserInputRequired
        );
    }

    #[test]
    fn returns_application_error_over_error() {
        let tabs = [
            ("tab1", ActivityState::Error),
            ("tab2", ActivityState::ApplicationError),
        ];
        assert_eq!(
            workspace(&tabs, &["tab1", "tab2"]),
            ActivityState::ApplicationError
        );
    }

    #[test]
    fn returns_error_over_completed_and_idle() {
        let tabs = [
            ("tab1", ActivityState::Completed),
            ("tab2", ActivityState::Error),
            ("tab3", ActivityState::Idle),
        ];
        assert_eq!(
            workspace(&tabs, &["tab1", "tab2", "tab3"]),
            ActivityState::Error
        );
    }

    #[test]
    fn returns_completed_over_idle() {
        let tabs = [
            ("tab1", ActivityState::Idle),
            ("tab2", ActivityState::Completed),
        ];
        assert_eq!(
            workspace(&tabs, &["tab1", "tab2"]),
            ActivityState::Completed
        );
    }

    #[test]
    fn ignores_tabs_not_in_the_provided_list() {
        let tabs = [
            ("tab1", ActivityState::Working),
            ("tab2", ActivityState::Idle),
            ("tab3", ActivityState::Working),
        ];
        assert_eq!(workspace(&tabs, &["tab1", "tab2"]), ActivityState::Working);
    }

    #[test]
    fn serializes_states_as_their_wire_values() {
        for state in ActivityState::ALL {
            assert_eq!(
                serde_json::to_value(state).unwrap(),
                serde_json::Value::String(state.as_str().to_owned())
            );
        }
    }
}
