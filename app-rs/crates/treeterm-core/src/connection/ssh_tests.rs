//! Port of `src/main/ssh.test.ts`, driven by the fake spawner.

use std::sync::Mutex;

use super::*;
use crate::connection::fake::{FakeResponse, FakeSpawner};
use crate::settings::{ConnectionLabel, Extra};

#[derive(Default)]
struct RecordingSink(Mutex<Vec<(OutputKind, String)>>);

impl LogSink for RecordingSink {
    fn line(&self, kind: OutputKind, line: String) {
        self.0.lock().unwrap().push((kind, line));
    }
}

impl RecordingSink {
    fn lines(&self, kind: OutputKind) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(k, _)| *k == kind)
            .map(|(_, line)| line.clone())
            .collect()
    }
}

fn make_config() -> SshConnectionConfig {
    SshConnectionConfig {
        id: "conn-1".to_owned(),
        host: "example.com".to_owned(),
        user: "testuser".to_owned(),
        port: 22,
        identity_file: IdentityFile::SshDefault,
        label: ConnectionLabel::UserAtHost,
        port_forwards: Vec::new(),
        extra: Extra::new(),
    }
}

const BINARY: &[u8] = b"mock-binary";

fn binary_hash() -> String {
    hex::encode(Sha256::digest(BINARY))
}

/// This machine, for the binary lookup: never the Linux remote of these tests.
fn mac() -> RemotePlatform {
    RemotePlatform {
        os: "darwin".to_owned(),
        arch: "aarch64".to_owned(),
    }
}

struct Harness {
    spawner: FakeSpawner,
    sink: Arc<RecordingSink>,
    dir: tempfile::TempDir,
}

impl Harness {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("tt")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::write(dir.path().join("treeterm-daemon-x86_64-linux"), BINARY).unwrap();
        Self {
            spawner: FakeSpawner::new(),
            sink: Arc::new(RecordingSink::default()),
            dir,
        }
    }

    fn connector(&self, outdated: OutdatedDaemon) -> SshConnector {
        self.connector_for(make_config(), outdated)
    }

    fn connector_for(&self, config: SshConnectionConfig, outdated: OutdatedDaemon) -> SshConnector {
        SshConnector::new(
            Arc::new(self.spawner.clone()),
            config,
            outdated,
            DaemonBinaries::with_local_platform(self.dir.path().to_owned(), mac()),
            self.sink.clone(),
        )
    }

    fn local_socket(&self) -> PathBuf {
        self.dir.path().join("sockets").join("r-test.sock")
    }
}

/// The local end of a tunnel command's `-L local:remote`.
fn forwarded_socket(command: &ProcessCommand) -> PathBuf {
    let spec = command
        .args
        .iter()
        .skip_while(|arg| *arg != "-L")
        .nth(1)
        .expect("tunnel command has -L");
    PathBuf::from(spec.split(':').next().unwrap())
}

/// Tunnels create their local socket and keep running; everything else runs too.
fn tunnel_creates_socket(command: &ProcessCommand) -> FakeResponse {
    if command.args.iter().any(|arg| arg == "-L") {
        std::fs::write(forwarded_socket(command), b"").unwrap();
    }
    FakeResponse::Run
}

fn last_arg(command: &ProcessCommand) -> &str {
    command.args.last().unwrap()
}

mod constructor_and_properties {
    use super::*;

    #[test]
    fn starts_disconnected() {
        let harness = Harness::new();
        let _connector = harness.connector(OutdatedDaemon::Report);
        assert!(harness.spawner.processes().is_empty());
        assert!(!harness.local_socket().exists());
    }
}

mod disconnect {
    use super::*;

    #[tokio::test]
    async fn kills_ssh_process_and_cleans_up_socket() {
        let harness = Harness::new();
        harness.spawner.respond(tunnel_creates_socket);
        let tunnel = harness
            .connector(OutdatedDaemon::Report)
            .open_tunnel(harness.local_socket(), "/tmp/remote.sock")
            .await
            .unwrap();
        assert!(harness.local_socket().exists());

        drop(tunnel);

        assert!(harness.spawner.processes()[0].is_killed());
        assert!(!harness.local_socket().exists());
    }

