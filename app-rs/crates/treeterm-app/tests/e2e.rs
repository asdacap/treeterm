//! End to end: the real app binary on its own HOME and daemon, driven through the automation
//! socket. Port of `e2e/daemon-lifecycle.spec.ts` and `e2e/session-persistence.spec.ts` (the
//! worktree workflow and terminal refresh are covered by `treeterm-model`'s tests against a
//! real daemon).
//!
//! These open a window, so they only run when asked:
//!
//! ```sh
//! (cd daemon-rs && cargo build -p treeterm-daemon)
//! cd app-rs && cargo test -p treeterm-app --features automation --test e2e -- --ignored --test-threads=1
//! ```
#![cfg(feature = "automation")]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const WAIT: Duration = Duration::from_secs(30);

/// An isolated HOME, daemon socket and git workspace. Dropping it stops the daemon it started.
struct Sandbox {
    /// Under `/tmp`: socket paths must stay short (macOS caps them near 104 bytes).
    root: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let root = tempfile::Builder::new().prefix("tte").tempdir_in("/tmp").expect("temp dir");
        let sandbox = Self { root };
        std::fs::create_dir_all(sandbox.home()).unwrap();
        let workspace = sandbox.workspace();
        std::fs::create_dir_all(&workspace).unwrap();
        for args in [
            &["init", "-q", "-b", "master"][..],
            &["-c", "user.name=e2e", "-c", "user.email=e2e@example.invalid", "commit", "-q", "--allow-empty", "-m", "init"],
        ] {
            let status = Command::new("git").args(args).current_dir(&workspace).status().expect("git");
            assert!(status.success(), "git {args:?}");
        }
        // Keep the analyzer's requests on this machine (it sends screen text).
        let settings = sandbox.config_dir().join("treeterm/settings.json");
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(&settings, r#"{ "llm": { "baseUrl": "http://127.0.0.1:9" } }"#).unwrap();
        sandbox
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn config_dir(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.home().join("Library/Application Support")
        } else {
            self.home().join(".config")
        }
    }

    fn workspace(&self) -> PathBuf {
        self.root.path().join("repo")
    }

    fn daemon_socket(&self) -> PathBuf {
        self.root.path().join("d.sock")
    }

    fn automation_socket(&self) -> PathBuf {
        self.root.path().join("a.sock")
    }

    fn daemon_dir() -> PathBuf {
        // crates/treeterm-app → <repo>
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).unwrap().to_owned();
        let dir = repo.join("daemon-rs/target/debug");
        assert!(
            dir.join("treeterm-daemon").exists(),
            "build the daemon first: (cd daemon-rs && cargo build -p treeterm-daemon)"
        );
        dir
    }

    fn daemon_pid(&self) -> Option<u32> {
        std::fs::read_to_string(self.home().join(".treeterm/daemon.pid")).ok()?.trim().parse().ok()
    }

    /// Start the app on the workspace and wait until its terminal shows the shell.
    fn launch(&self) -> App {
        match std::fs::remove_file(self.automation_socket()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("remove the old automation socket: {error}"),
        }
        let log = std::fs::File::options()
            .create(true)
            .append(true)
            .open(self.root.path().join("app.log"))
            .unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_treeterm"))
            .arg(self.workspace())
            .env("HOME", self.home())
            .env("XDG_CONFIG_HOME", self.home().join(".config"))
            .env("TREETERM_SOCKET_PATH", self.daemon_socket())
            .env("TREETERM_DAEMON_DIR", Self::daemon_dir())
            .env("TREETERM_AUTOMATION_SOCKET", self.automation_socket())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .expect("start the app");
        let app = App { child, socket: self.automation_socket() };
        app.wait_for("a focused terminal with a prompt", |state| {
            terminal(state).is_some_and(|t| {
                t["overlay"].is_null() && t["focused"] == true && !t["screen"].as_str().unwrap_or("").trim().is_empty()
            })
        });
        app
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if let Some(pid) = self.daemon_pid()
            && alive(pid)
            && !Command::new("kill").arg(pid.to_string()).status().is_ok_and(|s| s.success())
        {
            eprintln!("could not stop the sandbox daemon {pid}");
        }
    }
}

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn terminal(state: &Value) -> Option<&Value> {
    state["terminals"].as_array()?.first()
}

fn screen(state: &Value) -> String {
    terminal(state).and_then(|t| t["screen"].as_str()).unwrap_or_default().to_owned()
}

struct App {
    child: Child,
    socket: PathBuf,
}

