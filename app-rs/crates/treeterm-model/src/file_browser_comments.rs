//! Review comments on the Files tab's file (the comment half of `FilesystemBrowser.tsx`):
//! a click on a line number starts one, `HEAD` (read when the tab opens, each time it is
//! shown again and when the workspace's `isGitRepo` changes, marking comments outdated whose
//! line moved since; none outside git or before the first commit) anchors it, and the
//! file's comments show with it.
//!
//! New comments get the workspace-relative path the Review tab writes, so they show in both
//! tabs and can be pushed to GitHub. Electron's Files tab writes the absolute path; those
//! show here too.

use gpui_kit::{App, Context};
use treeterm_core::review::{
    CommitAnchor, MetadataError, NewReviewComment, REVIEW_COMMENTS_KEY, ReviewComment, ReviewCommentStore,
    ReviewComments, Side,
};
use treeterm_core::search::to_workspace_relative;

use super::{FileBrowser, FileView};
use crate::review::{GitOp, Notice};
use crate::review_metadata::{Blame, Later, Now};

/// What new comments are anchored to (`currentCommitHash`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anchor {
    /// The workspace is not a git repository: comments have no commit (`null`).
    NotInRepo,
    Reading,
    Head(String),
    /// `HEAD` could not be read (a repository with no commits yet): comments have no commit
    /// (`null`), as outside git.
    Failed(String),
}

/// Whether the tab is on screen (Electron's `FilesystemBrowser` mounted), as last told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Visibility {
    Hidden,
    Shown,
}

/// The comment being written under a line of the shown file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineDraft {
    /// The file, as [`FileBrowser::selected`] names it.
    pub path: String,
    /// 1-based.
    pub line_number: u32,
}

/// The comment state of the tab.
pub(super) struct Comments {
    pub anchor: Anchor,
    pub visibility: Visibility,
    pub draft: Option<LineDraft>,
    pub adding: GitOp,
    /// A failed delete or outdated check, until the next comment action succeeds.
    pub notice: Notice,
    pub _anchor: Option<gpui_kit::Task<()>>,
    pub _add: Option<gpui_kit::Task<()>>,
}

impl Comments {
    /// Its anchor is set as the tab is made ([`FileBrowser::refresh_anchor`]), and a view
    /// shows it later.
    pub fn new() -> Self {
        Self {
            anchor: Anchor::Reading,
            visibility: Visibility::Hidden,
            draft: None,
            adding: GitOp::Idle,
            notice: Notice::Clear,
            _anchor: None,
            _add: None,
        }
    }
}

impl FileBrowser {
    fn blame(&self) -> Blame {
        Blame { git: self.git.clone(), path: self.root.clone() }
    }

    pub fn anchor(&self) -> &Anchor {
        &self.comments.anchor
    }

    pub fn comment_draft(&self) -> Option<&LineDraft> {
        self.comments.draft.as_ref()
    }

    pub fn adding_comment(&self) -> &GitOp {
        &self.comments.adding
    }

    pub fn comment_notice(&self) -> &Notice {
        &self.comments.notice
    }

