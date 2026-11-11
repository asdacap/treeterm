//! Packaging for the native app (`cargo run -p xtask -- <command>` from `app-rs/`).
//!
//! Plain `std::process` around `cargo`, `codesign`, `sips`/`iconutil`, `ditto` and `tar`: no
//! packaging dependencies, nothing fetched at build time. See [`USAGE`] and the "Packaging"
//! section of `app-rs/README.md`.

mod bundle;
mod cargo;
mod daemons;

use std::fmt::Display;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
Usage: cargo run -p xtask -- <command> [--debug]

Commands:
  build-daemons  Build the daemons into <repo>/out/daemon-rs/: the local one for this host
                 and the static musl ones uploaded to remote Linux hosts (x86_64, aarch64).
                 A remote target whose Rust target or linker is missing is skipped.
  bundle         Build the app (release) and package it with the daemons in out/daemon-rs/:
                   macOS  app-rs/target/dist/TreeTerm.app and TreeTerm-<version>-<arch>-macos.zip
                   Linux  app-rs/target/dist/treeterm-<version>-<arch>-linux/ and .tar.gz
  dist           build-daemons, then bundle.

Options:
  --debug        Package a debug build of the app (bundle, dist), to try out a bundle quickly.
";

pub type Result<T, E = String> = std::result::Result<T, E>;

/// Attach what was being done to an error (`io::Error`s do not name the path).
pub trait Context<T> {
    fn context(self, what: impl FnOnce() -> String) -> Result<T>;
}

impl<T, E: Display> Context<T> for std::result::Result<T, E> {
    fn context(self, what: impl FnOnce() -> String) -> Result<T> {
        self.map_err(|error| format!("{}: {error}", what()))
    }
}

/// Where everything is, from the repo root.
pub struct Repo {
    pub root: PathBuf,
}

impl Repo {
    fn locate() -> Self {
        // app-rs/xtask → repo root.
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("xtask lives at <repo>/app-rs/xtask")
            .to_owned();
        Self { root }
    }

    pub fn app_rs(&self) -> PathBuf {
        self.root.join("app-rs")
    }

    pub fn daemon_rs(&self) -> PathBuf {
        self.root.join("daemon-rs")
    }

    /// Built daemons, shared with Electron's `npm run build:daemon-rs*` and the native app's
    /// debug-build fallback (`treeterm-app/src/daemon_binary.rs`).
    pub fn daemon_out(&self) -> PathBuf {
        self.root.join("out/daemon-rs")
    }

    /// Templates and the icon (`app-rs/packaging/`).
    pub fn packaging(&self) -> PathBuf {
        self.app_rs().join("packaging")
    }

    pub fn dist(&self) -> PathBuf {
        self.app_rs().join("target/dist")
    }
}

/// Which build of the app to package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Release,
    Debug,
}

#[derive(Debug, PartialEq, Eq)]
enum Task {
    BuildDaemons,
    Bundle(Profile),
    Dist(Profile),
    Help,
}

fn parse(args: &[String]) -> Result<Task> {
    let mut profile = Profile::Release;
    let mut command = None;
    for arg in args {
        match arg.as_str() {
            "--debug" => profile = Profile::Debug,
            "-h" | "--help" | "help" => return Ok(Task::Help),
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}")),
            name if command.is_none() => command = Some(name.to_owned()),
            extra => return Err(format!("unexpected argument {extra}")),
        }
    }
    match (command.as_deref(), profile) {
        (None, _) => Ok(Task::Help),
        (Some("build-daemons"), Profile::Release) => Ok(Task::BuildDaemons),
        (Some("build-daemons"), Profile::Debug) => Err(
            "build-daemons always builds release daemons; --debug is for bundle and dist".into(),
        ),
        (Some("bundle"), profile) => Ok(Task::Bundle(profile)),
        (Some("dist"), profile) => Ok(Task::Dist(profile)),
        (Some(other), _) => Err(format!("unknown command {other}")),
    }
}

