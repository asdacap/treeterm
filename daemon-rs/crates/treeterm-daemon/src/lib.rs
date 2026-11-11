//! TreeTerm daemon as a library: `serve` runs the gRPC service on a bound Unix listener.
//!
//! The `treeterm-daemon` binary (`main.rs`) owns process concerns — pid file, socket
//! path, signals. Keeping them out of here lets test harnesses (e.g. the native app's
//! testkit) run a real daemon in-process on a temporary socket and data directory.

pub mod connection_id;
mod exec_manager;
pub mod file_watcher;
pub mod filesystem;
mod grpc_server;
pub mod paths;
pub mod pty_manager;
pub mod session_store;
pub mod socket_path;

use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use tokio::net::UnixListener;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::Server;
use treeterm_proto::treeterm::tree_term_daemon_server::TreeTermDaemonServer;

/// Largest gRPC message either side may send.
pub const MAX_MESSAGE_SIZE: usize = 8 * 1024 * 1024;

pub struct DaemonConfig {
    /// Directory holding the per-workspace JSON files, advertised to clients as
    /// `Session.workspace_data_dir`.
    pub workspaces_dir: PathBuf,
}

/// Serve the daemon on `listener` until `shutdown` resolves, then kill all PTYs.
pub async fn serve(
    config: DaemonConfig,
    listener: UnixListener,
    shutdown: impl Future<Output = ()>,
) -> Result<(), tonic::transport::Error> {
    let session_store = session_store::SessionStore::new(config.workspaces_dir);
    session_store.start_heartbeat();

    let file_watcher = file_watcher::FileWatcher::new(Duration::from_secs(2));
    let svc = grpc_server::DaemonService::new(session_store.clone(), file_watcher);

    let shutdown = async move {
        shutdown.await;
        tracing::info!("shutting down");
        session_store.shutdown().await;
    };

    Server::builder()
        .layer(connection_id::ConnectionIdLayer)
        .add_service(
            TreeTermDaemonServer::new(svc)
                .max_decoding_message_size(MAX_MESSAGE_SIZE)
                .max_encoding_message_size(MAX_MESSAGE_SIZE),
        )
        .serve_with_incoming_shutdown(UnixListenerStream::new(listener), shutdown)
        .await
}
