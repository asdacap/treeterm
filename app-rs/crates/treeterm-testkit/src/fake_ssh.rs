//! A fake ssh host (no ssh in tests) whose daemon is a [`crate::TestDaemon`]: bootstraps
//! report the daemon's socket, and each tunnel is a [`Proxy`] from the forwarded local
//! socket to it. Cutting the proxies closes every tunnelled connection, as a dead tunnel
//! would.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinSet;
use treeterm_core::connection::SshEnvironment;
use treeterm_core::connection::fake::{FakeResponse, FakeSpawner};
use treeterm_core::connection::process::ProcessCommand;
use treeterm_core::connection::ssh::DaemonBinaries;
use treeterm_core::settings::{ConnectionLabel, Extra, IdentityFile, SshConnectionConfig};

/// Forwards connections on `listen` to `target` until dropped; dropping it cuts them all.
pub struct Proxy {
    task: tokio::task::JoinHandle<()>,
}

impl Proxy {
    /// Must be called on a tokio runtime, which keeps driving the proxy.
    pub fn start(listen: &Path, target: &Path) -> Self {
        match std::fs::remove_file(listen) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("cannot clear {listen:?}: {error}"),
        }
        let listener = UnixListener::bind(listen).expect("bind proxy");
        let target = target.to_owned();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                let (mut inbound, _) = listener.accept().await.expect("proxy accept");
                let target = target.clone();
                connections.spawn(async move {
                    let mut outbound = UnixStream::connect(&target).await.expect("proxy dial");
                    // Ends when either side closes; nothing to report then.
                    if tokio::io::copy_bidirectional(&mut inbound, &mut outbound)
                        .await
                        .is_err()
                    {}
                });
            }
        });
        Self { task }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Where the connection manager looks for daemon binaries and puts tunnel sockets: `dir`
/// (keep it short, macOS caps socket paths at ~104 bytes).
pub fn ssh_env(dir: &Path) -> SshEnvironment {
    SshEnvironment {
        daemon_binaries: DaemonBinaries::new(dir.to_owned()),
        forward_socket_dir: dir.join("s"),
    }
}

/// A fake ssh host whose daemon is `daemon_socket` and whose tunnels are proxies to it,
/// kept in `tunnels` (clear it to cut them).
pub fn fake_ssh_host(daemon_socket: PathBuf, tunnels: Arc<Mutex<Vec<Proxy>>>) -> FakeSpawner {
    FakeSpawner::with_responder(move |command: &ProcessCommand| {
        fake_ssh_response(command, &daemon_socket, &tunnels)
    })
}

/// How [`fake_ssh_host`] answers `command`, for responders that answer some commands
/// otherwise: a bootstrap reports `daemon_socket`, a tunnel starts a proxy to it.
pub fn fake_ssh_response(
    command: &ProcessCommand,
    daemon_socket: &Path,
    tunnels: &Mutex<Vec<Proxy>>,
) -> FakeResponse {
    let script = command.args.last().cloned().unwrap_or_default();
    if script.contains("TREETERM_OS") {
        return FakeResponse::stdout(format!(
            "TREETERM_OS:Linux\nTREETERM_ARCH:x86_64\nTREETERM_HOME:/home/test\nTREETERM_REMOTE_HASH:NONE\nTREETERM_SOCKET:{}\n",
            daemon_socket.display()
        ));
    }
    let forward = command
        .args
        .iter()
        .skip_while(|arg| *arg != "-L")
        .nth(1)
        .expect("only bootstraps and tunnels are expected");
    let (local, remote) = forward.split_once(':').unwrap();
    tunnels
        .lock()
        .unwrap()
        .push(Proxy::start(Path::new(local), Path::new(remote)));
    FakeResponse::Run
}

/// A saved connection to `test@example.com`, id `remote-1`.
pub fn remote_config() -> SshConnectionConfig {
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