    #[test]
    fn handles_disconnect_when_not_connected() {
        let harness = Harness::new();
        drop(harness.connector(OutdatedDaemon::Report));
        assert!(harness.spawner.processes().is_empty());
    }

    #[tokio::test]
    async fn ignores_cleanup_errors() {
        let harness = Harness::new();
        let socket = harness.local_socket();
        let socket_for_ssh = socket.clone();
        // ssh leaves a directory where the socket was: removing it as a file fails.
        harness.spawner.respond(move |_| {
            std::fs::create_dir_all(&socket_for_ssh).unwrap();
            FakeResponse::Run
        });
        let tunnel = harness
            .connector(OutdatedDaemon::Report)
            .open_tunnel(socket.clone(), "/tmp/remote.sock")
            .await
            .unwrap();

        drop(tunnel);

        assert!(harness.spawner.processes()[0].is_killed());
        assert!(socket.is_dir());
    }
}

mod build_base_ssh_args {
    use super::*;

    #[test]
    fn builds_args_with_standard_options() {
        let args = base_ssh_args(&make_config());
        assert_eq!(
            args,
            [
                "-o",
                "StrictHostKeyChecking=accept-new",
                "-o",
                "BatchMode=yes",
                "-p",
                "22",
                "testuser@example.com"
            ]
        );
    }

    #[test]
    fn includes_identity_file_when_configured() {
        let config = SshConnectionConfig {
            identity_file: IdentityFile::Path("/home/user/.ssh/id_rsa".to_owned()),
            ..make_config()
        };
        let args = base_ssh_args(&config);
        let i = args.iter().position(|arg| arg == "-i").unwrap();
        assert_eq!(args[i + 1], "/home/user/.ssh/id_rsa");
        assert_eq!(args.last().unwrap(), "testuser@example.com");
    }

    #[test]
    fn uses_custom_port() {
        let config = SshConnectionConfig {
            port: 2222,
            ..make_config()
        };
        assert!(base_ssh_args(&config).contains(&"2222".to_owned()));
    }
}

mod normalize_arch_tests {
    use super::*;

    #[test]
    fn maps_uname_aliases_to_the_daemon_binary_arch_names() {
        assert_eq!(normalize_arch("arm64"), "aarch64");
        assert_eq!(normalize_arch("amd64"), "x86_64");
        assert_eq!(normalize_arch("x64"), "x86_64");
        assert_eq!(normalize_arch("aarch64"), "aarch64");
        assert_eq!(normalize_arch("x86_64"), "x86_64");
    }

    #[test]
    fn names_this_machine_like_uname() {
        let local = local_platform();
        assert_ne!(local.os, "macos");
        assert!(
            ["aarch64", "x86_64"].contains(&local.arch.as_str()),
            "{local:?}"
        );
    }
}

mod find_daemon_binary_tests {
    use super::*;

    fn base_with(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for file in files {
            std::fs::write(dir.path().join(file), b"bin").unwrap();
        }
        dir
    }

    fn linux_arm() -> RemotePlatform {
        RemotePlatform {
            os: "linux".to_owned(),
            arch: "aarch64".to_owned(),
        }
    }

    #[test]
    fn uses_the_platform_specific_binary_when_present() {
        let base = base_with(&["treeterm-daemon-aarch64-linux", "treeterm-daemon"]);
        assert_eq!(
            find_daemon_binary(base.path(), &linux_arm(), &mac()).unwrap(),
            base.path().join("treeterm-daemon-aarch64-linux")
        );
    }

    #[test]
    fn falls_back_to_the_local_binary_when_the_remote_matches_this_machine() {
        let base = base_with(&["treeterm-daemon"]);
        assert_eq!(
            find_daemon_binary(base.path(), &mac(), &mac()).unwrap(),
            base.path().join("treeterm-daemon")
        );
    }

    #[test]
    fn never_uses_the_local_binary_for_a_different_os_or_arch() {
        let base = base_with(&["treeterm-daemon"]);
        let other_os = RemotePlatform {
            os: "linux".to_owned(),
            ..mac()
        };
        let error = find_daemon_binary(base.path(), &other_os, &mac()).unwrap_err();
        assert!(
            error.to_string().starts_with("No daemon binary found"),
            "{error}"
        );
        assert!(
            error.to_string().contains("npm run build:daemon-rs:remote"),
            "{error}"
        );
        let other_arch = RemotePlatform {
            arch: "x86_64".to_owned(),
            ..mac()
        };
        let error = find_daemon_binary(base.path(), &other_arch, &mac()).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "No daemon binary found for remote platform \"x86_64-darwin\" (looked for {}). \
                 Build the daemon for x86_64-darwin and place it at that path.",
                base.path().join("treeterm-daemon-x86_64-darwin").display()
            )
        );
    }
}

