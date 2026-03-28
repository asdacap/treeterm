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
