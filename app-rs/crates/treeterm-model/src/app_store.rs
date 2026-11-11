//! Root store: starts (or finds) the local daemon, owns the connected session, and
//! reconnects when the daemon goes away.
//!
//! Reconnect follows the Electron connection manager for the local target: when the
//! session watch fails, the dead session store is disposed (explicitly — a ghost store left
//! alive keeps writing the same files as its replacement), then the daemon is ensured
//! running again with exponential backoff (1 s · 2ⁿ, capped at 30 s, unbounded) and a fresh
//! session store is built on a new connection generation.

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::{AppContext as _, Context, Entity, Subscription, Task};
use treeterm_core::daemon::DaemonConnection;
use treeterm_core::local_daemon::{LocalDaemonSpec, ensure_running};
use treeterm_core::rt::Rt;
use treeterm_core::settings::Settings;

use crate::session_store::{SessionEvent, SessionStore};

const RECONNECT_BASE: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(30);

pub enum AppState {
    StartingDaemon,
    Connected(Entity<SessionStore>),
    /// The daemon went away; trying again.
    Reconnecting { attempt: u32, last_error: String },
    /// The first start failed.
    Failed(String),
}

pub struct AppStore {
    rt: Rt,
    spec: LocalDaemonSpec,
    state: AppState,
    settings: Rc<Settings>,
    /// Identifies this process to the daemon (`sender_id`, `listener_id`), so it can skip
    /// echoes of its own session updates. One per process: windows share stores.
    client_id: String,
    /// Starting or reconnecting; dropping it cancels the attempt.
    _connect: Option<Task<()>>,
    _session_events: Option<Subscription>,
}

/// Backoff before reconnect attempt `attempt` (1-based).
pub fn reconnect_delay(attempt: u32) -> Duration {
    let factor = 1u32.checked_shl(attempt.saturating_sub(1)).unwrap_or(u32::MAX);
    RECONNECT_BASE
        .checked_mul(factor)
        .map_or(RECONNECT_MAX, |delay| delay.min(RECONNECT_MAX))
}

impl AppStore {
    pub fn new(rt: Rt, spec: LocalDaemonSpec, settings: Settings, cx: &mut Context<Self>) -> Self {
        let mut store = Self {
            rt,
            spec,
            state: AppState::StartingDaemon,
            settings: Rc::new(settings),
            client_id: uuid::Uuid::new_v4().to_string(),
            _connect: None,
            _session_events: None,
        };
        store.connect(Duration::ZERO, cx);
        store
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// Ensure the daemon runs and connect, after `delay`.
    fn connect(&mut self, delay: Duration, cx: &mut Context<Self>) {
        let rt = self.rt.clone();
        let spec = self.spec.clone();
        let executor = cx.background_executor().clone();
        self._connect = Some(cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                executor.timer(delay).await;
            }
            let connect = rt.run({
                let rt = rt.clone();
                async move {
                    let start = ensure_running(&spec).await.map_err(|e| e.to_string())?;
                    tracing::info!(?start, socket = ?spec.socket_path, "local daemon ready");
                    DaemonConnection::connect(rt, spec.socket_path)
                        .await
                        .map_err(|e| e.to_string())
                }
            });
            let result = match connect.await {
                Ok(Ok(conn)) => Ok(conn),
                Ok(Err(error)) => Err(error),
                Err(lost) => Err(lost.to_string()),
            };
            match this.update(cx, |store, cx| store.on_connected(result, cx)) {
                Ok(()) => {}
                Err(_released) => tracing::debug!("app store dropped while connecting"),
            }
        }));
    }

    fn on_connected(&mut self, result: Result<DaemonConnection, String>, cx: &mut Context<Self>) {
        match result {
            Ok(conn) => {
                self._connect = None;
                let client_id = self.client_id.clone();
                let settings = self.settings.clone();
                let session = cx.new(|cx| SessionStore::new(conn, client_id, settings, cx));
                cx.observe(&session, |_, _, cx| cx.notify()).detach();
                self._session_events = Some(cx.subscribe(
                    &session,
                    |store, session, event: &SessionEvent, cx| match event {
                        SessionEvent::Failed(reason) => store.on_session_failed(session, reason.clone(), cx),
                        SessionEvent::WorkspaceRemoved(_) => {}
                    },
                ));
                self.state = AppState::Connected(session);
            }
            Err(error) => {
                tracing::error!(%error, "cannot reach the local daemon");
                let next = match &self.state {
                    AppState::Reconnecting { attempt, .. } => Some(attempt + 1),
                    AppState::StartingDaemon | AppState::Connected(_) | AppState::Failed(_) => None,
                };
                self.state = match next {
                    Some(attempt) => {
                        self.connect(reconnect_delay(attempt), cx);
                        AppState::Reconnecting {
                            attempt,
                            last_error: error,
                        }
                    }
                    None => {
                        self._connect = None;
                        AppState::Failed(error)
                    }
                };
            }
        }
        cx.notify();
    }

    /// The session's watch died: its connection generation is gone. Retire the store and
    /// rebuild on a new connection.
    fn on_session_failed(&mut self, session: Entity<SessionStore>, reason: String, cx: &mut Context<Self>) {
        let AppState::Connected(current) = &self.state else {
            return;
        };
        if *current != session {
            return; // already replaced
        }
        tracing::warn!(%reason, "lost the daemon; reconnecting");
        session.update(cx, |session, cx| session.dispose(cx));
        self._session_events = None;
        self.state = AppState::Reconnecting {
            attempt: 1,
            last_error: reason,
        };
        self.connect(reconnect_delay(1), cx);
        cx.notify();
    }

    /// "Exit and Kill Daemon": stop the local daemon (and with it every PTY).
    pub fn shutdown_daemon(
        &self,
        cx: &gpui_kit::App,
    ) -> Option<treeterm_core::rt::Fut<Result<(), treeterm_core::daemon::ApiError>>> {
        match &self.state {
            AppState::Connected(session) => Some(session.read(cx).connection().shutdown()),
            AppState::StartingDaemon | AppState::Reconnecting { .. } | AppState::Failed(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_from_one_second_up_to_thirty() {
        let delays: Vec<u64> = (1..=7).map(|n| reconnect_delay(n).as_secs()).collect();
        assert_eq!(delays, vec![1, 2, 4, 8, 16, 30, 30]);
        assert_eq!(reconnect_delay(500), Duration::from_secs(30));
    }
}
