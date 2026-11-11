//! Remote daemon bootstrap and socket tunnel over the system `ssh` (port of Electron's
//! `ssh.ts`).
//!
//! [`SshConnector::bootstrap`] runs one shell script on the host that reports its platform,
//! the cached daemon binary's hash and whether a daemon is listening, starting one if the
//! binary is there. A missing binary (or a refresh) is uploaded with `scp` and started. A
//! cached binary that differs from the local build is never replaced silently: that is
//! [`BootstrapResult::HashMismatch`] unless [`OutdatedDaemon`] says otherwise.
//! [`SshConnector::open_tunnel`] then forwards the remote daemon socket to a local one with
//! `ssh -L local.sock:remote.sock … cat`. Every line goes to a [`LogSink`].

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use futures::StreamExt;
use regex::Regex;
use sha2::{Digest, Sha256};

use crate::settings::{IdentityFile, SshConnectionConfig};

use super::process::{
    ExitStatus, Process, ProcessCommand, ProcessEnd, ProcessEvent, ProcessSpawner, run_to_end,
};
use super::types::{OutdatedDaemon, OutputKind, short_hash};

/// How long the tunnel's local socket may take to appear.
pub const SOCKET_WAIT: Duration = Duration::from_secs(15);
const SOCKET_POLL: Duration = Duration::from_millis(200);

/// Receives every bootstrap and tunnel line.
pub trait LogSink: Send + Sync {
    fn line(&self, kind: OutputKind, line: String);
}

// === arguments and scripts ===================================================================

fn destination(config: &SshConnectionConfig) -> String {
    format!("{}@{}", config.user, config.host)
}

fn push_identity(args: &mut Vec<String>, config: &SshConnectionConfig) {
    match &config.identity_file {
        IdentityFile::Path(path) => args.extend(["-i".to_owned(), path.clone()]),
        IdentityFile::SshDefault => {}
    }
}

/// Options every ssh invocation shares, ending with `user@host`.
pub fn base_ssh_args(config: &SshConnectionConfig) -> Vec<String> {
    let mut args: Vec<String> = [
        "-o",
        "StrictHostKeyChecking=accept-new",
        "-o",
        "BatchMode=yes",
        "-p",
    ]
    .map(str::to_owned)
    .to_vec();
    args.push(config.port.to_string());
    push_identity(&mut args, config);
    args.push(destination(config));
    args
}

/// `scp` arguments copying `local` to the absolute `remote` path. scp takes the port as
/// `-P`; an absolute destination because scp over SFTP (OpenSSH 9+) does not expand `~`.
pub fn scp_args(config: &SshConnectionConfig, local: &Path, remote: &str) -> Vec<String> {
    let mut args: Vec<String> = [
        "-o",
        "StrictHostKeyChecking=accept-new",
        "-o",
        "BatchMode=yes",
        "-P",
    ]
    .map(str::to_owned)
    .to_vec();
    args.push(config.port.to_string());
    push_identity(&mut args, config);
    args.push(local.to_string_lossy().into_owned());
    args.push(format!("{}:{remote}", destination(config)));
    args
}

/// The tunnel: forward `local_socket` to `remote_socket`, kept open by a remote `cat`.
pub fn tunnel_args(
    config: &SshConnectionConfig,
    local_socket: &Path,
    remote_socket: &str,
) -> Vec<String> {
    let mut args = base_ssh_args(config);
    args.push("-L".to_owned());
    args.push(format!("{}:{remote_socket}", local_socket.display()));
    args.extend(
        [
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ServerAliveInterval=10",
            "-o",
            "ServerAliveCountMax=3",
            "cat",
        ]
        .map(str::to_owned),
    );
    args
}

