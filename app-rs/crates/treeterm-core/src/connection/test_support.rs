//! Fakes for the manager tests: a daemon dialer whose connections and heartbeats the test
//! controls, and an ssh host simulated through the fake spawner.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::channel::{mpsc, oneshot};
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use futures::{FutureExt, StreamExt};
use sha2::{Digest, Sha256};

use super::dialer::Dialer;
use super::fake::{FakeProcess, FakeResponse, FakeSpawner};
use super::process::ProcessCommand;
use super::ssh::{DaemonBinaries, RemotePlatform};
use super::types::{ConnectionId, DialFailure};
use super::{ConnectionInfo, ConnectionManager, OutdatedDaemon, SshEnvironment};
use crate::rt::Rt;
use crate::settings::{ConnectionLabel, Extra, IdentityFile, SshConnectionConfig};

/// One fake connection generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FakeDaemon {
    pub(crate) serial: u32,
    pub(crate) socket: PathBuf,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct FakeDialError(pub(crate) String);

enum DialPlan {
    Fail(String),
    Gate(oneshot::Receiver<()>),
}

struct Watch {
    listener_id: String,
    serial: u32,
    tx: mpsc::UnboundedSender<Result<(), String>>,
}

#[derive(Default)]
struct DialerState {
    serial: u32,
    dials: Vec<PathBuf>,
    plans: VecDeque<DialPlan>,
    /// Applied when `plans` is empty.
    always_fail: Option<String>,
    watches: Vec<Watch>,
}

pub(crate) const LOCAL_SOCKET: &str = "/fake/local.sock";

#[derive(Clone, Default)]
pub(crate) struct FakeDialer {
    state: Arc<Mutex<DialerState>>,
}

impl FakeDialer {
    fn state(&self) -> std::sync::MutexGuard<'_, DialerState> {
        self.state.lock().unwrap()
    }

    /// The next dial fails with `message`.
    pub(crate) fn fail_next(&self, message: &str) {
        self.state()
            .plans
            .push_back(DialPlan::Fail(message.to_owned()));
    }

    /// Every dial without a plan fails (until [`Self::stop_failing`]).
    pub(crate) fn fail_always(&self, message: &str) {
        self.state().always_fail = Some(message.to_owned());
    }

    pub(crate) fn stop_failing(&self) {
        self.state().always_fail = None;
    }

    /// The next dial waits until the returned sender fires (or is dropped).
    pub(crate) fn gate_next(&self) -> oneshot::Sender<()> {
        let (tx, rx) = oneshot::channel();
        self.state().plans.push_back(DialPlan::Gate(rx));
        tx
    }

    pub(crate) fn dials(&self) -> Vec<PathBuf> {
        self.state().dials.clone()
    }

    /// Listener ids of the heartbeat watches opened, in order.
    pub(crate) fn watch_listeners(&self) -> Vec<String> {
        self.state()
            .watches
            .iter()
            .map(|watch| watch.listener_id.clone())
            .collect()
    }

    /// Whether the newest watch on `daemon` was dropped by its owner.
    pub(crate) fn watch_dropped(&self, daemon: &FakeDaemon) -> bool {
        self.state()
            .watches
            .iter()
            .rev()
            .find(|watch| watch.serial == daemon.serial)
            .expect("a heartbeat watch on this daemon")
            .tx
            .is_closed()
    }

    pub(crate) fn has_watch(&self, daemon: &FakeDaemon) -> bool {
        self.state()
            .watches
            .iter()
            .any(|watch| watch.serial == daemon.serial)
    }

    pub(crate) fn beat(&self, daemon: &FakeDaemon) {
        self.send(daemon, Ok(()));
    }

    /// The session watch on `daemon` fails (the gRPC connection dropped).
    pub(crate) fn fail_watch(&self, daemon: &FakeDaemon, message: &str) {
        self.send(daemon, Err(message.to_owned()));
    }

    fn send(&self, daemon: &FakeDaemon, item: Result<(), String>) {
        for watch in self
            .state()
            .watches
            .iter()
            .filter(|watch| watch.serial == daemon.serial)
        {
            if watch.tx.unbounded_send(item.clone()).is_err() {
                // That watch was dropped: its connection generation is gone.
            }
        }
    }
}

