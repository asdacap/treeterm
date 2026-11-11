//! Runs one [`ConnectionMachine`] on tokio: the shared state ([`Shared`]) that the manager
//! reads and commands synchronously, and the driver task that carries out the machine's
//! effects — connect attempts, heartbeat watch, tunnel and port forward processes — and
//! feeds their outcomes back.
//!
//! The driver owns every resource of the connection (attempt futures, daemon connection,
//! ssh processes). Aborting it (dropping its [`crate::rt::Fut`]) drops them all, which
//! kills the ssh processes; the daemon and its PTYs are untouched.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use futures::future::{BoxFuture, pending};
use futures::stream::BoxStream;
use futures::{FutureExt, StreamExt};
use tokio::sync::Notify;
use tokio_stream::StreamMap;

use crate::settings::SshConnectionConfig;

use super::dialer::Dialer;
use super::machine::{
    AttemptId, AttemptPlan, AttemptTarget, AttemptVerdict, ConnectionMachine, Effect, MachineEvent,
};
use super::output::{Listeners, OutputLog, Updates, Watch};
use super::process::{ProcessEvent, ProcessSpawner};
use super::socket_path::remote_forward_socket_path;
use super::ssh::{BootstrapResult, DaemonBinaries, LogSink, SshConnector, Tunnel};
use super::types::{
    ConnectPhase, ConnectionError, ConnectionInfo, OutputKind, PortForwardId, PortForwardInfo,
};

/// Where remote connections get their daemon binaries and tunnel sockets.
#[derive(Debug, Clone)]
pub struct SshEnvironment {
    pub daemon_binaries: DaemonBinaries,
    /// Directory for tunnel sockets; see [`super::socket_path::forward_socket_dir`].
    pub forward_socket_dir: PathBuf,
}

/// What every connection's driver uses.
pub(crate) struct Deps<D> {
    pub(crate) dialer: D,
    pub(crate) spawner: Arc<dyn ProcessSpawner>,
    pub(crate) ssh: SshEnvironment,
}

/// The current time on the clock tokio uses (paused in tests).
pub(crate) fn now() -> Instant {
    tokio::time::Instant::now().into_std()
}

// === shared state ============================================================================

struct LogStream {
    log: OutputLog,
    listeners: Listeners<String>,
}

pub(crate) struct Hub<C> {
    pub(crate) machine: ConnectionMachine<C>,
    bootstrap: LogStream,
    tunnel: LogStream,
    daemon: LogStream,
    status: Listeners<ConnectionInfo<C>>,
    forward_status: HashMap<PortForwardId, Listeners<PortForwardInfo>>,
    forward_output: HashMap<PortForwardId, Listeners<String>>,
    global: Arc<Mutex<Listeners<ConnectionInfo<C>>>>,
}

impl<C: Clone> Hub<C> {
    fn stream(&mut self, kind: OutputKind) -> &mut LogStream {
        match kind {
            OutputKind::Bootstrap => &mut self.bootstrap,
            OutputKind::Tunnel => &mut self.tunnel,
            OutputKind::Daemon => &mut self.daemon,
        }
    }

    /// Hand the machine's events to their listeners.
    fn dispatch(&mut self) {
        while let Some(event) = self.machine.poll_event() {
            match event {
                MachineEvent::Status(info) => {
                    self.status.send(&info);
                    lock(&self.global).send(&info);
                }
                MachineEvent::ForwardStatus(info) => {
                    if let Some(listeners) = self.forward_status.get_mut(&info.config.id) {
                        listeners.send(&info);
                    }
                }
                MachineEvent::ForwardOutput { id, line } => {
                    if let Some(listeners) = self.forward_output.get_mut(&id) {
                        listeners.send(&line);
                    }
                }
                MachineEvent::ForwardRemoved(id) => {
                    self.forward_status.remove(&id);
                    self.forward_output.remove(&id);
                }
            }
        }
    }
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().expect("connection state poisoned")
}

