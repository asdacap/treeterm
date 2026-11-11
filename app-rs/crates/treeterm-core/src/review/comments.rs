//! Review comments, stored as a JSON array under the `reviewComments` metadata key.

use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use futures::future::{LocalBoxFuture, try_join_all};
use indexmap::IndexSet;
use serde::Deserialize;
use serde_json::{Map, Value};

use super::{MetadataError, WorkspaceMetadata};
use super::js_json::stringify;
use crate::git::GitError;

pub const REVIEW_COMMENTS_KEY: &str = "reviewComments";

/// Which side of a diff a comment is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Original,
    Modified,
}

impl Side {
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Original => "original",
            Side::Modified => "modified",
        }
    }
}

/// `commitHash`: `HEAD` when the comment was made.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CommitAnchor {
    /// The file is not in a git repository (`null`).
    NotInRepo,
    Head(String),
}

/// `lineCommitHash`: the commit that last changed the commented line when the comment was
/// made (`git blame`; all zeros while uncommitted). The comment goes outdated only when the
/// line's blame moves.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LineAnchor {
    /// Not tracked (`null`): original-side comments, files outside git, or comments saved
    /// before line tracking. These go outdated when `HEAD` moves past their commit.
    Untracked,
    Blamed(String),
}

/// `isOutdated`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Freshness {
    Current,
    Outdated,
}

/// `addressed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Resolution {
    Open,
    Addressed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewComment {
    pub id: String,
    pub file_path: String,
    /// 1-based.
    pub line_number: u32,
    pub text: String,
    pub commit: CommitAnchor,
    pub line: LineAnchor,
    /// Milliseconds since the epoch.
    pub created_at: i64,
    pub freshness: Freshness,
    pub resolution: Resolution,
    pub side: Side,
}

/// What the UI supplies for a new comment; the store blames the line and stamps the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewReviewComment {
    pub file_path: String,
    pub line_number: u32,
    pub text: String,
    pub commit: CommitAnchor,
    pub side: Side,
}

/// The stored shape. `lineCommitHash` is missing on comments saved before line tracking and
/// `addressed` on those saved before it existed; both read as their "not set" value.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireComment {
    id: String,
    file_path: String,
    line_number: u32,
    text: String,
    commit_hash: Option<String>,
    #[serde(default)]
    line_commit_hash: Option<String>,
    created_at: i64,
    is_outdated: bool,
    #[serde(default)]
    addressed: bool,
    side: Side,
}

impl From<WireComment> for ReviewComment {
    fn from(wire: WireComment) -> Self {
        ReviewComment {
            id: wire.id,
            file_path: wire.file_path,
            line_number: wire.line_number,
            text: wire.text,
            commit: wire
                .commit_hash
                .map_or(CommitAnchor::NotInRepo, CommitAnchor::Head),
            line: wire
                .line_commit_hash
                .map_or(LineAnchor::Untracked, LineAnchor::Blamed),
            created_at: wire.created_at,
            freshness: if wire.is_outdated {
                Freshness::Outdated
            } else {
                Freshness::Current
            },
            resolution: if wire.addressed {
                Resolution::Addressed
            } else {
                Resolution::Open
            },
            side: wire.side,
        }
    }
}

/// One comment and the JSON object it is stored as (kept so a rewrite preserves it).
#[derive(Debug, Clone, PartialEq)]
struct Stored {
    comment: ReviewComment,
    raw: Map<String, Value>,
}

impl Stored {
    fn set_resolution(&mut self, resolution: Resolution) {
        self.comment.resolution = resolution;
        self.raw.insert(
            "addressed".to_owned(),
            Value::Bool(resolution == Resolution::Addressed),
        );
    }

    fn set_freshness(&mut self, freshness: Freshness) {
        self.comment.freshness = freshness;
        self.raw.insert(
            "isOutdated".to_owned(),
            Value::Bool(freshness == Freshness::Outdated),
        );
    }
}

/// The parsed `reviewComments` value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReviewComments {
    items: Vec<Stored>,
}

impl ReviewComments {
    /// Parse the stored value; a missing or empty value is no comments (`parseReviewComments`).
    pub fn parse(value: Option<&str>) -> Result<Self, MetadataError> {
        let invalid = |reason: String| MetadataError {
            key: REVIEW_COMMENTS_KEY,
            reason,
        };
        let json = match value {
            None | Some("") => return Ok(Self::default()),
            Some(json) => json,
        };
        let entries: Vec<Map<String, Value>> =
            serde_json::from_str(json).map_err(|error| invalid(error.to_string()))?;
        let items = entries
            .into_iter()
            .map(|mut raw| {
                let wire: WireComment = serde_json::from_value(Value::Object(raw.clone()))
                    .map_err(|error| invalid(error.to_string()))?;
                // Electron fills this in on read, so it writes it back.
                raw.entry("lineCommitHash").or_insert(Value::Null);
                Ok(Stored {
                    comment: wire.into(),
                    raw,
                })
            })
            .collect::<Result<_, MetadataError>>()?;
        Ok(Self { items })
    }

