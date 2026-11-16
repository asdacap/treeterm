//! Local UI state Electron kept in `localStorage` (`sessionNames`: names and sidebar order;
//! `recentDirectories`: the Open Workspace dialog's list), stored next to `settings.json`
//! as `ui-state.json`. Unknown keys are kept on save.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::connection::ConnectionTarget;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiState {
    /// Custom session names by [`session_key`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub session_names: BTreeMap<String, String>,
    /// Sidebar order by [`session_key`], set by dragging sessions; unlisted ones go last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_order: Vec<String>,
    /// Directories opened as workspaces by [`session_key`], most recent first.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub recent_directories: BTreeMap<String, Vec<String>>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum UiStateError {
    #[error("cannot read {path}: {source}")]
    Read { path: String, source: io::Error },
    #[error("{path} is not valid UI state ({parse}), and cannot be moved to {backup}: {source}")]
    MoveAside { path: String, backup: String, parse: serde_json::Error, source: io::Error },
    #[error("cannot write {path}: {source}")]
    Write { path: String, source: io::Error },
}

/// What [`load`] found.
#[derive(Debug)]
pub enum Loaded {
    /// The saved state; empty when there is no file yet.
    Saved(UiState),
    /// The file was not valid UI state. It was moved to `backup` and the state starts empty,
    /// so saving carries on (Electron dropped a corrupt stored value and overwrote it).
    MovedAside { error: serde_json::Error, backup: PathBuf },
}

/// What a session's name is stored under: stable across reconnects (Electron keyed it by
/// the session id, which changes on every connect).
pub fn session_key(target: &ConnectionTarget) -> String {
    match target {
        ConnectionTarget::Local => "local".to_owned(),
        ConnectionTarget::Remote(config) => format!("ssh:{}@{}:{}", config.user, config.host, config.port),
    }
}

/// `ui-state.json` in Electron's userData directory, beside `settings.json`.
pub fn ui_state_path(user_data_dir: &Path) -> PathBuf {
    user_data_dir.join("ui-state.json")
}

/// A missing file is empty state. A corrupt one is moved aside to
/// `ui-state.json.corrupt-<now, in ms since the epoch>` ([`Loaded::MovedAside`]).
pub fn load(path: &Path, now: SystemTime) -> Result<Loaded, UiStateError> {
    let shown = |path: &Path| path.display().to_string();
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Loaded::Saved(UiState::default())),
        Err(source) => return Err(UiStateError::Read { path: shown(path), source }),
    };
    let error = match serde_json::from_slice(&bytes) {
        Ok(state) => return Ok(Loaded::Saved(state)),
        Err(error) => error,
    };
    let millis = now.duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis());
    let mut backup = path.as_os_str().to_owned();
    backup.push(format!(".corrupt-{millis}"));
    let backup = PathBuf::from(backup);
    match std::fs::rename(path, &backup) {
        Ok(()) => Ok(Loaded::MovedAside { error, backup }),
        Err(source) => {
            Err(UiStateError::MoveAside { path: shown(path), backup: shown(&backup), parse: error, source })
        }
    }
}

/// Written whole through a temporary file, so a crash never leaves half a file.
pub fn save(path: &Path, state: &UiState) -> Result<(), UiStateError> {
    let write = |source| UiStateError::Write { path: path.display().to_string(), source };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(write)?;
    }
    let text = serde_json::to_string_pretty(state).map_err(|e| write(io::Error::other(e)))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, text).map_err(write)?;
    std::fs::rename(&temp, path).map_err(write)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn saved(path: &Path) -> UiState {
        match load(path, SystemTime::now()).unwrap() {
            Loaded::Saved(state) => state,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_missing_file_is_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(saved(&ui_state_path(dir.path())), UiState::default());
    }

    #[test]
    fn names_round_trip_and_unknown_keys_survive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("ui-state.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"other":{"x":1},"sessionNames":{"local":"Laptop"}}"#).unwrap();
        let mut state = saved(&path);
        assert_eq!(state.session_names.get("local").map(String::as_str), Some("Laptop"));
        state.session_names.insert("ssh:me@box:22".to_owned(), "Box".to_owned());
        state.session_order = vec!["ssh:me@box:22".to_owned(), "local".to_owned()];
        state.recent_directories.insert("local".to_owned(), vec!["/work/a".to_owned()]);
        save(&path, &state).unwrap();
        let again = saved(&path);
        assert_eq!(again.recent_directories["local"], ["/work/a"]);
        assert_eq!(again.session_names.len(), 2);
        assert_eq!(again.session_order, ["ssh:me@box:22", "local"]);
        assert_eq!(again.extra["other"]["x"], 1);
    }

    #[test]
    fn a_corrupt_file_is_moved_aside_with_its_bytes_and_the_state_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = ui_state_path(dir.path());
        let now = UNIX_EPOCH + Duration::from_millis(1_763_288_100_123);
        for bytes in [&b"{nope"[..], b"\xff\xfe not UTF-8", br#"{"sessionNames":["not a map"]}"#] {
            std::fs::write(&path, bytes).unwrap();
            let Loaded::MovedAside { backup, .. } = load(&path, now).unwrap() else { panic!("not moved aside") };
            assert_eq!(backup, dir.path().join("ui-state.json.corrupt-1763288100123"));
            assert_eq!(std::fs::read(&backup).unwrap(), bytes);
            assert!(!path.exists());
            assert_eq!(saved(&path), UiState::default(), "the next start finds no file");
        }
    }

    #[test]
    fn a_corrupt_file_that_cannot_be_moved_aside_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = ui_state_path(dir.path());
        std::fs::write(&path, "{nope").unwrap();
        // The backup's name is taken by a directory that is not empty.
        let backup = dir.path().join("ui-state.json.corrupt-0");
        std::fs::create_dir_all(backup.join("full")).unwrap();
        assert!(matches!(load(&path, UNIX_EPOCH), Err(UiStateError::MoveAside { .. })));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{nope");
    }

    #[test]
    fn an_unreadable_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = ui_state_path(dir.path());
        std::fs::create_dir(&path).unwrap();
        assert!(matches!(load(&path, SystemTime::now()), Err(UiStateError::Read { .. })));
    }

    #[test]
    fn keys_are_stable_per_target() {
        assert_eq!(session_key(&ConnectionTarget::Local), "local");
    }
}
