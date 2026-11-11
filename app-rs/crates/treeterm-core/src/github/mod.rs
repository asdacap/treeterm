//! GitHub pull requests over REST and GraphQL (port of `renderer/lib/githubClient.ts`).
//!
//! The token comes from `gh auth token` (on the daemon's host) or the PAT in settings; the
//! repository from `origin`'s URL. Requests go through [`HttpClient`] from the app process,
//! as Electron's renderer `fetch` did.
//!
//! Differences from the TS client:
//! - When the GraphQL details cannot be loaded, the PR is reported with
//!   [`PrDetails::Unavailable`] and the reason, instead of as an `OPEN` PR without reviews or
//!   checks (which could misreport a merged PR as open).
//! - Query values are percent-encoded (TS interpolated branch names raw, breaking on `#`,
//!   `&` or `+`).

mod graphql;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use futures::FutureExt;
use futures::future::BoxFuture;
use regex::Regex;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::exec::{
    DEFAULT_EXEC_TIMEOUT, ExecApi, ExecError, ExecRequest, ExecTimeout, collect_output,
};
use crate::http::{
    Body, HttpClient, HttpError, HttpRequest, HttpResponse, Method, encode_query_value,
};
use crate::review::{ReviewComment, Side};
use crate::settings::GithubSettings;

pub use graphql::PR_DETAILS_QUERY;

const API: &str = "https://api.github.com";
const ACCEPT: &str = "application/vnd.github+json";

