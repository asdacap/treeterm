//! Files marked viewed in a review, stored as a JSON object (path → the diff stats the file
//! had when marked) under the `reviewViewedFiles` metadata key. A mark is dropped once the
//! file's stats change.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use serde_json::{Map, Value, json};

use super::{MetadataError, WorkspaceMetadata};
use super::js_json::stringify;
use crate::git::{DiffFile, FileChangeStats, UncommittedFile};

pub const REVIEW_VIEWED_FILES_KEY: &str = "reviewViewedFiles";

/// The stats a file had when it was marked viewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewedFileStats {
    /// `{ kind: 'text', additions, deletions }`, or the untagged `{ additions, deletions }`
    /// older versions wrote.
    Text {
        additions: u64,
        deletions: u64,
    },
    Binary {
        byte_change: i64,
    },
    /// Marked while counts were not computed.
    Unknown,
}

impl From<FileChangeStats> for ViewedFileStats {
    fn from(stats: FileChangeStats) -> Self {
        match stats {
            FileChangeStats::Text {
                additions,
                deletions,
            } => ViewedFileStats::Text {
                additions,
                deletions,
            },
            FileChangeStats::Binary { byte_change } => ViewedFileStats::Binary { byte_change },
            FileChangeStats::Unknown => ViewedFileStats::Unknown,
        }
    }
}

/// A file of the current diff (`ViewedFileEntry`): what keys and invalidates a mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewedFile<'a> {
    pub path: &'a str,
    pub stats: FileChangeStats,
}

impl<'a> From<&'a DiffFile> for ViewedFile<'a> {
    fn from(file: &'a DiffFile) -> Self {
        ViewedFile {
            path: &file.path,
            stats: file.stats,
        }
    }
}

impl<'a> From<&'a UncommittedFile> for ViewedFile<'a> {
    fn from(file: &'a UncommittedFile) -> Self {
        ViewedFile {
            path: &file.entry.path,
            stats: file.stats,
        }
    }
}

/// Whether a mark still holds for the file's current stats.
fn stats_match(current: FileChangeStats, marked: ViewedFileStats) -> bool {
    match current {
        // No counts to compare against: keep the mark rather than dropping it on every refresh.
        FileChangeStats::Unknown => true,
        FileChangeStats::Binary { byte_change } => {
            marked == ViewedFileStats::Binary { byte_change }
        }
        FileChangeStats::Text {
            additions,
            deletions,
        } => {
            marked
                == ViewedFileStats::Text {
                    additions,
                    deletions,
                }
        }
    }
}

/// The stored form of a new mark (always tagged).
fn stats_json(stats: FileChangeStats) -> Value {
    match stats {
        FileChangeStats::Text {
            additions,
            deletions,
        } => json!({ "kind": "text", "additions": additions, "deletions": deletions }),
        FileChangeStats::Binary { byte_change } => {
            json!({ "kind": "binary", "byteChange": byte_change })
        }
        FileChangeStats::Unknown => json!({ "kind": "unknown" }),
    }
}

fn parse_stats(path: &str, value: &Value) -> Result<ViewedFileStats, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{path}: not an object"))?;
    let unsigned = |key: &str| -> Result<u64, String> {
        object
            .get(key)
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("{path}: `{key}` is not a count"))
    };
    match object.get("kind").map(|kind| kind.as_str()) {
        None | Some(Some("text")) => Ok(ViewedFileStats::Text {
            additions: unsigned("additions")?,
            deletions: unsigned("deletions")?,
        }),
        Some(Some("binary")) => Ok(ViewedFileStats::Binary {
            byte_change: object
                .get("byteChange")
                .and_then(Value::as_i64)
                .ok_or_else(|| format!("{path}: `byteChange` is not a number"))?,
        }),
        Some(Some("unknown")) => Ok(ViewedFileStats::Unknown),
        Some(other) => Err(format!("{path}: unknown kind {other:?}")),
    }
}

/// The parsed `reviewViewedFiles` value, in stored order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewedFiles {
    /// The parsed stats and the JSON they are stored as (kept so a rewrite preserves it).
    entries: IndexMap<String, (ViewedFileStats, Value)>,
}