mod bootstrap_remote_daemon {
    use super::*;

    fn exits_with(stdout: &'static str) -> impl FnMut(&ProcessCommand) -> FakeResponse {
        move |_| FakeResponse::stdout(stdout)
    }

    #[tokio::test]
    async fn resolves_with_socket_path_on_success() {
        let harness = Harness::new();
        harness.spawner.respond(exits_with(
            "TREETERM_OS:Linux\nTREETERM_ARCH:x86_64\nTREETERM_SOCKET:/tmp/treeterm-1000/daemon.sock\n",
        ));
        let result = harness
            .connector(OutdatedDaemon::Report)
            .bootstrap()
            .await
            .unwrap();
        assert_eq!(
            result,
            BootstrapResult::Connected {
                socket_path: "/tmp/treeterm-1000/daemon.sock".to_owned()
            }
        );
        let log = harness.sink.lines(OutputKind::Bootstrap);
        assert_eq!(
            log[0],
            "Bootstrapping remote daemon on testuser@example.com..."
        );
        assert!(log[1].starts_with("$ ssh -o StrictHostKeyChecking=accept-new"));
        assert!(log.contains(&"TREETERM_ARCH:x86_64".to_owned()));
    }

    #[tokio::test]
    async fn rejects_on_non_zero_exit_code() {
        let harness = Harness::new();
        harness.spawner.respond(|_| FakeResponse::Exit {
            stdout: String::new(),
            stderr: "Permission denied\n".to_owned(),
            code: 1,
        });
        let error = harness
            .connector(OutdatedDaemon::Report)
            .bootstrap()
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "SSH bootstrap failed (exit 1). See log output for details."
        );
        assert!(
            harness
                .sink
                .lines(OutputKind::Bootstrap)
                .contains(&"[stderr] Permission denied".to_owned())
        );
    }

    #[tokio::test]
    async fn rejects_on_spawn_error() {
        let harness = Harness::new();
        harness
            .spawner
            .respond(|_| FakeResponse::Refuse("spawn ENOENT".to_owned()));
        let error = harness
            .connector(OutdatedDaemon::Report)
            .bootstrap()
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "Failed to spawn ssh: spawn ENOENT");
    }

    #[tokio::test]
    async fn rejects_on_an_error_after_spawn() {
        let harness = Harness::new();
        let connector = harness.connector(OutdatedDaemon::Report);
        let bootstrap = tokio::spawn(async move { connector.bootstrap().await });
        harness.spawner.process(0).await.fail("EPIPE");
        let error = bootstrap.await.unwrap().unwrap_err();
        assert_eq!(error.to_string(), "Failed to spawn ssh: EPIPE");
    }

    #[tokio::test]
    async fn rejects_when_treeterm_arch_marker_is_missing() {
        let harness = Harness::new();
        harness.spawner.respond(exits_with("uid=1001\n"));
        let error = harness
            .connector(OutdatedDaemon::Report)
            .bootstrap()
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("Could not detect remote architecture")
        );
    }

    #[tokio::test]
    async fn rejects_when_treeterm_os_marker_is_missing() {
        let harness = Harness::new();
        harness
            .spawner
            .respond(exits_with("TREETERM_ARCH:x86_64\n"));
        let error = harness
            .connector(OutdatedDaemon::Report)
            .bootstrap()
            .await
            .unwrap_err();
        assert!(error.to_string().starts_with("Could not detect remote OS"));
    }

    #[tokio::test]
    async fn includes_refresh_daemon_1_when_refresh_daemon_option_set() {
        let harness = Harness::new();
        harness.spawner.respond(exits_with(
            "TREETERM_OS:Linux\nTREETERM_ARCH:x86_64\nTREETERM_SOCKET:/tmp/daemon.sock\n",
        ));
        harness
            .connector(OutdatedDaemon::Refresh)
            .bootstrap()
            .await
            .unwrap();
        let script = last_arg(&harness.spawner.commands()[0]).to_owned();
        assert!(script.contains("REFRESH_DAEMON=1"));

        harness
            .connector(OutdatedDaemon::Accept)
            .bootstrap()
            .await
            .unwrap();
        assert!(last_arg(&harness.spawner.commands()[1]).contains("REFRESH_DAEMON=0"));
    }

    #[tokio::test]
    async fn bootstrap_script_includes_sha256sum_checksum_reporting() {
        let harness = Harness::new();
        harness.spawner.respond(exits_with(
            "TREETERM_OS:Linux\nTREETERM_ARCH:x86_64\nTREETERM_SOCKET:/tmp/daemon.sock\n",
        ));
        harness
            .connector(OutdatedDaemon::Report)
            .bootstrap()
            .await
            .unwrap();
        let command = &harness.spawner.commands()[0];
        assert_eq!(command.program, "ssh");
        let script = last_arg(command);
        assert!(script.contains("sha256sum"));
        assert!(script.contains("TREETERM_REMOTE_HASH"));
        assert!(script.contains("TREETERM_HOME:$HOME"));
        assert!(script.contains("REFRESH_DAEMON=0"));
    }

    #[tokio::test]
    async fn resolves_immediately_when_hash_matches() {
        let harness = Harness::new();
        let stdout = format!(
            "TREETERM_OS:Linux\nTREETERM_ARCH:x86_64\nTREETERM_REMOTE_HASH:{}\nTREETERM_SOCKET:/tmp/treeterm-1000/daemon.sock\n",
            binary_hash()
        );
        harness
            .spawner
            .respond(move |_| FakeResponse::stdout(stdout.clone()));
        let result = harness
            .connector(OutdatedDaemon::Report)
            .bootstrap()
            .await
            .unwrap();
        assert_eq!(
            result,
            BootstrapResult::Connected {
                socket_path: "/tmp/treeterm-1000/daemon.sock".to_owned()
            }
        );
        // Only the bootstrap itself: no kill or upload.
        assert_eq!(harness.spawner.processes().len(), 1);
    }

    #[tokio::test]
    async fn kills_and_re_uploads_when_hash_mismatches_with_refresh_daemon() {
        let harness = Harness::new();
        let mut spawns = 0;
        harness.spawner.respond(move |_| {
            spawns += 1;
            match spawns {
                1 => FakeResponse::stdout(
                    "TREETERM_OS:Linux\nTREETERM_ARCH:x86_64\nTREETERM_HOME:/home/testuser\nTREETERM_REMOTE_HASH:remotehash999\nTREETERM_SOCKET:/tmp/treeterm-1000/daemon.sock\n",
                ),
                2 | 3 => FakeResponse::exit(0),
                _ => FakeResponse::stdout("TREETERM_SOCKET:/tmp/treeterm-1000/daemon.sock\n"),
            }
        });

        let result = harness
            .connector(OutdatedDaemon::Refresh)
            .bootstrap()
            .await
            .unwrap();

        assert_eq!(
            result,
            BootstrapResult::Connected {
                socket_path: "/tmp/treeterm-1000/daemon.sock".to_owned()
            }
        );
        let commands = harness.spawner.commands();
        assert_eq!(commands.len(), 4);
        assert!(last_arg(&commands[1]).starts_with("pkill -x \"treeterm-daemon\""));
        // scp destination must be an absolute path (no `~/`), to survive SFTP-mode scp.
        assert_eq!(commands[2].program, "scp");
        let scp_dest = last_arg(&commands[2]);
        assert_eq!(
            scp_dest,
            "testuser@example.com:/home/testuser/.treeterm/treeterm-daemon"
        );
        assert!(!scp_dest.contains('~'));
        assert_eq!(
            commands[2].args[commands[2].args.len() - 2],
            harness
                .dir
                .path()
                .join("treeterm-daemon-x86_64-linux")
                .to_string_lossy()
        );
        assert!(
            last_arg(&commands[3])
                .starts_with("chmod +x '/home/testuser/.treeterm/treeterm-daemon'")
        );
        let log = harness.sink.lines(OutputKind::Bootstrap);
        let local = &binary_hash()[..12];
        assert!(log.contains(&format!(
            "Daemon binary mismatch (local={local}... remote=remotehash99...), re-uploading..."
        )));
        assert!(log.contains(&"Upload complete".to_owned()));
    }

    #[tokio::test]
    async fn rejects_when_upload_needed_but_treeterm_home_marker_is_missing() {
        let harness = Harness::new();
        harness.spawner.respond(exits_with(
            "TREETERM_OS:Linux\nTREETERM_ARCH:x86_64\nTREETERM_REMOTE_HASH:NONE\nTREETERM_SOCKET:NEEDS_UPLOAD\n",
        ));
        let error = harness
            .connector(OutdatedDaemon::Report)
            .bootstrap()
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("Could not detect remote home directory")
        );
    }

    #[tokio::test]
    async fn resolves_with_a_hash_mismatch_result_no_flags_without_killing_or_uploading() {
        let harness = Harness::new();
        harness.spawner.respond(exits_with(
            "TREETERM_OS:Linux\nTREETERM_ARCH:x86_64\nTREETERM_REMOTE_HASH:remotehash999\nTREETERM_SOCKET:/tmp/treeterm-1000/daemon.sock\n",
        ));
        let result = harness
            .connector(OutdatedDaemon::Report)
            .bootstrap()
            .await
            .unwrap();
        assert_eq!(
            result,
            BootstrapResult::HashMismatch {
                local_hash: binary_hash(),
                remote_hash: "remotehash999".to_owned()
            }
        );
        assert_eq!(harness.spawner.processes().len(), 1);
    }

    #[tokio::test]
    async fn resolves_with_existing_socket_when_allow_outdated_daemon_and_hash_mismatches() {
        let harness = Harness::new();
        harness.spawner.respond(exits_with(
            "TREETERM_OS:Linux\nTREETERM_ARCH:x86_64\nTREETERM_REMOTE_HASH:remotehash999\nTREETERM_SOCKET:/tmp/treeterm-1000/daemon.sock\n",
        ));
        let result = harness
            .connector(OutdatedDaemon::Accept)
            .bootstrap()
            .await
            .unwrap();
        assert_eq!(
            result,
            BootstrapResult::Connected {
                socket_path: "/tmp/treeterm-1000/daemon.sock".to_owned()
            }
        );
        assert_eq!(harness.spawner.processes().len(), 1);
    }

    async fn does_not_replace_a_different_remote_binary_whose_daemon_is_not_running(
        outdated: OutdatedDaemon,
    ) {
        let harness = Harness::new();
        harness.spawner.respond(exits_with(
            "TREETERM_OS:Linux\nTREETERM_ARCH:x86_64\nTREETERM_HOME:/home/testuser\nTREETERM_REMOTE_HASH:remotehash999\nTREETERM_SOCKET:NEEDS_UPLOAD\n",
        ));
        let result = harness.connector(outdated).bootstrap().await.unwrap();
        assert_eq!(
            result,
            BootstrapResult::HashMismatch {
                local_hash: binary_hash(),
                remote_hash: "remotehash999".to_owned()
            }
        );
        // Only the bootstrap: no upload.
        assert_eq!(harness.spawner.processes().len(), 1);
    }

    #[tokio::test]
    async fn does_not_replace_a_different_remote_binary_whose_daemon_is_not_running_no_flags() {
        does_not_replace_a_different_remote_binary_whose_daemon_is_not_running(
            OutdatedDaemon::Report,
        )
        .await;
    }

    #[tokio::test]
    async fn does_not_replace_a_different_remote_binary_whose_daemon_is_not_running_allow_outdated_daemon()
     {
        does_not_replace_a_different_remote_binary_whose_daemon_is_not_running(
            OutdatedDaemon::Accept,
        )
        .await;
    }

    #[tokio::test]
    async fn trusts_running_daemon_when_sha256sum_is_unavailable_none_hash() {
        let harness = Harness::new();
        harness.spawner.respond(exits_with(
            "TREETERM_OS:Linux\nTREETERM_ARCH:x86_64\nTREETERM_REMOTE_HASH:NONE\nTREETERM_SOCKET:/tmp/treeterm-1000/daemon.sock\n",
        ));
        let result = harness
            .connector(OutdatedDaemon::Report)
            .bootstrap()
            .await
            .unwrap();
        assert_eq!(
            result,
            BootstrapResult::Connected {
                socket_path: "/tmp/treeterm-1000/daemon.sock".to_owned()
            }
        );
        assert_eq!(harness.spawner.processes().len(), 1);
    }

    #[tokio::test]
    async fn uploads_and_starts_a_missing_daemon() {
        let harness = Harness::new();
        let mut spawns = 0;
        harness.spawner.respond(move |_| {
            spawns += 1;
            match spawns {
                1 => FakeResponse::stdout(
                    "TREETERM_OS:Linux\nTREETERM_ARCH:amd64\nTREETERM_HOME:/home/u\nTREETERM_REMOTE_HASH:NONE\nTREETERM_SOCKET:NEEDS_UPLOAD\n",
                ),
                2 => FakeResponse::exit(0),
                _ => FakeResponse::stdout("TREETERM_SOCKET:/tmp/treeterm-7/daemon.sock\n"),
            }
        });
        let result = harness
            .connector(OutdatedDaemon::Report)
            .bootstrap()
            .await
            .unwrap();
        assert_eq!(
            result,
            BootstrapResult::Connected {
                socket_path: "/tmp/treeterm-7/daemon.sock".to_owned()
            }
        );
        assert!(
            harness
                .sink
                .lines(OutputKind::Bootstrap)
                .contains(&"Uploading daemon binary (remote platform: x86_64-linux)...".to_owned())
        );
    }

    #[tokio::test]
    async fn fails_loudly_when_no_binary_matches_the_remote_platform() {
        let harness = Harness::new();
        harness.spawner.respond(exits_with(
            "TREETERM_OS:Linux\nTREETERM_ARCH:riscv64\nTREETERM_HOME:/home/u\nTREETERM_REMOTE_HASH:NONE\nTREETERM_SOCKET:NEEDS_UPLOAD\n",
        ));
        let error = harness
            .connector(OutdatedDaemon::Report)
            .bootstrap()
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("Failed to start remote daemon: No daemon binary found for remote platform \"riscv64-linux\""),
            "{error}"
        );
    }

    #[tokio::test]
    async fn reports_a_daemon_that_does_not_start_after_upload() {
        let harness = Harness::new();
        let mut spawns = 0;
        harness.spawner.respond(move |_| {
            spawns += 1;
            match spawns {
                1 => FakeResponse::stdout(
                    "TREETERM_OS:Linux\nTREETERM_ARCH:x86_64\nTREETERM_HOME:/home/u\nTREETERM_SOCKET:NEEDS_UPLOAD\n",
                ),
                2 => FakeResponse::exit(0),
                _ => FakeResponse::Exit {
                    stdout: String::new(),
                    stderr: "\nDaemon failed to start after upload.\n".to_owned(),
                    code: 1,
                },
            }
        });
        let error = harness
            .connector(OutdatedDaemon::Report)
            .bootstrap()
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Failed to start remote daemon: SSH command failed (exit 1): Daemon failed to start after upload."
        );
    }
}

