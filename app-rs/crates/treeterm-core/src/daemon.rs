//! Typed access to one daemon over gRPC.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::{Stream, StreamExt, TryFutureExt};
use tokio::sync::watch;
use treeterm_proto::treeterm::{
    CreatePtyRequest, Empty, FileWatchEvent, ForceUnlockSessionRequest, KillPtyRequest,
    LockSessionRequest, PtySessionInfo, Session, SessionWatchEvent, SessionWatchRequest,
    UnlockSessionRequest, UpdateSessionRequest, WatchFileRequest, WorkspaceRef,
};

use crate::connection::ConnectionId;
use crate::pty::{AttachKind, CreatePty, PtyAttachment};
use crate::rt::{Fut, Rt, Sub, TaskLost};
use crate::traffic::ByteCounter;
use crate::transport::{
    CONNECT_TIMEOUT, ConnectError, DaemonClient, connect_unix_ready, daemon_client,
};

#[derive(Debug, Clone, thiserror::Error)]
pub enum ApiError {
    /// Shown as grpc-js's error message, `<code> <NAME>: <details>`, as Electron shows it.
    #[error("{}", grpc_message(.0))]
    Status(Box<tonic::Status>),
    #[error(transparent)]
    TaskLost(#[from] TaskLost),
    #[error("daemon protocol violation: {0}")]
    Protocol(&'static str),
    /// grpc-js's text for a passed call deadline (it appends timing diagnostics after a comma).
    #[error("4 DEADLINE_EXCEEDED: Deadline exceeded after {:.3}s", .0.as_secs_f64())]
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

/// grpc-js's `callErrorFromStatus` message: `13 INTERNAL: spawn failed`.
fn grpc_message(status: &tonic::Status) -> String {
    let code = status.code();
    format!(
        "{} {}: {}",
        i32::from(code),
        grpc_code_name(code),
        status.message()
    )
}

/// The `Status` enum names of grpc-js (and of the gRPC spec).
fn grpc_code_name(code: tonic::Code) -> &'static str {
    match code {
        tonic::Code::Ok => "OK",
        tonic::Code::Cancelled => "CANCELLED",
        tonic::Code::Unknown => "UNKNOWN",
        tonic::Code::InvalidArgument => "INVALID_ARGUMENT",
        tonic::Code::DeadlineExceeded => "DEADLINE_EXCEEDED",
        tonic::Code::NotFound => "NOT_FOUND",
        tonic::Code::AlreadyExists => "ALREADY_EXISTS",
        tonic::Code::PermissionDenied => "PERMISSION_DENIED",
        tonic::Code::ResourceExhausted => "RESOURCE_EXHAUSTED",
        tonic::Code::FailedPrecondition => "FAILED_PRECONDITION",
        tonic::Code::Aborted => "ABORTED",
        tonic::Code::OutOfRange => "OUT_OF_RANGE",
        tonic::Code::Unimplemented => "UNIMPLEMENTED",
        tonic::Code::Internal => "INTERNAL",
        tonic::Code::Unavailable => "UNAVAILABLE",
        tonic::Code::DataLoss => "DATA_LOSS",
        tonic::Code::Unauthenticated => "UNAUTHENTICATED",
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
    /// Every byte received over both connections.
    received: ByteCounter,
    /// Set once the connection manager gives this generation up; its execs then fail.
    lost: Arc<watch::Sender<Option<ConnectionId>>>,
}

impl DaemonConnection {
    /// Open both connections, waiting up to [`CONNECT_TIMEOUT`] in all for the daemon to
    /// accept them (see [`connect_unix_ready`]). Must run on the tokio runtime behind `rt`.
    pub async fn connect(rt: Rt, socket_path: PathBuf) -> Result<Self, ConnectError> {
        let received = ByteCounter::default();
        let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
        let main = connect_unix_ready(&socket_path, received.clone(), deadline).await?;
        let session = connect_unix_ready(&socket_path, received.clone(), deadline).await?;
        Ok(Self {
            rt,
            socket_path,
            main: daemon_client(main),
            session: daemon_client(session),
            received,
            lost: Arc::new(watch::Sender::new(None)),
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

    /// The runtime this connection's I/O runs on (for other clients the app builds, e.g.
    /// GitHub over HTTP).
    pub fn rt(&self) -> &Rt {
        &self.rt
    }

    /// Run commands on the daemon's host (`ExecStream`).
    pub fn exec_client(&self) -> crate::exec::ExecClient {
        crate::exec::ExecClient::on_connection(
            self.rt.clone(),
            self.main.clone(),
            self.lost.subscribe(),
        )
    }

    /// `connection` was lost on this generation (it went to reconnecting): every command still
    /// running on it ends with [`crate::exec::ExecError::ConnectionLost`] and is cancelled, so
    /// the daemon kills it, and later ones fail at once (Electron's `failAllForConnection`).
    /// Only the first loss counts.
    pub fn fail_running_execs(&self, connection: ConnectionId) {
        self.lost.send_modify(|lost| {
            if lost.is_none() {
                *lost = Some(connection);
            }
        });
    }

    /// Files on the daemon's host (ReadFile/WriteFile/ReadDirectory/SearchFiles/...).
    pub fn fs_client(&self) -> crate::fs::FsClient {
        crate::fs::FsClient::new(self.rt.clone(), self.main.clone())
    }

    /// Git on the daemon's host, through its exec and file clients.
    pub fn git(&self) -> crate::git::GitApi {
        crate::git::GitApi::new(std::sync::Arc::new(self.exec_client()), std::sync::Arc::new(self.fs_client()))
    }

    /// Attach to a PTY. See [`crate::pty`].
    pub fn attach_pty(&self, session_id: &str, kind: AttachKind) -> PtyAttachment {
        crate::pty::attach(&self.rt, self.main.clone(), session_id, kind)
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Bytes received from the daemon so far on this connection generation.
    pub fn received(&self) -> &ByteCounter {
        &self.received
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_read_as_grpc_js_errors() {
        let internal = ApiError::from(tonic::Status::internal("spawn failed"));
        assert_eq!(internal.to_string(), "13 INTERNAL: spawn failed");
        let unavailable = ApiError::from(tonic::Status::unavailable("transport error"));
        assert_eq!(unavailable.to_string(), "14 UNAVAILABLE: transport error");
    }

    #[test]
    fn every_code_has_its_grpc_name() {
        let messages: Vec<String> = (0..=16)
            .map(|code| {
                ApiError::from(tonic::Status::new(tonic::Code::from(code), "x")).to_string()
            })
            .collect();
        assert_eq!(
            messages,
            [
                "0 OK: x",
                "1 CANCELLED: x",
                "2 UNKNOWN: x",
                "3 INVALID_ARGUMENT: x",
                "4 DEADLINE_EXCEEDED: x",
                "5 NOT_FOUND: x",
                "6 ALREADY_EXISTS: x",
                "7 PERMISSION_DENIED: x",
                "8 RESOURCE_EXHAUSTED: x",
                "9 FAILED_PRECONDITION: x",
                "10 ABORTED: x",
                "11 OUT_OF_RANGE: x",
                "12 UNIMPLEMENTED: x",
                "13 INTERNAL: x",
                "14 UNAVAILABLE: x",
                "15 DATA_LOSS: x",
                "16 UNAUTHENTICATED: x",
            ]
        );
    }

    #[test]
    fn a_passed_deadline_reads_as_grpc_js_deadline_error() {
        assert_eq!(
            ApiError::DeadlineExceeded(UNARY_DEADLINE).to_string(),
            "4 DEADLINE_EXCEEDED: Deadline exceeded after 30.000s"
        );
    }
}