    pub fn comments(&self) -> Vec<ReviewComment> {
        self.items
            .iter()
            .map(|stored| stored.comment.clone())
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The value to store (`JSON.stringify(comments)`).
    pub fn to_json(&self) -> String {
        stringify(&Value::Array(
            self.items
                .iter()
                .map(|stored| Value::Object(stored.raw.clone()))
                .collect(),
        ))
    }

    /// Append a comment, keyed and laid out as Electron's `addReviewComment` writes it.
    pub fn push(&mut self, new: NewReviewComment, line: LineAnchor, id: String, created_at: i64) {
        let mut raw = Map::new();
        raw.insert("filePath".to_owned(), Value::from(new.file_path.clone()));
        raw.insert("lineNumber".to_owned(), Value::from(new.line_number));
        raw.insert("text".to_owned(), Value::from(new.text.clone()));
        raw.insert(
            "commitHash".to_owned(),
            match &new.commit {
                CommitAnchor::NotInRepo => Value::Null,
                CommitAnchor::Head(hash) => Value::from(hash.clone()),
            },
        );
        raw.insert("isOutdated".to_owned(), Value::Bool(false));
        raw.insert("addressed".to_owned(), Value::Bool(false));
        raw.insert("side".to_owned(), Value::from(new.side.as_str()));
        raw.insert(
            "lineCommitHash".to_owned(),
            match &line {
                LineAnchor::Untracked => Value::Null,
                LineAnchor::Blamed(hash) => Value::from(hash.clone()),
            },
        );
        raw.insert("id".to_owned(), Value::from(id.clone()));
        raw.insert("createdAt".to_owned(), Value::from(created_at));
        self.items.push(Stored {
            comment: ReviewComment {
                id,
                file_path: new.file_path,
                line_number: new.line_number,
                text: new.text,
                commit: new.commit,
                line,
                created_at,
                freshness: Freshness::Current,
                resolution: Resolution::Open,
                side: new.side,
            },
            raw,
        });
    }

    pub fn delete(&mut self, comment_id: &str) {
        self.items.retain(|stored| stored.comment.id != comment_id);
    }

    pub fn toggle_addressed(&mut self, comment_id: &str) {
        for stored in self
            .items
            .iter_mut()
            .filter(|stored| stored.comment.id == comment_id)
        {
            let toggled = match stored.comment.resolution {
                Resolution::Open => Resolution::Addressed,
                Resolution::Addressed => Resolution::Open,
            };
            stored.set_resolution(toggled);
        }
    }

    pub fn mark_addressed(&mut self, ids: &HashSet<&str>) {
        for stored in &mut self.items {
            if ids.contains(stored.comment.id.as_str())
                && stored.comment.resolution == Resolution::Open
            {
                stored.set_resolution(Resolution::Addressed);
            }
        }
    }

    /// Set each listed comment's freshness; comments not listed (added since) are left alone.
    fn apply_freshness(&mut self, freshness_by_id: &HashMap<String, Freshness>) {
        for stored in &mut self.items {
            match freshness_by_id.get(&stored.comment.id) {
                Some(freshness) if *freshness != stored.comment.freshness => {
                    stored.set_freshness(*freshness)
                }
                Some(_) | None => {}
            }
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ReviewCommentError {
    #[error(transparent)]
    Metadata(#[from] MetadataError),
    #[error("Failed to blame {file_path}: {source}")]
    Blame { file_path: String, source: GitError },
}

/// What the store reads and writes besides the metadata: line commits from `git blame` of
/// the working-tree file.
pub trait ReviewCommentDeps: WorkspaceMetadata {
    /// See [`crate::git::GitApi::get_line_commits`].
    fn get_line_commits(
        &self,
        file_path: &str,
    ) -> LocalBoxFuture<'static, Result<Vec<String>, GitError>>;
}

/// `createReviewCommentStore`: every operation reads the latest metadata and writes the
/// whole list back.
pub struct ReviewCommentStore<D> {
    deps: D,
}

impl<D: ReviewCommentDeps> ReviewCommentStore<D> {
    pub fn new(deps: D) -> Self {
        Self { deps }
    }

    pub fn deps(&self) -> &D {
        &self.deps
    }

    fn read(&self) -> Result<ReviewComments, MetadataError> {
        ReviewComments::parse(self.deps.metadata_value(REVIEW_COMMENTS_KEY).as_deref())
    }

    fn write(&self, comments: &ReviewComments, reason: &str) {
        self.deps
            .update_metadata(REVIEW_COMMENTS_KEY, comments.to_json(), reason);
    }

    async fn line_commits(&self, file_path: &str) -> Result<Vec<String>, ReviewCommentError> {
        self.deps
            .get_line_commits(file_path)
            .await
            .map_err(|source| ReviewCommentError::Blame {
                file_path: file_path.to_owned(),
                source,
            })
    }

    pub fn get_review_comments(&self) -> Result<Vec<ReviewComment>, ReviewCommentError> {
        Ok(self.read()?.comments())
    }

    /// Only the modified side is the working-tree file that blame describes, and only inside
    /// a repository; other comments are untracked.
    pub async fn add_review_comment(
        &self,
        comment: NewReviewComment,
    ) -> Result<(), ReviewCommentError> {
        let tracks_line =
            comment.side == Side::Modified && comment.commit != CommitAnchor::NotInRepo;
        let line = if tracks_line {
            let blame = self.line_commits(&comment.file_path).await?;
            line_commit(&blame, comment.line_number).map_or(LineAnchor::Untracked, |hash| {
                LineAnchor::Blamed(hash.to_owned())
            })
        } else {
            LineAnchor::Untracked
        };
        let mut comments = self.read()?;
        comments.push(
            comment,
            line,
            uuid::Uuid::new_v4().to_string(),
            now_millis(),
        );
        self.write(&comments, "addReviewComment");
        Ok(())
    }

    pub fn delete_review_comment(&self, comment_id: &str) -> Result<(), ReviewCommentError> {
        let mut comments = self.read()?;
        comments.delete(comment_id);
        self.write(&comments, "deleteReviewComment");
        Ok(())
    }

    pub fn toggle_review_comment_addressed(
        &self,
        comment_id: &str,
    ) -> Result<(), ReviewCommentError> {
        let mut comments = self.read()?;
        comments.toggle_addressed(comment_id);
        self.write(&comments, "toggleReviewCommentAddressed");
        Ok(())
    }

    /// Re-blame every file with a tracked comment (once per file) and mark comments outdated
    /// whose line moved; untracked comments compare their commit with `current_commit_hash`.
    pub async fn update_outdated_review_comments(
        &self,
        current_commit_hash: &str,
    ) -> Result<(), ReviewCommentError> {
        let comments = self.read()?.comments();
        if comments.is_empty() {
            return Ok(());
        }

        let tracked_paths: IndexSet<&str> = comments
            .iter()
            .filter(|comment| matches!(comment.line, LineAnchor::Blamed(_)))
            .map(|comment| comment.file_path.as_str())
            .collect();
        let blames: HashMap<&str, Vec<String>> =
            try_join_all(tracked_paths.into_iter().map(|file_path| async move {
                Ok::<_, ReviewCommentError>((file_path, self.line_commits(file_path).await?))
            }))
            .await?
            .into_iter()
            .collect();

        let freshness_by_id: HashMap<String, Freshness> = comments
            .iter()
            .map(|comment| {
                let outdated = match (&comment.line, &comment.commit) {
                    (LineAnchor::Blamed(hash), _) => {
                        let blame = blames
                            .get(comment.file_path.as_str())
                            .map(Vec::as_slice)
                            .unwrap_or_default();
                        line_commit(blame, comment.line_number) != Some(hash.as_str())
                    }
                    (LineAnchor::Untracked, CommitAnchor::Head(hash)) => {
                        hash != current_commit_hash
                    }
                    (LineAnchor::Untracked, CommitAnchor::NotInRepo) => false,
                };
                let freshness = if outdated {
                    Freshness::Outdated
                } else {
                    Freshness::Current
                };
                (comment.id.clone(), freshness)
            })
            .collect();

        // Comments may have been added or removed while blame ran: apply to the latest list.
        let mut latest = self.read()?;
        latest.apply_freshness(&freshness_by_id);
        self.write(&latest, "updateOutdatedReviewComments");
        Ok(())
    }

    pub fn clear_review_comments(&self) {
        self.write(&ReviewComments::default(), "clearReviewComments");
    }

    pub fn mark_review_comments_addressed(&self, ids: &[String]) -> Result<(), ReviewCommentError> {
        if ids.is_empty() {
            return Ok(());
        }
        let ids: HashSet<&str> = ids.iter().map(String::as_str).collect();
        let mut comments = self.read()?;
        comments.mark_addressed(&ids);
        self.write(&comments, "markReviewCommentsAddressed");
        Ok(())
    }
}

/// The blamed commit of 1-based `line_number`, if the file has that line.
fn line_commit(blame: &[String], line_number: u32) -> Option<&str> {
    let index = usize::try_from(line_number).ok()?.checked_sub(1)?;
    blame.get(index).map(String::as_str)
}

fn now_millis() -> i64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970");
    i64::try_from(elapsed.as_millis()).expect("milliseconds since 1970 fit in i64")
}

#[cfg(test)]
#[path = "comments_tests.rs"]
mod tests;
