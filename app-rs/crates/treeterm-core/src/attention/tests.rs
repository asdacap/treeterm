use std::time::SystemTime;

use super::*;
use crate::activity::TransitionDetail;

fn metadata(revision: &str, acknowledged_revision: &str) -> Metadata {
    let value =
        format!(r#"{{"revision":"{revision}","acknowledgedRevision":"{acknowledged_revision}"}}"#);
    Metadata::from([(WORKSPACE_ATTENTION_KEY.to_owned(), value)])
}

fn record(revision: &str) -> AttentionMutation {
    AttentionMutation {
        kind: AttentionMutationKind::Record,
        revision: revision.to_owned(),
    }
}

fn pending(state: &AttentionSyncState) -> Vec<AttentionMutation> {
    state.mutations().cloned().collect()
}

// === workspaceAttention.test.ts ===

#[test]
fn acknowledges_an_own_echo_while_retaining_mutations_queued_after_the_write() {
    let mut state = AttentionSyncState::default();
    assert!(state.queue(record("first")));
    state.register_write("own-sha");
    assert!(state.queue(record("second")));
    state
        .reconcile_write(&metadata("first", ""), "own-sha")
        .unwrap();
    assert_eq!(pending(&state), vec![record("second")]);
    assert!(state.writes.is_empty());
}

#[test]
fn reconciles_a_coalesced_cross_window_acknowledgement_and_cleans_skipped_write_hashes() {
    let mut state = AttentionSyncState::default();
    assert!(state.queue(record("first")));
    assert!(state.queue(AttentionMutation::acknowledge("first")));
    state.register_write("skipped");
    assert!(state.queue(record("second")));
    state.register_write("pending");
    state
        .reconcile_write(&metadata("first", "first"), "external")
        .unwrap();
    assert_eq!(pending(&state), vec![record("second")]);
    assert_eq!(state.writes.keys().collect::<Vec<_>>(), vec!["pending"]);
}

#[test]
fn acknowledges_superseded_mutations_included_in_the_committed_body_by_hash() {
    let mut state = AttentionSyncState::default();
    assert!(state.queue(record("first")));
    assert!(state.queue(record("second")));
    state.register_write("own");
    state
        .reconcile_write(&metadata("second", ""), "own")
        .unwrap();
    assert_eq!(pending(&state), vec![]);
    assert!(state.writes.is_empty());
}

#[test]
fn rejects_invalid_attention_before_changing_queued_mutations() {
    let mut state = AttentionSyncState::default();
    assert!(state.queue(record("pending")));
    let invalid = Metadata::from([(WORKSPACE_ATTENTION_KEY.to_owned(), "{}".to_owned())]);
    assert!(matches!(
        state.reconcile_write(&invalid, "invalid"),
        Err(AttentionError::Invalid(_))
    ));
    assert_eq!(pending(&state), vec![record("pending")]);
}

#[test]
fn keeps_local_metadata_while_publishing_only_watched_attention() {
    let mut current = metadata("old", "");
    current.insert("displayName".to_owned(), "local name".to_owned());
    let mut watched = metadata("new", "");
    watched.insert("displayName".to_owned(), "old name".to_owned());
    let mut expected = metadata("new", "");
    expected.insert("displayName".to_owned(), "local name".to_owned());
    assert_eq!(*with_watched_attention(&current, &watched), expected);
}

#[test]
fn removes_attention_absent_from_the_watched_body_and_preserves_identity_when_unchanged() {
    let mut current = metadata("old", "");
    current.insert("displayName".to_owned(), "local name".to_owned());
    let local_only = Metadata::from([("displayName".to_owned(), "local name".to_owned())]);
    assert_eq!(
        *with_watched_attention(&current, &Metadata::new()),
        local_only
    );
    assert!(matches!(
        with_watched_attention(&current, &metadata("old", "")),
        Cow::Borrowed(_)
    ));
    let empty = Metadata::new();
    assert!(matches!(
        with_watched_attention(&empty, &Metadata::new()),
        Cow::Borrowed(_)
    ));
}

// --- Rust additions: the metadata format shared with Electron ---

#[test]
fn writes_attention_exactly_as_json_stringify() {
    let base = Metadata::from([("displayName".to_owned(), "x".to_owned())]);
    let updated = apply_attention_mutations(&base, &[record("r\"1")]).unwrap();
    assert_eq!(
        updated[WORKSPACE_ATTENTION_KEY],
        r#"{"revision":"r\"1","acknowledgedRevision":""}"#
    );
    assert_eq!(updated["displayName"], "x");
}

#[test]
fn applies_mutations_in_order_and_acknowledges_only_the_current_revision() {
    let base = metadata("a", "");
    let stale_ack =
        apply_attention_mutations(&base, &[record("b"), AttentionMutation::acknowledge("a")])
            .unwrap();
    assert_eq!(
        workspace_attention(&stale_ack).unwrap(),
        WorkspaceAttention {
            revision: "b".into(),
            acknowledged_revision: "".into()
        }
    );
    let acked =
        apply_attention_mutations(&base, &[record("b"), AttentionMutation::acknowledge("b")])
            .unwrap();
    assert!(!has_unread_workspace_attention(&acked).unwrap());
    assert!(has_unread_workspace_attention(&stale_ack).unwrap());
}

#[test]
fn leaves_metadata_untouched_without_mutations_even_if_invalid() {
    let invalid = Metadata::from([(WORKSPACE_ATTENTION_KEY.to_owned(), "nope".to_owned())]);
    assert!(matches!(
        apply_attention_mutations(&invalid, &[]),
        Ok(Cow::Borrowed(_))
    ));
    assert!(apply_attention_mutations(&invalid, &[record("x")]).is_err());
}

#[test]
fn reads_missing_attention_as_read_and_ignores_unknown_keys() {
    assert!(!has_unread_workspace_attention(&Metadata::new()).unwrap());
    let extra = Metadata::from([(
        WORKSPACE_ATTENTION_KEY.to_owned(),
        r#"{"revision":"a","acknowledgedRevision":"","other":1}"#.to_owned(),
    )]);
    assert!(has_unread_workspace_attention(&extra).unwrap());
}

#[test]
fn queues_each_pending_mutation_once_and_forgets_discarded_writes() {
    let mut state = AttentionSyncState::default();
    assert!(state.queue(AttentionMutation::acknowledge("a")));
    assert!(!state.queue(AttentionMutation::acknowledge("a")));
    state.register_write("sha");
    state.discard_write("sha");
    assert!(state.writes.is_empty());
    assert!(state.has_pending());
    let empty = &mut AttentionSyncState::default();
    empty.register_write("sha");
    assert!(empty.writes.is_empty());
}

// === workspaceAttentionCoordinator.test.ts ===

const SESSION: &str = "session-1";

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    None,
    Session(&'static str),
    Workspace(&'static str, &'static str),
}

struct Workspace {
    id: &'static str,
    tabs: Vec<&'static str>,
    metadata: Metadata,
}

fn workspace(tabs: &[&'static str], metadata: Metadata) -> Workspace {
    Workspace {
        id: "ws-1",
        tabs: tabs.to_vec(),
        metadata,
    }
}

fn plain(tabs: &[&'static str]) -> Workspace {
    workspace(tabs, Metadata::new())
}

/// The coordinator's subscriptions, reduced to explicit `observe` calls: activity changes
/// observe with `activity_changed`, session and navigation changes without.
struct Harness {
    tracker: Option<AttentionTracker>,
    disposed: bool,
    connected: bool,
    restoring: bool,
    workspaces: Vec<Workspace>,
    activity: ActivityStates,
    view: View,
    sound: bool,
    /// Re-observe the session from inside a record, as a synchronous session publication.
    reenter_on_record: bool,
    records: Vec<String>,
    acknowledgements: Vec<(String, String)>,
    dings: usize,
}

impl Harness {
    fn setup(initial: Workspace) -> Self {
        Self::setup_with(initial, ActivityStates::default(), View::None)
    }

    fn setup_with(initial: Workspace, activity: ActivityStates, view: View) -> Self {
        let mut h = Self {
            tracker: None,
            disposed: false,
            connected: true,
            restoring: false,
            workspaces: vec![initial],
            activity,
            view,
            sound: true,
            reenter_on_record: false,
            records: Vec::new(),
            acknowledgements: Vec::new(),
            dings: 0,
        };
        h.set_sessions(true);
        h
    }

    fn observe(&mut self, activity_changed: bool) {
        let Some(tracker) = self.tracker.as_mut() else {
            return;
        };
        let observed: Vec<ObservedWorkspace<'_>> = self
            .workspaces
            .iter()
            .map(|ws| ObservedWorkspace {
                id: ws.id,
                tab_ids: ws.tabs.clone(),
                metadata: &ws.metadata,
            })
            .collect();
        let status = if self.connected && !self.restoring {
            SessionStatus::Ready(&observed)
        } else {
            SessionStatus::Unavailable
        };
        let view = match self.view {
            View::Workspace(session_id, workspace_id) => ActiveView::Workspace {
                session_id,
                workspace_id,
            },
            View::None | View::Session(_) => ActiveView::Other,
        };
        let session = ObservedSession {
            session_id: SESSION,
            status,
        };
        let actions = tracker
            .observe(session, &self.activity, view, activity_changed)
            .unwrap();
        for action in actions {
            match action {
                AttentionAction::Record { workspace_id } => {
                    self.records.push(workspace_id);
                    if self.sound {
                        self.dings += 1;
                    }
                    if self.reenter_on_record {
                        self.observe(false);
                    }
                }
                AttentionAction::Acknowledge {
                    workspace_id,
                    revision,
                } => {
                    self.acknowledgements.push((workspace_id, revision));
                }
            }
        }
    }

    fn set_tab(&mut self, state: ActivityState, tab: &str) {
        let detail = TransitionDetail::ViewportChanged {
            snapshot: String::new(),
        };
        self.activity
            .set_tab_state(tab, state, detail, SystemTime::UNIX_EPOCH);
        self.observe(true);
    }

    fn tab(&mut self, state: ActivityState) {
        self.set_tab(state, "tab-1");
    }

    fn remove_tab_state(&mut self, tab: &str) {
        self.activity.remove_tab_state(tab);
        self.observe(true);
    }

    fn update_workspace(&mut self, data: Workspace) {
        self.workspaces = vec![data];
        self.observe(false);
    }

    fn set_workspaces(&mut self, data: Vec<Workspace>) {
        self.workspaces = data;
        self.observe(false);
    }

    fn navigate(&mut self, view: View) {
        self.view = view;
        self.observe(false);
    }

    fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
        self.observe(false);
    }

    fn set_restoring(&mut self, restoring: bool) {
        self.restoring = restoring;
        self.observe(false);
    }

    /// `setSessions([session])` / `setSessions([])`.
    fn set_sessions(&mut self, present: bool) {
        if self.disposed {
            return;
        }
        match (present, self.tracker.is_some()) {
            (true, false) => {
                self.tracker = Some(AttentionTracker::default());
                self.observe(false);
            }
            (false, true) => self.tracker = None,
            (true, true) | (false, false) => {}
        }
    }

    fn dispose(&mut self) {
        self.disposed = true;
        self.tracker = None;
    }
}

fn attention(revision: &str, acknowledged: &str) -> Metadata {
    metadata(revision, acknowledged)
}

fn ack(revision: &str) -> (String, String) {
    ("ws-1".to_owned(), revision.to_owned())
}

#[test]
fn records_and_dings_once_for_working_to_any_other_state() {
    for destination in [
        ActivityState::Idle,
        ActivityState::Completed,
        ActivityState::Error,
        ActivityState::PermissionRequest,
        ActivityState::SafePermissionRequested,
        ActivityState::UserInputRequired,
    ] {
        let mut h = Harness::setup(plain(&["tab-1"]));
        h.tab(ActivityState::Working);
        h.tab(destination);
        h.tab(destination);
        assert_eq!(h.records, vec!["ws-1"], "{destination:?}");
        assert_eq!(h.dings, 1, "{destination:?}");
    }
}

#[test]
fn waits_until_the_last_working_tab_stops_and_ignores_subsequent_nonworking_changes() {
    let mut h = Harness::setup(plain(&["tab-1", "tab-2"]));
    h.tab(ActivityState::Working);
    h.set_tab(ActivityState::Working, "tab-2");
    h.tab(ActivityState::Completed);
    assert!(h.records.is_empty());
    h.set_tab(ActivityState::PermissionRequest, "tab-2");
    h.tab(ActivityState::Idle);
    assert_eq!(h.records.len(), 1);
}

#[test]
fn records_while_muted_and_reads_the_current_setting_for_the_next_transition() {
    let mut h = Harness::setup(plain(&["tab-1"]));
    h.sound = false;
    h.tab(ActivityState::Working);
    h.tab(ActivityState::Idle);
    assert_eq!(h.records.len(), 1);
    assert_eq!(h.dings, 0);
    h.sound = true;
    h.tab(ActivityState::Working);
    h.tab(ActivityState::Completed);
    assert_eq!(h.dings, 1);
}

#[test]
fn does_not_record_or_ding_in_the_active_view_acknowledges_only_watched_unread_metadata() {
    let mut h = Harness::setup_with(
        plain(&["tab-1"]),
        ActivityStates::default(),
        View::Workspace(SESSION, "ws-1"),
    );
    h.tab(ActivityState::Working);
    h.tab(ActivityState::Completed);
    assert!(h.records.is_empty());
    assert_eq!(h.dings, 0);
    assert!(h.acknowledgements.is_empty());
    h.update_workspace(workspace(&["tab-1"], attention("rev-1", "")));
    assert_eq!(h.acknowledgements, vec![ack("rev-1")]);
    h.tab(ActivityState::Idle);
    assert_eq!(h.acknowledgements.len(), 1);
    h.update_workspace(workspace(&["tab-1"], attention("rev-2", "")));
    assert_eq!(h.acknowledgements.last(), Some(&ack("rev-2")));
}

#[test]
fn does_not_record_or_ding_while_the_workspace_is_already_unread() {
    let mut h = Harness::setup(workspace(&["tab-1"], attention("rev-1", "")));
    h.tab(ActivityState::Working);
    h.tab(ActivityState::Idle);
    assert!(h.records.is_empty());
    assert_eq!(h.dings, 0);
    h.update_workspace(workspace(&["tab-1"], attention("rev-1", "rev-1")));
    h.tab(ActivityState::Working);
    h.tab(ActivityState::Idle);
    assert_eq!(h.records, vec!["ws-1"]);
    assert_eq!(h.dings, 1);
}

#[test]
fn dings_for_a_background_workspace_when_another_workspace_is_viewed() {
    let mut h = Harness::setup_with(
        plain(&["tab-1"]),
        ActivityStates::default(),
        View::Workspace(SESSION, "other"),
    );
    h.tab(ActivityState::Working);
    h.tab(ActivityState::Idle);
    assert_eq!(h.records, vec!["ws-1"]);
    assert_eq!(h.dings, 1);
}

#[test]
fn requires_the_exact_workspace_view_not_a_session_another_workspace_or_another_session() {
    let mut h = Harness::setup(workspace(&["tab-1"], attention("rev-1", "")));
    h.navigate(View::Session(SESSION));
    h.navigate(View::Workspace(SESSION, "other"));
    h.navigate(View::Workspace("other", "ws-1"));
    assert!(h.acknowledgements.is_empty());
    h.navigate(View::Workspace(SESSION, "ws-1"));
    assert_eq!(h.acknowledgements, vec![ack("rev-1")]);
    assert_eq!(h.dings, 0);
}

#[test]
fn baselines_initial_snapshots_and_session_workspace_membership_changes() {
    let mut activity = ActivityStates::default();
    activity.set_tab_state(
        "tab-1",
        ActivityState::Working,
        TransitionDetail::ViewportChanged {
            snapshot: String::new(),
        },
        SystemTime::UNIX_EPOCH,
    );
    let mut h = Harness::setup_with(plain(&["tab-1"]), activity, View::None);
    assert!(h.records.is_empty());
    h.update_workspace(plain(&[]));
    h.update_workspace(plain(&["tab-2"]));
    h.set_tab(ActivityState::Idle, "tab-2");
    assert!(h.records.is_empty());
    h.set_sessions(false);
    h.tab(ActivityState::Completed);
    h.set_sessions(true);
    assert!(h.records.is_empty());
    h.update_workspace(plain(&["tab-1"]));
    h.tab(ActivityState::Working);
    h.tab(ActivityState::Completed);
    assert_eq!(h.records.len(), 1);
}

#[test]
fn rebaselines_activity_state_cleanup_without_ringing() {
    let mut h = Harness::setup(plain(&["tab-1"]));
    h.tab(ActivityState::Working);
    h.remove_tab_state("tab-1");
    h.tab(ActivityState::Idle);
    assert!(h.records.is_empty());
    h.tab(ActivityState::Working);
    h.tab(ActivityState::Completed);
    assert_eq!(h.records.len(), 1);
}

#[test]
fn ignores_restoration_and_disconnected_changes_and_baselines_reconnect() {
    let mut h = Harness::setup(plain(&["tab-1"]));
    h.tab(ActivityState::Working);
    h.set_connected(false);
    h.tab(ActivityState::Completed);
    h.set_connected(true);
    h.set_restoring(true);
    h.tab(ActivityState::Working);
    h.tab(ActivityState::Completed);
    h.set_restoring(false);
    assert!(h.records.is_empty());
    h.tab(ActivityState::Working);
    h.tab(ActivityState::Idle);
    assert_eq!(h.records.len(), 1);
}

#[test]
fn disposes_all_subscriptions_including_removed_sessions() {
    let mut h = Harness::setup(plain(&["tab-1"]));
    h.set_sessions(false);
    h.navigate(View::Workspace(SESSION, "ws-1"));
    h.update_workspace(workspace(&["tab-1"], attention("rev-1", "")));
    assert!(h.acknowledgements.is_empty());
    h.dispose();
    h.set_sessions(true);
    h.tab(ActivityState::Working);
    h.tab(ActivityState::Completed);
    h.update_workspace(workspace(&["tab-1"], attention("rev-2", "")));
    assert!(h.records.is_empty());
    assert!(h.acknowledgements.is_empty());
}

#[test]
fn commits_all_baselines_before_synchronous_record_publications_can_re_enter() {
    let mut h = Harness::setup(plain(&["tab-1"]));
    let other = Workspace {
        id: "ws-2",
        tabs: vec!["tab-2"],
        metadata: Metadata::new(),
    };
    h.set_workspaces(vec![plain(&["tab-1"]), other]);
    h.reenter_on_record = true;
    let detail = || TransitionDetail::ViewportChanged {
        snapshot: String::new(),
    };
    for state in [ActivityState::Working, ActivityState::Completed] {
        // Both tabs change in one store update.
        h.activity
            .set_tab_state("tab-1", state, detail(), SystemTime::UNIX_EPOCH);
        h.activity
            .set_tab_state("tab-2", state, detail(), SystemTime::UNIX_EPOCH);
        h.observe(true);
    }
    assert_eq!(h.records, vec!["ws-1", "ws-2"]);
    assert_eq!(h.dings, 2);
}

#[test]
fn removes_baselines_for_unavailable_workspaces_and_handles_operation_errors_as_loaded() {
    // A loading workspace is not part of the observed session; one whose last operation
    // failed is (the owner passes it like a loaded one).
    let mut h = Harness::setup(plain(&["tab-1"]));
    h.tab(ActivityState::Working);
    h.set_workspaces(Vec::new());
    h.tab(ActivityState::Completed);
    h.update_workspace(plain(&["tab-1"]));
    assert!(h.records.is_empty());
    h.update_workspace(plain(&["tab-1"]));
    h.tab(ActivityState::Working);
    h.tab(ActivityState::Completed);
    assert_eq!(h.records.len(), 1);
}

#[test]
fn acknowledges_initial_unread_metadata_but_never_already_acknowledged_metadata() {
    let mut h = Harness::setup_with(
        workspace(&["tab-1"], attention("rev-1", "")),
        ActivityStates::default(),
        View::Workspace(SESSION, "ws-1"),
    );
    assert_eq!(h.acknowledgements, vec![ack("rev-1")]);
    h.update_workspace(workspace(&["tab-1"], attention("rev-1", "rev-1")));
    h.update_workspace(workspace(&["tab-1"], attention("rev-2", "rev-2")));
    assert_eq!(h.acknowledgements.len(), 1);
}

// `reports rejected sound playback without undoing attention`: playing the ding is the
// owner's (the tracker only says Record); see `AttentionAction::Record`.

#[test]
fn surfaces_invalid_attention_metadata() {
    let invalid = Metadata::from([(WORKSPACE_ATTENTION_KEY.to_owned(), "{}".to_owned())]);
    let workspaces = [ObservedWorkspace {
        id: "ws-1",
        tab_ids: vec!["tab-1"],
        metadata: &invalid,
    }];
    let session = ObservedSession {
        session_id: SESSION,
        status: SessionStatus::Ready(&workspaces),
    };
    let result = AttentionTracker::default().observe(
        session,
        &ActivityStates::default(),
        ActiveView::Other,
        false,
    );
    assert!(matches!(result, Err(AttentionError::Invalid(_))));
}
