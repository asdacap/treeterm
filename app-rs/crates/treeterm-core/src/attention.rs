//! Unread markers on workspaces: a revision bumped when background work finishes and
//! acknowledged when the workspace is viewed. Port of `store/workspaceAttention.ts` (the
//! metadata format and its write reconciliation) and `store/workspaceAttentionCoordinator.ts`
//! (when to record and acknowledge, as [`AttentionTracker`]).
//!
//! The marker lives in workspace `metadata[workspaceAttention]` as
//! `{"revision":"…","acknowledgedRevision":"…"}`, shared through the workspace files with the
//! Electron app, so the JSON is written exactly as `JSON.stringify` writes it.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::activity::{ActivityState, ActivityStates};

pub const WORKSPACE_ATTENTION_KEY: &str = "workspaceAttention";

/// Workspace metadata as stored in the workspace file.
pub type Metadata = BTreeMap<String, String>;

/// Unread while `revision != acknowledged_revision`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceAttention {
    pub revision: String,
    pub acknowledged_revision: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AttentionError {
    /// `metadata.workspaceAttention` is not `{revision: string, acknowledgedRevision: string}`.
    #[error("invalid workspace attention metadata: {0}")]
    Invalid(#[from] serde_json::Error),
}

/// The workspace's marker; none recorded yet reads as read (both revisions empty).
pub fn workspace_attention(metadata: &Metadata) -> Result<WorkspaceAttention, AttentionError> {
    match metadata.get(WORKSPACE_ATTENTION_KEY) {
        None => Ok(WorkspaceAttention::default()),
        Some(raw) => Ok(serde_json::from_str(raw)?),
    }
}

pub fn has_unread_workspace_attention(metadata: &Metadata) -> Result<bool, AttentionError> {
    let attention = workspace_attention(metadata)?;
    Ok(attention.revision != attention.acknowledged_revision)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttentionMutationKind {
    /// Mark unread with a fresh revision.
    Record,
    /// Mark read, if the revision is still the one that was seen.
    Acknowledge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionMutation {
    pub kind: AttentionMutationKind,
    pub revision: String,
}

impl AttentionMutation {
    /// Record with a fresh random revision (`crypto.randomUUID()`).
    pub fn record() -> Self {
        Self {
            kind: AttentionMutationKind::Record,
            revision: uuid::Uuid::new_v4().to_string(),
        }
    }

    pub fn acknowledge(observed_revision: impl Into<String>) -> Self {
        Self {
            kind: AttentionMutationKind::Acknowledge,
            revision: observed_revision.into(),
        }
    }
}

/// `metadata` with `mutations` applied in order; borrowed back unchanged when there are none.
pub fn apply_attention_mutations<'a, 'm>(
    metadata: &'a Metadata,
    mutations: impl IntoIterator<Item = &'m AttentionMutation>,
) -> Result<Cow<'a, Metadata>, AttentionError> {
    let mut mutations = mutations.into_iter().peekable();
    if mutations.peek().is_none() {
        return Ok(Cow::Borrowed(metadata));
    }
    let mut attention = workspace_attention(metadata)?;
    for mutation in mutations {
        match mutation.kind {
            AttentionMutationKind::Record => attention.revision.clone_from(&mutation.revision),
            AttentionMutationKind::Acknowledge => {
                if attention.revision == mutation.revision {
                    attention
                        .acknowledged_revision
                        .clone_from(&mutation.revision);
                }
            }
        }
    }
    let mut updated = metadata.clone();
    updated.insert(WORKSPACE_ATTENTION_KEY.to_owned(), serialize(&attention));
    Ok(Cow::Owned(updated))
}

fn serialize(attention: &WorkspaceAttention) -> String {
    // Two string fields cannot fail to serialize.
    serde_json::to_string(attention).expect("attention serializes")
}

/// Identity of one queued mutation. Equal mutations queued at different times are different
/// entries (TS compared the objects by reference).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MutationId(u64);

/// Attention owns its revision acknowledgements; file transport supplies only the committed
/// body hash. Writes are registered before `WriteFile` so an early watch echo can acknowledge
/// them, and discarded by transport when a write fails.
#[derive(Debug, Default)]
pub struct AttentionSyncState {
    mutations: Vec<(MutationId, AttentionMutation)>,
    /// Body sha → the mutations that body carried, in registration order.
    writes: IndexMap<String, Vec<MutationId>>,
    next_id: u64,
}

impl AttentionSyncState {
    /// Queue `mutation` unless an equal one is already pending (`queueAttention`). Returns
    /// whether it was queued.
    pub fn queue(&mut self, mutation: AttentionMutation) -> bool {
        if self
            .mutations
            .iter()
            .any(|(_, pending)| *pending == mutation)
        {
            return false;
        }
        let id = MutationId(self.next_id);
        self.next_id += 1;
        self.mutations.push((id, mutation));
        true
    }

    /// Mutations not yet seen committed, oldest first.
    pub fn mutations(&self) -> impl Iterator<Item = &AttentionMutation> {
        self.mutations.iter().map(|(_, mutation)| mutation)
    }

    pub fn has_pending(&self) -> bool {
        !self.mutations.is_empty()
    }

    /// Before writing a body that carries every pending mutation: remember them under its
    /// sha. Nothing is registered when none are pending.
    pub fn register_write(&mut self, sha: &str) {
        if self.mutations.is_empty() {
            return;
        }
        let ids = self.mutations.iter().map(|(id, _)| *id).collect();
        self.writes.insert(sha.to_owned(), ids);
    }

    /// The write of `sha` did not land.
    pub fn discard_write(&mut self, sha: &str) {
        self.writes.shift_remove(sha);
    }

    /// The watch delivered a body with `metadata` and hash `sha` (`reconcileAttentionWrite`).
    /// Invalid attention is rejected before anything changes.
    pub fn reconcile_write(
        &mut self,
        metadata: &Metadata,
        sha: &str,
    ) -> Result<(), AttentionError> {
        let watched = workspace_attention(metadata)?;
        // A coalesced watch can skip our body and contain another window's acknowledgement.
        self.mutations.retain(|(_, mutation)| match mutation.kind {
            AttentionMutationKind::Record => mutation.revision != watched.revision,
            AttentionMutationKind::Acknowledge => {
                mutation.revision != watched.acknowledged_revision
            }
        });
        if let Some(committed) = self.writes.shift_remove(sha) {
            self.mutations.retain(|(id, _)| !committed.contains(id));
        }
        let pending: HashSet<MutationId> = self.mutations.iter().map(|(id, _)| *id).collect();
        self.writes
            .retain(|_, ids| ids.iter().any(|id| pending.contains(id)));
        Ok(())
    }
}

/// Own echoes must publish watched attention without replacing optimistic tab or other
/// metadata edits made while the write was in flight. Borrowed back when already equal.
pub fn with_watched_attention<'a>(current: &'a Metadata, watched: &Metadata) -> Cow<'a, Metadata> {
    let value = watched.get(WORKSPACE_ATTENTION_KEY);
    if current.get(WORKSPACE_ATTENTION_KEY) == value {
        return Cow::Borrowed(current);
    }
    let mut metadata = current.clone();
    match value {
        Some(value) => metadata.insert(WORKSPACE_ATTENTION_KEY.to_owned(), value.clone()),
        None => metadata.remove(WORKSPACE_ATTENTION_KEY),
    };
    Cow::Owned(metadata)
}