mod start_tunnel {
    use super::*;

    #[tokio::test]
    async fn spawns_ssh_with_socket_forwarding_args() {
        let harness = Harness::new();
        harness.spawner.respond(tunnel_creates_socket);
        let _tunnel = harness
            .connector(OutdatedDaemon::Report)
            .open_tunnel(harness.local_socket(), "/tmp/remote.sock")
            .await
            .unwrap();
        let command = &harness.spawner.commands()[0];
        assert_eq!(command.program, "ssh");
        let mapping = format!("{}:/tmp/remote.sock", harness.local_socket().display());
        let l = command.args.iter().position(|arg| arg == "-L").unwrap();
        assert_eq!(command.args[l + 1], mapping);
        assert!(
            command
                .args
                .contains(&"ExitOnForwardFailure=yes".to_owned())
        );
        assert_eq!(last_arg(command), "cat");
        let log = harness.sink.lines(OutputKind::Tunnel);
        assert_eq!(
            log[0],
            format!(
                "Starting tunnel: {} -> /tmp/remote.sock",
                harness.local_socket().display()
            )
        );
        assert_eq!(
            log.last().unwrap(),
            &format!("Local socket ready: {}", harness.local_socket().display())
        );
    }

    #[tokio::test]
    async fn notifies_disconnect_listeners_on_process_close_when_connected() {
        let harness = Harness::new();
        harness.spawner.respond(tunnel_creates_socket);
        let mut tunnel = harness
            .connector(OutdatedDaemon::Report)
            .open_tunnel(harness.local_socket(), "/tmp/remote.sock")
            .await
            .unwrap();
        let ssh = harness.spawner.process(0).await;
        ssh.stdout("hello\n");
        ssh.stderr("Connection reset\nbroken pipe\n");
        ssh.exit(1);

        let end = tunnel.closed().await;

        assert_eq!(
            end.to_string(),
            "SSH tunnel closed (exit 1): Connection reset; broken pipe"
        );
        let log = harness.sink.lines(OutputKind::Tunnel);
        assert!(log.contains(&"hello".to_owned()));
        assert!(log.contains(&"[stderr] broken pipe".to_owned()));
        assert_eq!(log.last().unwrap(), &end.to_string());
    }

