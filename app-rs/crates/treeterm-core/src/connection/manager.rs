//! The registry of connections (port of Electron's `ConnectionManager`).
//!
//! Every method is synchronous and cheap: it updates the connection's state machine under a
//! lock and returns; the connection's driver task (on the [`Rt`]'s tokio runtime) does the
//! waiting. Statuses, logs and port forward changes arrive on [`Updates`] streams that any
//! executor can poll. Dropping the manager (or disconnecting a connection) aborts the drivers,
//! which kills their ssh processes; daemons and their PTYs are left running.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures::StreamExt;
use indexmap::IndexMap;

use crate::rt::{Fut, Rt};
use crate::settings::{PortForwardSpec, SshConnectionConfig};

use super::dialer::Dialer;
use super::driver::{Deps, Shared, SshEnvironment, drive, lock, now};
use super::machine::ConnectionMachine;
use super::output::{self, Listeners, Updates, Watch};
use super::process::ProcessSpawner;
use super::ssh::{UploadError, upload_file};
use super::types::{
    ConnectionError, ConnectionId, ConnectionInfo, ConnectionStatus, ConnectionTarget,
    OutdatedDaemon, OutputKind, Persistence, PortForwardConfig, PortForwardId, PortForwardInfo,
    StatusKind,
};

/// A connection's status now and its changes.
pub type StatusWatch<C> = Watch<ConnectionInfo<C>, ConnectionInfo<C>>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Connection not found: {0}")]
pub struct ConnectionNotFound(pub ConnectionId);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Port forward not found: {0}")]
pub struct PortForwardNotFound(pub PortForwardId);

