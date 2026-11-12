//! Local UI state Electron kept in `localStorage` (`sessionNames`: names and sidebar order),
//! stored next to `settings.json` as `ui-state.json`. Unknown keys are kept on save.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

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
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum UiStateError {
    #[error("cannot read {path}: {source}")]
    Read { path: String, source: io::Error },
    #[error("{path} is not valid UI state: {source}")]
    Parse { path: String, source: serde_json::Error },
    #[error("cannot write {path}: {source}")]
    Write { path: String, source: io::Error },
}

/// What a session's name is stored under: stable across reconnects (Electron keyed it by
/// the session id, which changes on every connect).
pub fn session_key(target: &ConnectionTarget) -> String {
    match target {
        ConnectionTarget::Local => "local".to_owned(),
        ConnectionTarget::Remote(config) => format!("ssh:{}@{}:{}", config.user, config.host, config.port),
    }
}

/// A missing file is empty state.
pub fn load(path: &Path) -> Result<UiState, UiStateError> {
    let shown = || path.display().to_string();
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|source| UiStateError::Parse { path: shown(), source }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(UiState::default()),
        Err(source) => Err(UiStateError::Read { path: shown(), source }),
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

    #[test]
    fn a_missing_file_is_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(&dir.path().join("ui-state.json")).unwrap(), UiState::default());
    }

    #[test]
    fn names_round_trip_and_unknown_keys_survive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("ui-state.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"other":{"x":1},"sessionNames":{"local":"Laptop"}}"#).unwrap();
        let mut state = load(&path).unwrap();
        assert_eq!(state.session_names.get("local").map(String::as_str), Some("Laptop"));
        state.session_names.insert("ssh:me@box:22".to_owned(), "Box".to_owned());
        state.session_order = vec!["ssh:me@box:22".to_owned(), "local".to_owned()];
        save(&path, &state).unwrap();
        let again = load(&path).unwrap();
        assert_eq!(again.session_names.len(), 2);
        assert_eq!(again.session_order, ["ssh:me@box:22", "local"]);
        assert_eq!(again.extra["other"]["x"], 1);
    }

    #[test]
    fn a_corrupt_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-state.json");
        std::fs::write(&path, "{nope").unwrap();
        assert!(matches!(load(&path), Err(UiStateError::Parse { .. })));
    }

    #[test]
    fn keys_are_stable_per_target() {
        assert_eq!(session_key(&ConnectionTarget::Local), "local");
    }
}