// === types ==================================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerRepo {
    pub owner: String,
    pub repo: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrState {
    Open,
    Closed,
    Merged,
    /// A state this version does not know, as GitHub spelled it.
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewState {
    Approved,
    ChangesRequested,
    Commented,
    Pending,
    Dismissed,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckStatus {
    Completed,
    InProgress,
    Queued,
    Waiting,
    Pending,
    Requested,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckConclusion {
    /// The check has not finished (`null`).
    NotYet,
    Success,
    Failure,
    Neutral,
    Cancelled,
    TimedOut,
    ActionRequired,
    Skipped,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    /// Empty for a deleted account.
    pub author: String,
    pub state: ReviewState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRun {
    pub name: String,
    pub status: CheckStatus,
    pub conclusion: CheckConclusion,
}

/// Where a thread's first comment points (`line: number | null`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadLine {
    Line(u64),
    /// The line is no longer in the diff, or the comment is on the whole file.
    None,
}

/// An unresolved review thread, described by its first comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewThread {
    pub path: String,
    pub body: String,
    pub author: String,
    pub line: ThreadLine,
}

/// What GraphQL reports about a PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrStatus {
    pub state: PrState,
    pub reviews: Vec<Review>,
    pub check_runs: Vec<CheckRun>,
    pub unresolved_threads: Vec<ReviewThread>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrDetails {
    Loaded(PrStatus),
    /// The GraphQL request failed; only the REST basics are known.
    Unavailable {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrInfo {
    pub number: u64,
    pub url: String,
    pub title: String,
    pub details: PrDetails,
}

impl PrInfo {
    pub fn unresolved_count(&self) -> usize {
        match &self.details {
            PrDetails::Loaded(status) => status.unresolved_threads.len(),
            PrDetails::Unavailable { .. } => 0,
        }
    }
}

/// `getPrInfo`'s answer for a branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrLookup {
    Found(PrInfo),
    /// No PR for the branch; this URL opens one.
    NoPr {
        create_url: String,
    },
}

/// Where a PR's head branch lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadRepo {
    Origin,
    /// A fork, or a fork that was deleted.
    Fork,
}

/// An open PR, for opening a workspace from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrListItem {
    pub number: u64,
    pub title: String,
    pub author: String,
    /// The head branch (e.g. `feature/foo`).
    pub head_ref_name: String,
    pub head_repo: HeadRepo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedComment {
    pub id: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostCommentsOutcome {
    pub posted: usize,
    pub failed: Vec<FailedComment>,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum GitHubError {
    #[error("Failed to get token from gh CLI. Is gh installed and authenticated?")]
    GhTokenUnavailable,
    #[error("No GitHub PAT configured. Set one in Settings > GitHub.")]
    NoPat,
    #[error("Failed to get remote URL: {stderr}")]
    RemoteUrl { stderr: String },
    #[error("Could not parse GitHub owner/repo from remote URL: {url}")]
    NotGitHubRemote { url: String },
    #[error("GitHub API error: {status} {status_text}")]
    Api { status: u16, status_text: String },
    #[error("No open PR found for this branch")]
    NoPrForBranch,
    #[error("Invalid GitHub response: {0}")]
    InvalidResponse(String),
    #[error(transparent)]
    Exec(#[from] ExecError),
    #[error(transparent)]
    Http(#[from] HttpError),
}

// === pure helpers ===========================================================================

static GITHUB_REMOTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"github\.com[:/]([^/]+)/([^/]+?)(?:\.git)?$").expect("valid regex")
});

/// Owner and repository of a GitHub remote, SSH (`git@github.com:o/r.git`) or HTTPS.
pub fn parse_github_owner_repo(remote_url: &str) -> Option<OwnerRepo> {
    let captures = GITHUB_REMOTE.captures(remote_url)?;
    Some(OwnerRepo {
        owner: captures[1].to_owned(),
        repo: captures[2].to_owned(),
    })
}

/// Where the token comes from (`settings.github`).
enum TokenSource {
    Gh,
    Pat(String),
}

fn token_source(settings: &GithubSettings) -> Result<TokenSource, GitHubError> {
    match (settings.autodetect_via_gh, settings.pat.as_str()) {
        (true, _) => Ok(TokenSource::Gh),
        (false, "") => Err(GitHubError::NoPat),
        (false, pat) => Ok(TokenSource::Pat(pat.to_owned())),
    }
}

// === client =================================================================================

/// The current GitHub settings (read on every call, as Electron loads them each time).
pub trait GithubSettingsSource: Send + Sync {
    fn github_settings(&self) -> GithubSettings;
}

impl<F: Fn() -> GithubSettings + Send + Sync> GithubSettingsSource for F {
    fn github_settings(&self) -> GithubSettings {
        self()
    }
}

/// GitHub operations, as a trait so their callers can be driven by fakes.
pub trait GitHubApi: Send + Sync {
    fn get_pr_info(
        &self,
        repo_path: &str,
        head: &str,
        base: &str,
    ) -> BoxFuture<'static, Result<PrLookup, GitHubError>>;
    fn list_open_prs(
        &self,
        repo_path: &str,
    ) -> BoxFuture<'static, Result<Vec<PrListItem>, GitHubError>>;
    fn post_review_comments(
        &self,
        repo_path: &str,
        head: &str,
        base: &str,
        comments: Vec<ReviewComment>,
    ) -> BoxFuture<'static, Result<PostCommentsOutcome, GitHubError>>;
}

/// The token and repository every call needs.
struct Resolved {
    token: String,
    owner: String,
    repo: String,
}

impl Resolved {
    fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }

    fn get(&self, url: String) -> HttpRequest {
        HttpRequest {
            method: Method::Get,
            url,
            headers: vec![
                ("Authorization".to_owned(), format!("Bearer {}", self.token)),
                ("Accept".to_owned(), ACCEPT.to_owned()),
            ],
            body: Body::Empty,
        }
    }

    /// `GET /repos/{o}/{r}/pulls` for `head` into `base`, any state: a merged PR still counts.
    fn pulls_for_branch(&self, head: &str, base: &str) -> HttpRequest {
        self.get(format!(
            "{API}/repos/{}/pulls?head={}&base={}&state=all",
            self.slug(),
            encode_query_value(&format!("{}:{head}", self.owner)),
            encode_query_value(base),
        ))
    }
}

/// Runs `gh`/`git` through the daemon and talks to GitHub. Cheap to clone.
#[derive(Clone)]
pub struct GitHubClient {
    exec: Arc<dyn ExecApi>,
    http: Arc<dyn HttpClient>,
    settings: Arc<dyn GithubSettingsSource>,
}

impl GitHubClient {
    pub fn new(
        exec: Arc<dyn ExecApi>,
        http: Arc<dyn HttpClient>,
        settings: Arc<dyn GithubSettingsSource>,
    ) -> Self {
        Self {
            exec,
            http,
            settings,
        }
    }

    async fn run(
        &self,
        cwd: &str,
        command: &str,
        args: &[&str],
    ) -> Result<crate::exec::CommandOutput, ExecError> {
        collect_output(self.exec.start(ExecRequest {
            cwd: cwd.to_owned(),
            command: command.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            env: HashMap::new(),
            timeout: ExecTimeout::After(DEFAULT_EXEC_TIMEOUT),
        }))
        .await
    }

    async fn resolve(&self, repo_path: &str) -> Result<Resolved, GitHubError> {
        let token = match token_source(&self.settings.github_settings())? {
            TokenSource::Gh => {
                let output = self.run(repo_path, "gh", &["auth", "token"]).await?;
                if output.exit_code != 0 {
                    return Err(GitHubError::GhTokenUnavailable);
                }
                String::from_utf8_lossy(&output.stdout).trim().to_owned()
            }
            TokenSource::Pat(pat) => pat,
        };
        let remote = self
            .run(repo_path, "git", &["remote", "get-url", "origin"])
            .await?;
        if remote.exit_code != 0 {
            return Err(GitHubError::RemoteUrl {
                stderr: String::from_utf8_lossy(&remote.stderr).into_owned(),
            });
        }
        let url = String::from_utf8_lossy(&remote.stdout).trim().to_owned();
        let OwnerRepo { owner, repo } =
            parse_github_owner_repo(&url).ok_or(GitHubError::NotGitHubRemote { url })?;
        Ok(Resolved { token, owner, repo })
    }

    /// Send `request` and decode a 2xx JSON body; any other status is [`GitHubError::Api`].
    async fn fetch_json<T: DeserializeOwned>(
        &self,
        request: HttpRequest,
    ) -> Result<T, GitHubError> {
        let response = ok_response(self.http.send(request).await?)?;
        serde_json::from_slice(&response.body)
            .map_err(|error| GitHubError::InvalidResponse(error.to_string()))
    }

    /// The PR for `head` into `base` (open, closed or merged), with reviews, checks and
    /// unresolved threads.
    pub async fn get_pr_info(
        &self,
        repo_path: &str,
        head: &str,
        base: &str,
    ) -> Result<PrLookup, GitHubError> {
        let resolved = self.resolve(repo_path).await?;
        let slug = resolved.slug();

        #[derive(Deserialize)]
        struct Pull {
            number: u64,
            title: String,
        }
        let pulls: Vec<Pull> = self
            .fetch_json(resolved.pulls_for_branch(head, base))
            .await?;
        let Some(pull) = pulls.into_iter().next() else {
            return Ok(PrLookup::NoPr {
                create_url: format!("https://github.com/{slug}/compare/{base}...{head}?expand=1"),
            });
        };

        let details = match self.pr_status(&resolved, pull.number).await {
            Ok(status) => PrDetails::Loaded(status),
            Err(reason) => {
                tracing::warn!(pr = pull.number, %reason, "GitHub PR details unavailable");
                PrDetails::Unavailable { reason }
            }
        };
        Ok(PrLookup::Found(PrInfo {
            number: pull.number,
            url: format!("https://github.com/{slug}/pull/{}", pull.number),
            title: pull.title,
            details,
        }))
    }

    async fn pr_status(&self, resolved: &Resolved, number: u64) -> Result<PrStatus, String> {
        let body = serde_json::json!({
            "query": PR_DETAILS_QUERY,
            "variables": { "owner": resolved.owner, "repo": resolved.repo, "prNumber": number },
        });
        let request = HttpRequest {
            method: Method::Post,
            url: format!("{API}/graphql"),
            headers: vec![
                (
                    "Authorization".to_owned(),
                    format!("Bearer {}", resolved.token),
                ),
                ("Content-Type".to_owned(), "application/json".to_owned()),
            ],
            body: Body::Text(body.to_string()),
        };
        let response = self
            .http
            .send(request)
            .await
            .map_err(|error| error.to_string())?;
        let response = ok_response(response).map_err(|error| error.to_string())?;
        graphql::parse_pr_status(&response.body)
    }

    /// Open PRs, most recently updated first (up to 100).
    pub async fn list_open_prs(&self, repo_path: &str) -> Result<Vec<PrListItem>, GitHubError> {
        let resolved = self.resolve(repo_path).await?;

        #[derive(Deserialize)]
        struct User {
            login: String,
        }
        #[derive(Deserialize)]
        struct HeadRepoJson {
            full_name: String,
        }
        #[derive(Deserialize)]
        struct Head {
            #[serde(rename = "ref")]
            ref_name: String,
            repo: Option<HeadRepoJson>,
        }
        #[derive(Deserialize)]
        struct Pull {
            number: u64,
            title: String,
            user: Option<User>,
            head: Head,
        }
        let url = format!(
            "{API}/repos/{}/pulls?state=open&per_page=100&sort=updated&direction=desc",
            resolved.slug()
        );
        let pulls: Vec<Pull> = self.fetch_json(resolved.get(url)).await?;
        let slug = resolved.slug();
        Ok(pulls
            .into_iter()
            .map(|pull| PrListItem {
                number: pull.number,
                title: pull.title,
                author: pull.user.map(|user| user.login).unwrap_or_default(),
                head_ref_name: pull.head.ref_name,
                head_repo: match pull.head.repo {
                    Some(repo) if repo.full_name == slug => HeadRepo::Origin,
                    Some(_) | None => HeadRepo::Fork,
                },
            })
            .collect())
    }

    /// Post `comments` as inline review comments on the branch's PR, anchored to its head
    /// commit. A comment GitHub rejects is reported in `failed`; the rest are still posted.
    pub async fn post_review_comments(
        &self,
        repo_path: &str,
        head: &str,
        base: &str,
        comments: &[ReviewComment],
    ) -> Result<PostCommentsOutcome, GitHubError> {
        if comments.is_empty() {
            return Ok(PostCommentsOutcome {
                posted: 0,
                failed: Vec::new(),
            });
        }
        let resolved = self.resolve(repo_path).await?;

        #[derive(Deserialize)]
        struct Head {
            sha: String,
        }
        #[derive(Deserialize)]
        struct Pull {
            number: u64,
            head: Head,
        }
        let pulls: Vec<Pull> = self
            .fetch_json(resolved.pulls_for_branch(head, base))
            .await?;
        let pull = pulls.into_iter().next().ok_or(GitHubError::NoPrForBranch)?;

        let mut outcome = PostCommentsOutcome {
            posted: 0,
            failed: Vec::new(),
        };
        for comment in comments {
            let side = match comment.side {
                Side::Modified => "RIGHT",
                Side::Original => "LEFT",
            };
            let body = serde_json::json!({
                "body": comment.text,
                "commit_id": pull.head.sha,
                "path": comment.file_path,
                "line": comment.line_number,
                "side": side,
            });
            let request = HttpRequest {
                method: Method::Post,
                url: format!(
                    "{API}/repos/{}/pulls/{}/comments",
                    resolved.slug(),
                    pull.number
                ),
                headers: vec![
                    (
                        "Authorization".to_owned(),
                        format!("Bearer {}", resolved.token),
                    ),
                    ("Accept".to_owned(), ACCEPT.to_owned()),
                    ("Content-Type".to_owned(), "application/json".to_owned()),
                ],
                body: Body::Text(body.to_string()),
            };
            let response = self.http.send(request).await?;
            if response.ok() {
                outcome.posted += 1;
            } else {
                outcome.failed.push(FailedComment {
                    id: comment.id.clone(),
                    error: format!("{} {}", response.status, response.status_text),
                });
            }
        }
        Ok(outcome)
    }
}

fn ok_response(response: HttpResponse) -> Result<HttpResponse, GitHubError> {
    if response.ok() {
        Ok(response)
    } else {
        Err(GitHubError::Api {
            status: response.status,
            status_text: response.status_text,
        })
    }
}

impl GitHubApi for GitHubClient {
    fn get_pr_info(
        &self,
        repo_path: &str,
        head: &str,
        base: &str,
    ) -> BoxFuture<'static, Result<PrLookup, GitHubError>> {
        let (client, repo_path, head, base) = (
            self.clone(),
            repo_path.to_owned(),
            head.to_owned(),
            base.to_owned(),
        );
        async move { client.get_pr_info(&repo_path, &head, &base).await }.boxed()
    }

    fn list_open_prs(
        &self,
        repo_path: &str,
    ) -> BoxFuture<'static, Result<Vec<PrListItem>, GitHubError>> {
        let (client, repo_path) = (self.clone(), repo_path.to_owned());
        async move { client.list_open_prs(&repo_path).await }.boxed()
    }

    fn post_review_comments(
        &self,
        repo_path: &str,
        head: &str,
        base: &str,
        comments: Vec<ReviewComment>,
    ) -> BoxFuture<'static, Result<PostCommentsOutcome, GitHubError>> {
        let (client, repo_path, head, base) = (
            self.clone(),
            repo_path.to_owned(),
            head.to_owned(),
            base.to_owned(),
        );
        async move {
            client
                .post_review_comments(&repo_path, &head, &base, &comments)
                .await
        }
        .boxed()
    }
}