    /// Read `HEAD` and mark comments outdated whose line moved since, if the workspace is a
    /// git repository (`in_repo`, its `isGitRepo`); elsewhere comments have no commit and
    /// are not tracked. An "Invalid tab" shows no comments, so it reads nothing.
    pub(super) fn refresh_anchor(&mut self, in_repo: bool, cx: &mut Context<Self>) {
        if self.is_invalid() {
            return;
        }
        cx.notify();
        if !in_repo {
            self.comments.anchor = Anchor::NotInRepo;
            self.comments._anchor = None;
            return;
        }
        let (git, root, workspace, blame) = (self.git.clone(), self.root.clone(), self.workspace.clone(), self.blame());
        self.comments.anchor = Anchor::Reading;
        self.comments._anchor = Some(cx.spawn(async move |this, cx| {
            let head = git.get_head_commit_hash(&root).await;
            let outdated = match &head {
                Ok(hash) => ReviewCommentStore::new(Later::new(workspace, cx.clone(), blame))
                    .update_outdated_review_comments(hash)
                    .await
                    .map_err(|error| format!("Failed to update review comments: {error}")),
                Err(_) => Ok(()),
            };
            match this.update(cx, |browser, cx| {
                browser.comments.anchor = match head {
                    Ok(hash) => Anchor::Head(hash),
                    Err(error) => Anchor::Failed(error.to_string()),
                };
                if let Err(error) = outdated {
                    browser.comments.notice = Notice::Error(error);
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
    }

    /// The workspace's `isGitRepo` as it is now: once it changes, `HEAD` is read, or comments
    /// have no commit (Electron's effect runs again when `isGitRepo` changes).
    pub(super) fn follow_repo(&mut self, in_repo: bool, cx: &mut Context<Self>) {
        if in_repo != (self.comments.anchor != Anchor::NotInRepo) {
            self.refresh_anchor(in_repo, cx);
        }
    }

    /// The tab is on screen (the shown tab of a tabset of the workspace shown) or not. Shown
    /// after being hidden, it reads `HEAD` again and marks comments outdated whose line moved
    /// since, as Electron does each time it mounts the tab (a tab switch, back to the
    /// workspace), so comments made after a commit anchor to it. Shown for the first time
    /// too: every Files tab of a session is made when the session connects, and a view
    /// shows it later.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        let next = if visible { Visibility::Shown } else { Visibility::Hidden };
        let was = std::mem::replace(&mut self.comments.visibility, next);
        if !visible || was == Visibility::Shown {
            return;
        }
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let in_repo = workspace.read(cx).file().is_git_repo;
        self.refresh_anchor(in_repo, cx);
    }

    /// The review comments on the shown file, in the order they were made.
    pub fn file_comments(&self, cx: &App) -> Result<Vec<ReviewComment>, MetadataError> {
        let FileView::Text { path, .. } = &self.view else {
            return Ok(Vec::new());
        };
        let relative = to_workspace_relative(&self.root, path);
        let raw = self
            .workspace
            .upgrade()
            .and_then(|workspace| workspace.read(cx).file().metadata.get(REVIEW_COMMENTS_KEY).cloned());
        Ok(ReviewComments::parse(raw.as_deref())?
            .comments()
            .into_iter()
            .filter(|comment| comment.file_path == relative || comment.file_path == *path)
            .collect())
    }

    /// Start a comment under 1-based `line_number` of the shown file (one at a time).
    pub fn start_comment(&mut self, line_number: u32, cx: &mut Context<Self>) {
        let FileView::Text { path, .. } = &self.view else {
            return;
        };
        self.comments.draft = Some(LineDraft { path: path.clone(), line_number });
        self.comments.adding = GitOp::Idle;
        self.comments._add = None;
        cx.notify();
    }

    pub fn cancel_comment(&mut self, cx: &mut Context<Self>) {
        self.comments.draft = None;
        self.comments.adding = GitOp::Idle;
        self.comments._add = None;
        cx.notify();
    }

    /// Save the draft with `text`: on the working-tree file (the modified side), anchored to
    /// `HEAD` and the line's blame.
    pub fn submit_comment(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = text.trim();
        let Some(draft) = self.comments.draft.clone() else {
            return;
        };
        if text.is_empty() || self.comments.adding == GitOp::Running {
            return;
        }
        let commit = match &self.comments.anchor {
            Anchor::NotInRepo | Anchor::Failed(_) => CommitAnchor::NotInRepo,
            Anchor::Head(hash) => CommitAnchor::Head(hash.clone()),
            Anchor::Reading => {
                self.comments.adding = GitOp::Failed("Still reading HEAD; try again".to_owned());
                cx.notify();
                return;
            }
        };
        let comment = NewReviewComment {
            file_path: to_workspace_relative(&self.root, &draft.path).to_owned(),
            line_number: draft.line_number,
            text: text.to_owned(),
            commit,
            side: Side::Modified,
        };
        let (workspace, blame) = (self.workspace.clone(), self.blame());
        self.comments.adding = GitOp::Running;
        self.comments._add = Some(cx.spawn(async move |this, cx| {
            let result =
                ReviewCommentStore::new(Later::new(workspace, cx.clone(), blame)).add_review_comment(comment).await;
            match this.update(cx, |browser, cx| {
                match result {
                    Ok(()) => {
                        browser.comments.draft = None;
                        browser.comments.adding = GitOp::Idle;
                        browser.comments.notice = Notice::Clear;
                    }
                    Err(error) => browser.comments.adding = GitOp::Failed(format!("Failed to add comment: {error}")),
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
        cx.notify();
    }

    pub fn delete_comment(&mut self, comment_id: &str, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let result = ReviewCommentStore::new(Now::new(workspace, cx, self.blame())).delete_review_comment(comment_id);
        self.comments.notice = match result {
            Ok(()) => Notice::Clear,
            Err(error) => Notice::Error(error.to_string()),
        };
        cx.notify();
    }

    /// Another file is shown: its draft goes.
    pub(super) fn drop_draft_unless(&mut self, path: &str) {
        if self.comments.draft.as_ref().is_some_and(|draft| draft.path != path) {
            self.comments.draft = None;
            self.comments.adding = GitOp::Idle;
            self.comments._add = None;
        }
    }
}