/// Why [`ConnectionManager::client`] has no daemon to hand out.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ClientError {
    #[error(transparent)]
    NotFound(#[from] ConnectionNotFound),
    /// Still on its first connect.
    #[error("Connection {0} is connecting")]
    Connecting(ConnectionId),
    /// Reconnecting or failed.
    #[error("Connection {id} is {status}: {error}")]
    Down {
        id: ConnectionId,
        status: StatusKind,
        error: ConnectionError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Connection not found or not remote: {0}")]
pub struct NotRemoteConnection(pub ConnectionId);

/// The first connect's outcome: resolves once the connection leaves
/// [`ConnectionStatus::Connecting`]. Dropping it does not cancel the connection.
pub struct Settled<C> {
    id: ConnectionId,
    updates: Updates<ConnectionInfo<C>>,
}

/// The connection went away (manager dropped, or the connection was replaced) before its
/// first connect finished.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("connection {0} went away while connecting")]
pub struct ConnectionGone(pub ConnectionId);

impl<C> Settled<C> {
    pub fn id(&self) -> &ConnectionId {
        &self.id
    }
}

impl<C> Future for Settled<C> {
    type Output = Result<ConnectionInfo<C>, ConnectionGone>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match self.updates.poll_next_unpin(cx) {
                Poll::Ready(Some(info)) => match info.status {
                    ConnectionStatus::Connecting { .. } => continue,
                    ConnectionStatus::Connected { .. }
                    | ConnectionStatus::Reconnecting { .. }
                    | ConnectionStatus::Disconnected
                    | ConnectionStatus::Error { .. } => return Poll::Ready(Ok(info)),
                },
                Poll::Ready(None) => return Poll::Ready(Err(ConnectionGone(self.id.clone()))),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

struct Entry<C> {
    shared: Arc<Shared<C>>,
    /// Aborts the driver (and everything it owns) when the entry is dropped.
    _driver: Fut<()>,
}

/// All connections (local + remotes), each with its reconnect loop and port forwards.
pub struct ConnectionManager<D: Dialer> {
    rt: Rt,
    deps: Arc<Deps<D>>,
    connections: IndexMap<ConnectionId, Entry<D::Daemon>>,
    global: Arc<Mutex<Listeners<ConnectionInfo<D::Daemon>>>>,
}

impl<D: Dialer> ConnectionManager<D> {
    pub fn new(rt: Rt, dialer: D, spawner: Arc<dyn ProcessSpawner>, ssh: SshEnvironment) -> Self {
        Self {
            rt,
            deps: Arc::new(Deps {
                dialer,
                spawner,
                ssh,
            }),
            connections: IndexMap::new(),
            global: Arc::new(Mutex::new(Listeners::default())),
        }
    }

    /// A new connection to this machine's daemon (started if needed), with a random id.
    pub fn connect_local(&mut self) -> Settled<D::Daemon> {
        self.start(
            ConnectionId::random(),
            ConnectionTarget::Local,
            OutdatedDaemon::Report,
        )
    }

    /// Connect to `config`'s host under `config.id`. An already connected (or connecting)
    /// connection is returned as is; a failed or reconnecting one is replaced.
    pub fn connect_remote(
        &mut self,
        config: SshConnectionConfig,
        outdated: OutdatedDaemon,
    ) -> Settled<D::Daemon> {
        let id = ConnectionId::new(config.id.clone());
        if let Some(entry) = self.connections.get(&id) {
            match entry.shared.read(|machine| machine.status().kind()) {
                StatusKind::Connected | StatusKind::Connecting => {
                    return settled(&id, &entry.shared);
                }
                StatusKind::Reconnecting | StatusKind::Error | StatusKind::Disconnected => {}
            }
        }
        tracing::info!(%id, host = %config.host, port = config.port, ?outdated, "connecting over ssh");
        self.start(id, ConnectionTarget::Remote(Arc::new(config)), outdated)
    }

    fn start(
        &mut self,
        id: ConnectionId,
        target: ConnectionTarget,
        outdated: OutdatedDaemon,
    ) -> Settled<D::Daemon> {
        let machine = ConnectionMachine::start(id.clone(), target, outdated);
        let shared = Shared::new(machine, self.global.clone());
        let driver = self.rt.run(drive(shared.clone(), self.deps.clone()));
        let settled = settled(&id, &shared);
        // A stale entry with the same id is replaced: dropping it aborts its driver, which
        // stops its reconnect loop and port forwards (they would hold the local ports the
        // new connection's forwards need).
        drop(self.connections.insert(
            id,
            Entry {
                shared,
                _driver: driver,
            },
        ));
        settled
    }

    /// The current daemon connection, if connected.
    pub fn client(&self, id: &ConnectionId) -> Result<D::Daemon, ClientError> {
        let entry = self.entry(id)?;
        match entry.shared.read(ConnectionMachine::status) {
            ConnectionStatus::Connected { daemon } => Ok(daemon),
            // A disconnected connection is removed at once; for the instant before, it is
            // as good as not found.
            ConnectionStatus::Disconnected => Err(ConnectionNotFound(id.clone()).into()),
            ConnectionStatus::Connecting { .. } => Err(ClientError::Connecting(id.clone())),
            ConnectionStatus::Reconnecting { error, .. } => Err(ClientError::Down {
                id: id.clone(),
                status: StatusKind::Reconnecting,
                error,
            }),
            ConnectionStatus::Error { error } => Err(ClientError::Down {
                id: id.clone(),
                status: StatusKind::Error,
                error,
            }),
        }
    }

    pub fn list_connections(&self) -> Vec<ConnectionInfo<D::Daemon>> {
        self.connections
            .values()
            .map(|entry| entry.shared.read(ConnectionMachine::info))
            .collect()
    }

    pub fn connection(&self, id: &ConnectionId) -> Option<ConnectionInfo<D::Daemon>> {
        self.connections
            .get(id)
            .map(|entry| entry.shared.read(ConnectionMachine::info))
    }

    /// Scrollback and new lines of one of the connection's logs. The stream ends when the
    /// connection is removed.
    pub fn watch_output(
        &self,
        id: &ConnectionId,
        kind: OutputKind,
    ) -> Result<Watch<Vec<String>, String>, ConnectionNotFound> {
        Ok(self.entry(id)?.shared.watch_output(kind))
    }

    /// The connection's status now and every change.
    pub fn watch_connection_status(
        &self,
        id: &ConnectionId,
    ) -> Result<StatusWatch<D::Daemon>, ConnectionNotFound> {
        Ok(self.entry(id)?.shared.watch_status())
    }

    /// Every status change of every connection (Electron's `onStatusChange`).
    pub fn status_changes(&self) -> Updates<ConnectionInfo<D::Daemon>> {
        lock(&self.global).subscribe()
    }

    /// Close the connection: stop its reconnect loop, tunnel and port forwards, report
    /// [`ConnectionStatus::Disconnected`] and forget it. No-op for an unknown id.
    pub fn disconnect(&mut self, id: &ConnectionId) {
        if let Some(entry) = self.connections.shift_remove(id) {
            entry.shared.update(ConnectionMachine::disconnect);
        }
    }

    pub fn disconnect_all(&mut self) {
        let ids: Vec<ConnectionId> = self.connections.keys().cloned().collect();
        for id in ids {
            self.disconnect(&id);
        }
    }

    /// Retry a failed connection. No-op unless it is in the error state.
    pub fn reconnect(&self, id: &ConnectionId) {
        if let Some(entry) = self.connections.get(id) {
            entry.shared.update(|machine| machine.reconnect(now()));
        }
    }

    /// Skip the backoff wait of a reconnecting connection.
    pub fn reconnect_now(&self, id: &ConnectionId) {
        if let Some(entry) = self.connections.get(id) {
            entry.shared.update(ConnectionMachine::reconnect_now);
        }
    }

    /// Reconnect even though nothing looks wrong (e.g. after the network changed).
    pub fn force_reconnect(&self, id: &ConnectionId) -> Result<(), ConnectionNotFound> {
        self.entry(id)?
            .shared
            .update(|machine| machine.force_reconnect(now()));
        Ok(())
    }

    /// Stop a reconnect loop; the connection shows its last error.
    pub fn cancel_reconnect(&self, id: &ConnectionId) {
        if let Some(entry) = self.connections.get(id) {
            entry.shared.update(ConnectionMachine::cancel_reconnect);
        }
    }

    /// Start a port forward through a remote connection's host.
    pub fn add_port_forward(
        &self,
        config: PortForwardConfig,
    ) -> Result<PortForwardInfo, NotRemoteConnection> {
        let id = config.connection_id.clone();
        let entry = self
            .connections
            .get(&id)
            .ok_or_else(|| NotRemoteConnection(id.clone()))?;
        entry
            .shared
            .update(|machine| machine.add_port_forward(config, now()))
            .map_err(|_not_remote| NotRemoteConnection(id))
    }

    /// Start the connection's saved forwards (Electron's `autoStartPortForwards`), each with
    /// a new id.
    pub fn start_saved_port_forwards(
        &self,
        id: &ConnectionId,
    ) -> Result<Vec<PortForwardInfo>, NotRemoteConnection> {
        let entry = self
            .connections
            .get(id)
            .ok_or_else(|| NotRemoteConnection(id.clone()))?;
        let ConnectionTarget::Remote(config) =
            entry.shared.read(|machine| machine.target().clone())
        else {
            return Err(NotRemoteConnection(id.clone()));
        };
        config
            .port_forwards
            .iter()
            .cloned()
            .map(|spec| {
                self.add_port_forward(PortForwardConfig {
                    id: PortForwardId::random(),
                    connection_id: id.clone(),
                    spec,
                    persistence: Persistence::Saved,
                })
            })
            .collect()
    }

    /// The connection's [`Persistence::Saved`] forwards, to write back into the saved
    /// connection (Electron's `syncSavedPortForwards`).
    pub fn saved_port_forward_specs(&self, id: &ConnectionId) -> Vec<PortForwardSpec> {
        self.list_port_forwards(id)
            .into_iter()
            .filter(|info| info.config.persistence == Persistence::Saved)
            .map(|info| info.config.spec)
            .collect()
    }

    /// Stop and forget a forward, whichever connection has it. No-op for an unknown id.
    pub fn remove_port_forward(&self, id: &PortForwardId) {
        for entry in self.connections.values() {
            if entry
                .shared
                .update(|machine| machine.remove_port_forward(id))
            {
                return;
            }
        }
    }

    /// Empty for an unknown connection.
    pub fn list_port_forwards(&self, id: &ConnectionId) -> Vec<PortForwardInfo> {
        self.connections
            .get(id)
            .map(|entry| entry.shared.read(ConnectionMachine::port_forwards))
            .unwrap_or_default()
    }

    pub fn watch_port_forward_output(
        &self,
        id: &PortForwardId,
    ) -> Result<Watch<Vec<String>, String>, PortForwardNotFound> {
        self.connections
            .values()
            .find_map(|entry| entry.shared.watch_forward_output(id))
            .ok_or_else(|| PortForwardNotFound(id.clone()))
    }

    pub fn watch_port_forward_status(
        &self,
        id: &PortForwardId,
    ) -> Result<Watch<PortForwardInfo, PortForwardInfo>, PortForwardNotFound> {
        self.connections
            .values()
            .find_map(|entry| entry.shared.watch_forward_status(id))
            .ok_or_else(|| PortForwardNotFound(id.clone()))
    }

    /// Copy a local file to an absolute path on a remote connection's host with `scp`.
    pub fn upload_file(
        &self,
        id: &ConnectionId,
        local_path: PathBuf,
        remote_path: String,
    ) -> Result<Fut<Result<(), UploadError>>, NotRemoteConnection> {
        let target = self
            .connections
            .get(id)
            .map(|entry| entry.shared.read(|machine| machine.target().clone()));
        let Some(ConnectionTarget::Remote(config)) = target else {
            return Err(NotRemoteConnection(id.clone()));
        };
        let spawner = self.deps.spawner.clone();
        Ok(self
            .rt
            .run(async move { upload_file(&*spawner, &config, &local_path, &remote_path).await }))
    }

    fn entry(&self, id: &ConnectionId) -> Result<&Entry<D::Daemon>, ConnectionNotFound> {
        self.connections
            .get(id)
            .ok_or_else(|| ConnectionNotFound(id.clone()))
    }
}

fn settled<C: Clone + Send + 'static>(id: &ConnectionId, shared: &Shared<C>) -> Settled<C> {
    let watch = shared.watch_status();
    let updates = match watch.initial.status {
        ConnectionStatus::Connecting { .. } => watch.updates,
        ConnectionStatus::Connected { .. }
        | ConnectionStatus::Reconnecting { .. }
        | ConnectionStatus::Disconnected
        | ConnectionStatus::Error { .. } => output::once(watch.initial),
    };
    Settled {
        id: id.clone(),
        updates,
    }
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
