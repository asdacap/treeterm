//! Command line (`bin/treeterm.js`): `treeterm [directory]`, `treeterm ssh user@host[:port]`,
//! `treeterm shutdown-daemon`. Electron's `list-sessions` and `status` called RPCs the daemon
//! does not have and are not ported.

use std::path::{Path, PathBuf};

use treeterm_core::crash_report;
use treeterm_core::settings::{ConnectionLabel, IdentityFile, SshConnectionConfig};

pub const USAGE: &str = "Usage: treeterm [command] [options]

Commands:
  (no command)          Open TreeTerm
  [directory]           Open TreeTerm with that directory as a workspace
  ssh user@host[:port]  Connect to a remote host over SSH
  shutdown-daemon       Stop the local daemon (and every terminal it runs)

Options:
  -h, --help            Show this help message";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    ShutdownDaemon,
    Gui {
        workspace: Option<PathBuf>,
        ssh: Option<Box<SshConnectionConfig>>,
    },
    /// Electron's "Application Error" screen for the report a panic on the UI thread left
    /// (`--crash-report=<path>`, not for people to type: the panic hook passes it).
    CrashScreen { report: PathBuf },
}

/// `user@host[:port]` (`parseSSHTarget`); labelled with the target as typed.
pub fn parse_ssh_target(target: &str, now_ms: u128) -> Option<SshConnectionConfig> {
    let (user, rest) = target.split_once('@')?;
    if user.is_empty() || rest.is_empty() {
        return None;
    }
    let (host, port) = match rest.split_once(':') {
        Some((host, port)) => (host, parse_port(port)?),
        None => (rest, 22),
    };
    if host.is_empty() || host.contains('@') {
        return None;
    }
    Some(SshConnectionConfig {
        id: format!("ssh-{host}-{now_ms}"),
        host: host.to_owned(),
        user: user.to_owned(),
        port,
        identity_file: IdentityFile::SshDefault,
        label: ConnectionLabel::Custom(target.to_owned()),
        port_forwards: Vec::new(),
        extra: Default::default(),
    })
}

/// The target's port: digits only, as `parseSSHTarget`'s `\d+` (no sign, no spaces), and one
/// ssh can use (1–65535; Electron also passed 0 and larger numbers on to ssh).
fn parse_port(port: &str) -> Option<u16> {
    if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    match port.parse::<u16>() {
        Ok(0) | Err(_) => None,
        Ok(port) => Some(port),
    }
}

/// `args` without the program name; `cwd` resolves a relative directory.
pub fn parse(args: &[String], cwd: &Path, now_ms: u128) -> Result<Command, String> {
    if let Some(report) = args.first().and_then(|arg| arg.strip_prefix(crash_report::ARG)) {
        return Ok(Command::CrashScreen { report: PathBuf::from(report) });
    }
    match args.first().map(String::as_str) {
        Some("-h" | "--help") => Ok(Command::Help),
        Some("shutdown-daemon") => Ok(Command::ShutdownDaemon),
        Some("ssh") => {
            let target = args.get(1).ok_or("Usage: treeterm ssh user@host[:port]")?;
            let config = parse_ssh_target(target, now_ms).ok_or_else(|| format!("Invalid SSH target: {target}"))?;
            Ok(Command::Gui { workspace: None, ssh: Some(Box::new(config)) })
        }
        _ => {
            let Some(dir) = args.iter().find(|arg| !arg.starts_with('-')) else {
                return Ok(Command::Gui { workspace: None, ssh: None });
            };
            let resolved = cwd.join(dir);
            if !resolved.exists() {
                return Err(format!("Error: Directory does not exist: {}", resolved.display()));
            }
            if !resolved.is_dir() {
                return Err(format!("Error: Path is not a directory: {}", resolved.display()));
            }
            let canonical = resolved.canonicalize().map_err(|e| format!("Error: {}: {e}", resolved.display()))?;
            Ok(Command::Gui { workspace: Some(canonical), ssh: None })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn ssh_targets_take_an_optional_port() {
        let config = parse_ssh_target("me@box.lan:2222", 5).unwrap();
        assert_eq!((config.user.as_str(), config.host.as_str(), config.port), ("me", "box.lan", 2222));
        assert_eq!(config.id, "ssh-box.lan-5");
        assert_eq!(config.display_name(), "me@box.lan:2222");
        assert_eq!(parse_ssh_target("me@box", 5).unwrap().port, 22);
        assert_eq!(parse_ssh_target("me@box:022", 5).unwrap().port, 22);
        assert_eq!(parse_ssh_target("me@box:65535", 5).unwrap().port, 65535);
        assert_eq!(parse_ssh_target("me:x@box", 5).unwrap().user, "me:x");
        // The port is `\d+` (no sign, no spaces) and one ssh can use; the host has no `@`.
        let bad = [
            "box", "@box", "me@", "me@box:", "me@box:x", "me@box:0", "me@box:+22", "me@box:-22", "me@box: 22",
            "me@box:22 ", "me@box:65536", "me@box:1:2", "me@a@box",
        ];
        for bad in bad {
            assert!(parse_ssh_target(bad, 5).is_none(), "{bad}");
        }
    }

    #[test]
    fn commands_and_directories() {
        let cwd = std::env::temp_dir();
        assert_eq!(parse(&args(&["--help"]), &cwd, 1), Ok(Command::Help));
        assert_eq!(parse(&args(&["shutdown-daemon"]), &cwd, 1), Ok(Command::ShutdownDaemon));
        assert_eq!(parse(&[], &cwd, 1), Ok(Command::Gui { workspace: None, ssh: None }));
        assert!(parse(&args(&["ssh"]), &cwd, 1).is_err());
        let dir = tempfile::tempdir().unwrap();
        let Ok(Command::Gui { workspace: Some(path), .. }) = parse(&args(&[dir.path().to_str().unwrap()]), &cwd, 1) else {
            panic!("a directory opens as a workspace");
        };
        assert_eq!(path, dir.path().canonicalize().unwrap());
        assert!(parse(&args(&["/definitely/not/here"]), &cwd, 1).unwrap_err().contains("does not exist"));
    }

    /// The panic hook's argument starts the crash screen, never a workspace (the directory
    /// scan skips options).
    #[test]
    fn the_crash_report_argument_starts_the_crash_screen() {
        let cwd = std::env::temp_dir();
        assert_eq!(
            parse(&args(&["--crash-report=/x/crash.json"]), &cwd, 1),
            Ok(Command::CrashScreen { report: PathBuf::from("/x/crash.json") })
        );
    }
}
