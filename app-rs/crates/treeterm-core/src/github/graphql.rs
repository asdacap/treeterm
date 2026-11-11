//! The GraphQL query for a PR's state, reviews, checks and threads, and its response.

use serde::Deserialize;

use super::{
    CheckConclusion, CheckRun, CheckStatus, PrState, PrStatus, Review, ReviewState, ReviewThread,
    ThreadLine,
};

pub const PR_DETAILS_QUERY: &str = r"query($owner: String!, $repo: String!, $prNumber: Int!) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $prNumber) {
      state
      reviewThreads(first: 100) {
        nodes {
          isResolved
          comments(first: 1) {
            nodes {
              body
              path
              line
              author { login }
            }
          }
        }
      }
      latestReviews(first: 20) {
        nodes {
          author { login }
          state
        }
      }
      commits(last: 1) {
        nodes {
          commit {
            statusCheckRollup {
              contexts(first: 50) {
                nodes {
                  ... on CheckRun {
                    __typename
                    name
                    status
                    conclusion
                  }
                  ... on StatusContext {
                    __typename
                    context
                    state
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}";

/// A connection's `nodes`; GraphQL lists may hold `null` items, which are skipped.
#[derive(Deserialize)]
#[serde(bound = "T: Deserialize<'de>")]
struct Nodes<T> {
    #[serde(default)]
    nodes: Option<Vec<Option<T>>>,
}

impl<T> Nodes<T> {
    fn items(connection: Option<Self>) -> impl Iterator<Item = T> {
        connection
            .and_then(|connection| connection.nodes)
            .unwrap_or_default()
            .into_iter()
            .flatten()
    }
}

#[derive(Deserialize)]
struct Author {
    login: String,
}

fn login(author: Option<Author>) -> String {
    author.map(|author| author.login).unwrap_or_default()
}

#[derive(Deserialize)]
struct Response {
    data: Option<Data>,
    #[serde(default)]
    errors: Vec<GraphqlError>,
}

#[derive(Deserialize)]
struct GraphqlError {
    message: String,
}

#[derive(Deserialize)]
struct Data {
    repository: Option<Repository>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Repository {
    pull_request: Option<PullRequest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequest {
    state: Option<String>,
    review_threads: Option<Nodes<Thread>>,
    latest_reviews: Option<Nodes<ReviewNode>>,
    commits: Option<Nodes<CommitNode>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Thread {
    is_resolved: bool,
    comments: Option<Nodes<ThreadComment>>,
}

#[derive(Deserialize)]
struct ThreadComment {
    body: Option<String>,
    path: Option<String>,
    line: Option<u64>,
    author: Option<Author>,
}

#[derive(Deserialize)]
struct ReviewNode {
    author: Option<Author>,
    state: String,
}

#[derive(Deserialize)]
struct CommitNode {
    commit: Option<Commit>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Commit {
    status_check_rollup: Option<Rollup>,
}

#[derive(Deserialize)]
struct Rollup {
    contexts: Option<Nodes<Context>>,
}

/// A `CheckRun` or a `StatusContext`; only check runs are reported.
#[derive(Deserialize)]
struct Context {
    #[serde(rename = "__typename")]
    typename: String,
    name: Option<String>,
    status: Option<String>,
    conclusion: Option<String>,
}

/// The status in a GraphQL response body, or why there is none.
pub(super) fn parse_pr_status(body: &[u8]) -> Result<PrStatus, String> {
    let response: Response = serde_json::from_slice(body)
        .map_err(|error| format!("invalid GraphQL response: {error}"))?;
    let pull_request = response
        .data
        .and_then(|data| data.repository)
        .and_then(|repository| repository.pull_request);
    let Some(pull_request) = pull_request else {
        let messages: Vec<String> = response
            .errors
            .into_iter()
            .map(|error| error.message)
            .collect();
        return Err(match messages.as_slice() {
            [] => "GraphQL response has no pullRequest".to_owned(),
            _ => messages.join("; "),
        });
    };

    let unresolved_threads = Nodes::items(pull_request.review_threads)
        .filter(|thread| !thread.is_resolved)
        .map(|thread| {
            let first = Nodes::items(thread.comments).next();
            match first {
                Some(comment) => ReviewThread {
                    path: comment.path.unwrap_or_default(),
                    body: comment.body.unwrap_or_default(),
                    author: login(comment.author),
                    line: comment.line.map_or(ThreadLine::None, ThreadLine::Line),
                },
                None => ReviewThread {
                    path: String::new(),
                    body: String::new(),
                    author: String::new(),
                    line: ThreadLine::None,
                },
            }
        })
        .collect();

    let reviews = Nodes::items(pull_request.latest_reviews)
        .map(|review| Review {
            author: login(review.author),
            state: review_state(review.state),
        })
        .collect();

    let check_runs = Nodes::items(pull_request.commits)
        .next()
        .and_then(|node| node.commit)
        .and_then(|commit| commit.status_check_rollup)
        .map(|rollup| {
            Nodes::items(rollup.contexts)
                .filter(|context| context.typename == "CheckRun")
                .map(|context| CheckRun {
                    name: context.name.unwrap_or_default(),
                    status: check_status(context.status.unwrap_or_else(|| "QUEUED".to_owned())),
                    conclusion: context
                        .conclusion
                        .map_or(CheckConclusion::NotYet, check_conclusion),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(PrStatus {
        state: pr_state(pull_request.state.unwrap_or_else(|| "OPEN".to_owned())),
        reviews,
        check_runs,
        unresolved_threads,
    })
}

fn pr_state(state: String) -> PrState {
    match state.as_str() {
        "OPEN" => PrState::Open,
        "CLOSED" => PrState::Closed,
        "MERGED" => PrState::Merged,
        _ => PrState::Other(state),
    }
}

fn review_state(state: String) -> ReviewState {
    match state.as_str() {
        "APPROVED" => ReviewState::Approved,
        "CHANGES_REQUESTED" => ReviewState::ChangesRequested,
        "COMMENTED" => ReviewState::Commented,
        "PENDING" => ReviewState::Pending,
        "DISMISSED" => ReviewState::Dismissed,
        _ => ReviewState::Other(state),
    }
}

fn check_status(status: String) -> CheckStatus {
    match status.as_str() {
        "COMPLETED" => CheckStatus::Completed,
        "IN_PROGRESS" => CheckStatus::InProgress,
        "QUEUED" => CheckStatus::Queued,
        "WAITING" => CheckStatus::Waiting,
        "PENDING" => CheckStatus::Pending,
        "REQUESTED" => CheckStatus::Requested,
        _ => CheckStatus::Other(status),
    }
}

fn check_conclusion(conclusion: String) -> CheckConclusion {
    match conclusion.as_str() {
        "SUCCESS" => CheckConclusion::Success,
        "FAILURE" => CheckConclusion::Failure,
        "NEUTRAL" => CheckConclusion::Neutral,
        "CANCELLED" => CheckConclusion::Cancelled,
        "TIMED_OUT" => CheckConclusion::TimedOut,
        "ACTION_REQUIRED" => CheckConclusion::ActionRequired,
        "SKIPPED" => CheckConclusion::Skipped,
        _ => CheckConclusion::Other(conclusion),
    }
}
