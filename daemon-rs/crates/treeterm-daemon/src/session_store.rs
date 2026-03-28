use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::Rng;
use tokio::sync::{Mutex, mpsc};
use treeterm_proto::treeterm::*;

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
    session_id: String,
    tx: mpsc::Sender<Result<SessionWatchEvent, tonic::Status>>,
}

struct Inner {
    sessions: HashMap<String, Session>,
    default_session_id: Option<String>,
    watchers: Vec<SessionWatcher>,
}

#[derive(Clone)]
pub struct SessionStore(Arc<Mutex<Inner>>);

impl SessionStore {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Inner {
            sessions: HashMap::new(),
            default_session_id: None,
            watchers: Vec::new(),
        })))
    }

    pub async fn create_session(&self, _client_id: &str, workspaces: Vec<Workspace>) -> Session {
        let mut inner = self.0.lock().await;
        let now = now_millis();

        let full_workspaces = workspaces
            .into_iter()
            .map(|mut ws| {
                if ws.id.is_empty() {
                    ws.id = generate_workspace_id();
                }
                ws.created_at = now;
                ws.last_activity = now;
                ws
            })
            .collect();

        let session = Session {
            id: generate_session_id(),
            workspaces: full_workspaces,
            created_at: now,
            last_activity: now,
            version: 1,
        };

        tracing::info!(session_id = %session.id, "session created");
        inner.sessions.insert(session.id.clone(), session.clone());
        session
    }

    /// Update a session's workspaces. Returns `(session, accepted)`.
    /// If `expected_version` is provided and doesn't match the current version,
    /// the update is rejected and the current session is returned unchanged.
    pub async fn update_session(
        &self,
        _client_id: &str,
        session_id: &str,
        workspaces: Vec<Workspace>,
        expected_version: Option<u64>,
    ) -> Option<(Session, bool)> {
        let mut inner = self.0.lock().await;
        let existing = inner.sessions.get(session_id)?;

        // Version mismatch → reject, return current state
        if let Some(ev) = expected_version {
            if ev != existing.version {
                tracing::info!(
                    session_id,
                    expected_version = ev,
                    actual_version = existing.version,
                    "session update rejected: version mismatch"
                );
                return Some((existing.clone(), false));
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
            id: session_id.to_string(),
            workspaces: full_workspaces,
            created_at: existing.created_at,
            last_activity: now,
            version: existing.version + 1,
        };

        tracing::info!(session_id, version = updated.version, "session updated");
        inner.sessions.insert(session_id.to_string(), updated.clone());
        Some((updated, true))
    }

    pub async fn delete_session(&self, session_id: &str) -> bool {
        let mut inner = self.0.lock().await;
        let existed = inner.sessions.remove(session_id).is_some();
        if existed {
            tracing::info!(session_id, "session deleted");
        }
        existed
    }

    pub async fn get_session(&self, session_id: &str) -> Option<Session> {
        self.0.lock().await.sessions.get(session_id).cloned()
    }

    pub async fn list_sessions(&self) -> Vec<Session> {
        self.0.lock().await.sessions.values().cloned().collect()
    }

    pub async fn get_default_session_id(&self) -> Option<String> {
        self.0.lock().await.default_session_id.clone()
    }

    pub async fn get_or_create_default_session(&self, client_id: &str) -> Session {
        // Check if default session exists
        {
            let inner = self.0.lock().await;
            if let Some(ref id) = inner.default_session_id {
                if let Some(session) = inner.sessions.get(id) {
                    return session.clone();
                }
            }
        }

        // Create new default session (releases lock above, create_session re-acquires)
        let session = self.create_session(client_id, vec![]).await;
        self.0.lock().await.default_session_id = Some(session.id.clone());
        tracing::info!(session_id = %session.id, "default session initialized");
        session
    }

    pub async fn add_watcher(
        &self,
        listener_id: String,
        session_id: String,
        tx: mpsc::Sender<Result<SessionWatchEvent, tonic::Status>>,
    ) {
        self.0.lock().await.watchers.push(SessionWatcher {
            listener_id,
            session_id,
            tx,
        });
    }

    pub async fn remove_watcher(&self, listener_id: &str) {
        self.0
            .lock()
            .await
            .watchers
            .retain(|w| w.listener_id != listener_id);
    }

    pub async fn broadcast_update(
        &self,
        session_id: &str,
        session: &Session,
        sender_id: &str,
    ) {
        let mut inner = self.0.lock().await;
        let event = SessionWatchEvent {
            session_id: session_id.to_string(),
            session: Some(session.clone()),
            sender_id: sender_id.to_string(),
        };

        // Send to matching watchers, remove those with closed channels
        inner.watchers.retain(|w| {
            if w.session_id != session_id || w.listener_id == sender_id {
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
                let sessions: Vec<Session> = store.0.lock().await.sessions.values().cloned().collect();
                for session in &sessions {
                    // Empty sender_id → sends to ALL watchers (listener_id is always non-empty)
                    store.broadcast_update(&session.id, session, "").await;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_session_generates_id_and_version() {
        let store = SessionStore::new();
        let session = store.create_session("test-client", vec![]).await;

        assert!(session.id.starts_with("session-"));
        assert_eq!(session.version, 1);
        assert!(session.created_at > 0);
        assert_eq!(session.workspaces.len(), 0);
    }

    #[tokio::test]
    async fn create_session_with_workspaces() {
        let store = SessionStore::new();
        let ws = Workspace {
            id: String::new(), // should be auto-generated
            path: "/tmp/test".into(),
            ..Default::default()
        };
        let session = store.create_session("c", vec![ws]).await;

        assert_eq!(session.workspaces.len(), 1);
        assert!(session.workspaces[0].id.starts_with("ws-"));
        assert_eq!(session.workspaces[0].path, "/tmp/test");
        assert!(session.workspaces[0].created_at > 0);
    }

    #[tokio::test]
    async fn create_session_preserves_provided_workspace_id() {
        let store = SessionStore::new();
        let ws = Workspace {
            id: "my-ws-id".into(),
            path: "/tmp".into(),
            ..Default::default()
        };
        let session = store.create_session("c", vec![ws]).await;
        assert_eq!(session.workspaces[0].id, "my-ws-id");
    }

    #[tokio::test]
    async fn get_session_returns_created() {
        let store = SessionStore::new();
        let session = store.create_session("c", vec![]).await;

        let got = store.get_session(&session.id).await;
        assert!(got.is_some());
        assert_eq!(got.unwrap().id, session.id);
    }

    #[tokio::test]
    async fn get_session_missing_returns_none() {
        let store = SessionStore::new();
        assert!(store.get_session("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn list_sessions_returns_all() {
        let store = SessionStore::new();
        store.create_session("c", vec![]).await;
        store.create_session("c", vec![]).await;

        let sessions = store.list_sessions().await;
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn delete_session_removes_it() {
        let store = SessionStore::new();
        let session = store.create_session("c", vec![]).await;

        assert!(store.delete_session(&session.id).await);
        assert!(store.get_session(&session.id).await.is_none());
    }

    #[tokio::test]
    async fn delete_nonexistent_returns_false() {
        let store = SessionStore::new();
        assert!(!store.delete_session("nope").await);
    }

    #[tokio::test]
    async fn update_session_increments_version() {
        let store = SessionStore::new();
        let session = store.create_session("c", vec![]).await;

        let result = store
            .update_session("c", &session.id, vec![], None)
            .await;
        let (updated, accepted) = result.unwrap();
        assert!(accepted);
        assert_eq!(updated.version, 2);
    }

    #[tokio::test]
    async fn update_session_version_match_accepted() {
        let store = SessionStore::new();
        let session = store.create_session("c", vec![]).await;

        let result = store
            .update_session("c", &session.id, vec![], Some(1))
            .await;
        let (updated, accepted) = result.unwrap();
        assert!(accepted);
        assert_eq!(updated.version, 2);
    }

    #[tokio::test]
    async fn update_session_version_mismatch_rejected() {
        let store = SessionStore::new();
        let session = store.create_session("c", vec![]).await;

        let result = store
            .update_session("c", &session.id, vec![], Some(999))
            .await;
        let (returned, accepted) = result.unwrap();
        assert!(!accepted);
        assert_eq!(returned.version, 1); // unchanged
    }

    #[tokio::test]
    async fn update_nonexistent_returns_none() {
        let store = SessionStore::new();
        assert!(store.update_session("c", "nope", vec![], None).await.is_none());
    }

    #[tokio::test]
    async fn update_preserves_existing_workspace_by_id() {
        let store = SessionStore::new();
        let ws = Workspace {
            id: "ws-1".into(),
            path: "/a".into(),
            ..Default::default()
        };
        let session = store.create_session("c", vec![ws]).await;
        let original_created_at = session.workspaces[0].created_at;

        let updated_ws = Workspace {
            id: "ws-1".into(),
            path: "/a-updated".into(),
            ..Default::default()
        };
        let (updated, _) = store
            .update_session("c", &session.id, vec![updated_ws], None)
            .await
            .unwrap();

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
        let session = store.create_session("c", vec![ws]).await;

        // Update with empty id but same path
        let updated_ws = Workspace {
            id: String::new(),
            path: "/same-path".into(),
            ..Default::default()
        };
        let (updated, _) = store
            .update_session("c", &session.id, vec![updated_ws], None)
            .await
            .unwrap();

        // Should reuse the original workspace id
        assert_eq!(updated.workspaces[0].id, "ws-original");
    }

    #[tokio::test]
    async fn get_or_create_default_session_creates_once() {
        let store = SessionStore::new();

        let s1 = store.get_or_create_default_session("c").await;
        let s2 = store.get_or_create_default_session("c").await;

        assert_eq!(s1.id, s2.id);
    }

    #[tokio::test]
    async fn get_default_session_id_initially_none() {
        let store = SessionStore::new();
        assert!(store.get_default_session_id().await.is_none());
    }

    #[tokio::test]
    async fn get_default_session_id_after_create() {
        let store = SessionStore::new();
        let session = store.get_or_create_default_session("c").await;

        let id = store.get_default_session_id().await;
        assert_eq!(id.unwrap(), session.id);
    }

    #[tokio::test]
    async fn broadcast_sends_to_matching_watchers_not_sender() {
        let store = SessionStore::new();
        let session = store.create_session("c", vec![]).await;

        let (tx1, mut rx1) = mpsc::channel(16);
        let (tx2, mut rx2) = mpsc::channel(16);

        // Watcher for same session, different listener
        store
            .add_watcher("listener-1".into(), session.id.clone(), tx1)
            .await;
        // Watcher for same session, is the sender
        store
            .add_watcher("sender".into(), session.id.clone(), tx2)
            .await;

        store
            .broadcast_update(&session.id, &session, "sender")
            .await;

        // listener-1 should receive
        let event = rx1.try_recv();
        assert!(event.is_ok());

        // sender should NOT receive
        let event = rx2.try_recv();
        assert!(event.is_err());
    }

    #[tokio::test]
    async fn broadcast_skips_other_sessions() {
        let store = SessionStore::new();
        let s1 = store.create_session("c", vec![]).await;
        let s2 = store.create_session("c", vec![]).await;

        let (tx, mut rx) = mpsc::channel(16);
        store
            .add_watcher("listener".into(), s2.id.clone(), tx)
            .await;

        // Broadcast for s1 should not reach s2's watcher
        store.broadcast_update(&s1.id, &s1, "other").await;
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn remove_watcher_stops_delivery() {
        let store = SessionStore::new();
        let session = store.create_session("c", vec![]).await;

        let (tx, mut rx) = mpsc::channel(16);
        store
            .add_watcher("listener".into(), session.id.clone(), tx)
            .await;
        store.remove_watcher("listener").await;

        store
            .broadcast_update(&session.id, &session, "other")
            .await;
        assert!(rx.try_recv().is_err());
    }
}