    #[tokio::test]
    async fn notifies_disconnect_listeners_on_process_error() {
        let harness = Harness::new();
        harness.spawner.respond(tunnel_creates_socket);
        let mut tunnel = harness
            .connector(OutdatedDaemon::Report)
            .open_tunnel(harness.local_socket(), "/tmp/remote.sock")
            .await
            .unwrap();
        harness.spawner.process(0).await.fail("connection refused");

        assert_eq!(
            tunnel.closed().await,
            TunnelEnd::Failed("connection refused".to_owned())
        );
        assert_eq!(
            TunnelEnd::Failed("connection refused".to_owned()).to_string(),
            "SSH tunnel error: connection refused"
        );
    }

    #[tokio::test]
    async fn connect_bootstraps_then_tunnels_to_the_reported_socket() {
        let harness = Harness::new();
        harness.spawner.respond(|command| {
            if command.args.iter().any(|arg| arg == "-L") {
                return tunnel_creates_socket(command);
            }
            FakeResponse::stdout(
                "TREETERM_OS:Linux\nTREETERM_ARCH:x86_64\nTREETERM_SOCKET:/tmp/treeterm-1000/daemon.sock\n",
            )
        });
        let outcome = harness
            .connector(OutdatedDaemon::Report)
            .connect(harness.local_socket())
            .await
            .unwrap();
        let TunnelOutcome::Connected(tunnel) = outcome else {
            panic!("expected a tunnel");
        };
        assert_eq!(tunnel.local_socket(), harness.local_socket());
        assert!(harness.spawner.commands()[1].args.contains(&format!(
            "{}:/tmp/treeterm-1000/daemon.sock",
            harness.local_socket().display()
        )));
    }
}

