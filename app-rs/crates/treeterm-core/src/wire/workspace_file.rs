//! Workspace body files (`<workspace_data_dir>/<id>.json`); mirrors `src/shared/workspaceFile.ts`.
//!
//! The daemon treats these files as opaque bytes. `id` and `path` are not stored here: they
//! come from the session's `WorkspaceRef` list.
//!
//! Interop rules while Electron coexists:
//! - Optional fields are omitted, never written as `null` (Zod's `.optional()` rejects null).
//! - Unknown top-level keys are kept in `extra` and written back, but Electron's Zod strips
//!   them on its next write, so data only the native app uses must live in `metadata`.

use std::collections::BTreeMap;

use indexmap::IndexMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceStatus {
    Active,
    Merged,
    Abandoned,
}

/// One tab. `state` is owned and validated by the tab's application.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub application_id: String,
    pub title: String,
    pub state: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSettings {
    /// Empty means inherit from the parent or use the global default.
    pub default_application_id: String,
}

/// The logical workspace body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFile {
    pub name: String,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub status: WorkspaceStatus,
    pub is_git_repo: bool,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub git_root_path: Option<String>,
    pub is_worktree: bool,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub is_detached: Option<bool>,
    /// In file order: tab order falls back to it where the layout does not decide.
    pub app_states: IndexMap<String, AppState>,
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub active_tab_id: Option<String>,
    pub metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub settings: WorktreeSettings,
    #[serde(default)]
    pub favourite_paths: Vec<String>,
    pub created_at: i64,
    pub last_activity: i64,
    /// Keys this version doesn't know, preserved on write.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// What is actually on disk: the body plus `parentHash`, the sha256 of the body this
/// write superseded (`""` for the first). Chaining it in means successive bodies never
/// hash alike, which is what lets a writer recognise its own watch echoes by sha alone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredWorkspaceFile {
    pub parent_hash: String,
    #[serde(flatten)]
    pub body: WorkspaceFile,
}

