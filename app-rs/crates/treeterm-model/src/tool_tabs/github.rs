//! The GitHub tab (`GitHubBrowser.tsx`): the workspace branch's pull request as the
//! session's git controller last loaded it (`GitController::status().pull_request`), with a
//! refresh and links to the PR. Nothing here fetches on its own.

use treeterm_core::git_status::PullRequest;
use treeterm_core::github::{CheckConclusion, CheckRun, CheckStatus, PrDetails, PrInfo, PrState, ReviewState, ReviewThread, ThreadLine};

/// The icon a review or check is shown with (Electron's lucide icons and their colours).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Approved review, successful check.
    Approved,
    /// Changes requested, failed check.
    Rejected,
    Commented,
    Pending,
    /// Dismissed review, neutral or skipped check.
    Dismissed,
    /// A check still running.
    Running,
    /// Any other finished state.
    Unknown,
}

/// What the tab shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrPanel<'a> {
    /// "No open pull request found for this branch."
    NoPr,
    Found {
        pr: &'a PrInfo,
        /// Reviews, checks and threads, unless GraphQL failed.
        details: Result<&'a treeterm_core::github::PrStatus, &'a str>,
    },
}

pub fn panel(pull_request: &PullRequest) -> PrPanel<'_> {
    match pull_request {
        PullRequest::None => PrPanel::NoPr,
        PullRequest::Found(pr) => PrPanel::Found {
            pr,
            details: match &pr.details {
                PrDetails::Loaded(status) => Ok(status),
                PrDetails::Unavailable { reason } => Err(reason),
            },
        },
    }
}

/// The state badge (`PrStateBadge`).
pub fn state_label(state: &PrState) -> String {
    match state {
        PrState::Open => "Open".to_owned(),
        PrState::Closed => "Closed".to_owned(),
        PrState::Merged => "Merged".to_owned(),
        PrState::Other(other) => other.clone(),
    }
}

/// `formatReviewState`.
pub fn review_label(state: &ReviewState) -> String {
    match state {
        ReviewState::Approved => "Approved".to_owned(),
        ReviewState::ChangesRequested => "Changes requested".to_owned(),
        ReviewState::Commented => "Commented".to_owned(),
        ReviewState::Pending => "Pending".to_owned(),
        ReviewState::Dismissed => "Dismissed".to_owned(),
        ReviewState::Other(other) => other.clone(),
    }
}

/// `ReviewStateIcon`.
pub fn review_mark(state: &ReviewState) -> Mark {
    match state {
        ReviewState::Approved => Mark::Approved,
        ReviewState::ChangesRequested => Mark::Rejected,
        ReviewState::Commented => Mark::Commented,
        ReviewState::Pending => Mark::Pending,
        ReviewState::Dismissed => Mark::Dismissed,
        ReviewState::Other(_) => Mark::Unknown,
    }
}

/// `CheckRunIcon`: running until completed, then by conclusion.
pub fn check_mark(check: &CheckRun) -> Mark {
    match &check.status {
        CheckStatus::Completed => {}
        CheckStatus::InProgress
        | CheckStatus::Queued
        | CheckStatus::Waiting
        | CheckStatus::Pending
        | CheckStatus::Requested
        | CheckStatus::Other(_) => return Mark::Running,
    }
    match &check.conclusion {
        CheckConclusion::Success => Mark::Approved,
        CheckConclusion::Failure => Mark::Rejected,
        CheckConclusion::Neutral | CheckConclusion::Skipped => Mark::Dismissed,
        CheckConclusion::NotYet
        | CheckConclusion::Cancelled
        | CheckConclusion::TimedOut
        | CheckConclusion::ActionRequired
        | CheckConclusion::Other(_) => Mark::Unknown,
    }
}

/// Where an unresolved thread points: `path:line`, or the path alone.
pub fn thread_location(thread: &ReviewThread) -> String {
    match thread.line {
        ThreadLine::Line(line) => format!("{}:{line}", thread.path),
        ThreadLine::None => thread.path.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use treeterm_core::github::{PrStatus, Review};

    fn check(status: CheckStatus, conclusion: CheckConclusion) -> CheckRun {
        CheckRun { name: "ci".into(), status, conclusion }
    }

    #[test]
    fn checks_run_until_completed_then_show_their_conclusion() {
        assert_eq!(check_mark(&check(CheckStatus::InProgress, CheckConclusion::NotYet)), Mark::Running);
        assert_eq!(check_mark(&check(CheckStatus::Completed, CheckConclusion::Success)), Mark::Approved);
        assert_eq!(check_mark(&check(CheckStatus::Completed, CheckConclusion::Failure)), Mark::Rejected);
        assert_eq!(check_mark(&check(CheckStatus::Completed, CheckConclusion::Skipped)), Mark::Dismissed);
        assert_eq!(check_mark(&check(CheckStatus::Completed, CheckConclusion::TimedOut)), Mark::Unknown);
    }

    #[test]
    fn reviews_and_states_read_like_electron() {
        assert_eq!(review_label(&ReviewState::ChangesRequested), "Changes requested");
        assert_eq!(review_mark(&ReviewState::ChangesRequested), Mark::Rejected);
        assert_eq!(review_label(&ReviewState::Other("NEW".into())), "NEW");
        assert_eq!(state_label(&PrState::Merged), "Merged");
        let thread = |line| ReviewThread { path: "src/a.rs".into(), body: "b".into(), author: "me".into(), line };
        assert_eq!(thread_location(&thread(ThreadLine::Line(7))), "src/a.rs:7");
        assert_eq!(thread_location(&thread(ThreadLine::None)), "src/a.rs");
    }

    #[test]
    fn the_panel_shows_details_or_why_they_are_missing() {
        assert_eq!(panel(&PullRequest::None), PrPanel::NoPr);
        let status = PrStatus {
            state: PrState::Open,
            reviews: vec![Review { author: "a".into(), state: ReviewState::Approved }],
            check_runs: Vec::new(),
            unresolved_threads: Vec::new(),
        };
        let pr = |details| PrInfo { number: 3, url: "u".into(), title: "t".into(), details };
        let loaded = PullRequest::Found(pr(PrDetails::Loaded(status.clone())));
        assert!(matches!(panel(&loaded), PrPanel::Found { details: Ok(s), .. } if *s == status));
        let failed = PullRequest::Found(pr(PrDetails::Unavailable { reason: "graphql".into() }));
        assert!(matches!(panel(&failed), PrPanel::Found { details: Err("graphql"), .. }));
    }
}