mod upload_file_tests {
    use super::*;

    #[tokio::test]
    async fn builds_scp_args_with_standard_options_and_absolute_destination() {
        let spawner = FakeSpawner::with_responder(|_| FakeResponse::exit(0));
        upload_file(
            &spawner,
            &make_config(),
            Path::new("/local/file.txt"),
            "/remote/dir/file.txt",
        )
        .await
        .unwrap();
        let command = &spawner.commands()[0];
        assert_eq!(command.program, "scp");
        let args = &command.args;
        assert!(args.contains(&"StrictHostKeyChecking=accept-new".to_owned()));
        assert!(args.contains(&"BatchMode=yes".to_owned()));
        // scp uses -P (capital) for the port
        let p = args.iter().position(|arg| arg == "-P").unwrap();
        assert_eq!(args[p + 1], "22");
        assert!(!args.contains(&"-i".to_owned()));
        assert_eq!(args[args.len() - 2], "/local/file.txt");
        assert_eq!(
            args[args.len() - 1],
            "testuser@example.com:/remote/dir/file.txt"
        );
    }

    #[tokio::test]
    async fn includes_identity_file_when_configured() {
        let spawner = FakeSpawner::with_responder(|_| FakeResponse::exit(0));
        let config = SshConnectionConfig {
            identity_file: IdentityFile::Path("/home/user/.ssh/id_rsa".to_owned()),
            port: 2222,
            ..make_config()
        };
        upload_file(
            &spawner,
            &config,
            Path::new("/local/file.txt"),
            "/remote/file.txt",
        )
        .await
        .unwrap();
        let args = &spawner.commands()[0].args;
        let i = args.iter().position(|arg| arg == "-i").unwrap();
        assert_eq!(args[i + 1], "/home/user/.ssh/id_rsa");
        assert!(args.contains(&"2222".to_owned()));
    }