/// Zod's `.optional()` accepts a missing key but not an explicit `null`; neither do we.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[derive(Debug, thiserror::Error)]
#[error("invalid workspace file: {0}")]
pub struct WorkspaceFileError(#[from] serde_json::Error);

/// Parse a stored body, dropping the `parentHash` bookkeeping.
pub fn parse_workspace_file(content: &[u8]) -> Result<WorkspaceFile, WorkspaceFileError> {
    let stored: StoredWorkspaceFile = serde_json::from_slice(content)?;
    Ok(stored.body)
}

/// The bytes to write for `body`: the stored envelope with `parent_hash` chained in, keys
/// sorted. `parent_hash` is the sha of the body this write supersedes (`""` for the first).
pub fn stored_json(body: &WorkspaceFile, parent_hash: &str) -> Result<String, serde_json::Error> {
    super::stable_json::to_string(&StoredWorkspaceFile {
        parent_hash: parent_hash.to_owned(),
        body: body.clone(),
    })
}

/// Where a workspace sits in the tree. `parentId` is optional on the wire only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parent<'a> {
    Root,
    Child(&'a str),
}

impl WorkspaceFile {
    pub fn parent(&self) -> Parent<'_> {
        match &self.parent_id {
            Some(id) => Parent::Child(id),
            None => Parent::Root,
        }
    }

    /// The name shown in the tree: the user's display name when set, else the directory name.
    pub fn display_name(&self) -> &str {
        match self.metadata.get("displayName") {
            Some(name) if !name.is_empty() => name,
            Some(_) | None => &self.name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_json_round_trips_and_omits_absent_optionals() {
        let body = parse_workspace_file(STORED.as_bytes()).unwrap();
        let mut minimal = body.clone();
        minimal.parent_id = None;
        minimal.git_branch = None;
        minimal.active_tab_id = None;
        let json = stored_json(&minimal, "abc").unwrap();
        assert!(!json.contains("null"), "{json}");
        assert!(json.starts_with(r#"{"activeTabId"#) || json.starts_with(r#"{"appStates"#));
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["parentHash"], "abc");
        assert_eq!(parse_workspace_file(json.as_bytes()).unwrap(), minimal);
        // Deterministic: the same body always serializes the same way.
        assert_eq!(json, stored_json(&minimal, "abc").unwrap());
    }

    const STORED: &str = r#"{
        "appStates": {"tab-1": {"applicationId": "terminal", "title": "Terminal", "state": {"ptyId": "pty-1", "custom": [1, 2]}}},
        "activeTabId": "tab-1",
        "createdAt": 1781392504626,
        "favouritePaths": ["src"],
        "gitBranch": "feat",
        "gitRootPath": "/repo",
        "isDetached": false,
        "isGitRepo": true,
        "isWorktree": true,
        "lastActivity": 1781392504999,
        "metadata": {"displayName": "Feature", "sortOrder": "2"},
        "name": "feat",
        "parentHash": "abc",
        "parentId": "ws-root",
        "settings": {"defaultApplicationId": ""},
        "status": "active",
        "futureField": {"x": 1}
    }"#;

    #[test]
    fn parses_a_stored_body_and_drops_parent_hash() {
        let ws = parse_workspace_file(STORED.as_bytes()).unwrap();
        assert_eq!(ws.name, "feat");
        assert_eq!(ws.parent(), Parent::Child("ws-root"));
        assert_eq!(ws.status, WorkspaceStatus::Active);
        assert_eq!(ws.app_states["tab-1"].application_id, "terminal");
        assert_eq!(ws.app_states["tab-1"].state["custom"], serde_json::json!([1, 2]));
        assert!(!ws.extra.contains_key("parentHash"));
        assert_eq!(ws.extra["futureField"], serde_json::json!({"x": 1}));
    }

    #[test]
    fn legacy_files_default_settings_and_favourites() {
        let legacy = r#"{"name":"a","status":"merged","isGitRepo":false,"isWorktree":false,
            "appStates":{},"metadata":{},"createdAt":1,"lastActivity":2,"parentHash":""}"#;
        let ws = parse_workspace_file(legacy.as_bytes()).unwrap();
        assert_eq!(ws.settings, WorktreeSettings::default());
        assert!(ws.favourite_paths.is_empty());
        assert_eq!(ws.parent(), Parent::Root);
        assert_eq!(ws.status, WorkspaceStatus::Merged);
    }

    #[test]
    fn rejects_bodies_electron_would_reject() {
        // Missing parentHash (StoredWorkspaceFileSchema requires it).
        let no_hash = r#"{"name":"a","status":"active","isGitRepo":false,"isWorktree":false,
            "appStates":{},"metadata":{},"createdAt":1,"lastActivity":2}"#;
        assert!(parse_workspace_file(no_hash.as_bytes()).is_err());
        // Unknown status.
        let bad_status = STORED.replace("\"active\"", "\"deleted\"");
        assert!(parse_workspace_file(bad_status.as_bytes()).is_err());
        // null where Zod expects an optional value.
        let null_parent = STORED.replace("\"ws-root\"", "null");
        assert!(parse_workspace_file(null_parent.as_bytes()).is_err());
    }

    #[test]
    fn serializing_omits_absent_optionals_and_keeps_unknown_keys() {
        let mut ws = parse_workspace_file(STORED.as_bytes()).unwrap();
        ws.parent_id = None;
        ws.active_tab_id = None;
        let stored = StoredWorkspaceFile { parent_hash: "h".into(), body: ws };
        let json: Value = serde_json::to_value(&stored).unwrap();
        let obj = json.as_object().unwrap();
        assert!(!obj.contains_key("parentId"));
        assert!(!obj.contains_key("activeTabId"));
        assert_eq!(obj["parentHash"], "h");
        assert_eq!(obj["futureField"], serde_json::json!({"x": 1}));
    }

    #[test]
    fn display_name_prefers_metadata() {
        let mut ws = parse_workspace_file(STORED.as_bytes()).unwrap();
        assert_eq!(ws.display_name(), "Feature");
        ws.metadata.insert("displayName".into(), String::new());
        assert_eq!(ws.display_name(), "feat");
    }
}
