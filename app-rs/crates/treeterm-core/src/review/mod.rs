//! Code review state kept in workspace metadata, shared with Electron (ports of
//! `createReviewCommentStore.ts`, `createReviewViewedFilesStore.ts` and `reviewPrompt.ts`).
//!
//! Both stores keep a JSON string under one metadata key. Values are rewritten the way
//! Electron's `JSON.stringify` would write them: entries this app does not touch keep their
//! keys, key order and unknown fields, so the two apps never disagree about the bytes of an
//! unchanged list.
//!
//! Unlike Electron, a value that is not valid JSON of the expected shape is an error
//! ([`MetadataError`]) instead of an empty list: an empty list hides the damage, and the next
//! write would silently replace whatever was there.

mod comments;
mod js_json;
mod prompt;
mod viewed_files;

pub use comments::{
    CommitAnchor, Freshness, LineAnchor, NewReviewComment, REVIEW_COMMENTS_KEY, Resolution,
    ReviewComment, ReviewCommentDeps, ReviewCommentError, ReviewCommentStore, ReviewComments, Side,
};
pub use prompt::{build_prompt_for_comments, generate_review_prompt};
pub use viewed_files::{
    REVIEW_VIEWED_FILES_KEY, ViewedFile, ViewedFileStats, ViewedFiles, ViewedFilesStore,
};

/// A workspace's metadata, where both stores keep their values. Reads must see the latest
/// value (including this app's own writes); a write replaces one key and syncs the workspace.
pub trait WorkspaceMetadata {
    fn metadata_value(&self, key: &str) -> Option<String>;
    /// `reason` names the operation, for logs.
    fn update_metadata(&self, key: &str, value: String, reason: &str);
}

/// A metadata value that does not hold what its key is for.
#[derive(Debug, Clone, thiserror::Error)]
#[error("workspace metadata `{key}` is invalid: {reason}")]
pub struct MetadataError {
    pub key: &'static str,
    pub reason: String,
}