// === coordinator ============================================================================

/// What the app shows, as far as attention cares (`navigation.activeView`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveView<'a> {
    Workspace {
        session_id: &'a str,
        workspace_id: &'a str,
    },
    /// Anything else: a session overview, settings, nothing.
    Other,
}

/// One session as the tracker sees it.
#[derive(Debug, Clone, Copy)]
pub struct ObservedSession<'a> {
    pub session_id: &'a str,
    pub status: SessionStatus<'a>,
}

#[derive(Debug, Clone, Copy)]
pub enum SessionStatus<'a> {
    /// Connected and not restoring: the loaded workspaces (including those whose last
    /// operation failed). Loading or failed-to-load workspaces are left out.
    Ready(&'a [ObservedWorkspace<'a>]),
    /// Disconnected or restoring: nothing is tracked until it is ready again.
    Unavailable,
}

#[derive(Debug, Clone)]
pub struct ObservedWorkspace<'a> {
    pub id: &'a str,
    pub tab_ids: Vec<&'a str>,
    pub metadata: &'a Metadata,
}

/// What the owner must do after an observation, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttentionAction {
    /// `session.recordWorkspaceAttention(id)`, then ding if sound is enabled.
    Record { workspace_id: String },
    /// `session.acknowledgeWorkspaceAttention(id, revision)`.
    Acknowledge {
        workspace_id: String,
        revision: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Baseline {
    /// Sorted tab ids, each with whether it currently has an activity state.
    membership: Vec<(String, bool)>,
    activity: ActivityState,
}

/// Attention decisions for one session (`createWorkspaceAttentionCoordinator`'s per-session
/// `observe`). The owner calls [`Self::observe`] whenever the session, the activity states
/// (`activity_changed = true`) or the active view change, and drops the tracker with the
/// session.
#[derive(Debug, Default)]
pub struct AttentionTracker {
    baselines: HashMap<String, Baseline>,
    acknowledged: HashMap<String, String>,
}

impl AttentionTracker {
    pub fn observe(
        &mut self,
        session: ObservedSession<'_>,
        activity: &ActivityStates,
        view: ActiveView<'_>,
        activity_changed: bool,
    ) -> Result<Vec<AttentionAction>, AttentionError> {
        let SessionStatus::Ready(workspaces) = session.status else {
            self.baselines.clear();
            self.acknowledged.clear();
            return Ok(Vec::new());
        };
        let mut present = HashSet::new();
        let mut actions = Vec::new();
        for workspace in workspaces {
            let id = workspace.id;
            present.insert(id);
            let mut tab_ids = workspace.tab_ids.clone();
            tab_ids.sort_unstable();
            // State removal is cleanup, not completion. Include presence as well as tab
            // membership so cleanup before the workspace watch arrives also rebaselines.
            let membership: Vec<(String, bool)> = tab_ids
                .iter()
                .map(|tab| ((*tab).to_owned(), activity.contains(tab)))
                .collect();
            let current = activity.workspace_state(tab_ids.iter().copied());
            let previous = self.baselines.insert(
                id.to_owned(),
                Baseline {
                    membership: membership.clone(),
                    activity: current,
                },
            );
            let is_active_view = view
                == ActiveView::Workspace {
                    session_id: session.session_id,
                    workspace_id: id,
                };
            let attention = workspace_attention(workspace.metadata)?;
            let unread = attention.revision != attention.acknowledged_revision;
            // Finishing while watched needs no marker (it would be acknowledged in the next
            // observe) and finishing while already unread must not re-ring or bump a revision
            // another window may be acknowledging.
            let finished = previous.is_some_and(|previous| {
                previous.membership == membership && previous.activity == ActivityState::Working
            }) && current != ActivityState::Working;
            if activity_changed && finished && !is_active_view && !unread {
                actions.push(AttentionAction::Record {
                    workspace_id: id.to_owned(),
                });
            }
            if !unread {
                self.acknowledged.remove(id);
                continue;
            }
            if !is_active_view {
                continue;
            }
            if self.acknowledged.get(id) == Some(&attention.revision) {
                continue;
            }
            self.acknowledged
                .insert(id.to_owned(), attention.revision.clone());
            actions.push(AttentionAction::Acknowledge {
                workspace_id: id.to_owned(),
                revision: attention.revision,
            });
        }
        self.baselines.retain(|id, _| present.contains(id.as_str()));
        self.acknowledged
            .retain(|id, _| present.contains(id.as_str()));
        Ok(actions)
    }
}

#[cfg(test)]
mod tests;
