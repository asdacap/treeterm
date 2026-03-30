use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::Rng;
use tokio::sync::{Mutex, mpsc};
use treeterm_proto::treeterm::*;

use crate::pty_manager::PtyManager;

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn random_hex_7() -> String {
    format!("{:07x}", rand::rng().random_range(0..0x10000000u32))
}

fn generate_session_id() -> String {
    format!("session-{}-{}", now_millis(), random_hex_7())
}

fn generate_workspace_id() -> String {
    format!("ws-{}-{}", now_millis(), random_hex_7())
}

struct SessionWatcher {
    listener_id: String,
    tx: mpsc::Sender<Result<SessionWatchEvent, tonic::Status>>,
}

struct Inner {
    session: Session,
    watchers: Vec<SessionWatcher>,
}

/// One session per daemon. Multiple sessions = multiple daemon instances.
#[derive(Clone)]
pub struct SessionStore {
    inner: Arc<Mutex<Inner>>,
    pty_manager: PtyManager,
}

impl SessionStore {
    pub fn new() -> Self {
        let now = now_millis();
        let session = Session {
            id: generate_session_id(),
            workspaces: vec![],
            created_at: now,
            last_activity: now,
            version: 1,
        };
        tracing::info!(session_id = %session.id, "session created");
        Self {
            inner: Arc::new(Mutex::new(Inner {
                session,
                watchers: Vec::new(),
            })),
            pty_manager: PtyManager::new(),
        }
    }

    pub fn pty_manager(&self) -> &PtyManager {
        &self.pty_manager
    }

    /// Return a snapshot of the current session.
    pub async fn session(&self) -> Session {
        self.inner.lock().await.session.clone()
    }

    /// Update the session's workspaces. Returns `(session, accepted)`.
    /// If `expected_version` is provided and doesn't match the current version,
    /// the update is rejected and the current session is returned unchanged.
    pub async fn update_session(
        &self,
        workspaces: Vec<Workspace>,
        expected_version: Option<u64>,
    ) -> (Session, bool) {
        let mut inner = self.inner.lock().await;
        let existing = &inner.session;

        // Version mismatch → reject, return current state
        if let Some(ev) = expected_version {
            if ev != existing.version {
                tracing::info!(
                    expected_version = ev,
                    actual_version = existing.version,
                    "session update rejected: version mismatch"
                );
                return (existing.clone(), false);
            }
        }

        let now = now_millis();
        let old_workspaces = &existing.workspaces;
        let full_workspaces = workspaces
            .into_iter()
            .map(|mut ws| {
                let prev = old_workspaces
                    .iter()
                    .find(|w| (!ws.id.is_empty() && w.id == ws.id) || w.path == ws.path);
                if ws.id.is_empty() {
                    ws.id = prev.map(|p| p.id.clone()).unwrap_or_else(generate_workspace_id);
                }
                ws.created_at = prev.map(|p| p.created_at).unwrap_or(now);
                ws.last_activity = now;
                ws
            })
            .collect();

        let updated = Session {
            id: existing.id.clone(),
            workspaces: full_workspaces,
            created_at: existing.created_at,
            last_activity: now,
            version: existing.version + 1,
        };

        tracing::info!(version = updated.version, "session updated");
        inner.session = updated.clone();
        (updated, true)
    }

    pub async fn add_watcher(
        &self,
        listener_id: String,
        tx: mpsc::Sender<Result<SessionWatchEvent, tonic::Status>>,
    ) {
        self.inner.lock().await.watchers.push(SessionWatcher {
            listener_id,
            tx,
        });
    }

    pub async fn remove_watcher(&self, listener_id: &str) {
        self.inner
            .lock()
            .await
            .watchers
            .retain(|w| w.listener_id != listener_id);
    }

    pub async fn broadcast_update(
        &self,
        session: &Session,
        sender_id: &str,
    ) {
        let mut inner = self.inner.lock().await;
        let event = SessionWatchEvent {
            session: Some(session.clone()),
            sender_id: sender_id.to_string(),
        };

        // Send to all watchers except the sender; remove those with closed channels
        inner.watchers.retain(|w| {
            if w.listener_id == sender_id {
                return true; // keep but don't send
            }
            w.tx.try_send(Ok(event.clone())).is_ok()
        });
    }

