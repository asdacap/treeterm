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

/// Internal lock state — tracks the daemon-generated connection ID of the holder.
/// Never exposed via proto; only timestamps are visible to clients.
struct InternalLock {
    connection_id: String,
    acquired_at: i64,
    expires_at: i64,
}

impl InternalLock {
    fn to_proto(&self) -> SessionLock {
        SessionLock {
            acquired_at: self.acquired_at,
            expires_at: self.expires_at,
        }
    }
}

struct SessionWatcher {
    listener_id: String,
    tx: mpsc::Sender<Result<SessionWatchEvent, tonic::Status>>,
}

struct Inner {
    session: Session,
    watchers: Vec<SessionWatcher>,
    lock: Option<InternalLock>,
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
            lock: None,
        };
        tracing::info!(session_id = %session.id, "session created");
        Self {
            inner: Arc::new(Mutex::new(Inner {
                session,
                watchers: Vec::new(),
                lock: None,
            })),
            pty_manager: PtyManager::new(),
        }
    }

    pub fn pty_manager(&self) -> &PtyManager {
        &self.pty_manager
    }

    /// Return a snapshot of the current session (with current lock state).
    pub async fn session(&self) -> Session {
        let inner = self.inner.lock().await;
        let mut session = inner.session.clone();
        session.lock = inner.lock.as_ref().map(InternalLock::to_proto);
        session
    }

    /// Update the session's workspaces. Returns `(session, accepted)`.
    ///
    /// Lock enforcement: if session is locked by another connection, the update is rejected.
    /// If the sender's connection holds the lock, the `expected_version` check is skipped (holder always wins).
    /// If `expected_version` is provided and doesn't match the current version,
    /// the update is rejected and the current session is returned unchanged.
    pub async fn update_session(
        &self,
        workspaces: Vec<Workspace>,
        expected_version: Option<u64>,
        sender_id: &str,
        connection_id: &str,
    ) -> (Session, bool) {
        let mut inner = self.inner.lock().await;
        let now = now_millis();

        // Clear expired lock
        if inner.lock.as_ref().is_some_and(|l| l.expires_at <= now) {
            tracing::info!("session lock expired, clearing");
            inner.lock = None;
            inner.session.lock = None;
        }

        let sender_holds_lock = inner
            .lock
            .as_ref()
            .is_some_and(|l| l.connection_id == connection_id);

        // Lock check: if locked by another connection, reject
        if inner.lock.is_some() && !sender_holds_lock {
            tracing::info!(
                sender_id,
                "session update rejected: locked by another client"
            );
            let mut session = inner.session.clone();
            session.lock = inner.lock.as_ref().map(InternalLock::to_proto);
            return (session, false);
        }

        // Version check (skipped if sender holds the lock)
        if !sender_holds_lock {
            if let Some(ev) = expected_version {
                if ev != inner.session.version {
                    tracing::info!(
                        expected_version = ev,
                        actual_version = inner.session.version,
                        "session update rejected: version mismatch"
                    );
                    let mut session = inner.session.clone();
                    session.lock = inner.lock.as_ref().map(InternalLock::to_proto);
                    return (session, false);
                }
            }
        }

        let old_workspaces = &inner.session.workspaces;
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
            id: inner.session.id.clone(),
            workspaces: full_workspaces,
            created_at: inner.session.created_at,
            last_activity: now,
            version: inner.session.version + 1,
            lock: inner.lock.as_ref().map(InternalLock::to_proto),
        };

        tracing::info!(version = updated.version, "session updated");
        inner.session = updated.clone();
        (updated, true)
    }

    /// Acquire a session lock. Returns `(acquired, session)`.
    ///
    /// If the session is unlocked or the lock has expired, the lock is granted.
    /// If the same connection re-acquires, the TTL is refreshed (idempotent).
    /// If locked by another connection, returns `(false, current_session)`.
    pub async fn lock_session(
        &self,
        connection_id: String,
        ttl_ms: i64,
    ) -> (bool, Session) {
        let mut inner = self.inner.lock().await;
        let now = now_millis();

        // Clear expired lock
        if inner.lock.as_ref().is_some_and(|l| l.expires_at <= now) {
            tracing::info!("session lock expired, clearing");
            inner.lock = None;
            inner.session.lock = None;
        }

        // If locked by another connection, reject
        if let Some(ref lock) = inner.lock {
            if lock.connection_id != connection_id {
                tracing::info!(
                    "session lock rejected: held by another client"
                );
                let mut session = inner.session.clone();
                session.lock = inner.lock.as_ref().map(InternalLock::to_proto);
                return (false, session);
            }
        }

        // Grant or refresh lock
        let lock = InternalLock {
            connection_id: connection_id.clone(),
            acquired_at: now,
            expires_at: now + ttl_ms,
        };
        inner.session.lock = Some(lock.to_proto());
        inner.lock = Some(lock);

        // Increment version so watchers see the lock change
        inner.session.version += 1;
        inner.session.last_activity = now;

        tracing::info!(
            version = inner.session.version,
            ttl_ms,
            "session locked"
        );
        (true, inner.session.clone())
    }

    /// Release a session lock. Only the holder connection can unlock.
    /// Returns the current session (with lock cleared if released).
    pub async fn unlock_session(
        &self,
        connection_id: &str,
    ) -> Session {
        let mut inner = self.inner.lock().await;
        let now = now_millis();

        if let Some(ref lock) = inner.lock {
            if lock.connection_id == connection_id {
                tracing::info!("session unlocked");
                inner.lock = None;

                inner.session.version += 1;
                inner.session.last_activity = now;
                inner.session.lock = None;
            } else {
                tracing::info!(
                    "session unlock ignored: not the holder"
                );
            }
        }

        let mut session = inner.session.clone();
        session.lock = inner.lock.as_ref().map(InternalLock::to_proto);
        session
    }

    /// Force-release a session lock regardless of holder.
    /// Returns the current session (with lock cleared).
    pub async fn force_unlock_session(&self) -> Session {
        let mut inner = self.inner.lock().await;
        let now = now_millis();

        if inner.lock.is_some() {
            tracing::info!("session force unlocked");
            inner.lock = None;

            inner.session.version += 1;
            inner.session.last_activity = now;
            inner.session.lock = None;
        }

        let mut session = inner.session.clone();
        session.lock = inner.lock.as_ref().map(InternalLock::to_proto);
        session
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

    /// Test helper: return the connection_id of the current lock holder.
    #[cfg(test)]
    pub(crate) async fn lock_holder(&self) -> Option<String> {
        self.inner.lock().await.lock.as_ref().map(|l| l.connection_id.clone())
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
        assert!(session.lock.is_none());
    }

    #[tokio::test]
    async fn update_session_increments_version() {
        let store = SessionStore::new();

        let (updated, accepted) = store.update_session(vec![], None, "window-a", "conn-a").await;
        assert!(accepted);
        assert_eq!(updated.version, 2);
    }

    #[tokio::test]
    async fn update_session_version_match_accepted() {
        let store = SessionStore::new();

        let (updated, accepted) = store.update_session(vec![], Some(1), "window-a", "conn-a").await;
        assert!(accepted);
        assert_eq!(updated.version, 2);
    }

    #[tokio::test]
    async fn update_session_version_mismatch_rejected() {
        let store = SessionStore::new();

        let (returned, accepted) = store.update_session(vec![], Some(999), "window-a", "conn-a").await;
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
        store.update_session(vec![ws], None, "window-a", "conn-a").await;
        let session = store.session().await;
        let original_created_at = session.workspaces[0].created_at;

        let updated_ws = Workspace {
            id: "ws-1".into(),
            path: "/a-updated".into(),
            ..Default::default()
        };
        let (updated, _) = store
            .update_session(vec![updated_ws], None, "window-a", "conn-a")
            .await;

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
        store.update_session(vec![ws], None, "window-a", "conn-a").await;

        // Update with empty id but same path
        let updated_ws = Workspace {
            id: String::new(),
            path: "/same-path".into(),
            ..Default::default()
        };
        let (updated, _) = store
            .update_session(vec![updated_ws], None, "window-a", "conn-a")
            .await;

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

    // ---- Lock tests ----

    #[tokio::test]
    async fn lock_acquires_when_unlocked() {
        let store = SessionStore::new();

        let (acquired, session) = store.lock_session("conn-a".into(), 60000).await;
        assert!(acquired);
        assert!(session.lock.is_some());
        let lock = session.lock.unwrap();
        assert!(lock.expires_at > lock.acquired_at);
        assert_eq!(store.lock_holder().await, Some("conn-a".into()));
    }

    #[tokio::test]
    async fn lock_rejects_when_held_by_other() {
        let store = SessionStore::new();

        let (acquired, _) = store.lock_session("conn-a".into(), 60000).await;
        assert!(acquired);

        let (acquired, session) = store.lock_session("conn-b".into(), 60000).await;
        assert!(!acquired);
        assert!(session.lock.is_some());
        assert_eq!(store.lock_holder().await, Some("conn-a".into()));
    }

    #[tokio::test]
    async fn lock_reacquire_by_same_holder() {
        let store = SessionStore::new();

        let (acquired, session1) = store.lock_session("conn-a".into(), 10000).await;
        assert!(acquired);
        let first_expires = session1.lock.unwrap().expires_at;

        // Re-acquire with longer TTL refreshes the lock
        let (acquired, session2) = store.lock_session("conn-a".into(), 60000).await;
        assert!(acquired);
        let second_expires = session2.lock.unwrap().expires_at;
        assert!(second_expires > first_expires);
    }

    #[tokio::test]
    async fn lock_succeeds_when_expired() {
        let store = SessionStore::new();

        // Lock with 0ms TTL (immediately expired)
        let (acquired, _) = store.lock_session("conn-a".into(), 0).await;
        assert!(acquired);

        // Another connection can now acquire
        let (acquired, _) = store.lock_session("conn-b".into(), 60000).await;
        assert!(acquired);
        assert_eq!(store.lock_holder().await, Some("conn-b".into()));
    }

    #[tokio::test]
    async fn lock_increments_version() {
        let store = SessionStore::new();
        let session_before = store.session().await;

        let (acquired, session_after) = store.lock_session("conn-a".into(), 60000).await;
        assert!(acquired);
        assert_eq!(session_after.version, session_before.version + 1);
    }

    #[tokio::test]
    async fn update_rejects_when_locked_by_other() {
        let store = SessionStore::new();

        let (acquired, _) = store.lock_session("conn-a".into(), 60000).await;
        assert!(acquired);

        // Different connection tries to update — rejected
        let (_, accepted) = store
            .update_session(vec![], None, "window-b", "conn-b")
            .await;
        assert!(!accepted);
    }

    #[tokio::test]
    async fn update_accepts_for_lock_holder() {
        let store = SessionStore::new();

        let (acquired, session) = store.lock_session("conn-a".into(), 60000).await;
        assert!(acquired);

        // Same connection can update
        let (updated, accepted) = store
            .update_session(vec![], Some(session.version), "window-a", "conn-a")
            .await;
        assert!(accepted);
        assert_eq!(updated.version, session.version + 1);
    }

    #[tokio::test]
    async fn update_skips_version_check_for_holder() {
        let store = SessionStore::new();

        let (acquired, _) = store.lock_session("conn-a".into(), 60000).await;
        assert!(acquired);

        // Holder sends wrong expected_version — still accepted because version check is skipped
        let (_, accepted) = store
            .update_session(vec![], Some(999), "window-a", "conn-a")
            .await;
        assert!(accepted);
    }

    #[tokio::test]
    async fn unlock_releases_lock() {
        let store = SessionStore::new();

        store.lock_session("conn-a".into(), 60000).await;
        let session = store.unlock_session("conn-a").await;
        assert!(session.lock.is_none());

        // Other connection can now update
        let (_, accepted) = store
            .update_session(vec![], Some(session.version), "window-b", "conn-b")
            .await;
        assert!(accepted);
    }

    #[tokio::test]
    async fn unlock_noop_for_non_holder() {
        let store = SessionStore::new();

        store.lock_session("conn-a".into(), 60000).await;
        let session = store.unlock_session("conn-b").await;
        // Lock still held
        assert!(session.lock.is_some());
        assert_eq!(store.lock_holder().await, Some("conn-a".into()));
    }

    #[tokio::test]
    async fn unlock_increments_version() {
        let store = SessionStore::new();

        let (_, lock_session) = store.lock_session("conn-a".into(), 60000).await;
        let session = store.unlock_session("conn-a").await;
        assert_eq!(session.version, lock_session.version + 1);
    }

    #[tokio::test]
    async fn session_snapshot_includes_lock_state() {
        let store = SessionStore::new();

        store.lock_session("conn-a".into(), 60000).await;
        let session = store.session().await;
        assert!(session.lock.is_some());
        assert_eq!(store.lock_holder().await, Some("conn-a".into()));

        store.unlock_session("conn-a").await;
        let session = store.session().await;
        assert!(session.lock.is_none());
    }
}