    #[tokio::test]
    async fn rejects_with_stderr_on_non_zero_exit() {
        let spawner = FakeSpawner::with_responder(|_| FakeResponse::Exit {
            stdout: String::new(),
            stderr: "Permission denied".to_owned(),
            code: 1,
        });
        let error = upload_file(&spawner, &make_config(), Path::new("/l"), "/r")
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "scp failed (exit 1): Permission denied");
    }

    #[tokio::test]
    async fn rejects_on_spawn_error() {
        let spawner =
            FakeSpawner::with_responder(|_| FakeResponse::Refuse("spawn ENOENT".to_owned()));
        let error = upload_file(&spawner, &make_config(), Path::new("/l"), "/r")
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "scp spawn error: spawn ENOENT");
    }
}

mod wait_for_socket {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn resolves_immediately_when_socket_exists() {
        let harness = Harness::new();
        harness.spawner.respond(tunnel_creates_socket);
        let started = tokio::time::Instant::now();
        let _tunnel = harness
            .connector(OutdatedDaemon::Report)
            .open_tunnel(harness.local_socket(), "/tmp/remote.sock")
            .await
            .unwrap();
        assert_eq!(started.elapsed(), Duration::ZERO);
    }

    #[tokio::test]
    async fn rejects_when_ssh_process_dies_before_socket_appears() {
        let harness = Harness::new();
        harness.spawner.respond(|_| FakeResponse::Exit {
            stdout: String::new(),
            stderr: "bind: Address already in use\n".to_owned(),
            code: 255,
        });
        let error = harness
            .connector(OutdatedDaemon::Report)
            .open_tunnel(harness.local_socket(), "/tmp/remote.sock")
            .await
            .err()
            .unwrap();
        assert_eq!(
            error.to_string(),
            "SSH process exited before socket was ready"
        );
        assert_eq!(
            harness.sink.lines(OutputKind::Tunnel).last().unwrap(),
            "SSH tunnel closed (exit 255): bind: Address already in use"
        );
    }