/// One connection's state, shared by the manager (commands, queries, watches) and its driver.
pub(crate) struct Shared<C> {
    hub: Mutex<Hub<C>>,
    /// Pokes the driver when the manager queued effects.
    wake: Notify,
}

impl<C: Clone + Send + 'static> Shared<C> {
    pub(crate) fn new(
        machine: ConnectionMachine<C>,
        global: Arc<Mutex<Listeners<ConnectionInfo<C>>>>,
    ) -> Arc<Self> {
        let mut hub = Hub {
            machine,
            bootstrap: LogStream {
                log: OutputLog::ssh(),
                listeners: Listeners::default(),
            },
            tunnel: LogStream {
                log: OutputLog::ssh(),
                listeners: Listeners::default(),
            },
            daemon: LogStream {
                log: OutputLog::daemon(),
                listeners: Listeners::default(),
            },
            status: Listeners::default(),
            forward_status: HashMap::new(),
            forward_output: HashMap::new(),
            global,
        };
        hub.dispatch();
        Arc::new(Self {
            hub: Mutex::new(hub),
            wake: Notify::new(),
        })
    }

    /// Feed the machine, dispatch what it reports and wake the driver if it has work.
    pub(crate) fn update<R>(&self, input: impl FnOnce(&mut ConnectionMachine<C>) -> R) -> R {
        let mut hub = lock(&self.hub);
        let result = input(&mut hub.machine);
        hub.dispatch();
        let has_effects = hub.machine.has_effects();
        drop(hub);
        if has_effects {
            self.wake.notify_one();
        }
        result
    }

    pub(crate) fn read<R>(&self, query: impl FnOnce(&ConnectionMachine<C>) -> R) -> R {
        query(&lock(&self.hub).machine)
    }

    pub(crate) fn log(&self, kind: OutputKind, line: String) {
        let mut hub = lock(&self.hub);
        let stream = hub.stream(kind);
        stream.log.push(line.clone());
        stream.listeners.send(&line);
    }

    fn clear_log(&self, kind: OutputKind) {
        lock(&self.hub).stream(kind).log.clear();
    }

    pub(crate) fn watch_status(&self) -> Watch<ConnectionInfo<C>, ConnectionInfo<C>> {
        let mut hub = lock(&self.hub);
        Watch {
            initial: hub.machine.info(),
            updates: hub.status.subscribe(),
        }
    }

    pub(crate) fn watch_output(&self, kind: OutputKind) -> Watch<Vec<String>, String> {
        let mut hub = lock(&self.hub);
        let stream = hub.stream(kind);
        Watch {
            initial: stream.log.snapshot(),
            updates: stream.listeners.subscribe(),
        }
    }

    /// `None` if the forward is not this connection's.
    pub(crate) fn watch_forward_status(
        &self,
        id: &PortForwardId,
    ) -> Option<Watch<PortForwardInfo, PortForwardInfo>> {
        let mut hub = lock(&self.hub);
        let initial = hub.machine.port_forward(id)?.info();
        let updates: Updates<PortForwardInfo> = hub
            .forward_status
            .entry(id.clone())
            .or_default()
            .subscribe();
        Some(Watch { initial, updates })
    }

    /// `None` if the forward is not this connection's.
    pub(crate) fn watch_forward_output(
        &self,
        id: &PortForwardId,
    ) -> Option<Watch<Vec<String>, String>> {
        let mut hub = lock(&self.hub);
        let initial = hub.machine.port_forward(id)?.output();
        let updates = hub
            .forward_output
            .entry(id.clone())
            .or_default()
            .subscribe();
        Some(Watch { initial, updates })
    }

    fn take_effects(&self) -> Vec<Effect> {
        let mut hub = lock(&self.hub);
        std::iter::from_fn(|| hub.machine.poll_effect()).collect()
    }
}

