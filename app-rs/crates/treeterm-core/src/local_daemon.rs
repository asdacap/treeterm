//! Find or start the daemon on this machine (port of Electron's `ensureDaemonRunning`).

use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::paths::SOCKET_PATH_ENV;
use crate::transport::{ConnectError, connect_unix};

const SOCKET_POLL_ATTEMPTS: u32 = 20;
const SOCKET_POLL_INTERVAL: Duration = Duration::from_millis(250);
/// The socket file appears at bind time; give the gRPC server a moment to start accepting.
const BIND_SETTLE: Duration = Duration::from_millis(300);

pub struct LocalDaemonSpec {
    pub socket_path: PathBuf,
    pub binary: PathBuf,
    /// Appended with the daemon's stdout/stderr.
    pub log_path: PathBuf,
    /// The daemon's `~/.treeterm/daemon.pid`.
    pub pid_file: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonStart {
    AlreadyRunning,
    Spawned { pid: u32 },
}

#[derive(Debug, thiserror::Error)]
pub enum LocalDaemonError {
    #[error("daemon executable not found at {0}")]
    BinaryMissing(PathBuf),
    #[error("cannot clear stale daemon state {path}: {source}")]
    StaleState { path: PathBuf, source: io::Error },
    #[error("cannot open daemon log {path}: {source}")]
    Log { path: PathBuf, source: io::Error },
    #[error("cannot spawn daemon {path}: {source}")]
    Spawn { path: PathBuf, source: io::Error },
    #[error("daemon did not create its socket at {0} in time")]
    SocketTimeout(PathBuf),
    #[error(transparent)]
    Connect(#[from] ConnectError),
}

/// Make sure a daemon is accepting connections on `spec.socket_path`, spawning one if not.
/// Must run on the tokio runtime.
pub async fn ensure_running(spec: &LocalDaemonSpec) -> Result<DaemonStart, LocalDaemonError> {
    match connect_unix(&spec.socket_path).await {
        Ok(_probe) => return Ok(DaemonStart::AlreadyRunning),
        Err(error) => tracing::info!(%error, "no daemon reachable; starting one"),
    }

    // A stale pid file whose pid was recycled to a live process would make the new
    // daemon's own already-running check exit immediately.
    remove_if_exists(&spec.pid_file)?;
    remove_if_exists(&spec.socket_path)?;

    let pid = spawn_detached(spec)?;
    wait_for_socket(&spec.socket_path).await?;
    tokio::time::sleep(BIND_SETTLE).await;
    connect_unix(&spec.socket_path).await?;
    Ok(DaemonStart::Spawned { pid })
}

fn remove_if_exists(path: &Path) -> Result<(), LocalDaemonError> {
    match fs::remove_file(path) {
        Ok(()) => {
            tracing::info!(?path, "removed stale daemon state");
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(LocalDaemonError::StaleState {
            path: path.to_owned(),
            source,
        }),
    }
}

fn spawn_detached(spec: &LocalDaemonSpec) -> Result<u32, LocalDaemonError> {
    if !spec.binary.exists() {
        return Err(LocalDaemonError::BinaryMissing(spec.binary.clone()));
    }
    if let Some(dir) = spec.log_path.parent() {
        fs::create_dir_all(dir).map_err(|source| LocalDaemonError::Log {
            path: spec.log_path.clone(),
            source,
        })?;
    }
    let open_log = || {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&spec.log_path)
            .map_err(|source| LocalDaemonError::Log {
                path: spec.log_path.clone(),
                source,
            })
    };
    let mut child = Command::new(&spec.binary)
        .env(SOCKET_PATH_ENV, &spec.socket_path)
        .stdin(Stdio::null())
        .stdout(open_log()?)
        .stderr(open_log()?)
        // Own process group: the daemon must outlive the app and not receive its ^C.
        .process_group(0)
        .spawn()
        .map_err(|source| LocalDaemonError::Spawn {
            path: spec.binary.clone(),
            source,
        })?;
    let pid = child.id();
    tracing::info!(pid, binary = ?spec.binary, "daemon spawned");
    // Reap the daemon if it exits while we run, so it never lingers as a zombie.
    std::thread::Builder::new()
        .name("daemon-reaper".into())
        .spawn(move || match child.wait() {
            Ok(status) => tracing::info!(pid, %status, "daemon exited"),
            Err(error) => tracing::error!(pid, %error, "waiting on daemon failed"),
        })
        .map_err(|source| LocalDaemonError::Spawn {
            path: spec.binary.clone(),
            source,
        })?;
    Ok(pid)
}

async fn wait_for_socket(path: &Path) -> Result<(), LocalDaemonError> {
    for _ in 0..SOCKET_POLL_ATTEMPTS {
        if path.exists() {
            return Ok(());
        }
        tokio::time::sleep(SOCKET_POLL_INTERVAL).await;
    }
    Err(LocalDaemonError::SocketTimeout(path.to_owned()))
}
