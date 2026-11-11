//! Connections to daemons: the local one and remote ones over ssh (port of Electron's
//! `connectionManager.ts`, `ssh.ts`, `portForward.ts` and `socketPath.ts`).
//!
//! - [`ConnectionManager`] is the registry a model owns. Its methods are synchronous; each
//!   connection's work runs in a driver task on the [`crate::rt::Rt`] runtime.
//! - [`machine::ConnectionMachine`] holds all of one connection's logic (statuses, heartbeat,
//!   reconnect backoff, port forwards) without I/O, so it is tested with made-up instants.
//! - [`ssh`] bootstraps the remote daemon and tunnels its socket; [`port_forward`] is one
//!   `ssh -L` forward. Both spawn processes only through [`process::ProcessSpawner`]
//!   ([`process::TokioSpawner`] for real, [`fake::FakeSpawner`] in tests).
//! - [`dialer::Dialer`] opens daemon connections ([`dialer::GrpcDialer`] for real).
//!
//! # Driving it from a model
//!
//! ```ignore
//! let ssh = SshEnvironment {
//!     daemon_binaries: DaemonBinaries::new(resources.join("daemon-rs")),
//!     forward_socket_dir: socket_path::forward_socket_dir(paths::current_uid()),
//! };
//! let mut manager =
//!     ConnectionManager::new(rt.clone(), GrpcDialer::new(rt, local_spec), Arc::new(TokioSpawner), ssh);
//! // Subscribe first: every status of every connection, on any executor.
//! let mut changes = manager.status_changes();
//! // Resolves with the first non-`Connecting` status; dropping it does not cancel.
//! let local = manager.connect_local().await?;
//! let remote = manager.connect_remote(saved, OutdatedDaemon::Report).await?;
//! while let Some(info) = changes.next().await {
//!     match info.status {
//!         // A new generation: (re)build this connection's stores on `daemon`.
//!         ConnectionStatus::Connected { daemon } => {}
//!         // The generation is dead: dispose its stores (explicitly; see app_store.rs).
//!         ConnectionStatus::Reconnecting { attempt, error } => {}
//!         ConnectionStatus::Error { error } => {} // `DaemonHashMismatch` → offer refresh / accept
//!         ConnectionStatus::Connecting { phase } | ConnectionStatus::Disconnected => {}
//!     }
//! }
//! ```
//!
//! User actions map to `reconnect` (retry from error), `reconnect_now`, `force_reconnect`,
//! `cancel_reconnect`, `disconnect`. Logs come from `watch_output(id, OutputKind::…)`,
//! forwards from `add_port_forward` / `start_saved_port_forwards` and
//! `watch_port_forward_status` / `watch_port_forward_output`; each watch is the scrollback
//! (or current value) plus an [`output::Updates`] stream that ends when its source goes away.

pub mod dialer;
mod driver;
pub mod fake;
pub mod machine;
mod manager;
pub mod output;
pub mod port_forward;
pub mod process;
pub mod socket_path;
pub mod ssh;
mod types;

#[cfg(test)]
mod test_support;

pub use driver::SshEnvironment;
pub use manager::{
    ClientError, ConnectionGone, ConnectionManager, ConnectionNotFound, NotRemoteConnection,
    PortForwardNotFound, Settled, StatusWatch,
};
pub use types::{
    ConnectPhase, ConnectionError, ConnectionErrorKind, ConnectionId, ConnectionInfo,
    ConnectionStatus, ConnectionTarget, DialFailure, OutdatedDaemon, OutputKind, Persistence,
    PortForwardConfig, PortForwardId, PortForwardInfo, PortForwardStatus, StatusKind,
};
