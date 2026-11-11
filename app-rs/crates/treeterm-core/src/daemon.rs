//! Typed access to one daemon over gRPC.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::{Stream, StreamExt, TryFutureExt};
use treeterm_proto::treeterm::{
    CreatePtyRequest, Empty, FileWatchEvent, ForceUnlockSessionRequest, KillPtyRequest,
    LockSessionRequest, PtySessionInfo, Session, SessionWatchEvent, SessionWatchRequest,
    UnlockSessionRequest, UpdateSessionRequest, WatchFileRequest, WorkspaceRef,
};

use crate::pty::{AttachKind, CreatePty, PtyAttachment};
use crate::rt::{Fut, Rt, Sub, TaskLost};
use crate::transport::{ConnectError, DaemonClient, connect_unix, daemon_client};

#[derive(Debug, Clone, thiserror::Error)]
pub enum ApiError {
    #[error("daemon call failed: {0}")]
    Status(Box<tonic::Status>),
    #[error(transparent)]
    TaskLost(#[from] TaskLost),
    #[error("daemon protocol violation: {0}")]
    Protocol(&'static str),
    #[error("daemon did not answer within {}s", .0.as_secs())]
    DeadlineExceeded(Duration),
    /// The daemon carried out the call and reported failure.
    #[error("{0}")]
    Operation(String),
}

/// Outcome of [`DaemonConnection::lock_session`], with the session after the call.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionLockResult {
    Acquired(Session),
    /// Another connection holds the lock.
    HeldElsewhere(Session),
}

/// Unary calls fail after this long instead of hanging on a wedged daemon. Streams have no
/// deadline: they are meant to stay open.
pub const UNARY_DEADLINE: Duration = Duration::from_secs(30);

/// Run a unary call with [`UNARY_DEADLINE`].
pub(crate) async fn unary<T>(
    call: impl Future<Output = Result<tonic::Response<T>, tonic::Status>>,
) -> Result<T, ApiError> {
    match tokio::time::timeout(UNARY_DEADLINE, call).await {
        Ok(response) => Ok(response?.into_inner()),
        Err(_elapsed) => Err(ApiError::DeadlineExceeded(UNARY_DEADLINE)),
    }
}

impl From<tonic::Status> for ApiError {
    fn from(status: tonic::Status) -> Self {
        ApiError::Status(Box::new(status))
    }
}

/// One connection generation to a daemon.
///
/// Two HTTP/2 connections: `main` carries streams and ordinary calls; `session` carries
/// only UpdateSession/Lock/Unlock. The daemon keys session locks by connection
/// (`daemon-rs/.../connection_id.rs`), so lock-sensitive calls get a connection of their own.
#[derive(Clone)]
pub struct DaemonConnection {
    pub(crate) rt: Rt,
    socket_path: PathBuf,
    pub(crate) main: DaemonClient,
    session: DaemonClient,
}

impl DaemonConnection {
    /// Open both connections. Must run on the tokio runtime behind `rt`.
    pub async fn connect(rt: Rt, socket_path: PathBuf) -> Result<Self, ConnectError> {
        let main = connect_unix(&socket_path).await?;
        let session = connect_unix(&socket_path).await?;
        Ok(Self {
            rt,
            socket_path,
            main: daemon_client(main),
            session: daemon_client(session),
        })
    }

    /// [`Self::connect`] from any executor.
    pub fn open(rt: &Rt, socket_path: &Path) -> Fut<Result<Self, ConnectError>> {
        rt.run(Self::connect(rt.clone(), socket_path.to_owned()))
    }

    /// Replace the session's workspace ref list. `expected_version` is the optimistic-lock
    /// check (skipped by the daemon when this connection holds the session lock);
    /// `sender_id` is excluded from the resulting broadcast. Returns the session after the
    /// call and whether the update was applied.
    pub fn update_session(
        &self,
        workspace_refs: Vec<WorkspaceRef>,
        expected_version: Option<u64>,
        sender_id: &str,
    ) -> Fut<Result<Session, ApiError>> {
        let mut client = self.session.clone();
        let request = UpdateSessionRequest {
            sender_id: Some(sender_id.to_owned()),
            expected_version,
            workspace_refs,
        };
        self.rt
            .run(async move { unary(client.update_session(request)).await })
    }

