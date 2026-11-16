//! Command line (`bin/treeterm.js`): `treeterm [directory]`, `treeterm ssh user@host[:port]`,
//! `treeterm shutdown-daemon`. Electron's `list-sessions` and `status` called RPCs the daemon
//! does not have and are not ported.

use std::path::{Component, Path, PathBuf};

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

/// `path.resolve(cwd, dir)`, as `bin/treeterm.js` resolves the directory: `dir` against `cwd`,
/// with `.` and `..` resolved by name (never above the root) and no trailing separator.
/// Symlinks are kept, so a directory reached through one keeps the path it was named by and
/// matches a workspace already open under that path.
fn resolve(cwd: &Path, dir: &str) -> PathBuf {
    let mut resolved = PathBuf::new();
    for component in cwd.join(dir).components() {
        match component {
            Component::CurDir => {}
            // At the root there is nothing to pop: `/..` is `/`.
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => resolved.push(component),
        }
    }
    resolved
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
            // An empty argument names no directory (`bin/treeterm.js` takes the first truthy one).
            let Some(dir) = args.iter().find(|arg| !arg.is_empty() && !arg.starts_with('-')) else {
                return Ok(Command::Gui { workspace: None, ssh: None });
            };
            let resolved = resolve(cwd, dir);
            // Both follow symlinks, as `fs.existsSync` and `fs.statSync` do.
            if !resolved.exists() {
                return Err(format!("Error: Directory does not exist: {}", resolved.display()));
            }
            if !resolved.is_dir() {
                return Err(format!("Error: Path is not a directory: {}", resolved.display()));
            }
            Ok(Command::Gui { workspace: Some(resolved), ssh: None })
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

    /// The workspace `args` open from `cwd`.
    fn workspace(list: &[&str], cwd: &Path) -> PathBuf {
        match parse(&args(list), cwd, 1) {
            Ok(Command::Gui { workspace: Some(path), ssh: None }) => path,
            other => panic!("{list:?} opens no workspace: {other:?}"),
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
        // As given: on macOS the temporary directory is under the `/var` symlink.
        assert_eq!(workspace(&[dir.path().to_str().unwrap()], &cwd), dir.path());
        assert_eq!(workspace(&["-x", dir.path().to_str().unwrap()], &cwd), dir.path());
        // An empty argument names no directory.
        assert_eq!(parse(&args(&[""]), &cwd, 1), Ok(Command::Gui { workspace: None, ssh: None }));
        assert_eq!(workspace(&["", dir.path().to_str().unwrap()], &cwd), dir.path());
        assert_eq!(
            parse(&args(&["/definitely/not/here"]), &cwd, 1),
            Err("Error: Directory does not exist: /definitely/not/here".to_owned())
        );
    }

    /// A relative directory resolves against the working directory by name, as
    /// `path.resolve(cwd, dir)` does, and the errors name the resolved path.
    #[test]
    fn a_relative_directory_resolves_by_name() {
        let cwd = tempfile::tempdir().unwrap();
        let (cwd, sub) = (cwd.path(), cwd.path().join("sub"));
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(cwd.join("file"), "").unwrap();
        for dir in ["sub", "sub/", "./sub/.", "sub/../sub/", "sub//"] {
            assert_eq!(workspace(&[dir], cwd), sub, "{dir}");
        }
        assert_eq!(workspace(&["."], cwd), cwd);
        assert_eq!(workspace(&[".."], &sub), cwd);
        assert_eq!(
            parse(&args(&["sub/../gone/"]), cwd, 1),
            Err(format!("Error: Directory does not exist: {}", cwd.join("gone").display()))
        );
        assert_eq!(
            parse(&args(&["./file"]), cwd, 1),
            Err(format!("Error: Path is not a directory: {}", cwd.join("file").display()))
        );
    }

    /// A directory reached through a symlink keeps the link's path, so a workspace already
    /// open under it is the one selected; `..` after the link goes back up the link's path.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_directory_keeps_the_link_path() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("elsewhere").join("target");
        std::fs::create_dir_all(&target).unwrap();
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(workspace(&[link.to_str().unwrap()], Path::new("/")), link);
        assert_eq!(workspace(&["link/"], root.path()), link);
        assert_eq!(workspace(&["link/.."], root.path()), root.path());
        // The checks follow the link, as `fs.existsSync` and `fs.statSync` do.
        std::os::unix::fs::symlink(root.path().join("gone"), root.path().join("dangling")).unwrap();
        assert_eq!(
            parse(&args(&["dangling"]), root.path(), 1),
            Err(format!("Error: Directory does not exist: {}", root.path().join("dangling").display()))
        );
    }

    #[test]
    fn resolve_never_goes_above_the_root() {
        assert_eq!(resolve(Path::new("/a/b"), "../../../c/"), Path::new("/c"));
        assert_eq!(resolve(Path::new("/a"), "/x/./y//"), Path::new("/x/y"));
        assert_eq!(resolve(Path::new("/"), ".."), Path::new("/"));
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