/// Attempt output into the connection's logs.
struct HubSink<C>(Arc<Shared<C>>);

impl<C: Clone + Send + 'static> LogSink for HubSink<C> {
    fn line(&self, kind: OutputKind, line: String) {
        self.0.log(kind, line);
    }
}

// === driver ==================================================================================

/// A live link to a daemon.
struct Transport<C> {
    daemon: C,
    link: Link,
}

enum Link {
    Local,
    /// Dropping the tunnel kills its ssh and removes its socket.
    Ssh(Tunnel),
}

type Attempt<C> = BoxFuture<'static, Result<Transport<C>, ConnectionError>>;

/// Carry out `shared`'s effects until aborted.
pub(crate) async fn drive<D: Dialer>(shared: Arc<Shared<D::Daemon>>, deps: Arc<Deps<D>>) {
    let connection = shared.read(|machine| machine.id().clone());
    let mut attempt: Option<(AttemptId, Attempt<D::Daemon>)> = None;
    let mut transport: Option<Transport<D::Daemon>> = None;
    let mut heartbeat: Option<BoxStream<'static, Result<(), String>>> = None;
    let mut forwards: StreamMap<PortForwardId, BoxStream<'static, (u64, ProcessEvent)>> =
        StreamMap::new();

    loop {
        for effect in shared.take_effects() {
            match effect {
                Effect::StartAttempt(plan) => {
                    let id = plan.id;
                    attempt = Some((id, connect(shared.clone(), deps.clone(), plan)));
                }
                Effect::CancelAttempt => attempt = None,
                Effect::DropTransport => {
                    heartbeat = None;
                    transport = None;
                }
                Effect::WatchHeartbeat => match &transport {
                    Some(live) => {
                        let listener = format!("heartbeat-{connection}-{}", uuid::Uuid::new_v4());
                        heartbeat = Some(deps.dialer.heartbeats(&live.daemon, &listener));
                    }
                    None => tracing::error!(%connection, "no transport to watch"),
                },
                Effect::SpawnForward {
                    id,
                    generation,
                    command,
                } => match deps.spawner.spawn(&command) {
                    Ok(process) => {
                        let events = process.map(move |event| (generation, event)).boxed();
                        // A replaced process is killed by being dropped.
                        drop(forwards.insert(id, events));
                    }
                    Err(error) => shared.update(|machine| {
                        machine.port_forward_spawn_failed(&id, generation, error.message);
                    }),
                },
                Effect::KillForward { id } => drop(forwards.remove(&id)),
            }
        }

        let deadline = shared.read(ConnectionMachine::poll_timeout);
        tokio::select! {
            () = shared.wake.notified() => {}
            () = sleep_until(deadline) => shared.update(|machine| machine.handle_timeout(now())),
            (id, result) = finished(&mut attempt) => {
                attempt = None;
                match result {
                    Ok(live) => {
                        let daemon = live.daemon.clone();
                        heartbeat = None;
                        transport = Some(live);
                        let verdict = shared.update(|machine| machine.attempt_finished(id, Ok(daemon), now()));
                        match verdict {
                            AttemptVerdict::Applied => {}
                            AttemptVerdict::Stale => transport = None,
                        }
                    }
                    Err(error) => {
                        let verdict = shared.update(|machine| machine.attempt_finished(id, Err(error), now()));
                        match verdict {
                            AttemptVerdict::Applied | AttemptVerdict::Stale => {}
                        }
                    }
                }
            }
            beat = next_beat(&mut heartbeat) => match beat {
                Some(Ok(())) => shared.update(|machine| machine.heartbeat(now())),
                Some(Err(reason)) => {
                    heartbeat = None;
                    shared.update(|machine| machine.heartbeat_failed(reason, now()));
                }
                None => {
                    heartbeat = None;
                    shared.update(|machine| machine.heartbeat_failed("session watch ended".to_owned(), now()));
                }
            },
            end = tunnel_end(&mut transport) => {
                // The link is dead whatever the machine makes of it.
                heartbeat = None;
                transport = None;
                shared.update(|machine| machine.tunnel_closed(end, now()));
            }
            Some((id, (generation, event))) = forwards.next(), if !forwards.is_empty() => {
                shared.update(|machine| machine.port_forward_event(&id, generation, event));
            }
        }
    }
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await,
        None => pending().await,
    }
}

