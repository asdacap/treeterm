//! The Comments tab (`CommentsList.tsx`): the workspace's review comments with the code
//! around each, filtered to unprompted or prompted ones, with prompt / delete / go to file,
//! the generated prompt, and "Push to GitHub".
//!
//! Comments live in the workspace metadata (`reviewComments`), shared with the Review tab
//! and Electron through [`treeterm_core::review`].

use std::collections::{HashMap, HashSet};

use gpui_kit::{App, Context, Entity, Subscription, Task, WeakEntity};
use serde_json::json;
use treeterm_core::files::max_read_bytes_for;
use treeterm_core::fs::{FsClient, ReadOutcome};
use treeterm_core::git::GitApi;
use treeterm_core::review::{
    MetadataError, REVIEW_COMMENTS_KEY, Resolution, ReviewComment, ReviewCommentStore, ReviewComments,
    build_prompt_for_comments, generate_review_prompt,
};

use crate::git_controller::GitController;
use crate::review_metadata::{Blame, Now};
use crate::workspace_store::WorkspaceStore;

/// Lines shown above and below a comment's line.
pub const CONTEXT_LINES: usize = 3;

/// One line of a comment's code context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeLine {
    /// 1-based.
    pub number: usize,
    pub text: String,
    /// The commented line.
    pub target: bool,
}

/// `extractCodeContext`: the commented line with [`CONTEXT_LINES`] around it; `None` when
/// the file has no such line.
pub fn code_context(content: &str, line_number: u32) -> Option<Vec<CodeLine>> {
    let lines: Vec<&str> = content.split('\n').collect();
    let target = usize::try_from(line_number).ok().filter(|line| (1..=lines.len()).contains(line))?;
    let start = target.saturating_sub(1 + CONTEXT_LINES);
    let end = lines.len().min(target + CONTEXT_LINES);
    Some(
        lines[start..end]
            .iter()
            .enumerate()
            .map(|(offset, text)| CodeLine {
                number: start + offset + 1,
                text: (*text).to_owned(),
                target: start + offset + 1 == target,
            })
            .collect(),
    )
}

/// A commented file's text, for the code context. Unreadable files show no context.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FileText {
    Loading,
    Loaded(String),
    Unreadable,
}

/// "Push to GitHub".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushState {
    Idle,
    Pushing,
    /// What happened, for the status line.
    Done(String),
}

pub struct CommentsModel {
    workspace: WeakEntity<WorkspaceStore>,
    fs: FsClient,
    git: GitApi,
    /// The worktree; comment paths are relative to it.
    root: String,
    filter: Resolution,
    prompt_expanded: bool,
    files: HashMap<String, FileText>,
    push: PushState,
    /// The last failed action (metadata unreadable, no harness to prompt).
    error: Option<String>,
    _reads: HashMap<String, Task<()>>,
    _push: Option<Task<()>>,
    _workspace: Subscription,
}

impl CommentsModel {
    pub fn new(fs: FsClient, git: GitApi, root: String, workspace: &Entity<WorkspaceStore>, cx: &mut Context<Self>) -> Self {
        // Comments are read from the metadata whenever shown; new files get their text.
        let observe = cx.observe(workspace, |model, _, cx| {
            model.load_files(cx);
            cx.notify();
        });
        // The workspace is mid-update while its tab is created: read it once that is over.
        let this = cx.weak_entity();
        cx.defer(move |cx| match this.update(cx, |model, cx| model.load_files(cx)) {
            Ok(()) | Err(_) => {} // Err: closed at once
        });
        Self {
            workspace: workspace.downgrade(),
            fs,
            git,
            root,
            filter: Resolution::Open,
            prompt_expanded: false,
            files: HashMap::new(),
            push: PushState::Idle,
            error: None,
            _reads: HashMap::new(),
            _push: None,
            _workspace: observe,
        }
    }