/// The script that reports the host's platform and daemon state and starts a cached
/// daemon (Electron's, verbatim).
pub fn bootstrap_script(outdated: OutdatedDaemon) -> String {
    let refresh = match outdated {
        OutdatedDaemon::Refresh => "1",
        OutdatedDaemon::Report | OutdatedDaemon::Accept => "0",
    };
    let refresh_line = format!("REFRESH_DAEMON={refresh}");
    [
        "set -e",
        r#"DAEMON_BIN="$HOME/.treeterm/treeterm-daemon""#,
        r#"DAEMON_SOCKET="/tmp/treeterm-$(id -u)/daemon.sock""#,
        &refresh_line,
        "",
        "# Report system OS and architecture for binary selection",
        r#"echo "TREETERM_OS:$(uname -s)""#,
        r#"echo "TREETERM_ARCH:$(uname -m)""#,
        "",
        "# Report absolute home directory — scp over SFTP does not reliably expand ~",
        r#"echo "TREETERM_HOME:$HOME""#,
        "",
        "# Check if daemon binary exists",
        "NEEDS_UPLOAD=0",
        r#"if [ ! -x "$DAEMON_BIN" ]; then"#,
        "  NEEDS_UPLOAD=1",
        "fi",
        "",
        "# Report checksum of remote binary for version verification",
        r#"if [ "$NEEDS_UPLOAD" = "0" ]; then"#,
        r#"  REMOTE_HASH=$( (sha256sum "$DAEMON_BIN" 2>/dev/null || shasum -a 256 "$DAEMON_BIN" 2>/dev/null) | cut -c1-64)"#,
        r#"  echo "TREETERM_REMOTE_HASH:${REMOTE_HASH:-NONE}""#,
        "else",
        r#"  echo "TREETERM_REMOTE_HASH:NONE""#,
        "fi",
        "",
        "# Kill old daemon if refresh requested",
        r#"if [ "$REFRESH_DAEMON" = "1" ] && [ -S "$DAEMON_SOCKET" ]; then"#,
        r#"  echo "Refreshing daemon: killing old process...""#,
        r#"  pkill -x "treeterm-daemon" 2>/dev/null || true"#,
        r#"  rm -f "$DAEMON_SOCKET""#,
        "  sleep 0.5",
        "fi",
        "",
        "# Detect stale socket (file exists but daemon process is dead)",
        r#"if [ -S "$DAEMON_SOCKET" ] && ! pgrep -x "treeterm-daemon" > /dev/null 2>&1; then"#,
        r#"  echo "Stale daemon socket detected (process not running), cleaning up...""#,
        r#"  rm -f "$DAEMON_SOCKET""#,
        "fi",
        "",
        "# Report if upload is needed",
        r#"echo "TREETERM_NEEDS_UPLOAD:$NEEDS_UPLOAD""#,
        "",
        "# Start daemon if not already running",
        r#"mkdir -p "$HOME/.treeterm""#,
        r#"mkdir -p "/tmp/treeterm-$(id -u)""#,
        r#"DAEMON_LOG="$HOME/.treeterm/daemon.log""#,
        r#"DAEMON_PID="""#,
        r#"if [ -x "$DAEMON_BIN" ] && [ ! -S "$DAEMON_SOCKET" ]; then"#,
        "  # Launch through a login shell: a non-interactive ssh session has a bare PATH",
        "  # (/usr/bin:/bin:...), so execs from the daemon could not find Homebrew tools",
        "  # like git-lfs. exec keeps $! pointing at the daemon itself.",
        r#"  "${SHELL:-/bin/sh}" -lc 'exec "$0"' "$DAEMON_BIN" >> "$DAEMON_LOG" 2>&1 &"#,
        "  DAEMON_PID=$!",
        "fi",
        "",
        "# Wait for socket to appear (only if binary exists)",
        r#"if [ -x "$DAEMON_BIN" ]; then"#,
        "  for i in $(seq 1 40); do",
        r#"    [ -S "$DAEMON_SOCKET" ] && break"#,
        "    # Check if daemon process died early",
        r#"    if [ -n "$DAEMON_PID" ] && ! kill -0 "$DAEMON_PID" 2>/dev/null; then"#,
        "      break",
        "    fi",
        "    sleep 0.25",
        "  done",
        "fi",
        "",
        r#"if [ ! -S "$DAEMON_SOCKET" ]; then"#,
        "  # Log daemon output if binary existed but failed (possibly wrong architecture)",
        r#"  if [ "$NEEDS_UPLOAD" = "0" ] && [ -f "$DAEMON_LOG" ]; then"#,
        r#"    echo "Daemon binary exists but failed to start (possibly wrong architecture)" >&2"#,
        r#"    tail -20 "$DAEMON_LOG" >&2"#,
        "  fi",
        r#"  echo "TREETERM_SOCKET:NEEDS_UPLOAD""#,
        "else",
        r#"  echo "TREETERM_SOCKET:$DAEMON_SOCKET""#,
        "fi",
    ]
    .join("\n")
}