    /// Take the session lock for this connection (the daemon keys locks by connection,
    /// hence the session channel). While held, other connections' updates are rejected.
    pub fn lock_session(&self, ttl: Duration) -> Fut<Result<SessionLockResult, ApiError>> {
        let mut client = self.session.clone();
        let request = LockSessionRequest {
            ttl_ms: Some(i64::try_from(ttl.as_millis()).unwrap_or(i64::MAX)),
        };
        self.rt.run(async move {
            let response = unary(client.lock_session(request)).await?;
            let session = response
                .session
                .ok_or(ApiError::Protocol("LockSession response without a session"))?;
            Ok(if response.acquired {
                SessionLockResult::Acquired(session)
            } else {
                SessionLockResult::HeldElsewhere(session)
            })
        })
    }

    pub fn unlock_session(&self) -> Fut<Result<Session, ApiError>> {
        let mut client = self.session.clone();
        self.rt
            .run(async move { unary(client.unlock_session(UnlockSessionRequest {})).await })
    }

    /// Break another window's stale lock.
    pub fn force_unlock_session(&self) -> Fut<Result<Session, ApiError>> {
        let mut client = self.session.clone();
        self.rt.run(async move {
            unary(client.force_unlock_session(ForceUnlockSessionRequest {})).await
        })
    }

    /// Start a PTY (or, for a known `handle`, find the live one) and return its id.
    pub fn create_pty(&self, params: CreatePty) -> Fut<Result<String, ApiError>> {
        let mut client = self.main.clone();
        let request = CreatePtyRequest {
            cwd: params.cwd,
            env: params.env,
            cols: params.size.map(|(cols, _)| i32::from(cols)),
            rows: params.size.map(|(_, rows)| i32::from(rows)),
            sandbox: None,
            startup_command: params.startup_command,
            handle: params.handle,
        };
        self.rt.run(async move {
            unary(client.create_pty(request))
                .await
                .map(|response| response.session_id)
        })
    }

    /// Destroy a PTY. This is close, not detach: every attachment sees it exit.
    pub fn kill_pty(&self, session_id: &str) -> Fut<Result<(), ApiError>> {
        let mut client = self.main.clone();
        let request = KillPtyRequest {
            session_id: session_id.to_owned(),
        };
        self.rt
            .run(async move { unary(client.kill_pty(request)).await.map(|Empty {}| ()) })
    }

    pub fn list_pty_sessions(&self) -> Fut<Result<Vec<PtySessionInfo>, ApiError>> {
        let mut client = self.main.clone();
        self.rt.run(async move {
            unary(client.list_pty_sessions(Empty {}))
                .await
                .map(|response| response.sessions)
        })
    }

    /// Stop the daemon. It exits while answering, so a dropped connection counts as done.
    pub fn shutdown(&self) -> Fut<Result<(), ApiError>> {
        let mut client = self.main.clone();
        self.rt.run(async move {
            match unary(client.shutdown(Empty {})).await {
                Ok(Empty {}) => Ok(()),
                Err(ApiError::Status(status)) if status.code() == tonic::Code::Unavailable
                    || status.code() == tonic::Code::Unknown
                    || status.code() == tonic::Code::Internal => Ok(()),
                Err(other) => Err(other),
            }
        })
    }

    /// Attach to a PTY. See [`crate::pty`].
    pub fn attach_pty(&self, session_id: &str, kind: AttachKind) -> PtyAttachment {
        crate::pty::attach(&self.rt, self.main.clone(), session_id, kind)
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Session snapshots; the first event is the current state.
    pub fn watch_session(&self, listener_id: &str) -> Sub<Result<SessionWatchEvent, ApiError>> {
        let mut client = self.main.clone();
        let request = SessionWatchRequest {
            listener_id: listener_id.to_owned(),
        };
        self.rt.subscribe(server_stream(async move {
            client.session_watch(request).await
        }))
    }

    /// A file's state (present with content + sha, or absent); the first event is the
    /// current state. `file_path` is relative to the `workspace_path` scope.
    pub fn watch_file(
        &self,
        workspace_path: &str,
        file_path: &str,
    ) -> Sub<Result<FileWatchEvent, ApiError>> {
        let mut client = self.main.clone();
        let request = WatchFileRequest {
            workspace_path: workspace_path.to_owned(),
            file_path: file_path.to_owned(),
            watcher_id: uuid::Uuid::new_v4().to_string(),
        };
        self.rt.subscribe(server_stream(async move {
            client.watch_file(request).await
        }))
    }
}

/// Flatten "start a server-streaming call" + "its messages" into one stream of results.
fn server_stream<T: Send + 'static>(
    call: impl Future<Output = Result<tonic::Response<tonic::Streaming<T>>, tonic::Status>>
    + Send
    + 'static,
) -> impl Stream<Item = Result<T, ApiError>> + Send + 'static {
    call.map_ok(tonic::Response::into_inner)
        .try_flatten_stream()
        .map(|item| item.map_err(ApiError::from))
}
