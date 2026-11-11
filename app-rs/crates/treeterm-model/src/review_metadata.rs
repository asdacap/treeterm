//! The review stores of [`treeterm_core::review`] over a live workspace's metadata.
//!
//! The stores read the latest metadata before every write, and their reads must see this
//! app's own writes: both go straight to the [`WorkspaceStore`] entity, whose
//! `update_metadata` persists through the session like any other body change.
//!
//! [`Now`] serves operations that finish inside one update; [`Later`] serves those that
//! await `git blame` first and so must reach the workspace from a task.

use std::cell::RefCell;

use futures::FutureExt as _;
use futures::future::LocalBoxFuture;
use gpui_kit::{App, AsyncApp, Entity, WeakEntity};
use treeterm_core::git::{GitApi, GitError};
use treeterm_core::review::{ReviewCommentDeps, WorkspaceMetadata};

use crate::workspace_store::WorkspaceStore;

/// Where line commits come from: `git blame` of the worktree's file.
#[derive(Clone)]
pub(crate) struct Blame {
    pub git: GitApi,
    /// The worktree; comment paths are relative to it.
    pub path: String,
}

impl Blame {
    fn line_commits(&self, file_path: &str) -> LocalBoxFuture<'static, Result<Vec<String>, GitError>> {
        let (git, path, file_path) = (self.git.clone(), self.path.clone(), file_path.to_owned());
        async move { git.get_line_commits(&path, &file_path).await }.boxed_local()
    }
}

/// The workspace's metadata, reached synchronously from inside an update.
pub(crate) struct Now<'a> {
    workspace: Entity<WorkspaceStore>,
    cx: RefCell<&'a mut App>,
    blame: Blame,
}

impl<'a> Now<'a> {
    pub fn new(workspace: Entity<WorkspaceStore>, cx: &'a mut App, blame: Blame) -> Self {
        Self { workspace, cx: RefCell::new(cx), blame }
    }
}

impl WorkspaceMetadata for Now<'_> {
    fn metadata_value(&self, key: &str) -> Option<String> {
        self.workspace.read(&self.cx.borrow()).file().metadata.get(key).cloned()
    }

    fn update_metadata(&self, key: &str, value: String, reason: &str) {
        tracing::debug!(%key, %reason, "review metadata");
        let mut cx = self.cx.borrow_mut();
        self.workspace.update(&mut **cx, |workspace, cx| workspace.update_metadata(key, &value, cx));
    }
}

impl ReviewCommentDeps for Now<'_> {
    fn get_line_commits(&self, file_path: &str) -> LocalBoxFuture<'static, Result<Vec<String>, GitError>> {
        self.blame.line_commits(file_path)
    }
}

/// The workspace's metadata, reached from a task (across awaits).
pub(crate) struct Later {
    workspace: WeakEntity<WorkspaceStore>,
    cx: AsyncApp,
    blame: Blame,
}

impl Later {
    pub fn new(workspace: WeakEntity<WorkspaceStore>, cx: AsyncApp, blame: Blame) -> Self {
        Self { workspace, cx, blame }
    }
}

impl WorkspaceMetadata for Later {
    fn metadata_value(&self, key: &str) -> Option<String> {
        match self.workspace.read_with(&self.cx, |workspace, _| workspace.file().metadata.get(key).cloned()) {
            Ok(value) => value,
            // The workspace left the session: the review tab is going with it.
            Err(error) => {
                tracing::debug!(%key, %error, "review metadata read after the workspace closed");
                None
            }
        }
    }

    fn update_metadata(&self, key: &str, value: String, reason: &str) {
        tracing::debug!(%key, %reason, "review metadata");
        let mut cx = self.cx.clone();
        if self
            .workspace
            .update(&mut cx, |workspace, cx| workspace.update_metadata(key, &value, cx))
            .is_err()
        {
            tracing::warn!(%key, %reason, "workspace closed before review metadata was written");
        }
    }
}

impl ReviewCommentDeps for Later {
    fn get_line_commits(&self, file_path: &str) -> LocalBoxFuture<'static, Result<Vec<String>, GitError>> {
        self.blame.line_commits(file_path)
    }
}