    /// Every review comment of the workspace, from its metadata.
    pub fn comments(&self, cx: &App) -> Result<Vec<ReviewComment>, MetadataError> {
        let raw = self
            .workspace
            .upgrade()
            .and_then(|workspace| workspace.read(cx).file().metadata.get(REVIEW_COMMENTS_KEY).cloned());
        Ok(ReviewComments::parse(raw.as_deref())?.comments())
    }

    pub fn filter(&self) -> Resolution {
        self.filter
    }

    pub fn set_filter(&mut self, filter: Resolution, cx: &mut Context<Self>) {
        self.filter = filter;
        cx.notify();
    }

    pub fn prompt_expanded(&self) -> bool {
        self.prompt_expanded
    }

    pub fn toggle_prompt(&mut self, cx: &mut Context<Self>) {
        self.prompt_expanded = !self.prompt_expanded;
        cx.notify();
    }

    pub fn push_state(&self) -> &PushState {
        &self.push
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The prompt for the unprompted comments ("Generated Prompt"); empty without any.
    pub fn prompt(&self, cx: &App) -> String {
        match self.comments(cx) {
            Ok(comments) => generate_review_prompt(&comments),
            Err(_) => String::new(),
        }
    }

    /// The code around `comment`, once its file is read.
    pub fn code_context(&self, comment: &ReviewComment) -> Option<Vec<CodeLine>> {
        match self.files.get(&comment.file_path)? {
            FileText::Loaded(text) => code_context(text, comment.line_number),
            FileText::Loading | FileText::Unreadable => None,
        }
    }

    /// Read every commented file not read yet (once per file).
    fn load_files(&mut self, cx: &mut Context<Self>) {
        let Ok(comments) = self.comments(cx) else {
            return; // the metadata error is shown with the list
        };
        let missing: HashSet<String> =
            comments.into_iter().map(|c| c.file_path).filter(|path| !self.files.contains_key(path)).collect();
        for path in missing {
            let absolute = format!("{}/{}", self.root.trim_end_matches('/'), path);
            let read = self.fs.read_file(&self.root, &absolute, max_read_bytes_for(&absolute));
            self.files.insert(path.clone(), FileText::Loading);
            let owned = path.clone();
            let task = cx.spawn(async move |this, cx| {
                let text = match read.await {
                    Ok(Ok(ReadOutcome::Read(file))) => FileText::Loaded(String::from_utf8_lossy(&file.content).into_owned()),
                    Ok(Ok(ReadOutcome::TooLarge { .. })) => FileText::Unreadable,
                    Ok(Err(error)) => {
                        tracing::debug!(path = %owned, %error, "no code context for a commented file");
                        FileText::Unreadable
                    }
                    Err(lost) => {
                        tracing::debug!(path = %owned, %lost, "no code context for a commented file");
                        FileText::Unreadable
                    }
                };
                match this.update(cx, |model, cx| {
                    model.files.insert(owned.clone(), text);
                    model._reads.remove(&owned);
                    cx.notify();
                }) {
                    Ok(()) | Err(_) => {} // Err: tab closed
                }
            });
            self._reads.insert(path, task);
        }
    }

    /// Run a comment-store operation on the live metadata; a failure is shown.
    fn with_store(&mut self, cx: &mut Context<Self>, op: impl FnOnce(&ReviewCommentStore<Now<'_>>) -> Result<(), String>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let blame = Blame { git: self.git.clone(), path: self.root.clone() };
        let result = op(&ReviewCommentStore::new(Now::new(workspace, cx, blame)));
        self.error = result.err();
        cx.notify();
    }

    pub fn delete(&mut self, comment_id: &str, cx: &mut Context<Self>) {
        self.with_store(cx, |store| store.delete_review_comment(comment_id).map_err(|e| e.to_string()));
    }

    /// "Prompt" / "Re-prompt": paste the comment into the workspace's AI harness; once sent
    /// it counts as prompted.
    pub fn prompt_comment(&mut self, comment_id: &str, cx: &mut Context<Self>) {
        let comments = match self.comments(cx) {
            Ok(comments) => comments,
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        let Some(comment) = comments.iter().find(|comment| comment.id == comment_id) else {
            return;
        };
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let prompt = build_prompt_for_comments(&[comment]);
        if let Err(error) = workspace.update(cx, |w, cx| w.prompt_harness(&prompt, cx)) {
            self.error = Some(error);
            cx.notify();
            return;
        }
        let ids = [comment.id.clone()];
        self.with_store(cx, |store| store.mark_review_comments_addressed(&ids).map_err(|e| e.to_string()));
    }

    /// "Go to file": a Files tab on the comment's file, scrolled to its line.
    pub fn go_to_file(&self, comment: &ReviewComment, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let path = format!("{}/{}", self.root.trim_end_matches('/'), comment.file_path);
        let state = json!({ "selectedPath": path, "scrollToLine": comment.line_number });
        if workspace.update(cx, |w, cx| w.add_tab_with("filesystem", state, cx)).is_none() {
            tracing::error!("the file browser is not registered");
        }
    }

    /// "Push to GitHub": post the unprompted comments on the branch's PR; those posted count
    /// as prompted. `github` is the session's controller for this workspace (`None` when
    /// the session has no GitHub client).
    pub fn push_to_github(&mut self, github: Option<Entity<GitController>>, cx: &mut Context<Self>) {
        if self.push == PushState::Pushing {
            return;
        }
        let unprompted: Vec<ReviewComment> = match self.comments(cx) {
            Ok(comments) => comments.into_iter().filter(|c| c.resolution == Resolution::Open).collect(),
            Err(error) => {
                self.push = PushState::Done(format!("Push failed: {error}"));
                cx.notify();
                return;
            }
        };
        if unprompted.is_empty() {
            return;
        }
        let Some(github) = github else {
            self.push = PushState::Done("Push failed: GitHub is not available in this session".to_owned());
            cx.notify();
            return;
        };
        let ids: Vec<String> = unprompted.iter().map(|c| c.id.clone()).collect();
        let push = github.update(cx, |controller, cx| controller.push_review_comments(unprompted, cx));
        self.push = PushState::Pushing;
        self._push = Some(cx.spawn(async move |this, cx| {
            let result = push.await;
            match this.update(cx, |model, cx| {
                let outcome = match result {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        model.push = PushState::Done(format!("Push failed: {error}"));
                        cx.notify();
                        return;
                    }
                };
                let failed: HashSet<&str> = outcome.failed.iter().map(|f| f.id.as_str()).collect();
                let posted: Vec<String> = ids.iter().filter(|id| !failed.contains(id.as_str())).cloned().collect();
                model.with_store(cx, |store| store.mark_review_comments_addressed(&posted).map_err(|e| e.to_string()));
                model.push = PushState::Done(match outcome.failed.len() {
                    0 => format!("Pushed {} comment(s) to GitHub", outcome.posted),
                    failed => format!("Pushed {}, {failed} failed", outcome.posted),
                });
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
        cx.notify();
    }
}

/// `toLocaleString` of a comment's creation time.
pub fn created_at(comment: &ReviewComment) -> String {
    treeterm_core::clock::date_time(treeterm_core::clock::from_millis(comment.created_at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_context_surrounds_the_line_and_stops_at_the_file_edges() {
        let text = (1..=10).map(|n| format!("l{n}")).collect::<Vec<_>>().join("\n");
        let around = code_context(&text, 5).unwrap();
        assert_eq!(around.iter().map(|l| l.number).collect::<Vec<_>>(), vec![2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(around.iter().filter(|l| l.target).map(|l| l.text.as_str()).collect::<Vec<_>>(), vec!["l5"]);
        let first = code_context(&text, 1).unwrap();
        assert_eq!(first.first().map(|l| l.number), Some(1));
        assert_eq!(first.len(), 4);
        let last = code_context(&text, 10).unwrap();
        assert_eq!(last.last().map(|l| l.number), Some(10));
        assert!(code_context(&text, 0).is_none());
        assert!(code_context(&text, 11).is_none());
    }
}
