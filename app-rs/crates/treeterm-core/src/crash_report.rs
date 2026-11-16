//! What a panic on the UI thread leaves for the crash screen.
//!
//! Electron wrapped its renderer in an error boundary (`App.tsx`): an error while rendering
//! replaced the page with "Application Error" (`AppErrorFallback`), whose "Reload Application"
//! reloaded the page while the main process and the daemon kept running. GPUI cannot carry on
//! after a panic on its thread (the panic unwinds into AppKit, which aborts the process), so
//! the app's panic hook writes a [`CrashReport`] and starts the app again on the crash screen
//! ([`hand_off`]). The daemon keeps every PTY, so the screen's "Reload Application"
//! ([`relaunch`]) brings the session back.

use std::ffi::OsString;
use std::fmt::Display;
use std::fs;
use std::io;
use std::os::unix::process::CommandExt as _;
use std::panic::{Location, PanicHookInfo};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

/// The argument that starts the app on the crash screen: `--crash-report=<report path>`.
pub const ARG: &str = "--crash-report=";

/// The report's file in the directory [`hand_off`] is given (the app's `user_data_dir`).
pub const FILE: &str = "crash.json";

/// What panicked, as Electron's boundary had it from the thrown `Error`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashReport {
    /// The panic's message (`error.message`).
    pub message: String,
    /// Where it panicked, then the backtrace (`error.stack`).
    pub stack: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CrashReportError {
    #[error("cannot read the crash report {}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("{} is not a crash report: {source}", path.display())]
    Parse { path: PathBuf, source: serde_json::Error },
    #[error("cannot write the crash report {}: {source}", path.display())]
    Write { path: PathBuf, source: io::Error },
    #[error("cannot start the crash screen: {0}")]
    Relaunch(io::Error),
}

impl CrashReport {
    /// The report of the panic a panic hook is called for, with `backtrace` captured there.
    pub fn from_panic(info: &PanicHookInfo<'_>, backtrace: &dyn Display) -> Self {
        Self::new(info.payload_as_str(), info.location(), backtrace)
    }

    /// `message` is the panic's payload when that is a string; any other payload reads
    /// `Box<dyn Any>`, as in Rust's own panic message.
    fn new(message: Option<&str>, location: Option<&Location<'_>>, backtrace: &dyn Display) -> Self {
        let stack = match location {
            Some(location) => format!("panicked at {location}:\n{backtrace}"),
            None => backtrace.to_string(),
        };
        Self { message: message.unwrap_or("Box<dyn Any>").to_owned(), stack }
    }

    pub fn read(path: &Path) -> Result<Self, CrashReportError> {
        let text = fs::read_to_string(path).map_err(|source| CrashReportError::Read { path: path.to_owned(), source })?;
        serde_json::from_str(&text).map_err(|source| CrashReportError::Parse { path: path.to_owned(), source })
    }

    fn write(&self, path: &Path) -> Result<(), CrashReportError> {
        let write = |source| CrashReportError::Write { path: path.to_owned(), source };
        let text = serde_json::to_string_pretty(self).map_err(|error| write(io::Error::other(error)))?;
        fs::write(path, text).map_err(write)
    }
}

/// Whether a panic on the thread named `name` ends the app: one on its UI thread (Rust names
/// it `main`). The I/O runtime's threads (`treeterm-io`) keep theirs to the task that panicked.
pub fn is_ui_thread(name: Option<&str>) -> bool {
    name == Some("main")
}

/// Write `report` to [`FILE`] in `dir` and start this program again on the crash screen for
/// it. The caller exits next.
pub fn hand_off(report: &CrashReport, dir: &Path) -> Result<(), CrashReportError> {
    let arg = save(report, dir)?;
    relaunch(&[arg]).map_err(CrashReportError::Relaunch)
}

/// Write `report` to [`FILE`] in `dir`, made if missing; the argument that shows it.
fn save(report: &CrashReport, dir: &Path) -> Result<OsString, CrashReportError> {
    let path = dir.join(FILE);
    fs::create_dir_all(dir).map_err(|source| CrashReportError::Write { path: path.clone(), source })?;
    report.write(&path)?;
    let mut arg = OsString::from(ARG);
    arg.push(&path);
    Ok(arg)
}

/// Start this program again with `args`, as the next process of the app: the caller exits
/// (or quits) right after, so nothing waits for it.
pub fn relaunch(args: &[OsString]) -> io::Result<()> {
    let program = std::env::current_exe()?;
    relaunch_command(&program, args).spawn()?;
    Ok(())
}

/// The new process gets its own process group, so it never gets the ^C of a terminal the
/// app was started from, and writes its log where this one's goes.
fn relaunch_command(program: &Path, args: &[OsString]) -> Command {
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).process_group(0);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_reads_the_panic_message_and_where_it_panicked() {
        let location = Location::caller();
        let report = CrashReport::new(Some("boom"), Some(location), &"   0: treeterm::main");
        assert_eq!(report.message, "boom");
        assert_eq!(report.stack, format!("panicked at {}:{}:{}:\n   0: treeterm::main", location.file(), location.line(), location.column()));
    }

    #[test]
    fn a_payload_that_is_not_a_string_reads_as_rusts_message_has_it() {
        let report = CrashReport::new(None, None, &"backtrace");
        assert_eq!(report, CrashReport { message: "Box<dyn Any>".into(), stack: "backtrace".into() });
    }

    #[test]
    fn only_the_ui_thread_ends_the_app() {
        assert!(is_ui_thread(Some("main")));
        assert!(!is_ui_thread(Some("treeterm-io")));
        assert!(!is_ui_thread(None));
    }

    /// The saved report comes back from the path the crash screen's argument names.
    #[test]
    fn a_saved_report_reads_back_from_the_argument() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("treeterm");
        let report = CrashReport { message: "boom".into(), stack: "panicked at src/main.rs:1:1:\n   0: main".into() };
        let arg = save(&report, &data_dir).unwrap();
        let path = arg.to_str().unwrap().strip_prefix(ARG).expect("the crash screen's argument");
        assert_eq!(Path::new(path), data_dir.join(FILE));
        assert_eq!(CrashReport::read(Path::new(path)).unwrap(), report);
    }

    #[test]
    fn a_missing_or_corrupt_report_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        assert!(matches!(CrashReport::read(&path), Err(CrashReportError::Read { .. })));
        fs::write(&path, "{").unwrap();
        assert!(matches!(CrashReport::read(&path), Err(CrashReportError::Parse { .. })));
    }

    #[test]
    fn a_report_that_cannot_be_written_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        fs::write(&file, "").unwrap();
        let report = CrashReport { message: "boom".into(), stack: String::new() };
        assert!(matches!(save(&report, &file), Err(CrashReportError::Write { .. })));
    }

    #[test]
    fn the_new_process_runs_this_program_with_the_arguments() {
        let command = relaunch_command(Path::new("/app/treeterm"), &[OsString::from("--crash-report=/x/crash.json")]);
        assert_eq!(command.get_program(), "/app/treeterm");
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["--crash-report=/x/crash.json"]);
    }
}
