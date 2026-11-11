//! Worktrees the user has opened, remembered across sessions in `~/.treeterm/worktrees.json`
//! on the daemon's host (port of `renderer/lib/worktreeRegistry.ts`).
//!
//! The file is shared by every window and app instance, Electron's included. Each change is
//! a compare-and-swap read-modify-write: when another writer got in between, the change is
//! re-applied to a fresh read, up to [`MAX_CAS_RETRIES`] times.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::exec::{
    DEFAULT_EXEC_TIMEOUT, ExecApi, ExecError, ExecRequest, ExecTimeout, collect_output,
};
use crate::fs::{
    FilesystemApi, FsError, MAX_READ_FILE_BYTES, ReadOutcome, WriteGuard, WriteOutcome, sha256_hex,
};
use crate::fs_ext::{FileWriteApi, is_missing_file_error};
use crate::wire::workspace_file::WorkspaceFile;

pub const REGISTRY_DIR_REL: &str = ".treeterm";
pub const REGISTRY_FILE: &str = "worktrees.json";
pub const MAX_CAS_RETRIES: usize = 5;

/// One remembered worktree, as stored. `null` display name and description are part of the
/// file format (it is not Zod-validated).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeRegistryEntry {
    pub path: String,
    pub branch: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    /// Milliseconds since the epoch.
    pub last_used_at: i64,
    /// Keys this version does not know, kept on rewrite.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// An entry to record; `upsert` stamps `lastUsedAt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryEntryInput {
    pub path: String,
    pub branch: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
}

#[derive(Serialize)]
struct RegistryFile<'a> {
    version: u32,
    entries: &'a [WorktreeRegistryEntry],
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum RegistryError {
    #[error(transparent)]
    Exec(#[from] ExecError),
    #[error("Failed to resolve remote home directory")]
    HomeDirUnresolved,
    #[error("Failed to read worktree registry: {0}")]
    Read(FsError),
    #[error(
        "Failed to read worktree registry: it is {size} bytes, over the {MAX_READ_FILE_BYTES} byte read limit"
    )]
    TooLarge { size: u64 },
    #[error("Worktree registry file is not valid JSON: {0}")]
    InvalidJson(String),
    #[error("Worktree registry file has invalid shape")]
    InvalidShape,
    #[error("Unsupported worktree registry version: {0}")]
    UnsupportedVersion(String),
    #[error("Worktree registry entries are invalid: {0}")]
    InvalidEntries(String),
    #[error("Failed to write worktree registry: {0}")]
    Write(FsError),
    #[error(
        "Failed to write worktree registry after {MAX_CAS_RETRIES} attempts (concurrent writers)"
    )]
    Contended,
}

/// The filesystem calls the registry makes.
pub trait RegistryFs: FilesystemApi + FileWriteApi {}

impl<T: FilesystemApi + FileWriteApi + ?Sized> RegistryFs for T {}

/// `resolveHomedir`: `$HOME` on the daemon's host.
pub async fn resolve_homedir(exec: &dyn ExecApi) -> Result<String, RegistryError> {
    let output = collect_output(exec.start(ExecRequest {
        cwd: "/".to_owned(),
        command: "sh".to_owned(),
        args: vec!["-c".to_owned(), "echo $HOME".to_owned()],
        env: HashMap::new(),
        timeout: ExecTimeout::After(DEFAULT_EXEC_TIMEOUT),
    }))
    .await?;
    let home = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if output.exit_code == 0 && !home.is_empty() {
        Ok(home)
    } else {
        Err(RegistryError::HomeDirUnresolved)
    }
}

async fn registry_dir(exec: &dyn ExecApi) -> Result<String, RegistryError> {
    Ok(format!(
        "{}/{REGISTRY_DIR_REL}",
        resolve_homedir(exec).await?
    ))
}

/// The registry's entries and the guard for writing over what was read.
struct Snapshot {
    entries: Vec<WorktreeRegistryEntry>,
    guard: WriteGuard,
}

async fn read_registry_file(fs: &dyn RegistryFs, dir: &str) -> Result<Snapshot, RegistryError> {
    let content = match fs.read_file(dir, REGISTRY_FILE, MAX_READ_FILE_BYTES).await {
        Ok(ReadOutcome::Read(file)) => file.content,
        Ok(ReadOutcome::TooLarge { size }) => return Err(RegistryError::TooLarge { size }),
        Err(FsError::Daemon(message)) if is_missing_file_error(&message) => {
            return Ok(Snapshot {
                entries: Vec::new(),
                guard: WriteGuard::MustNotExist,
            });
        }
        Err(error) => return Err(RegistryError::Read(error)),
    };
    let parsed: Value = serde_json::from_slice(&content)
        .map_err(|error| RegistryError::InvalidJson(error.to_string()))?;
    let Value::Object(mut file) = parsed else {
        return Err(RegistryError::InvalidShape);
    };
    let version = file.remove("version");
    let entries = match (version, file.remove("entries")) {
        (Some(version), Some(entries @ Value::Array(_))) if version == 1 => entries,
        (version, _) => {
            return Err(RegistryError::UnsupportedVersion(js_string(
                version.as_ref(),
            )));
        }
    };
    let entries = serde_json::from_value(entries)
        .map_err(|error| RegistryError::InvalidEntries(error.to_string()))?;
    Ok(Snapshot {
        entries,
        guard: WriteGuard::Matches(sha256_hex(&content)),
    })
}

