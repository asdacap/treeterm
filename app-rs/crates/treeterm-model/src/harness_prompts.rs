//! The AI harness's floating prompt buttons, stacked above the terminal's circle buttons:
//! `PromptCommitButton`, `PromptRebaseButton`, `ReviewCommentsButton`,
//! `ParentReviewCommentsButton` and `PromptGitHubCommentsButton`.

use gpui_kit::{App, Entity};
use treeterm_core::git_status::{ParentConflicts, ParentDiff, PullRequest, WorkingTree};
use treeterm_core::review::{
    REVIEW_COMMENTS_KEY, Resolution, ReviewComment, ReviewCommentStore, ReviewComments, build_prompt_for_comments,
    generate_review_prompt,
};

use crate::review_metadata::{Blame, Now};
use crate::session_store::SessionStore;
use crate::workspace_store::WorkspaceStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessPrompt {
    /// Uncommitted changes and no conflict with the parent.
    Commit,
    /// The branch conflicts with its parent's.
    Rebase { branch: String, parent: String },
    /// This workspace has review comments not yet sent.
    ReviewComments,
    /// Nothing to merge yet, and the parent has review comments not yet sent.
    ParentReviewComments { count: usize },
    /// The PR has unresolved threads.
    GitHubComments { count: usize },
}

impl HarnessPrompt {
    pub fn label(&self) -> String {
        match self {
            HarnessPrompt::Commit => "Prompt Commit".to_owned(),
            HarnessPrompt::Rebase { .. } => "Prompt Rebase".to_owned(),
            HarnessPrompt::ReviewComments => "Address Comments".to_owned(),
            HarnessPrompt::ParentReviewComments { count } => format!("Address Parent Comments ({count})"),
            HarnessPrompt::GitHubComments { count } => format!("Address PR Comments ({count})"),
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            HarnessPrompt::Commit => "Prompt AI to commit changes",
            HarnessPrompt::Rebase { .. } => "Prompt AI to rebase onto parent branch",
            HarnessPrompt::ReviewComments => "Address review comments",
            HarnessPrompt::ParentReviewComments { .. } => "Prompt AI to address the parent workspace's review comments",
            HarnessPrompt::GitHubComments { .. } => "Prompt AI to pull and address GitHub PR comments",
        }
    }
}

fn comments_of(workspace: &WorkspaceStore) -> Vec<ReviewComment> {
    let raw = workspace.file().metadata.get(REVIEW_COMMENTS_KEY).cloned();
    match ReviewComments::parse(raw.as_deref()) {
        Ok(comments) => comments.comments(),
        Err(error) => {
            tracing::debug!(%error, "unreadable review comments");
            Vec::new()
        }
    }
}

fn open_comments(workspace: &WorkspaceStore) -> Vec<ReviewComment> {
    comments_of(workspace).into_iter().filter(|c| c.resolution == Resolution::Open).collect()
}

/// The buttons `workspace_id`'s harness shows, in Electron's order.
pub fn harness_prompts(session: &SessionStore, workspace_id: &str, cx: &App) -> Vec<HarnessPrompt> {
    let Some(store) = session.workspace_store(workspace_id) else {
        return Vec::new();
    };
    let workspace = store.read(cx);
    let file = workspace.file();
    let status = session.git_controller(workspace_id).map(|git| git.read(cx).status().clone()).unwrap_or_default();
    let conflicting = status.parent_conflicts == ParentConflicts::Conflicting;
    let mut prompts = Vec::new();
    if status.working_tree == WorkingTree::Dirty && !conflicting {
        prompts.push(HarnessPrompt::Commit);
    }
    if let (true, Some(branch), Some(parent)) = (conflicting, file.git_branch.as_deref(), workspace.parent_branch()) {
        prompts.push(HarnessPrompt::Rebase { branch: branch.to_owned(), parent: parent.to_owned() });
    }
    if !generate_review_prompt(&comments_of(workspace)).is_empty() {
        prompts.push(HarnessPrompt::ReviewComments);
    }
    let parent = file.parent_id.as_deref().filter(|p| !p.is_empty()).and_then(|p| session.workspace_store(p));
    if let (ParentDiff::Clean, Some(parent)) = (status.parent_diff, parent) {
        let count = open_comments(parent.read(cx)).len();
        if count > 0 {
            prompts.push(HarnessPrompt::ParentReviewComments { count });
        }
    }
    if let PullRequest::Found(pr) = &status.pull_request {
        let count = pr.unresolved_count();
        if count > 0 {
            prompts.push(HarnessPrompt::GitHubComments { count });
        }
    }
    prompts
}

fn mark_addressed(session: &Entity<SessionStore>, workspace: Entity<WorkspaceStore>, ids: Vec<String>, cx: &mut App) -> Result<(), String> {
    let blame = Blame { git: session.read(cx).connection().git(), path: workspace.read(cx).workspace().path.clone() };
    ReviewCommentStore::new(Now::new(workspace, cx, blame))
        .mark_review_comments_addressed(&ids)
        .map_err(|e| e.to_string())
}

/// Paste `prompt`'s text into the harness; comments it sends count as addressed.
pub fn send(prompt: &HarnessPrompt, session: &Entity<SessionStore>, workspace_id: &str, cx: &mut App) -> Result<(), String> {
    let store = session.read(cx).workspace_store(workspace_id).cloned().ok_or("The workspace is gone")?;
    match prompt {
        HarnessPrompt::Commit => store.update(cx, |w, cx| w.prompt_harness("commit", cx)),
        HarnessPrompt::Rebase { branch, parent } => {
            let text = format!("rebase local branch {branch} onto {parent}");
            store.update(cx, |w, cx| w.prompt_harness(&text, cx))
        }
        HarnessPrompt::GitHubComments { .. } => store.update(cx, |w, cx| w.prompt_harness("Pull Github comment and address", cx)),
        HarnessPrompt::ReviewComments => {
            let comments = comments_of(store.read(cx));
            let text = generate_review_prompt(&comments);
            store.update(cx, |w, cx| w.prompt_harness(&text, cx))?;
            let ids = comments.into_iter().filter(|c| c.resolution == Resolution::Open).map(|c| c.id).collect();
            mark_addressed(session, store, ids, cx)
        }
        HarnessPrompt::ParentReviewComments { .. } => {
            let parent_id = store.read(cx).file().parent_id.clone().filter(|p| !p.is_empty()).ok_or("No parent workspace")?;
            let parent = session.read(cx).workspace_store(&parent_id).cloned().ok_or("The parent workspace is gone")?;
            let open = open_comments(parent.read(cx));
            if open.is_empty() {
                return Ok(());
            }
            // Files-browser comments carry absolute paths into the parent's worktree.
            let parent_prefix = format!("{}/", parent.read(cx).workspace().path);
            let own_prefix = format!("{}/", store.read(cx).workspace().path);
            let rebased: Vec<ReviewComment> = open
                .iter()
                .cloned()
                .map(|mut c| {
                    if let Some(rest) = c.file_path.strip_prefix(&parent_prefix) {
                        c.file_path = format!("{own_prefix}{rest}");
                    }
                    c
                })
                .collect();
            let refs: Vec<&ReviewComment> = rebased.iter().collect();
            let text = build_prompt_for_comments(&refs);
            store.update(cx, |w, cx| w.prompt_harness(&text, cx))?;
            mark_addressed(session, parent, open.into_iter().map(|c| c.id).collect(), cx)
        }
    }
}
