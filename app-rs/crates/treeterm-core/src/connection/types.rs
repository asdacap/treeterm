//! Connection data shared by the manager, its state machine and its consumers (Electron's
//! `ConnectionTarget`, `ConnectionStatus`, `ConnectionInfo`, `PortForwardConfig`, …).

use std::fmt;
use std::sync::Arc;

use crate::settings::{PortForwardSpec, SshConnectionConfig};

use super::port_forward::PortForwardError;
use super::ssh::{SshError, TunnelEnd};

/// Identifies a connection: a random UUID for a local connection, the saved connection's id
/// for a remote one (so a remote host has at most one connection).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnectionId(String);

impl ConnectionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn random() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where the daemon runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionTarget {
    /// This machine's daemon, started if needed.
    Local,
    /// A daemon on an ssh host, bootstrapped and reached through an ssh socket tunnel.
    Remote(Arc<SshConnectionConfig>),
}

/// Step of a first connect, for progress display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectPhase {
    /// Checking (and if needed uploading and starting) the remote daemon.
    Bootstrap,
    /// Forwarding the remote daemon socket to a local one.
    Tunnel,
    /// Opening the gRPC connection to the daemon.
    Daemon,
}

/// What to do when the remote daemon binary differs from the local build
/// (Electron's `refreshDaemon` / `allowOutdatedDaemon` options).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutdatedDaemon {
    /// Stop with [`ConnectionError::DaemonHashMismatch`] so the user can choose.
    #[default]
    Report,
    /// Kill the remote daemon (and with it every remote PTY) and upload this build.
    Refresh,
    /// Connect to the outdated daemon anyway. Reconnects keep accepting it.
    Accept,
}

/// A connection's state, with what each state needs.
#[derive(Debug, Clone)]
pub enum ConnectionStatus<C> {
    /// The first connect is in progress.
    Connecting { phase: ConnectPhase },
    /// Usable. `daemon` is this connection generation; every reconnect brings a new one.
    Connected { daemon: C },
    /// Lost and retrying with backoff. `attempt` counts attempts started since the loss.
    Reconnecting {
        attempt: u32,
        error: ConnectionError,
    },
    /// Closed by [`super::ConnectionManager::disconnect`]; the connection is gone.
    Disconnected,
    /// The first connect failed, or reconnecting was cancelled. Retry with
    /// [`super::ConnectionManager::reconnect`].
    Error { error: ConnectionError },
}

impl<C> ConnectionStatus<C> {
    pub fn kind(&self) -> StatusKind {
        match self {
            ConnectionStatus::Connecting { .. } => StatusKind::Connecting,
            ConnectionStatus::Connected { .. } => StatusKind::Connected,
            ConnectionStatus::Reconnecting { .. } => StatusKind::Reconnecting,
            ConnectionStatus::Disconnected => StatusKind::Disconnected,
            ConnectionStatus::Error { .. } => StatusKind::Error,
        }
    }

    /// The error a reconnecting or failed connection shows.
    pub fn error(&self) -> Option<&ConnectionError> {
        match self {
            ConnectionStatus::Reconnecting { error, .. } | ConnectionStatus::Error { error } => {
                Some(error)
            }
            ConnectionStatus::Connecting { .. }
            | ConnectionStatus::Connected { .. }
            | ConnectionStatus::Disconnected => None,
        }
    }
}

/// [`ConnectionStatus`] without its data (Electron's `ConnectionStatus` string enum).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Connecting,
    Connected,
    Reconnecting,
    Disconnected,
    Error,
}

impl fmt::Display for StatusKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            StatusKind::Connecting => "connecting",
            StatusKind::Connected => "connected",
            StatusKind::Reconnecting => "reconnecting",
            StatusKind::Disconnected => "disconnected",
            StatusKind::Error => "error",
        })
    }
}

/// One connection as seen from outside.
#[derive(Debug, Clone)]
pub struct ConnectionInfo<C> {
    pub id: ConnectionId,
    pub target: ConnectionTarget,
    pub status: ConnectionStatus<C>,
}

/// Electron's `ConnectionErrorKind`: whether the error has a dedicated recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionErrorKind {
    Generic,
    /// Offer "refresh" ([`OutdatedDaemon::Refresh`]) or "connect anyway"
    /// ([`OutdatedDaemon::Accept`]).
    DaemonHashMismatch,
}