/// `String(value)` for a JSON value (`undefined` when absent).
fn js_string(value: Option<&Value>) -> String {
    match value {
        None => "undefined".to_owned(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

pub async fn load_registry(
    fs: &dyn RegistryFs,
    exec: &dyn ExecApi,
) -> Result<Vec<WorktreeRegistryEntry>, RegistryError> {
    let dir = registry_dir(exec).await?;
    Ok(read_registry_file(fs, &dir).await?.entries)
}

/// What a mutation decided.
enum Mutation {
    Write(Vec<WorktreeRegistryEntry>),
    Unchanged,
}

/// Read, apply `mutate`, write over what was read; on a conflict, start over from a fresh read
/// so no writer's change is lost.
async fn mutate_registry(
    fs: &dyn RegistryFs,
    exec: &dyn ExecApi,
    mutate: impl Fn(Vec<WorktreeRegistryEntry>) -> Mutation,
) -> Result<(), RegistryError> {
    let dir = registry_dir(exec).await?;
    for _attempt in 0..MAX_CAS_RETRIES {
        let Snapshot { entries, guard } = read_registry_file(fs, &dir).await?;
        let Mutation::Write(entries) = mutate(entries) else {
            return Ok(());
        };
        let content = serde_json::to_string_pretty(&RegistryFile {
            version: 1,
            entries: &entries,
        })
        .expect("registry entries always serialize");
        match fs
            .write_file(&dir, REGISTRY_FILE, content.into_bytes(), guard)
            .await
        {
            Ok(WriteOutcome::Written { .. }) => return Ok(()),
            Ok(WriteOutcome::Conflict) => continue,
            Err(error) => return Err(RegistryError::Write(error)),
        }
    }
    Err(RegistryError::Contended)
}

/// Record `entry` (replacing the one at its path) as used now.
pub async fn upsert_registry_entry(
    fs: &dyn RegistryFs,
    exec: &dyn ExecApi,
    entry: &RegistryEntryInput,
) -> Result<(), RegistryError> {
    mutate_registry(fs, exec, |mut entries| {
        let stamped = WorktreeRegistryEntry {
            path: entry.path.clone(),
            branch: entry.branch.clone(),
            display_name: entry.display_name.clone(),
            description: entry.description.clone(),
            last_used_at: now_millis(),
            extra: Map::new(),
        };
        match entries
            .iter_mut()
            .find(|existing| existing.path == entry.path)
        {
            Some(existing) => *existing = stamped,
            None => entries.push(stamped),
        }
        Mutation::Write(entries)
    })
    .await
}

/// Forget the entry at `path`; nothing is written when there is none.
pub async fn remove_registry_entry(
    fs: &dyn RegistryFs,
    exec: &dyn ExecApi,
    path: &str,
) -> Result<(), RegistryError> {
    mutate_registry(fs, exec, |entries| {
        let before = entries.len();
        let filtered: Vec<_> = entries
            .into_iter()
            .filter(|entry| entry.path != path)
            .collect();
        if filtered.len() == before {
            Mutation::Unchanged
        } else {
            Mutation::Write(filtered)
        }
    })
    .await
}

/// The entry for a workspace: its branch, and its display name and description from metadata.
pub fn build_entry_from_workspace(path: &str, workspace: &WorkspaceFile) -> RegistryEntryInput {
    RegistryEntryInput {
        path: path.to_owned(),
        branch: workspace.git_branch.clone().unwrap_or_default(),
        display_name: workspace.metadata.get("displayName").cloned(),
        description: workspace.metadata.get("description").cloned(),
    }
}

/// `createWorktreeRegistryApi`: the registry operations, one at a time. Read-modify-write
/// of one shared file is not atomic, so this process never runs two at once (the CAS covers
/// other processes). Cheap to clone; clones share the queue.
#[derive(Clone)]
pub struct WorktreeRegistry {
    fs: Arc<dyn RegistryFs>,
    exec: Arc<dyn ExecApi>,
    queue: Arc<futures::lock::Mutex<()>>,
}

impl WorktreeRegistry {
    pub fn new(fs: Arc<dyn RegistryFs>, exec: Arc<dyn ExecApi>) -> Self {
        Self {
            fs,
            exec,
            queue: Arc::new(futures::lock::Mutex::new(())),
        }
    }

    pub async fn list(&self) -> Result<Vec<WorktreeRegistryEntry>, RegistryError> {
        let _turn = self.queue.lock().await;
        load_registry(self.fs.as_ref(), self.exec.as_ref()).await
    }

    pub async fn upsert(&self, entry: &RegistryEntryInput) -> Result<(), RegistryError> {
        let _turn = self.queue.lock().await;
        upsert_registry_entry(self.fs.as_ref(), self.exec.as_ref(), entry).await
    }

    pub async fn remove(&self, path: &str) -> Result<(), RegistryError> {
        let _turn = self.queue.lock().await;
        remove_registry_entry(self.fs.as_ref(), self.exec.as_ref(), path).await
    }
}

fn now_millis() -> i64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970");
    i64::try_from(elapsed.as_millis()).expect("milliseconds since 1970 fit in i64")
}

#[cfg(test)]
#[path = "worktree_registry_tests.rs"]
mod tests;
