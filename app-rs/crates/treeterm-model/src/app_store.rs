//! Root store: starts (or finds) the local daemon and owns the connected session.

use gpui_kit::{AppContext as _, Context, Entity, Task};
use treeterm_core::daemon::DaemonConnection;
use treeterm_core::local_daemon::{LocalDaemonSpec, ensure_running};
use treeterm_core::rt::Rt;

use crate::session_store::SessionStore;

pub enum AppState {
    StartingDaemon,
    Connected(Entity<SessionStore>),
    Failed(String),
}

pub struct AppStore {
    state: AppState,
    /// Identifies this process to the daemon (`sender_id`, `listener_id`), so it can skip
    /// echoes of its own session updates. One per process: windows share stores.
    client_id: String,
    _start: Task<()>,
}

impl AppStore {
    pub fn new(rt: Rt, spec: LocalDaemonSpec, cx: &mut Context<Self>) -> Self {
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
        let start = cx.spawn(async move |this, cx| {
            let state = match connect.await {
                Ok(Ok(conn)) => Ok(conn),
                Ok(Err(error)) => Err(error),
                Err(lost) => Err(lost.to_string()),
            };
            match this.update(cx, |store, cx| store.on_started(state, cx)) {
                Ok(()) => {}
                Err(_released) => tracing::debug!("app store dropped while the daemon started"),
            }
        });
        Self {
            state: AppState::StartingDaemon,
            client_id: uuid::Uuid::new_v4().to_string(),
            _start: start,
        }
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    fn on_started(&mut self, result: Result<DaemonConnection, String>, cx: &mut Context<Self>) {
        self.state = match result {
            Ok(conn) => {
                let client_id = self.client_id.clone();
                let session = cx.new(|cx| SessionStore::new(conn, client_id, cx));
                cx.observe(&session, |_, _, cx| cx.notify()).detach();
                AppState::Connected(session)
            }
            Err(error) => {
                tracing::error!(%error, "cannot reach the local daemon");
                AppState::Failed(error)
            }
        };
        cx.notify();
    }
}