    /// Spawn a background task that broadcasts current session state to all watchers every 15s.
    /// Ensures clients eventually converge even if they miss a watch event.
    pub fn start_heartbeat(&self) {
        let store = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                let session = store.session().await;
                // Empty sender_id → sends to ALL watchers
                store.broadcast_update(&session, "").await;
            }
        });
    }

    /// Shutdown: kill all PTYs owned by this session.
    pub async fn shutdown(&self) {
        self.pty_manager.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn new_creates_session_with_id_and_version() {
        let store = SessionStore::new();
        let session = store.session().await;

        assert!(session.id.starts_with("session-"));
        assert_eq!(session.version, 1);
        assert!(session.created_at > 0);
        assert_eq!(session.workspaces.len(), 0);
    }

    #[tokio::test]
    async fn update_session_increments_version() {
        let store = SessionStore::new();

        let (updated, accepted) = store.update_session(vec![], None).await;
        assert!(accepted);
        assert_eq!(updated.version, 2);
    }

    #[tokio::test]
    async fn update_session_version_match_accepted() {
        let store = SessionStore::new();

        let (updated, accepted) = store.update_session(vec![], Some(1)).await;
        assert!(accepted);
        assert_eq!(updated.version, 2);
    }

    #[tokio::test]
    async fn update_session_version_mismatch_rejected() {
        let store = SessionStore::new();

        let (returned, accepted) = store.update_session(vec![], Some(999)).await;
        assert!(!accepted);
        assert_eq!(returned.version, 1); // unchanged
    }

    #[tokio::test]
    async fn update_preserves_existing_workspace_by_id() {
        let store = SessionStore::new();
        let ws = Workspace {
            id: "ws-1".into(),
            path: "/a".into(),
            ..Default::default()
        };
        store.update_session(vec![ws], None).await;
        let session = store.session().await;
        let original_created_at = session.workspaces[0].created_at;

        let updated_ws = Workspace {
            id: "ws-1".into(),
            path: "/a-updated".into(),
            ..Default::default()
        };
        let (updated, _) = store.update_session(vec![updated_ws], None).await;

        assert_eq!(updated.workspaces[0].id, "ws-1");
        assert_eq!(updated.workspaces[0].created_at, original_created_at);
    }

    #[tokio::test]
    async fn update_matches_workspace_by_path_when_id_empty() {
        let store = SessionStore::new();
        let ws = Workspace {
            id: "ws-original".into(),
            path: "/same-path".into(),
            ..Default::default()
        };
        store.update_session(vec![ws], None).await;

        // Update with empty id but same path
        let updated_ws = Workspace {
            id: String::new(),
            path: "/same-path".into(),
            ..Default::default()
        };
        let (updated, _) = store.update_session(vec![updated_ws], None).await;

        // Should reuse the original workspace id
        assert_eq!(updated.workspaces[0].id, "ws-original");
    }

    #[tokio::test]
    async fn broadcast_sends_to_watchers_not_sender() {
        let store = SessionStore::new();
        let session = store.session().await;

        let (tx1, mut rx1) = mpsc::channel(16);
        let (tx2, mut rx2) = mpsc::channel(16);

        store.add_watcher("listener-1".into(), tx1).await;
        store.add_watcher("sender".into(), tx2).await;

        store.broadcast_update(&session, "sender").await;

        // listener-1 should receive
        assert!(rx1.try_recv().is_ok());

        // sender should NOT receive
        assert!(rx2.try_recv().is_err());
    }

    #[tokio::test]
    async fn remove_watcher_stops_delivery() {
        let store = SessionStore::new();
        let session = store.session().await;

        let (tx, mut rx) = mpsc::channel(16);
        store.add_watcher("listener".into(), tx).await;
        store.remove_watcher("listener").await;

        store.broadcast_update(&session, "other").await;
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn pty_manager_accessible() {
        let store = SessionStore::new();
        // PtyManager should be functional
        let sessions = store.pty_manager().list_sessions().await;
        assert!(sessions.is_empty());
    }
}