/// Start a freshly uploaded daemon and report its socket.
fn start_script(daemon_bin: &str, daemon_log: &str) -> String {
    [
        format!("chmod +x '{daemon_bin}'"),
        r#"DAEMON_SOCKET="/tmp/treeterm-$(id -u)/daemon.sock""#.to_owned(),
        r#"mkdir -p "/tmp/treeterm-$(id -u)""#.to_owned(),
        format!("'{daemon_bin}' >> '{daemon_log}' 2>&1 &"),
        "DAEMON_PID=$!".to_owned(),
        "for i in $(seq 1 40); do".to_owned(),
        r#"  [ -S "$DAEMON_SOCKET" ] && break"#.to_owned(),
        r#"  kill -0 "$DAEMON_PID" 2>/dev/null || break"#.to_owned(),
        "  sleep 0.25".to_owned(),
        "done".to_owned(),
        r#"if [ -S "$DAEMON_SOCKET" ]; then"#.to_owned(),
        r#"  echo "TREETERM_SOCKET:$DAEMON_SOCKET""#.to_owned(),
        "else".to_owned(),
        r#"  echo "Daemon failed to start after upload." >&2"#.to_owned(),
        format!("  if [ -f '{daemon_log}' ]; then"),
        r#"    echo "Last 20 lines of daemon.log:" >&2"#.to_owned(),
        format!("    tail -20 '{daemon_log}' >&2"),
        "  fi".to_owned(),
        r#"  if [ -n "$DAEMON_PID" ] && ! kill -0 "$DAEMON_PID" 2>/dev/null; then"#.to_owned(),
        r#"    wait "$DAEMON_PID" 2>/dev/null"#.to_owned(),
        r#"    echo "Daemon process $DAEMON_PID exited (died early)." >&2"#.to_owned(),
        "  fi".to_owned(),
        "  exit 1".to_owned(),
        "fi".to_owned(),
    ]
    .join("\n")
}

/// Stop an outdated daemon before re-uploading (refresh only).
const KILL_SCRIPT: &str = r#"pkill -x "treeterm-daemon" 2>/dev/null || true; rm -f /tmp/treeterm-$(id -u)/daemon.sock; sleep 0.5"#;

// === platforms and binaries ==================================================================

/// Map `uname -m` (or Node's `process.arch`) names to the daemon binaries' arch suffix.
pub fn normalize_arch(arch: &str) -> String {
    match arch {
        "arm64" => "aarch64",
        "amd64" | "x64" => "x86_64",
        other => other,
    }
    .to_owned()
}

/// A host's lowercased `uname -s` and normalized arch, which pick the daemon binary
/// (`treeterm-daemon-<arch>-<os>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePlatform {
    pub os: String,
    pub arch: String,
}

impl RemotePlatform {
    /// `<arch>-<os>`.
    pub fn target(&self) -> String {
        format!("{}-{}", self.arch, self.os)
    }
}

