//! Typed access to one daemon over gRPC.

use std::path::{Path, PathBuf};

use futures::{Stream, StreamExt, TryFutureExt};
use treeterm_proto::treeterm::{
    FileWatchEvent, Session, SessionWatchEvent, SessionWatchRequest, UpdateSessionRequest,
    WatchFileRequest, WorkspaceRef,
};

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
    rt: Rt,
    socket_path: PathBuf,
    main: DaemonClient,
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
        self.rt.run(async move {
            let response = client.update_session(request).await?;
            Ok(response.into_inner())
        })
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
