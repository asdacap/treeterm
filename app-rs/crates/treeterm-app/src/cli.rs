//! Command line (`bin/treeterm.js`): `treeterm [directory]`, `treeterm ssh user@host[:port]`,
//! `treeterm shutdown-daemon`. Electron's `list-sessions` and `status` called RPCs the daemon
//! does not have and are not ported.

use std::path::{Path, PathBuf};

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
        ssh: Option<SshConnectionConfig>,
    },
}

/// `user@host[:port]` (`parseSSHTarget`); labelled with the target as typed.
pub fn parse_ssh_target(target: &str, now_ms: u128) -> Option<SshConnectionConfig> {
    let (user, rest) = target.split_once('@')?;
    if user.is_empty() || rest.is_empty() {
        return None;
    }
    let (host, port) = match rest.split_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().ok().filter(|p| *p > 0)?),
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

/// `args` without the program name; `cwd` resolves a relative directory.
pub fn parse(args: &[String], cwd: &Path, now_ms: u128) -> Result<Command, String> {
    match args.first().map(String::as_str) {
        Some("-h" | "--help") => Ok(Command::Help),
        Some("shutdown-daemon") => Ok(Command::ShutdownDaemon),
        Some("ssh") => {
            let target = args.get(1).ok_or("Usage: treeterm ssh user@host[:port]")?;
            let config = parse_ssh_target(target, now_ms).ok_or_else(|| format!("Invalid SSH target: {target}"))?;
            Ok(Command::Gui { workspace: None, ssh: Some(config) })
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
        for bad in ["box", "@box", "me@", "me@box:x", "me@box:0"] {
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
}
