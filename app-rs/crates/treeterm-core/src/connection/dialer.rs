//! How the connection manager reaches daemons, behind a trait so its logic runs against a
//! fake daemon in unit tests. [`GrpcDialer`] is the real one.

use std::path::Path;

use futures::future::BoxFuture;
use futures::stream::BoxStream;
use futures::{FutureExt, StreamExt};

use crate::daemon::DaemonConnection;
use crate::local_daemon::{LocalDaemonSpec, ensure_running};
use crate::rt::Rt;

use super::types::DialFailure;

/// Opens daemon connections and watches their liveness.
pub trait Dialer: Send + Sync + 'static {
    /// One connection generation (cheap to clone; clones share the connection).
    type Daemon: Clone + Send + Sync + 'static;

    /// Start this machine's daemon if it is not running, and connect to it.
    fn open_local(&self) -> BoxFuture<'static, Result<Self::Daemon, DialFailure>>;

    /// Connect to the daemon behind `socket_path` (a tunnel's local end).
    fn dial(&self, socket_path: &Path) -> BoxFuture<'static, Result<Self::Daemon, DialFailure>>;

    /// One item per session watch event (the first is the current session; the daemon
    /// re-broadcasts every 15 s). Errs or ends when the connection is gone.
    fn heartbeats(
        &self,
        daemon: &Self::Daemon,
        listener_id: &str,
    ) -> BoxStream<'static, Result<(), String>>;
}

/// The daemon over gRPC: [`ensure_running`] + [`DaemonConnection::connect`] locally,
/// [`DaemonConnection::connect`] through tunnels, `SessionWatch` as heartbeat.
pub struct GrpcDialer {
    rt: Rt,
    local: LocalDaemonSpec,
}

impl GrpcDialer {
    pub fn new(rt: Rt, local: LocalDaemonSpec) -> Self {
        Self { rt, local }
    }
}

impl Dialer for GrpcDialer {
    type Daemon = DaemonConnection;

    fn open_local(&self) -> BoxFuture<'static, Result<DaemonConnection, DialFailure>> {
        let rt = self.rt.clone();
        let spec = self.local.clone();
        async move {
            let start = ensure_running(&spec).await.map_err(DialFailure::new)?;
            tracing::info!(?start, socket = ?spec.socket_path, "local daemon ready");
            DaemonConnection::connect(rt, spec.socket_path)
                .await
                .map_err(DialFailure::new)
        }
        .boxed()
    }

    fn dial(
        &self,
        socket_path: &Path,
    ) -> BoxFuture<'static, Result<DaemonConnection, DialFailure>> {
        DaemonConnection::connect(self.rt.clone(), socket_path.to_owned())
            .map(|result| result.map_err(DialFailure::new))
            .boxed()
    }

    fn heartbeats(
        &self,
        daemon: &DaemonConnection,
        listener_id: &str,
    ) -> BoxStream<'static, Result<(), String>> {
        daemon
            .watch_session(listener_id)
            .map(|event| event.map(|_| ()).map_err(|error| error.to_string()))
            .boxed()
    }
}
