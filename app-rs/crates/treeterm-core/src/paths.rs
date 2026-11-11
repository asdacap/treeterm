//! Filesystem locations shared with the Electron app.
//!
//! While both apps coexist they must agree on every path below: a different socket path
//! means a second daemon with its own in-memory session (see `daemon-rs/.../socket_path.rs`,
//! whose `/tmp` default differs from Electron's — Electron passes `TREETERM_SOCKET_PATH`
//! when it spawns the daemon, and so do we).

use std::path::PathBuf;

/// Overrides the local daemon socket for both the app and the daemon it spawns.
pub const SOCKET_PATH_ENV: &str = "TREETERM_SOCKET_PATH";

/// Electron's `app.getName()` (package.json `name`), which names its userData directory.
const APP_NAME: &str = "treeterm";

/// Environment lookup, injected so path rules are testable without mutating the process env.
pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

pub fn process_env(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

fn non_empty(env: Env<'_>, key: &str) -> Option<String> {
    env(key).filter(|value| !value.is_empty())
}

/// Node's `os.tmpdir()` on POSIX: `TMPDIR || TMP || TEMP || "/tmp"` with one trailing
/// slash stripped. Deliberately not `std::env::temp_dir()`, which ignores `TMP`/`TEMP`.
pub fn node_tmpdir(env: Env<'_>) -> PathBuf {
    let dir = non_empty(env, "TMPDIR")
        .or_else(|| non_empty(env, "TMP"))
        .or_else(|| non_empty(env, "TEMP"))
        .unwrap_or_else(|| "/tmp".to_owned());
    match dir.strip_suffix('/') {
        Some(stripped) if !stripped.is_empty() => PathBuf::from(stripped),
        Some(_) | None => PathBuf::from(dir),
    }
}

/// The local daemon socket, resolved exactly as Electron's main process does
/// (`process.env.TREETERM_SOCKET_PATH || getDefaultSocketPath()`).
pub fn local_socket_path(env: Env<'_>, uid: u32) -> PathBuf {
    match non_empty(env, SOCKET_PATH_ENV) {
        Some(path) => PathBuf::from(path),
        None => node_tmpdir(env)
            .join(format!("treeterm-{uid}"))
            .join("daemon.sock"),
    }
}

pub fn current_uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("HOME is not set; cannot locate ~/.treeterm")]
    NoHome,
    #[error("no per-user config directory on this platform")]
    NoConfigDir,
}

/// `~/.treeterm`: daemon pid file, workspace bodies, worktree registry.
pub fn data_dir(env: Env<'_>) -> Result<PathBuf, PathError> {
    non_empty(env, "HOME")
        .map(|home| PathBuf::from(home).join(".treeterm"))
        .ok_or(PathError::NoHome)
}

/// Electron's `app.getPath('userData')`: `settings.json`, `daemon.log`.
/// macOS: `~/Library/Application Support/treeterm`; Linux: `$XDG_CONFIG_HOME/treeterm`.
pub fn user_data_dir() -> Result<PathBuf, PathError> {
    dirs::config_dir()
        .map(|dir| dir.join(APP_NAME))
        .ok_or(PathError::NoConfigDir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn tmpdir_prefers_tmpdir_then_tmp_then_temp() {
        let env = env_of(&[("TMPDIR", "/a"), ("TMP", "/b"), ("TEMP", "/c")]);
        assert_eq!(node_tmpdir(&env), PathBuf::from("/a"));
        let env = env_of(&[("TMP", "/b"), ("TEMP", "/c")]);
        assert_eq!(node_tmpdir(&env), PathBuf::from("/b"));
        let env = env_of(&[("TEMP", "/c")]);
        assert_eq!(node_tmpdir(&env), PathBuf::from("/c"));
    }

    #[test]
    fn tmpdir_defaults_to_tmp_and_skips_empty_values() {
        assert_eq!(node_tmpdir(&env_of(&[])), PathBuf::from("/tmp"));
        let env = env_of(&[("TMPDIR", ""), ("TMP", "/b")]);
        assert_eq!(node_tmpdir(&env), PathBuf::from("/b"));
    }

    #[test]
    fn tmpdir_strips_one_trailing_slash_like_node() {
        // macOS sets TMPDIR=/var/folders/xx/yyyy/T/
        let env = env_of(&[("TMPDIR", "/var/folders/xx/yy/T/")]);
        assert_eq!(node_tmpdir(&env), PathBuf::from("/var/folders/xx/yy/T"));
        assert_eq!(node_tmpdir(&env_of(&[("TMPDIR", "/")])), PathBuf::from("/"));
    }

    #[test]
    fn socket_path_honours_override() {
        let env = env_of(&[(SOCKET_PATH_ENV, "/tmp/tt/d.sock"), ("TMPDIR", "/x/")]);
        assert_eq!(local_socket_path(&env, 501), PathBuf::from("/tmp/tt/d.sock"));
    }

    #[test]
    fn socket_path_matches_electron_default() {
        let env = env_of(&[("TMPDIR", "/var/folders/xx/yy/T/")]);
        assert_eq!(
            local_socket_path(&env, 501),
            PathBuf::from("/var/folders/xx/yy/T/treeterm-501/daemon.sock")
        );
        let env = env_of(&[(SOCKET_PATH_ENV, "")]);
        assert_eq!(
            local_socket_path(&env, 1000),
            PathBuf::from("/tmp/treeterm-1000/daemon.sock")
        );
    }

    #[test]
    fn data_dir_is_under_home() {
        let env = env_of(&[("HOME", "/home/u")]);
        assert_eq!(data_dir(&env).unwrap(), PathBuf::from("/home/u/.treeterm"));
        assert!(matches!(data_dir(&env_of(&[])), Err(PathError::NoHome)));
    }
}