/// This machine, named like `uname` names it (Node's `process.platform` is `darwin`).
pub fn local_platform() -> RemotePlatform {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    RemotePlatform {
        os: os.to_owned(),
        arch: normalize_arch(std::env::consts::ARCH),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("No daemon binary found for remote platform \"{target}\" (looked for {}). {hint}", .looked_for.display())]
pub struct NoDaemonBinary {
    pub target: String,
    pub looked_for: PathBuf,
    pub hint: String,
}

/// The daemon binary in `base_dir` for `remote`: `treeterm-daemon-<arch>-<os>`, or the
/// plain `treeterm-daemon` when `remote` is this machine's platform (`local`). Never a
/// binary for another platform.
pub fn find_daemon_binary(
    base_dir: &Path,
    remote: &RemotePlatform,
    local: &RemotePlatform,
) -> Result<PathBuf, NoDaemonBinary> {
    let target = remote.target();
    let platform_binary = base_dir.join(format!("treeterm-daemon-{target}"));
    if platform_binary.exists() {
        return Ok(platform_binary);
    }
    let local_binary = base_dir.join("treeterm-daemon");
    if target == local.target() && local_binary.exists() {
        return Ok(local_binary);
    }
    let hint = match remote.os.as_str() {
        "linux" => "Run `npm run build:daemon-rs:remote` to cross-compile for Linux.".to_owned(),
        _ => format!("Build the daemon for {target} and place it at that path."),
    };
    Err(NoDaemonBinary {
        target,
        looked_for: platform_binary,
        hint,
    })
}

/// The directory of daemon binaries to upload (Electron: `resources/daemon-rs`).
#[derive(Debug, Clone)]
pub struct DaemonBinaries {
    dir: PathBuf,
    local: RemotePlatform,
}

impl DaemonBinaries {
    pub fn new(dir: PathBuf) -> Self {
        Self::with_local_platform(dir, local_platform())
    }

    /// As if this machine were `local` (tests).
    pub fn with_local_platform(dir: PathBuf, local: RemotePlatform) -> Self {
        Self { dir, local }
    }

    pub fn find(&self, remote: &RemotePlatform) -> Result<PathBuf, NoDaemonBinary> {
        find_daemon_binary(&self.dir, remote, &self.local)
    }

    /// SHA-256 (hex) of the binary for `remote`.
    pub async fn checksum(&self, remote: &RemotePlatform) -> Result<String, RemoteSetupError> {
        let path = self.find(remote)?;
        let read_path = path.clone();
        let hashed = tokio::task::spawn_blocking(move || {
            std::fs::read(&read_path).map(|bytes| hex::encode(Sha256::digest(&bytes)))
        })
        .await
        .unwrap_or_else(|join| Err(io::Error::other(join.to_string())));
        hashed.map_err(|source| RemoteSetupError::ReadBinary {
            path,
            source: Arc::new(source),
        })
    }
}

// === errors ==================================================================================

/// Connecting over ssh failed. Messages match Electron's.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SshError {
    #[error("Failed to spawn ssh: {0}")]
    BootstrapSpawn(String),
    #[error("SSH bootstrap failed (exit {0}). See log output for details.")]
    BootstrapExit(ExitStatus),
    #[error("Could not detect remote architecture (TREETERM_ARCH not reported)")]
    NoRemoteArch,
    #[error("Could not detect remote OS (TREETERM_OS not reported)")]
    NoRemoteOs,
    #[error("Could not detect remote home directory (TREETERM_HOME not reported)")]
    NoRemoteHome,
    #[error("Failed to start remote daemon: {0}")]
    StartDaemon(#[source] RemoteSetupError),
    #[error("cannot prepare local socket {}: {source}", .path.display())]
    LocalSocket {
        path: PathBuf,
        source: Arc<io::Error>,
    },
    /// The tunnel's ssh ended (or could not start) before its socket appeared; the tunnel
    /// log has why.
    #[error("SSH process exited before socket was ready")]
    TunnelExited,
    #[error("Timed out waiting for local socket: {}", .0.display())]
    SocketTimeout(PathBuf),
}

/// Checking, uploading or starting the remote daemon failed.
#[derive(Debug, Clone, thiserror::Error)]
pub enum RemoteSetupError {
    #[error(transparent)]
    NoDaemonBinary(#[from] NoDaemonBinary),
    #[error("cannot read daemon binary {}: {source}", .path.display())]
    ReadBinary {
        path: PathBuf,
        source: Arc<io::Error>,
    },
    #[error(transparent)]
    Command(#[from] SshCommandError),
    #[error(transparent)]
    Upload(#[from] UploadError),
    #[error("Daemon failed to start after upload — check daemon log on remote")]
    DaemonDidNotStart,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SshCommandError {
    #[error("SSH command failed (exit {exit}): {first_line}")]
    Exit {
        exit: ExitStatus,
        first_line: String,
    },
    #[error("{0}")]
    Spawn(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UploadError {
    #[error("scp failed (exit {exit}): {stderr}")]
    Exit { exit: ExitStatus, stderr: String },
    #[error("scp spawn error: {0}")]
    Spawn(String),
}

/// How a tunnel's ssh process ended. Messages match Electron's.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TunnelEnd {
    #[error("SSH tunnel closed (exit {exit}){}", stderr_detail(.stderr))]
    Closed {
        exit: ExitStatus,
        stderr: Vec<String>,
    },
    #[error("SSH tunnel error: {0}")]
    Failed(String),
}

fn stderr_detail(stderr: &[String]) -> String {
    match stderr {
        [] => String::new(),
        lines => format!(": {}", lines.join("; ")),
    }
}

// === bootstrap ===============================================================================

/// Outcome of [`SshConnector::bootstrap`]. A mismatch is expected, not an error: the caller
/// offers refresh / connect anyway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapResult {
    /// A daemon listens on `socket_path` on the host.
    Connected { socket_path: String },
    HashMismatch {
        local_hash: String,
        remote_hash: String,
    },
}

/// Outcome of [`SshConnector::connect`].
pub enum TunnelOutcome {
    Connected(Tunnel),
    HashMismatch {
        local_hash: String,
        remote_hash: String,
    },
}

static OS_MARKER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"TREETERM_OS:(\S+)").unwrap());
static ARCH_MARKER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"TREETERM_ARCH:(\S+)").unwrap());
static HOME_MARKER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"TREETERM_HOME:(.+)").unwrap());
static HASH_MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"TREETERM_REMOTE_HASH:(\S+)").unwrap());
static SOCKET_MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"TREETERM_SOCKET:(.+)").unwrap());