    #[tokio::test]
    async fn rejects_when_ssh_cannot_be_spawned() {
        let harness = Harness::new();
        harness
            .spawner
            .respond(|_| FakeResponse::Refuse("spawn ssh ENOENT".to_owned()));
        let error = harness
            .connector(OutdatedDaemon::Report)
            .open_tunnel(harness.local_socket(), "/tmp/remote.sock")
            .await
            .err()
            .unwrap();
        assert!(matches!(error, SshError::TunnelExited));
        assert_eq!(
            harness.sink.lines(OutputKind::Tunnel).last().unwrap(),
            "SSH tunnel error: spawn ssh ENOENT"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn times_out_when_the_socket_never_appears() {
        let harness = Harness::new();
        let started = tokio::time::Instant::now();
        let error = harness
            .connector(OutdatedDaemon::Report)
            .open_tunnel(harness.local_socket(), "/tmp/remote.sock")
            .await
            .err()
            .unwrap();
        assert_eq!(
            error.to_string(),
            format!(
                "Timed out waiting for local socket: {}",
                harness.local_socket().display()
            )
        );
        assert!(started.elapsed() >= SOCKET_WAIT);
        assert!(harness.spawner.processes()[0].is_killed());
    }

    #[tokio::test]
    async fn removes_a_stale_socket_before_tunnelling() {
        let harness = Harness::new();
        std::fs::create_dir_all(harness.local_socket().parent().unwrap()).unwrap();
        std::fs::write(harness.local_socket(), b"stale").unwrap();
        let socket = harness.local_socket();
        harness.spawner.respond(move |_| {
            assert!(!socket.exists(), "stale socket still there when ssh starts");
            FakeResponse::exit(255)
        });
        let result = harness
            .connector(OutdatedDaemon::Report)
            .open_tunnel(harness.local_socket(), "/tmp/remote.sock")
            .await;
        assert!(matches!(result, Err(SshError::TunnelExited)));
    }
}