impl App {
    /// One automation command; `None` while the socket is not up yet.
    fn try_command(&self, line: &str) -> Option<Value> {
        let mut stream = UnixStream::connect(&self.socket).ok()?;
        stream.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
        writeln!(stream, "{line}").ok()?;
        let mut reply = String::new();
        BufReader::new(stream).read_line(&mut reply).ok()?;
        serde_json::from_str(&reply).ok()
    }

    fn command(&self, line: &str) -> Value {
        let reply = self.try_command(line).unwrap_or_else(|| panic!("no reply to {line:?}"));
        assert!(reply.get("error").is_none(), "{line:?} failed: {reply}");
        reply
    }

    fn wait_for(&self, what: &str, done: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + WAIT;
        let mut last = Value::Null;
        while Instant::now() < deadline {
            if let Some(state) = self.try_command("state") {
                if done(&state) {
                    return state;
                }
                last = state;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!("timed out waiting for {what}; last state: {last}");
    }

    fn run(&self, shell_line: &str) {
        self.command(&format!("type {shell_line}"));
        self.command("key enter");
    }

    /// Stop the app (the daemon keeps running, as when the window is closed).
    fn quit(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        // Already exited is fine: only a running app needs stopping.
        match self.child.kill() {
            Ok(()) | Err(_) => {}
        }
        match self.child.wait() {
            Ok(_) | Err(_) => {}
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.stop();
    }
}

#[test]
#[ignore = "opens the app window; run with --ignored"]
fn the_daemon_starts_outlives_the_app_and_is_reused() {
    let sandbox = Sandbox::new();
    assert_eq!(sandbox.daemon_pid(), None);

    let app = sandbox.launch();
    let first = sandbox.daemon_pid().expect("the app started a daemon");
    assert!(alive(first));
    app.quit();

    std::thread::sleep(Duration::from_secs(1));
    assert!(alive(first), "the daemon outlives the app");

    let app = sandbox.launch();
    assert_eq!(sandbox.daemon_pid(), Some(first), "the running daemon is reused");
    app.quit();
}

#[test]
#[ignore = "opens the app window; run with --ignored"]
fn terminal_output_survives_an_app_restart() {
    let sandbox = Sandbox::new();
    let app = sandbox.launch();
    app.run("echo e2e-marker-$((6*7))");
    app.wait_for("the marker", |state| screen(state).contains("e2e-marker-42"));
    app.quit();

    let app = sandbox.launch();
    app.wait_for("the replayed marker", |state| screen(state).contains("e2e-marker-42"));
    app.quit();
}

#[test]
#[ignore = "opens the app window; run with --ignored"]
fn tab_and_shift_tab_reach_the_terminal() {
    let sandbox = Sandbox::new();
    let app = sandbox.launch();
    // The marker shows once the shell has handed the tty to `cat` (cooked mode), so the
    // keys below are not taken by the line editor. The quotes keep the echoed command
    // from matching it.
    app.run(r#"echo rea""dy; cat -t"#);
    app.wait_for("cat to start", |state| screen(state).contains("ready"));
    // Root (GPUI Kit) binds these to focus navigation; the terminal must get them instead.
    app.command("key tab");
    app.command("key shift-tab");
    app.command("key enter");
    app.wait_for("cat to echo Tab and Shift-Tab", |state| screen(state).contains("^I^[[Z"));
    app.quit();
}

#[test]
#[ignore = "opens the app window; run with --ignored"]
fn a_typed_folder_opens_as_a_workspace() {
    let sandbox = Sandbox::new();
    let folder = sandbox.root.path().join("typed-folder");
    std::fs::create_dir_all(&folder).unwrap();
    let app = sandbox.launch();
    assert_eq!(app.command("workspacedialog")["ok"], true);
    app.command(&format!("type {}", folder.display()));
    // Enter in the folder field opens it (the dialog must not just close).
    app.command("key enter");
    app.wait_for("the typed folder as a workspace", |state| {
        state["workspaces"]
            .as_array()
            .is_some_and(|workspaces| workspaces.iter().any(|w| w["entry"]["name"] == "typed-folder"))
    });
    app.quit();
}

#[test]
#[ignore = "opens the app window; run with --ignored"]
fn a_terminal_works_after_the_daemon_dies() {
    let sandbox = Sandbox::new();
    let app = sandbox.launch();
    let first = sandbox.daemon_pid().expect("the app started a daemon");
    app.quit();

    let status = Command::new("kill").args(["-9", &first.to_string()]).status().unwrap();
    assert!(status.success());
    let deadline = Instant::now() + WAIT;
    while alive(first) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!alive(first));

    let app = sandbox.launch();
    let second = sandbox.daemon_pid().expect("a new daemon");
    assert_ne!(second, first);
    app.run("echo after-$((40+2))");
    app.wait_for("output from the new daemon's shell", |state| screen(state).contains("after-42"));
    app.quit();
}