fn run(task: Task, repo: &Repo) -> Result<()> {
    match task {
        Task::Help => {
            print!("{USAGE}");
            Ok(())
        }
        Task::BuildDaemons => daemons::build(repo),
        Task::Bundle(profile) => bundle::bundle(repo, profile),
        Task::Dist(profile) => {
            daemons::build(repo)?;
            bundle::bundle(repo, profile)
        }
    }
}

/// A step heading.
pub fn step(message: impl Display) {
    eprintln!("==> {message}");
}

pub fn warn(message: impl Display) {
    eprintln!("warning: {message}");
}

/// Run `command` to completion with inherited output; a non-zero exit is an error.
pub fn exec(command: &mut Command) -> Result<()> {
    let shown = format!("{command:?}");
    let status = command.status().context(|| format!("start {shown}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{shown} failed ({status})"))
    }
}

/// Copy an executable into place through a new file (permissions kept): overwriting a binary
/// in place can kill a daemon running from it, and macOS kills a re-signed binary that starts
/// from the old inode.
pub fn install_file(from: &Path, to: &Path) -> Result<()> {
    let staging = to.with_extension("xtask-new");
    fs::copy(from, &staging)
        .context(|| format!("copy {} to {}", from.display(), staging.display()))?;
    fs::rename(&staging, to).context(|| format!("move {} to {}", staging.display(), to.display()))
}

pub fn remove_dir_if_exists(dir: &Path) -> Result<()> {
    if dir.exists() {
        fs::remove_dir_all(dir).context(|| format!("remove {}", dir.display()))?;
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = parse(&args).and_then(|task| run(task, &Repo::locate()));
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn parses_commands_and_the_debug_flag() {
        assert_eq!(parse(&args(&["build-daemons"])), Ok(Task::BuildDaemons));
        assert_eq!(
            parse(&args(&["bundle"])),
            Ok(Task::Bundle(Profile::Release))
        );
        assert_eq!(
            parse(&args(&["bundle", "--debug"])),
            Ok(Task::Bundle(Profile::Debug))
        );
        assert_eq!(
            parse(&args(&["--debug", "dist"])),
            Ok(Task::Dist(Profile::Debug))
        );
        assert_eq!(parse(&args(&[])), Ok(Task::Help));
        assert_eq!(parse(&args(&["dist", "--help"])), Ok(Task::Help));
    }

    #[test]
    fn rejects_what_it_does_not_know() {
        assert_eq!(
            parse(&args(&["package"])),
            Err("unknown command package".to_owned())
        );
        assert_eq!(
            parse(&args(&["bundle", "--sign"])),
            Err("unknown option --sign".to_owned())
        );
        assert_eq!(
            parse(&args(&["bundle", "dist"])),
            Err("unexpected argument dist".to_owned())
        );
        assert!(parse(&args(&["build-daemons", "--debug"])).is_err());
    }

    #[test]
    fn install_file_replaces_the_destination_with_a_new_file() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("built");
        let to = dir.path().join("treeterm-daemon");
        fs::write(&from, b"new").unwrap();
        fs::set_permissions(&from, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(&to, b"old").unwrap();
        let old_inode = fs::metadata(&to).unwrap().ino();

        install_file(&from, &to).unwrap();

        let installed = fs::metadata(&to).unwrap();
        assert_eq!(fs::read(&to).unwrap(), b"new");
        assert_ne!(installed.ino(), old_inode);
        assert_eq!(installed.permissions().mode() & 0o777, 0o755);
        assert!(!to.with_extension("xtask-new").exists());
    }

    #[test]
    fn the_repo_is_two_levels_above_the_xtask_crate() {
        let repo = Repo::locate();
        assert!(repo.app_rs().join("Cargo.toml").is_file());
        assert!(repo.daemon_rs().join("Cargo.toml").is_file());
        assert!(repo.packaging().join("treeterm.svg").is_file());
    }
}