impl Dialer for FakeDialer {
    type Daemon = FakeDaemon;

    fn open_local(&self) -> BoxFuture<'static, Result<FakeDaemon, DialFailure>> {
        self.dial(Path::new(LOCAL_SOCKET))
    }

    fn dial(&self, socket_path: &Path) -> BoxFuture<'static, Result<FakeDaemon, DialFailure>> {
        let mut state = self.state();
        state.dials.push(socket_path.to_owned());
        let plan = state.plans.pop_front();
        let always_fail = state.always_fail.clone();
        state.serial += 1;
        let daemon = FakeDaemon {
            serial: state.serial,
            socket: socket_path.to_owned(),
        };
        async move {
            match (plan, always_fail) {
                (Some(DialPlan::Fail(message)), _) | (None, Some(message)) => {
                    Err(DialFailure::new(FakeDialError(message)))
                }
                (Some(DialPlan::Gate(gate)), _) => match gate.await {
                    Ok(()) | Err(oneshot::Canceled) => Ok(daemon),
                },
                (None, None) => Ok(daemon),
            }
        }
        .boxed()
    }

    fn heartbeats(
        &self,
        daemon: &FakeDaemon,
        listener_id: &str,
    ) -> BoxStream<'static, Result<(), String>> {
        let (tx, rx) = mpsc::unbounded();
        self.state().watches.push(Watch {
            listener_id: listener_id.to_owned(),
            serial: daemon.serial,
            tx,
        });
        rx.boxed()
    }
}

// === ssh host ================================================================================

pub(crate) const REMOTE_SOCKET: &str = "/tmp/treeterm-1000/daemon.sock";
pub(crate) const BINARY: &[u8] = b"mock-binary";