/// Why a connection is not (or no longer) connected. Messages match Electron's.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConnectionError {
    #[error(transparent)]
    Ssh(#[from] SshError),
    /// The remote daemon binary differs from the local build and [`OutdatedDaemon::Report`]
    /// was asked for (always the case on reconnect unless the user accepted it).
    #[error(
        "Daemon binary hash mismatch (local={}... remote={}...). The remote daemon is outdated. \
         Refresh it to re-upload (drops running processes), or connect anyway.",
        short_hash(.local_hash),
        short_hash(.remote_hash)
    )]
    DaemonHashMismatch {
        local_hash: String,
        remote_hash: String,
    },
    /// The local daemon could not be started or reached.
    #[error("{0}")]
    LocalDaemon(DialFailure),
    /// The tunnel is up but the remote daemon did not accept a connection through it.
    #[error("SSH tunnel OK, but daemon not responding: {0}")]
    RemoteDaemon(DialFailure),
    /// The session watch went quiet for [`super::machine::HEARTBEAT_TIMEOUT`].
    #[error("Connection lost (no heartbeat from daemon)")]
    HeartbeatLost,
    /// The session watch failed or ended: the gRPC connection is gone.
    #[error("gRPC connection lost: {0}")]
    SessionWatchFailed(String),
    /// The ssh process carrying the daemon socket ended.
    #[error(transparent)]
    TunnelClosed(TunnelEnd),
    /// [`super::ConnectionManager::force_reconnect`] while nothing was wrong.
    #[error("Reconnecting...")]
    ReconnectRequested,
    #[error("Reconnection cancelled")]
    ReconnectCancelled,
}

impl ConnectionError {
    pub fn kind(&self) -> ConnectionErrorKind {
        match self {
            ConnectionError::DaemonHashMismatch { .. } => ConnectionErrorKind::DaemonHashMismatch,
            ConnectionError::Ssh(_)
            | ConnectionError::LocalDaemon(_)
            | ConnectionError::RemoteDaemon(_)
            | ConnectionError::HeartbeatLost
            | ConnectionError::SessionWatchFailed(_)
            | ConnectionError::TunnelClosed(_)
            | ConnectionError::ReconnectRequested
            | ConnectionError::ReconnectCancelled => ConnectionErrorKind::Generic,
        }
    }
}

/// First 12 characters, as Electron's `hash.substring(0, 12)`.
pub(crate) fn short_hash(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

/// A daemon could not be connected to. Shared so statuses stay cheap to clone.
#[derive(Clone)]
pub struct DialFailure(Arc<dyn std::error::Error + Send + Sync>);

impl DialFailure {
    pub fn new(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self(Arc::new(error))
    }
}

impl fmt::Debug for DialFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.0, f)
    }
}

impl fmt::Display for DialFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&*self.0, f)
    }
}

/// Identifies a port forward across all connections.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PortForwardId(String);

impl PortForwardId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn random() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PortForwardId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether a forward belongs in the saved connection (Electron's `persist`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Persistence {
    /// Saved with the ssh connection and started on every connect.
    Saved,
    /// Owned by an application tab (e.g. a web app); dies with it, never saved.
    Ephemeral,
}

/// A local port forwarded to `remote_host:remote_port` as seen from the ssh host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortForwardConfig {
    pub id: PortForwardId,
    pub connection_id: ConnectionId,
    pub spec: PortForwardSpec,
    pub persistence: Persistence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortForwardStatus {
    /// Started less than [`super::port_forward::STABILIZATION`] ago.
    Connecting,
    /// Its ssh process has stayed up past the stabilization delay.
    Active,
    Error(PortForwardError),
    Stopped,
}

impl PortForwardStatus {
    /// Electron's status string.
    pub fn as_str(&self) -> &'static str {
        match self {
            PortForwardStatus::Connecting => "connecting",
            PortForwardStatus::Active => "active",
            PortForwardStatus::Error(_) => "error",
            PortForwardStatus::Stopped => "stopped",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortForwardInfo {
    pub config: PortForwardConfig,
    pub status: PortForwardStatus,
}

/// A connection's log streams (Electron's bootstrap/tunnel/daemon output panes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputKind {
    /// Remote daemon check, upload and start.
    Bootstrap,
    /// The ssh socket tunnel.
    Tunnel,
    /// The gRPC connection to the daemon.
    Daemon,
}
