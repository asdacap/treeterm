//! gRPC transport to a daemon over its Unix socket.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hyper_util::rt::TokioIo;
use tokio::net::UnixStream;
use tonic::transport::{Channel, Endpoint, Uri};
use treeterm_proto::treeterm::tree_term_daemon_client::TreeTermDaemonClient;

pub type DaemonClient = TreeTermDaemonClient<Channel>;

/// Largest message either side sends; matches the daemon's limit.
pub const MAX_MESSAGE_SIZE: usize = 8 * 1024 * 1024;

/// Electron waits this long for a fresh channel to become ready.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

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

/// Open one HTTP/2 connection to the daemon.
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
    let attempt = async {
        let stream = UnixStream::connect(path)
            .await
            .map_err(|source| ConnectError::Io {
                path: path.to_owned(),
                source,
            })?;
        let slot = Arc::new(Mutex::new(Some(stream)));
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
    };
    tokio::time::timeout(CONNECT_TIMEOUT, attempt)
        .await
        .map_err(|_elapsed| ConnectError::Timeout(path.to_owned()))?
}

pub fn daemon_client(channel: Channel) -> DaemonClient {
    TreeTermDaemonClient::new(channel)
        .max_decoding_message_size(MAX_MESSAGE_SIZE)
        .max_encoding_message_size(MAX_MESSAGE_SIZE)
}