pub(crate) fn binary_hash() -> String {
    hex::encode(Sha256::digest(BINARY))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpawnKind {
    Bootstrap,
    Tunnel,
    PortForward,
    Scp,
    /// Kill or start scripts.
    Command,
}

pub(crate) fn kind_of(command: &ProcessCommand) -> SpawnKind {
    let last = command.args.last().map(String::as_str).unwrap_or_default();
    if command.program == "scp" {
        SpawnKind::Scp
    } else if last.contains("TREETERM_OS") {
        SpawnKind::Bootstrap
    } else if command.args.iter().any(|arg| arg == "-L") {
        if command.args.iter().any(|arg| arg.contains(".sock:")) {
            SpawnKind::Tunnel
        } else {
            SpawnKind::PortForward
        }
    } else {
        SpawnKind::Command
    }
}

/// The local end of a tunnel command's `-L local:remote`.
pub(crate) fn forwarded_socket(command: &ProcessCommand) -> PathBuf {
    let spec = command
        .args
        .iter()
        .skip_while(|arg| *arg != "-L")
        .nth(1)
        .expect("tunnel command has -L");
    PathBuf::from(spec.split(':').next().unwrap())
}

#[derive(Default)]
struct HostState {
    remote_hash: Option<String>,
    bootstraps: VecDeque<FakeResponse>,
}

/// A healthy host by default: a daemon of this build listens on [`REMOTE_SOCKET`], tunnels
/// come up, forwards run.
#[derive(Clone, Default)]
pub(crate) struct FakeHost {
    state: Arc<Mutex<HostState>>,
}

impl FakeHost {
    /// The cached daemon binary's hash from now on.
    pub(crate) fn set_remote_hash(&self, hash: &str) {
        self.state.lock().unwrap().remote_hash = Some(hash.to_owned());
    }

    /// The next bootstrap answers this way instead.
    pub(crate) fn next_bootstrap(&self, response: FakeResponse) {
        self.state.lock().unwrap().bootstraps.push_back(response);
    }

    fn respond(&self, command: &ProcessCommand) -> FakeResponse {
        let mut state = self.state.lock().unwrap();
        match kind_of(command) {
            SpawnKind::Bootstrap => state.bootstraps.pop_front().unwrap_or_else(|| {
                let hash = state.remote_hash.clone().unwrap_or_else(binary_hash);
                FakeResponse::stdout(format!(
                    "TREETERM_OS:Linux\nTREETERM_ARCH:x86_64\nTREETERM_HOME:/home/test\nTREETERM_REMOTE_HASH:{hash}\nTREETERM_SOCKET:{REMOTE_SOCKET}\n"
                ))
            }),
            SpawnKind::Tunnel => {
                let socket = forwarded_socket(command);
                std::fs::write(&socket, b"").expect("create the fake tunnel socket");
                FakeResponse::Run
            }
            SpawnKind::PortForward => FakeResponse::Run,
            SpawnKind::Scp => FakeResponse::exit(0),
            SpawnKind::Command => FakeResponse::stdout(format!("TREETERM_SOCKET:{REMOTE_SOCKET}\n")),
        }
    }
}

// === harness =================================================================================

pub(crate) struct Harness {
    pub(crate) manager: ConnectionManager<FakeDialer>,
    pub(crate) dialer: FakeDialer,
    pub(crate) spawner: FakeSpawner,
    pub(crate) host: FakeHost,
    pub(crate) dir: tempfile::TempDir,
}

pub(crate) fn remote_config() -> SshConnectionConfig {
    SshConnectionConfig {
        id: "remote-1".to_owned(),
        host: "example.com".to_owned(),
        user: "test".to_owned(),
        port: 22,
        identity_file: IdentityFile::SshDefault,
        label: ConnectionLabel::UserAtHost,
        port_forwards: Vec::new(),
        extra: Extra::new(),
    }
}

pub(crate) fn remote_id() -> ConnectionId {
    ConnectionId::new("remote-1")
}

impl Harness {
    /// Must be created on the (paused) test runtime.
    pub(crate) fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("tt")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::write(dir.path().join("treeterm-daemon-x86_64-linux"), BINARY).unwrap();
        let host = FakeHost::default();
        let responder_host = host.clone();
        let spawner = FakeSpawner::with_responder(move |command| responder_host.respond(command));
        let dialer = FakeDialer::default();
        let ssh = SshEnvironment {
            daemon_binaries: DaemonBinaries::with_local_platform(
                dir.path().to_owned(),
                RemotePlatform {
                    os: "darwin".to_owned(),
                    arch: "aarch64".to_owned(),
                },
            ),
            forward_socket_dir: dir.path().join("s"),
        };
        let manager = ConnectionManager::new(
            Rt::new(tokio::runtime::Handle::current()),
            dialer.clone(),
            Arc::new(spawner.clone()),
            ssh,
        );
        Self {
            manager,
            dialer,
            spawner,
            host,
            dir,
        }
    }

    /// A harness with one connected local connection (the TS suite's `beforeEach`).
    pub(crate) async fn with_local() -> (Self, ConnectionId) {
        let mut harness = Self::new();
        let info = harness.manager.connect_local().await.unwrap();
        (harness, info.id)
    }

    pub(crate) async fn connect_remote(&mut self) -> ConnectionInfo<FakeDaemon> {
        self.connect_remote_with(OutdatedDaemon::Report).await
    }

    pub(crate) async fn connect_remote_with(
        &mut self,
        outdated: OutdatedDaemon,
    ) -> ConnectionInfo<FakeDaemon> {
        self.manager
            .connect_remote(remote_config(), outdated)
            .await
            .unwrap()
    }

    pub(crate) fn spawned(&self, kind: SpawnKind) -> Vec<FakeProcess> {
        self.spawner
            .processes()
            .into_iter()
            .filter(|process| kind_of(process.command()) == kind)
            .collect()
    }

    pub(crate) fn latest(&self, kind: SpawnKind) -> FakeProcess {
        self.spawned(kind)
            .pop()
            .unwrap_or_else(|| panic!("no {kind:?} spawned"))
    }
}

/// Let every ready task run (and timers due within 1ms fire).
pub(crate) async fn flush() {
    tokio::time::sleep(Duration::from_millis(1)).await;
}

/// Let `duration` of fake time pass, running everything due on the way.
pub(crate) async fn advance(duration: Duration) {
    tokio::time::sleep(duration).await;
    flush().await;
}