impl ViewedFiles {
    /// Parse the stored value; a missing or empty value is nothing viewed (`parseViewedFiles`).
    pub fn parse(value: Option<&str>) -> Result<Self, MetadataError> {
        let invalid = |reason: String| MetadataError {
            key: REVIEW_VIEWED_FILES_KEY,
            reason,
        };
        let json = match value {
            None | Some("") => return Ok(Self::default()),
            Some(json) => json,
        };
        let stored: Map<String, Value> =
            serde_json::from_str(json).map_err(|error| invalid(error.to_string()))?;
        let entries = stored
            .into_iter()
            .map(|(path, raw)| {
                let stats = parse_stats(&path, &raw).map_err(invalid)?;
                Ok((path, (stats, raw)))
            })
            .collect::<Result<_, MetadataError>>()?;
        Ok(Self { entries })
    }

    pub fn get(&self, path: &str) -> Option<ViewedFileStats> {
        self.entries.get(path).map(|(stats, _)| *stats)
    }

    pub fn stats(&self) -> IndexMap<String, ViewedFileStats> {
        self.entries
            .iter()
            .map(|(path, (stats, _))| (path.clone(), *stats))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The value to store (`JSON.stringify(viewed)`).
    pub fn to_json(&self) -> String {
        stringify(&Value::Object(
            self.entries
                .iter()
                .map(|(path, (_, raw))| (path.clone(), raw.clone()))
                .collect(),
        ))
    }

    fn insert(&mut self, file: ViewedFile<'_>) {
        self.entries.insert(
            file.path.to_owned(),
            (file.stats.into(), stats_json(file.stats)),
        );
    }

    fn remove(&mut self, path: &str) -> bool {
        self.entries.shift_remove(path).is_some()
    }
}

/// `createReviewViewedFilesStore`: every operation reads the latest metadata and writes back
/// only when something changed (except toggling, which always changes something).
pub struct ViewedFilesStore<D> {
    deps: D,
}

impl<D: WorkspaceMetadata> ViewedFilesStore<D> {
    pub fn new(deps: D) -> Self {
        Self { deps }
    }

    pub fn deps(&self) -> &D {
        &self.deps
    }

    fn read(&self) -> Result<ViewedFiles, MetadataError> {
        ViewedFiles::parse(self.deps.metadata_value(REVIEW_VIEWED_FILES_KEY).as_deref())
    }

    fn persist(&self, viewed: &ViewedFiles, reason: &str) {
        self.deps
            .update_metadata(REVIEW_VIEWED_FILES_KEY, viewed.to_json(), reason);
    }

    pub fn get_viewed_files(&self) -> Result<IndexMap<String, ViewedFileStats>, MetadataError> {
        Ok(self.read()?.stats())
    }

    pub fn toggle_viewed_file(&self, file: ViewedFile<'_>) -> Result<(), MetadataError> {
        let mut viewed = self.read()?;
        if !viewed.remove(file.path) {
            viewed.insert(file);
        }
        self.persist(&viewed, "toggleViewedFile");
        Ok(())
    }

    /// Mark every file not yet viewed; a viewed file keeps the stats it was marked with.
    pub fn mark_files_viewed(&self, files: &[ViewedFile<'_>]) -> Result<(), MetadataError> {
        let mut viewed = self.read()?;
        let mut changed = false;
        for file in files {
            if viewed.get(file.path).is_none() {
                viewed.insert(*file);
                changed = true;
            }
        }
        if changed {
            self.persist(&viewed, "markFilesViewed");
        }
        Ok(())
    }

    pub fn unmark_files_viewed(&self, paths: &[&str]) -> Result<(), MetadataError> {
        let mut viewed = self.read()?;
        let drop: HashSet<&str> = paths.iter().copied().collect();
        let mut changed = false;
        for path in drop {
            changed |= viewed.remove(path);
        }
        if changed {
            self.persist(&viewed, "unmarkFilesViewed");
        }
        Ok(())
    }

    /// Drop marks whose file's stats changed since it was marked. Marks for files absent
    /// from `files` stay: they belong to another view mode (committed, uncommitted, a commit).
    pub fn reconcile_viewed_files(&self, files: &[ViewedFile<'_>]) -> Result<(), MetadataError> {
        let mut viewed = self.read()?;
        let current: HashMap<&str, FileChangeStats> =
            files.iter().map(|file| (file.path, file.stats)).collect();
        let before = viewed.len();
        viewed.entries.retain(|path, (marked, _)| {
            current
                .get(path.as_str())
                .is_none_or(|stats| stats_match(*stats, *marked))
        });
        if viewed.len() != before {
            self.persist(&viewed, "reconcileViewedFiles");
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "viewed_files_tests.rs"]
mod tests;