/// The first `MARKER:value` in the output, trimmed; empty counts as absent.
fn marker(pattern: &Regex, stdout: &str) -> Option<String> {
    pattern
        .captures(stdout)
        .map(|captures| captures[1].trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// The cached binary's hash as the bootstrap script reports it.
enum RemoteHash {
    /// No binary, or neither `sha256sum` nor `shasum` on the host (`NONE`).
    Unavailable,
    Sha256(String),
}

/// Where the bootstrap script left the daemon.
enum RemoteSocket {
    /// No daemon came up (`NEEDS_UPLOAD`).
    NeedsUpload,
    Listening(String),
}

/// Bootstraps the daemon on one ssh host and tunnels to it (Electron's `SSHTunnel`, minus the
/// running tunnel, which is a [`Tunnel`]).
pub struct SshConnector {
    spawner: Arc<dyn ProcessSpawner>,
    config: SshConnectionConfig,
    outdated: OutdatedDaemon,
    binaries: DaemonBinaries,
    log: Arc<dyn LogSink>,
}

impl SshConnector {
    pub fn new(
        spawner: Arc<dyn ProcessSpawner>,
        config: SshConnectionConfig,
        outdated: OutdatedDaemon,
        binaries: DaemonBinaries,
        log: Arc<dyn LogSink>,
    ) -> Self {
        Self {
            spawner,
            config,
            outdated,
            binaries,
            log,
        }
    }

    /// Bootstrap, then tunnel to the daemon through `local_socket` (Electron's
    /// `SSHTunnel.connect`).
    pub async fn connect(&self, local_socket: PathBuf) -> Result<TunnelOutcome, SshError> {
        match self.bootstrap().await? {
            BootstrapResult::HashMismatch {
                local_hash,
                remote_hash,
            } => Ok(TunnelOutcome::HashMismatch {
                local_hash,
                remote_hash,
            }),
            BootstrapResult::Connected { socket_path } => Ok(TunnelOutcome::Connected(
                self.open_tunnel(local_socket, &socket_path).await?,
            )),
        }
    }

    /// Make sure a daemon of the right build listens on the host; see the module docs.
    pub async fn bootstrap(&self) -> Result<BootstrapResult, SshError> {
        let mut args = base_ssh_args(&self.config);
        args.push(bootstrap_script(self.outdated));
        let command = ProcessCommand::new("ssh", args);
        self.log(
            OutputKind::Bootstrap,
            format!(
                "Bootstrapping remote daemon on {}...",
                destination(&self.config)
            ),
        );
        self.log(OutputKind::Bootstrap, format!("$ {}", command.display()));
        let process = self
            .spawner
            .spawn(&command)
            .map_err(|error| SshError::BootstrapSpawn(error.message))?;
        let finished = run_to_end(process, |event| {
            self.log_output(OutputKind::Bootstrap, event)
        })
        .await;
        match finished.end {
            ProcessEnd::Exited(status) if status.success() => {}
            ProcessEnd::Exited(status) => return Err(SshError::BootstrapExit(status)),
            ProcessEnd::Failed(message) => return Err(SshError::BootstrapSpawn(message)),
        }
        let stdout = finished.stdout;

        let arch = marker(&ARCH_MARKER, &stdout)
            .map(|arch| normalize_arch(&arch))
            .ok_or(SshError::NoRemoteArch)?;
        let os = marker(&OS_MARKER, &stdout)
            .map(|os| os.to_lowercase())
            .ok_or(SshError::NoRemoteOs)?;
        let remote = RemotePlatform { os, arch };
        let remote_hash = match marker(&HASH_MARKER, &stdout) {
            Some(hash) if hash != "NONE" => RemoteHash::Sha256(hash),
            Some(_) | None => RemoteHash::Unavailable,
        };
        let socket = match marker(&SOCKET_MARKER, &stdout) {
            Some(socket) if socket != "NEEDS_UPLOAD" => RemoteSocket::Listening(socket),
            Some(_) | None => RemoteSocket::NeedsUpload,
        };

        if let RemoteSocket::Listening(socket_path) = socket {
            // A daemon is running: use it if it is this build.
            let RemoteHash::Sha256(remote_hash) = &remote_hash else {
                // No hash tool on the host: trust the running daemon.
                return Ok(BootstrapResult::Connected { socket_path });
            };
            let local_hash = self.checksum(&remote).await?;
            if *remote_hash == local_hash {
                return Ok(BootstrapResult::Connected { socket_path });
            }
            let hashes = format!(
                "local={}... remote={}...",
                short_hash(&local_hash),
                short_hash(remote_hash)
            );
            match self.outdated {
                OutdatedDaemon::Refresh => {
                    self.log(
                        OutputKind::Bootstrap,
                        format!("Daemon binary mismatch ({hashes}), re-uploading..."),
                    );
                    self.run_ssh_command(KILL_SCRIPT.to_owned())
                        .await
                        .map_err(|error| SshError::StartDaemon(error.into()))?;
                }
                OutdatedDaemon::Accept => {
                    self.log(
                        OutputKind::Bootstrap,
                        format!(
                            "Daemon binary mismatch ({hashes}), proceeding with outdated daemon as requested."
                        ),
                    );
                    return Ok(BootstrapResult::Connected { socket_path });
                }
                OutdatedDaemon::Report => {
                    self.log(
                        OutputKind::Bootstrap,
                        format!("Daemon binary hash mismatch ({hashes})."),
                    );
                    return Ok(BootstrapResult::HashMismatch {
                        local_hash,
                        remote_hash: remote_hash.clone(),
                    });
                }
            }
        }

        // Upload path: the absolute home avoids SFTP's unreliable `~`.
        let home = marker(&HOME_MARKER, &stdout).ok_or(SshError::NoRemoteHome)?;

        // No daemon came up. Replacing a different cached binary is a daemon update: only
        // on an explicit refresh (a reconnect after the old daemon died must not upgrade it).
        match (&remote_hash, self.outdated) {
            (RemoteHash::Sha256(remote_hash), OutdatedDaemon::Report | OutdatedDaemon::Accept) => {
                let local_hash = self.checksum(&remote).await?;
                if *remote_hash != local_hash {
                    self.log(
                        OutputKind::Bootstrap,
                        format!(
                            "Remote daemon is not running and its binary differs from the local build (local={}... remote={}...). Not replacing it without a refresh.",
                            short_hash(&local_hash),
                            short_hash(remote_hash)
                        ),
                    );
                    return Ok(BootstrapResult::HashMismatch {
                        local_hash,
                        remote_hash: remote_hash.clone(),
                    });
                }
            }
            (RemoteHash::Unavailable, _) | (RemoteHash::Sha256(_), OutdatedDaemon::Refresh) => {}
        }

        let socket_path = self
            .upload_and_start_daemon(&remote, &home)
            .await
            .map_err(SshError::StartDaemon)?;
        Ok(BootstrapResult::Connected { socket_path })
    }

    async fn checksum(&self, remote: &RemotePlatform) -> Result<String, SshError> {
        self.binaries
            .checksum(remote)
            .await
            .map_err(SshError::StartDaemon)
    }

    /// Upload the daemon binary to `<home>/.treeterm/treeterm-daemon`, start it, and return
    /// its socket.
    async fn upload_and_start_daemon(
        &self,
        remote: &RemotePlatform,
        home: &str,
    ) -> Result<String, RemoteSetupError> {
        let daemon_dir = format!("{home}/.treeterm");
        let daemon_bin = format!("{daemon_dir}/treeterm-daemon");
        let daemon_log = format!("{daemon_dir}/daemon.log");

        self.log(
            OutputKind::Bootstrap,
            format!(
                "Uploading daemon binary (remote platform: {})...",
                remote.target()
            ),
        );
        let local_path = self.binaries.find(remote)?;
        self.log(
            OutputKind::Bootstrap,
            format!("Uploading daemon binary to {daemon_bin}..."),
        );
        upload_file(&*self.spawner, &self.config, &local_path, &daemon_bin).await?;
        self.log(OutputKind::Bootstrap, "Upload complete".to_owned());

        let stdout = self
            .run_ssh_command(start_script(&daemon_bin, &daemon_log))
            .await?;
        marker(&SOCKET_MARKER, &stdout).ok_or(RemoteSetupError::DaemonDidNotStart)
    }

    /// Run `script` on the host; its output goes to the bootstrap log. Returns stdout.
    async fn run_ssh_command(&self, script: String) -> Result<String, SshCommandError> {
        let mut args = base_ssh_args(&self.config);
        args.push(script);
        let process = self
            .spawner
            .spawn(&ProcessCommand::new("ssh", args))
            .map_err(|error| SshCommandError::Spawn(error.message))?;
        let finished = run_to_end(process, |event| {
            self.log_output(OutputKind::Bootstrap, event)
        })
        .await;
        match finished.end {
            ProcessEnd::Exited(status) if status.success() => Ok(finished.stdout),
            ProcessEnd::Exited(exit) => Err(SshCommandError::Exit {
                exit,
                first_line: finished
                    .stderr
                    .split('\n')
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("non-zero exit")
                    .to_owned(),
            }),
            ProcessEnd::Failed(message) => Err(SshCommandError::Spawn(message)),
        }
    }

    /// Forward `local_socket` to the host's `remote_socket` and wait for the local end to
    /// appear. The returned [`Tunnel`] owns the ssh process.
    pub async fn open_tunnel(
        &self,
        local_socket: PathBuf,
        remote_socket: &str,
    ) -> Result<Tunnel, SshError> {
        prepare_local_socket(&local_socket)?;
        let command = ProcessCommand::new(
            "ssh",
            tunnel_args(&self.config, &local_socket, remote_socket),
        );
        self.log(
            OutputKind::Tunnel,
            format!(
                "Starting tunnel: {} -> {remote_socket}",
                local_socket.display()
            ),
        );
        self.log(OutputKind::Tunnel, format!("$ {}", command.display()));
        let process = match self.spawner.spawn(&command) {
            Ok(process) => process,
            Err(error) => {
                self.log(
                    OutputKind::Tunnel,
                    TunnelEnd::Failed(error.message).to_string(),
                );
                return Err(SshError::TunnelExited);
            }
        };
        let mut tunnel = Tunnel {
            process,
            local_socket,
            stderr: Vec::new(),
            log: self.log.clone(),
        };
        tunnel.wait_for_socket().await?;
        Ok(tunnel)
    }

    fn log(&self, kind: OutputKind, line: String) {
        tracing::debug!(host = %self.config.host, ?kind, "{line}");
        self.log.line(kind, line);
    }

    fn log_output(&self, kind: OutputKind, event: &ProcessEvent) {
        match event {
            ProcessEvent::Stdout(line) => self.log(kind, line.clone()),
            ProcessEvent::Stderr(line) => self.log(kind, format!("[stderr] {line}")),
            ProcessEvent::Exited(_) | ProcessEvent::Failed(_) => {}
        }
    }
}

/// Create the socket's directory and remove a stale socket file.
fn prepare_local_socket(path: &Path) -> Result<(), SshError> {
    let fail = |source: io::Error| SshError::LocalSocket {
        path: path.to_owned(),
        source: Arc::new(source),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(fail(error)),
    }
}

/// Copy `local` to the absolute `remote` path on the host with `scp`.
pub async fn upload_file(
    spawner: &dyn ProcessSpawner,
    config: &SshConnectionConfig,
    local: &Path,
    remote: &str,
) -> Result<(), UploadError> {
    let process = spawner
        .spawn(&ProcessCommand::new("scp", scp_args(config, local, remote)))
        .map_err(|error| UploadError::Spawn(error.message))?;
    let finished = run_to_end(process, |_| {}).await;
    match finished.end {
        ProcessEnd::Exited(status) if status.success() => Ok(()),
        ProcessEnd::Exited(exit) => Err(UploadError::Exit {
            exit,
            stderr: finished.stderr,
        }),
        ProcessEnd::Failed(message) => Err(UploadError::Spawn(message)),
    }
}

// === tunnel ==================================================================================

/// A running `ssh -L` socket tunnel. Dropping it kills ssh and removes the local socket.
#[must_use = "dropping a Tunnel closes it"]
pub struct Tunnel {
    process: Process,
    local_socket: PathBuf,
    stderr: Vec<String>,
    log: Arc<dyn LogSink>,
}

impl Tunnel {
    /// Where the daemon is reachable locally.
    pub fn local_socket(&self) -> &Path {
        &self.local_socket
    }

    /// Run until ssh ends, logging its output; returns how it ended. Cancel-safe: dropping
    /// the future between lines loses nothing.
    pub async fn closed(&mut self) -> TunnelEnd {
        loop {
            let event = self.process.next().await;
            if let Some(end) = self.on_event(event) {
                return end;
            }
        }
    }

    async fn wait_for_socket(&mut self) -> Result<(), SshError> {
        let deadline = tokio::time::Instant::now() + SOCKET_WAIT;
        loop {
            if self.local_socket.exists() {
                self.log(format!(
                    "Local socket ready: {}",
                    self.local_socket.display()
                ));
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(SshError::SocketTimeout(self.local_socket.clone()));
            }
            tokio::select! {
                () = tokio::time::sleep(SOCKET_POLL) => {}
                event = self.process.next() => {
                    if self.on_event(event).is_some() {
                        return Err(SshError::TunnelExited);
                    }
                }
            }
        }
    }

    /// Log one event; the end if it was the last.
    fn on_event(&mut self, event: Option<ProcessEvent>) -> Option<TunnelEnd> {
        let end = match event {
            Some(ProcessEvent::Stdout(line)) => {
                self.log(line);
                return None;
            }
            Some(ProcessEvent::Stderr(line)) => {
                self.log(format!("[stderr] {line}"));
                self.stderr.push(line);
                return None;
            }
            Some(ProcessEvent::Exited(exit)) => TunnelEnd::Closed {
                exit,
                stderr: std::mem::take(&mut self.stderr),
            },
            Some(ProcessEvent::Failed(message)) => TunnelEnd::Failed(message),
            None => TunnelEnd::Failed("ssh output ended without an exit status".to_owned()),
        };
        self.log(end.to_string());
        Some(end)
    }

    fn log(&self, line: String) {
        tracing::debug!(socket = ?self.local_socket, "{line}");
        self.log.line(OutputKind::Tunnel, line);
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        match std::fs::remove_file(&self.local_socket) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(%error, socket = ?self.local_socket, "cannot remove tunnel socket");
            }
        }
    }
}

#[cfg(test)]
#[path = "ssh_tests.rs"]
mod tests;