async fn finished<C>(
    attempt: &mut Option<(AttemptId, Attempt<C>)>,
) -> (AttemptId, Result<Transport<C>, ConnectionError>) {
    match attempt {
        Some((id, future)) => {
            let result = future.await;
            (*id, result)
        }
        None => pending().await,
    }
}

async fn next_beat(
    heartbeat: &mut Option<BoxStream<'static, Result<(), String>>>,
) -> Option<Result<(), String>> {
    match heartbeat {
        Some(stream) => stream.next().await,
        None => pending().await,
    }
}

async fn tunnel_end<C>(transport: &mut Option<Transport<C>>) -> super::ssh::TunnelEnd {
    match transport {
        Some(Transport {
            link: Link::Ssh(tunnel),
            ..
        }) => tunnel.closed().await,
        Some(Transport {
            link: Link::Local, ..
        })
        | None => pending().await,
    }
}

/// One connect attempt: a new transport or why not.
fn connect<D: Dialer>(
    shared: Arc<Shared<D::Daemon>>,
    deps: Arc<Deps<D>>,
    plan: AttemptPlan,
) -> Attempt<D::Daemon> {
    async move {
        match plan.target {
            AttemptTarget::Local => {
                let daemon = deps
                    .dialer
                    .open_local()
                    .await
                    .map_err(ConnectionError::LocalDaemon)?;
                Ok(Transport {
                    daemon,
                    link: Link::Local,
                })
            }
            AttemptTarget::Remote { config, outdated } => {
                // A new tunnel starts with fresh bootstrap and tunnel logs (Electron's new
                // SSHTunnel per attempt); live watchers keep receiving.
                shared.clear_log(OutputKind::Bootstrap);
                shared.clear_log(OutputKind::Tunnel);
                let connector = SshConnector::new(
                    deps.spawner.clone(),
                    SshConnectionConfig::clone(&config),
                    outdated,
                    deps.ssh.daemon_binaries.clone(),
                    Arc::new(HubSink(shared.clone())),
                );
                let remote_socket = match connector.bootstrap().await? {
                    BootstrapResult::Connected { socket_path } => socket_path,
                    BootstrapResult::HashMismatch {
                        local_hash,
                        remote_hash,
                    } => {
                        return Err(ConnectionError::DaemonHashMismatch {
                            local_hash,
                            remote_hash,
                        });
                    }
                };
                shared.update(|machine| machine.connect_phase(plan.id, ConnectPhase::Tunnel));
                let local_socket =
                    remote_forward_socket_path(&deps.ssh.forward_socket_dir, &config.id);
                let tunnel = connector.open_tunnel(local_socket, &remote_socket).await?;
                shared.update(|machine| machine.connect_phase(plan.id, ConnectPhase::Daemon));
                shared.log(
                    OutputKind::Daemon,
                    "Connecting to daemon via gRPC...".to_owned(),
                );
                match deps.dialer.dial(tunnel.local_socket()).await {
                    Ok(daemon) => {
                        shared.log(OutputKind::Daemon, "Connected to daemon".to_owned());
                        Ok(Transport {
                            daemon,
                            link: Link::Ssh(tunnel),
                        })
                    }
                    Err(error) => {
                        shared.log(OutputKind::Daemon, format!("Failed: {error}"));
                        Err(ConnectionError::RemoteDaemon(error))
                    }
                }
            }
        }
    }
    .boxed()
}
