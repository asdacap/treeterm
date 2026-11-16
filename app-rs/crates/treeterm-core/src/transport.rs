//! gRPC transport to a daemon over its Unix socket.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hyper_util::rt::TokioIo;
use tokio::net::UnixStream;
use tokio::time::Instant;
use tonic::transport::{Channel, Endpoint, Uri};
use treeterm_proto::treeterm::tree_term_daemon_client::TreeTermDaemonClient;

use crate::traffic::{ByteCounter, CountingStream};

pub type DaemonClient = TreeTermDaemonClient<Channel>;

/// Largest message either side sends; matches the daemon's limit.
pub const MAX_MESSAGE_SIZE: usize = 8 * 1024 * 1024;

/// Electron's `waitForReady` deadline: how long a fresh connection keeps trying.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// grpc-js's connect backoff: the next attempt starts this long after the previous one
/// started, then [`CONNECT_BACKOFF_MULTIPLIER`] times longer each time. (grpc-js adds ±20 %
/// jitter to spread many clients out; one connection has nothing to spread.)
const CONNECT_BACKOFF_INITIAL: Duration = Duration::from_secs(1);
const CONNECT_BACKOFF_MULTIPLIER: f64 = 1.6;

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("daemon socket not found at {0}")]
    SocketMissing(PathBuf),
    #[error("cannot connect to daemon socket {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("gRPC handshake with daemon at {path} failed: {source}")]
    Transport {
        path: PathBuf,
        source: tonic::transport::Error,
    },
    #[error("timed out connecting to daemon at {0}")]
    Timeout(PathBuf),
}

/// Open one HTTP/2 connection to the daemon, in a single try of at most [`CONNECT_TIMEOUT`].
/// For probing whether a daemon answers; connections to use are opened with
/// [`connect_unix_ready`].
///
/// The connector hands out the socket exactly once. If the connection later drops, tonic
/// would otherwise redial silently, and the daemon would see a new connection — a new lock
/// identity (`daemon-rs/.../connection_id.rs`) — in the middle of a locked operation. With
/// a single-use connector every call on a dead channel fails instead, and the connection
/// manager builds a fresh generation of channels.
pub async fn connect_unix(path: &Path) -> Result<Channel, ConnectError> {
    if !path.exists() {
        return Err(ConnectError::SocketMissing(path.to_owned()));
    }
    tokio::time::timeout(CONNECT_TIMEOUT, attempt(path, ByteCounter::default()))
        .await
        .map_err(|_elapsed| ConnectError::Timeout(path.to_owned()))?
}

/// [`connect_unix`], waiting for the daemon as grpc-js's `waitForReady` does: a missing
/// socket fails at once (Electron checks for the file first), while a refused connection or
/// a failed handshake is retried on grpc-js's connect backoff until `deadline`, so a daemon
/// that starts accepting meanwhile (behind a fresh tunnel, say) is reached. Fails with the
/// last attempt's error once no attempt can start before `deadline`, or with
/// [`ConnectError::Timeout`] if the last attempt hangs past it. Every byte received from the
/// daemon is added to `received`.
pub async fn connect_unix_ready(
    path: &Path,
    received: ByteCounter,
    deadline: Instant,
) -> Result<Channel, ConnectError> {
    if !path.exists() {
        return Err(ConnectError::SocketMissing(path.to_owned()));
    }
    let mut backoff = CONNECT_BACKOFF_INITIAL;
    loop {
        let started = Instant::now();
        let error = match tokio::time::timeout_at(deadline, attempt(path, received.clone())).await {
            Ok(Ok(channel)) => return Ok(channel),
            Ok(Err(error)) => error,
            Err(_elapsed) => return Err(ConnectError::Timeout(path.to_owned())),
        };
        let next = started + backoff;
        if next >= deadline {
            return Err(error);
        }
        tracing::debug!(%error, "daemon not ready; retrying");
        tokio::time::sleep_until(next).await;
        backoff = backoff.mul_f64(CONNECT_BACKOFF_MULTIPLIER);
    }
}

/// One connect and handshake, however long they take.
async fn attempt(path: &Path, received: ByteCounter) -> Result<Channel, ConnectError> {
    let stream = UnixStream::connect(path)
        .await
        .map_err(|source| ConnectError::Io {
            path: path.to_owned(),
            source,
        })?;
    let slot = Arc::new(Mutex::new(Some(CountingStream::new(stream, received))));
    Endpoint::from_static("http://treeterm-daemon")
        .connect_with_connector(tower::service_fn(move |_: Uri| {
            let taken = slot.lock().expect("connector slot poisoned").take();
            async move {
                taken.map(TokioIo::new).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotConnected,
                        "daemon connection closed; a new connection generation is required",
                    )
                })
            }
        }))
        .await
        .map_err(|source| ConnectError::Transport {
            path: path.to_owned(),
            source,
        })
}

pub fn daemon_client(channel: Channel) -> DaemonClient {
    TreeTermDaemonClient::new(channel)
        .max_decoding_message_size(MAX_MESSAGE_SIZE)
        .max_encoding_message_size(MAX_MESSAGE_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("tt")
            .tempdir_in("/tmp")
            .unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn a_missing_socket_fails_at_once() {
        let dir = scratch();
        let started = Instant::now();
        let result = connect_unix_ready(
            &dir.path().join("d.sock"),
            ByteCounter::default(),
            started + CONNECT_TIMEOUT,
        )
        .await;
        assert!(
            matches!(result, Err(ConnectError::SocketMissing(_))),
            "{result:?}"
        );
        assert_eq!(started.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn a_refusing_socket_is_retried_on_grpc_js_backoff_until_the_deadline() {
        let dir = scratch();
        let path = dir.path().join("d.sock");
        // A stale socket: the file is there, nobody accepts.
        drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
        let started = Instant::now();
        let result =
            connect_unix_ready(&path, ByteCounter::default(), started + CONNECT_TIMEOUT).await;
        assert!(matches!(result, Err(ConnectError::Io { .. })), "{result:?}");
        // Attempts at 0 s, 1 s and 2.6 s; the next (5.16 s) would start past the deadline.
        assert_eq!(started.elapsed(), Duration::from_millis(2600));

        // The single try of a probe does not wait.
        let started = Instant::now();
        let probe = connect_unix(&path).await;
        assert!(matches!(probe, Err(ConnectError::Io { .. })), "{probe:?}");
        assert_eq!(started.elapsed(), Duration::ZERO);
    }
}
