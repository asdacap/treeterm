//! Port of `lib/githubClient.test.ts`, case by case. `vi.spyOn(globalThis, 'fetch')` becomes
//! the scripted [`FakeHttp`]; the exec mock becomes [`FakeExec`].

use serde_json::json;

use super::*;
use crate::fakes::{
    FakeExec, FakeHttp, block_on, exits, json_response, start_fails, status_response,
};
use crate::review::{CommitAnchor, Freshness, LineAnchor, Resolution};

fn settings(autodetect_via_gh: bool, pat: &str) -> Arc<dyn GithubSettingsSource> {
    let settings = GithubSettings {
        pat: pat.to_owned(),
        autodetect_via_gh,
        ..GithubSettings::default()
    };
    Arc::new(move || settings.clone())
}

fn gh() -> Arc<dyn GithubSettingsSource> {
    settings(true, "")
}

/// `gh auth token` then `git remote get-url origin` succeed (`resolveTokenAndRemote`).
fn resolving_exec() -> Arc<FakeExec> {
    FakeExec::new(vec![
        exits("ghp_token123\n", 0),
        exits("git@github.com:owner/repo.git\n", 0),
    ])
}

fn client(
    exec: &Arc<FakeExec>,
    http: &Arc<FakeHttp>,
    settings: Arc<dyn GithubSettingsSource>,
) -> GitHubClient {
    GitHubClient::new(exec.clone(), http.clone(), settings)
}

fn pr_info(lookup: Result<PrLookup, GitHubError>) -> PrInfo {
    match lookup {
        Ok(PrLookup::Found(info)) => info,
        other => panic!("expected prInfo, got {other:?}"),
    }
}

fn loaded(info: &PrInfo) -> &PrStatus {
    match &info.details {
        PrDetails::Loaded(status) => status,
        PrDetails::Unavailable { reason } => panic!("details unavailable: {reason}"),
    }
}

mod parse_github_owner_repo {
    use super::*;

    fn owner_repo(owner: &str, repo: &str) -> Option<OwnerRepo> {
        Some(OwnerRepo {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
        })
    }

    #[test]
    fn parses_ssh_url() {
        assert_eq!(
            parse_github_owner_repo("git@github.com:owner/repo.git"),
            owner_repo("owner", "repo")
        );
    }

    #[test]
    fn parses_ssh_url_without_git() {
        assert_eq!(
            parse_github_owner_repo("git@github.com:owner/repo"),
            owner_repo("owner", "repo")
        );
    }

    #[test]
    fn parses_https_url() {
        assert_eq!(
            parse_github_owner_repo("https://github.com/owner/repo.git"),
            owner_repo("owner", "repo")
        );
    }

    #[test]
    fn parses_https_url_without_git() {
        assert_eq!(
            parse_github_owner_repo("https://github.com/owner/repo"),
            owner_repo("owner", "repo")
        );
    }

    #[test]
    fn returns_null_for_non_github_url() {
        assert_eq!(
            parse_github_owner_repo("https://gitlab.com/owner/repo.git"),
            None
        );
    }

    #[test]
    fn returns_null_for_invalid_url() {
        assert_eq!(parse_github_owner_repo("not-a-url"), None);
    }
}

mod create_github_api {
    use super::*;

    #[test]
    fn returns_error_when_gh_auth_token_fails() {
        let exec = FakeExec::new(vec![exits("", 1)]);
        let http = FakeHttp::new(vec![]);
        let result = block_on(client(&exec, &http, gh()).get_pr_info("/repo", "feature", "main"));
        assert_eq!(
            result.unwrap_err().to_string(),
            "Failed to get token from gh CLI. Is gh installed and authenticated?"
        );
        assert_eq!(exec.call(1).command, "gh");
        assert_eq!(exec.call(1).args, vec!["auth", "token"]);
        assert_eq!(exec.call(1).cwd, "/repo");
    }

    #[test]
    fn returns_error_when_no_pat_configured_and_autodetect_disabled() {
        let exec = FakeExec::new(vec![]);
        let http = FakeHttp::new(vec![]);
        let result = block_on(
            client(&exec, &http, settings(false, "")).get_pr_info("/repo", "feature", "main"),
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "No GitHub PAT configured. Set one in Settings > GitHub."
        );
    }

    #[test]
    fn returns_error_when_git_remote_fails() {
        let exec = FakeExec::new(vec![exits("ghp_token123\n", 0), exits("", 1)]);
        let http = FakeHttp::new(vec![]);
        let result = block_on(client(&exec, &http, gh()).get_pr_info("/repo", "feature", "main"));
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to get remote URL")
        );
    }

    #[test]
    fn returns_no_pr_result_when_no_open_prs_found() {
        let exec = resolving_exec();
        let http = FakeHttp::new(vec![json_response(200, "[]")]);
        let result = block_on(client(&exec, &http, gh()).get_pr_info("/repo", "feature", "main"));
        assert_eq!(
            result.unwrap(),
            PrLookup::NoPr {
                create_url: "https://github.com/owner/repo/compare/main...feature?expand=1"
                    .to_owned()
            }
        );
    }

    #[test]
    fn queries_with_state_all_so_merged_prs_are_still_discovered() {
        let exec = resolving_exec();
        let http = FakeHttp::new(vec![json_response(200, "[]")]);
        block_on(client(&exec, &http, gh()).get_pr_info("/repo", "feature", "main")).unwrap();
        let request = &http.requests()[0];
        assert!(request.url.contains("state=all"));
        assert!(!request.url.contains("state=open"));
        assert_eq!(
            request.url,
            "https://api.github.com/repos/owner/repo/pulls?head=owner%3Afeature&base=main&state=all"
        );
        assert_eq!(request.header("Authorization"), Some("Bearer ghp_token123"));
        assert_eq!(
            request.header("Accept"),
            Some("application/vnd.github+json")
        );
    }

    #[test]
    fn reports_merged_state_when_graphql_returns_a_merged_pr() {
        let exec = resolving_exec();
        let graphql = json!({ "data": { "repository": { "pullRequest": {
            "state": "MERGED",
            "reviewThreads": { "nodes": [] },
            "latestReviews": { "nodes": [] },
            "commits": { "nodes": [{ "commit": { "statusCheckRollup": { "contexts": { "nodes": [] } } } }] },
        } } } });
        let http = FakeHttp::new(vec![
            json_response(
                200,
                &json!([{ "number": 7, "title": "Merged PR" }]).to_string(),
            ),
            json_response(200, &graphql.to_string()),
        ]);
        let info = pr_info(block_on(
            client(&exec, &http, gh()).get_pr_info("/repo", "feature", "main"),
        ));
        assert_eq!(loaded(&info).state, PrState::Merged);
        assert_eq!(info.number, 7);
    }

    #[test]
    fn returns_full_pr_info_on_happy_path_with_graphql() {
        let exec = resolving_exec();
        let graphql = json!({ "data": { "repository": { "pullRequest": {
            "state": "OPEN",
            "reviewThreads": { "nodes": [
                { "isResolved": false, "comments": { "nodes": [{ "body": "fix this", "path": "src/a.ts", "line": 10, "author": { "login": "reviewer" } }] } },
                { "isResolved": true, "comments": { "nodes": [{ "body": "resolved", "path": "src/b.ts", "line": 5, "author": { "login": "other" } }] } },
            ] },
            "latestReviews": { "nodes": [{ "author": { "login": "reviewer" }, "state": "CHANGES_REQUESTED" }] },
            "commits": { "nodes": [{ "commit": { "statusCheckRollup": { "contexts": { "nodes": [
                { "__typename": "CheckRun", "name": "CI", "status": "COMPLETED", "conclusion": "SUCCESS" },
                { "__typename": "StatusContext", "context": "deploy", "state": "success" },
            ] } } } }] },
        } } } });
        let http = FakeHttp::new(vec![
            json_response(
                200,
                &json!([{ "number": 42, "title": "My PR" }]).to_string(),
            ),
            json_response(200, &graphql.to_string()),
        ]);
        let info = pr_info(block_on(
            client(&exec, &http, gh()).get_pr_info("/repo", "feature", "main"),
        ));
        assert_eq!(info.number, 42);
        assert_eq!(info.url, "https://github.com/owner/repo/pull/42");
        assert_eq!(info.title, "My PR");
        let status = loaded(&info);
        assert_eq!(status.state, PrState::Open);
        assert_eq!(
            status.reviews,
            vec![Review {
                author: "reviewer".to_owned(),
                state: ReviewState::ChangesRequested
            }]
        );
        assert_eq!(
            status.check_runs,
            vec![CheckRun {
                name: "CI".to_owned(),
                status: CheckStatus::Completed,
                conclusion: CheckConclusion::Success
            }]
        );
        assert_eq!(
            status.unresolved_threads,
            vec![ReviewThread {
                path: "src/a.ts".to_owned(),
                body: "fix this".to_owned(),
                author: "reviewer".to_owned(),
                line: ThreadLine::Line(10),
            }]
        );
        assert_eq!(info.unresolved_count(), 1);

        let graphql_request = &http.requests()[1];
        assert_eq!(graphql_request.url, "https://api.github.com/graphql");
        assert_eq!(graphql_request.method, Method::Post);
        let Body::Text(body) = &graphql_request.body else {
            panic!("GraphQL request without a body")
        };
        let body: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(
            body["variables"],
            json!({ "owner": "owner", "repo": "repo", "prNumber": 42 })
        );
        assert_eq!(body["query"], PR_DETAILS_QUERY);
    }

    /// TS degraded to an `OPEN` PR with no reviews or checks; the details are now reported
    /// as unavailable, with the reason.
    #[test]
    fn returns_basic_pr_info_when_graphql_returns_non_ok() {
        let exec = resolving_exec();
        let http = FakeHttp::new(vec![
            json_response(200, &json!([{ "number": 1, "title": "PR" }]).to_string()),
            status_response(500, "Internal Server Error"),
        ]);
        let info = pr_info(block_on(
            client(&exec, &http, gh()).get_pr_info("/repo", "feature", "main"),
        ));
        assert_eq!(info.number, 1);
        assert_eq!(
            info.details,
            PrDetails::Unavailable {
                reason: "GitHub API error: 500 Internal Server Error".to_owned()
            }
        );
        assert_eq!(info.unresolved_count(), 0);
    }

    #[test]
    fn returns_basic_pr_info_when_graphql_fetch_throws() {
        let exec = resolving_exec();
        let http = FakeHttp::new(vec![
            json_response(200, &json!([{ "number": 1, "title": "PR" }]).to_string()),
            Err(HttpError("network error".to_owned())),
        ]);
        let info = pr_info(block_on(
            client(&exec, &http, gh()).get_pr_info("/repo", "feature", "main"),
        ));
        assert_eq!(info.number, 1);
        assert_eq!(info.unresolved_count(), 0);
        assert_eq!(
            info.details,
            PrDetails::Unavailable {
                reason: "network error".to_owned()
            }
        );
    }

    #[test]
    fn graphql_errors_make_the_details_unavailable_with_their_messages() {
        let exec = resolving_exec();
        let http = FakeHttp::new(vec![
            json_response(200, &json!([{ "number": 1, "title": "PR" }]).to_string()),
            json_response(
                200,
                &json!({ "data": null, "errors": [{ "message": "Bad credentials" }] }).to_string(),
            ),
        ]);
        let info = pr_info(block_on(
            client(&exec, &http, gh()).get_pr_info("/repo", "feature", "main"),
        ));
        assert_eq!(
            info.details,
            PrDetails::Unavailable {
                reason: "Bad credentials".to_owned()
            }
        );
    }

    #[test]
    fn missing_graphql_fields_default_as_in_electron() {
        let exec = resolving_exec();
        let graphql = json!({ "data": { "repository": { "pullRequest": {
            "reviewThreads": { "nodes": [null, { "isResolved": false, "comments": { "nodes": [{ "body": "b", "path": "p", "line": null, "author": null }] } }] },
            "latestReviews": { "nodes": [{ "author": null, "state": "SOMETHING_NEW" }] },
            "commits": { "nodes": [{ "commit": { "statusCheckRollup": { "contexts": { "nodes": [
                { "__typename": "CheckRun", "name": "CI", "status": null, "conclusion": null },
            ] } } } }] },
        } } } });
        let http = FakeHttp::new(vec![
            json_response(200, &json!([{ "number": 3, "title": "PR" }]).to_string()),
            json_response(200, &graphql.to_string()),
        ]);
        let info = pr_info(block_on(
            client(&exec, &http, gh()).get_pr_info("/repo", "feature", "main"),
        ));
        let status = loaded(&info);
        assert_eq!(status.state, PrState::Open);
        assert_eq!(status.unresolved_threads[0].author, "");
        assert_eq!(status.unresolved_threads[0].line, ThreadLine::None);
        assert_eq!(
            status.reviews[0].state,
            ReviewState::Other("SOMETHING_NEW".to_owned())
        );
        assert_eq!(status.check_runs[0].status, CheckStatus::Queued);
        assert_eq!(status.check_runs[0].conclusion, CheckConclusion::NotYet);
    }

    #[test]
    fn returns_error_when_rest_api_returns_non_ok() {
        let exec = resolving_exec();
        let http = FakeHttp::new(vec![status_response(403, "Forbidden")]);
        let result = block_on(client(&exec, &http, gh()).get_pr_info("/repo", "feature", "main"));
        assert_eq!(
            result.unwrap_err().to_string(),
            "GitHub API error: 403 Forbidden"
        );
    }

    #[test]
    fn uses_pat_when_autodetect_is_disabled() {
        let exec = FakeExec::new(vec![exits("git@github.com:owner/repo.git\n", 0)]);
        let http = FakeHttp::new(vec![json_response(200, "[]")]);
        let result = block_on(
            client(&exec, &http, settings(false, "ghp_pat123"))
                .get_pr_info("/repo", "feature", "main"),
        );
        assert!(matches!(result, Ok(PrLookup::NoPr { .. })));
        assert_eq!(exec.calls().len(), 1);
        assert_eq!(exec.call(1).args, vec!["remote", "get-url", "origin"]);
        assert_eq!(
            http.requests()[0].header("Authorization"),
            Some("Bearer ghp_pat123")
        );
    }

    #[test]
    fn returns_error_when_remote_url_is_not_from_github() {
        let exec = FakeExec::new(vec![
            exits("ghp_token123\n", 0),
            exits("https://gitlab.com/owner/repo.git\n", 0),
        ]);
        let http = FakeHttp::new(vec![]);
        let result = block_on(client(&exec, &http, gh()).get_pr_info("/repo", "feature", "main"));
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Could not parse GitHub owner/repo")
        );
    }

    #[test]
    fn returns_error_when_exec_start_fails() {
        let exec = FakeExec::new(vec![start_fails("exec failed")]);
        let http = FakeHttp::new(vec![]);
        let result = block_on(client(&exec, &http, gh()).get_pr_info("/repo", "feature", "main"));
        let error = result.unwrap_err();
        assert!(matches!(error, GitHubError::Exec(_)));
        assert!(error.to_string().contains("exec failed"));
    }

    fn make_comment(id: &str, side: Side, file_path: &str, line_number: u32) -> ReviewComment {
        ReviewComment {
            id: id.to_owned(),
            file_path: file_path.to_owned(),
            line_number,
            text: "fix this".to_owned(),
            commit: CommitAnchor::NotInRepo,
            line: LineAnchor::Untracked,
            created_at: 1,
            freshness: Freshness::Current,
            resolution: Resolution::Open,
            side,
        }
    }

    #[test]
    fn post_review_comments_posts_each_comment_maps_side_anchors_to_head_sha() {
        let exec = resolving_exec();
        let http = FakeHttp::new(vec![
            json_response(
                200,
                &json!([{ "number": 42, "head": { "sha": "sha123" } }]).to_string(),
            ),
            json_response(201, "{}"),
            json_response(201, "{}"),
        ]);
        let comments = [
            make_comment("c1", Side::Modified, "src/a.ts", 10),
            make_comment("c2", Side::Original, "src/b.ts", 5),
        ];
        let result = block_on(
            client(&exec, &http, gh()).post_review_comments("/repo", "feature", "main", &comments),
        );
        assert_eq!(
            result.unwrap(),
            PostCommentsOutcome {
                posted: 2,
                failed: vec![]
            }
        );

        let body = |n: usize| -> serde_json::Value {
            let Body::Text(body) = &http.requests()[n].body else {
                panic!("comment request without a body")
            };
            serde_json::from_str(body).unwrap()
        };
        let first = body(1);
        assert_eq!(first["side"], "RIGHT");
        assert_eq!(first["commit_id"], "sha123");
        assert_eq!(first["path"], "src/a.ts");
        assert_eq!(first["line"], 10);
        assert_eq!(first["body"], "fix this");
        assert_eq!(body(2)["side"], "LEFT");
        assert_eq!(
            http.requests()[1].url,
            "https://api.github.com/repos/owner/repo/pulls/42/comments"
        );
        assert_eq!(
            http.requests()[1].header("Content-Type"),
            Some("application/json")
        );
    }

    #[test]
    fn post_review_comments_records_per_comment_failures_without_aborting() {
        let exec = resolving_exec();
        let http = FakeHttp::new(vec![
            json_response(
                200,
                &json!([{ "number": 42, "head": { "sha": "sha123" } }]).to_string(),
            ),
            json_response(201, "{}"),
            status_response(422, "Unprocessable Entity"),
        ]);
        let comments = [
            make_comment("c1", Side::Modified, "src/a.ts", 10),
            make_comment("c2", Side::Modified, "src/a.ts", 10),
        ];
        let result = block_on(
            client(&exec, &http, gh()).post_review_comments("/repo", "feature", "main", &comments),
        );
        assert_eq!(
            result.unwrap(),
            PostCommentsOutcome {
                posted: 1,
                failed: vec![FailedComment {
                    id: "c2".to_owned(),
                    error: "422 Unprocessable Entity".to_owned()
                }]
            }
        );
    }

    #[test]
    fn post_review_comments_returns_error_when_no_pr_exists() {
        let exec = resolving_exec();
        let http = FakeHttp::new(vec![json_response(200, "[]")]);
        let comments = [make_comment("c1", Side::Modified, "src/a.ts", 10)];
        let result = block_on(
            client(&exec, &http, gh()).post_review_comments("/repo", "feature", "main", &comments),
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "No open PR found for this branch"
        );
    }

    #[test]
    fn post_review_comments_returns_posted_0_for_empty_comment_list_without_hitting_the_api() {
        let exec = FakeExec::new(vec![]);
        let http = FakeHttp::new(vec![]);
        let result = block_on(client(&exec, &http, gh()).post_review_comments(
            "/repo",
            "feature",
            "main",
            &[],
        ));
        assert_eq!(
            result.unwrap(),
            PostCommentsOutcome {
                posted: 0,
                failed: vec![]
            }
        );
        assert!(exec.calls().is_empty());
        assert!(http.requests().is_empty());
    }

    #[test]
    fn list_open_prs_maps_open_prs_and_flags_fork_prs_as_cross_repo() {
        let exec = resolving_exec();
        let http = FakeHttp::new(vec![json_response(
            200,
            &json!([
                { "number": 12, "title": "Add login", "user": { "login": "alice" }, "head": { "ref": "feat/login", "repo": { "full_name": "owner/repo" } } },
                { "number": 7, "title": "Fork patch", "user": { "login": "bob" }, "head": { "ref": "patch-1", "repo": { "full_name": "bob/repo" } } },
                { "number": 3, "title": "Gone fork", "user": null, "head": { "ref": "old", "repo": null } },
            ])
            .to_string(),
        )]);
        let result = block_on(client(&exec, &http, gh()).list_open_prs("/repo"));
        assert_eq!(
            result.unwrap(),
            vec![
                PrListItem {
                    number: 12,
                    title: "Add login".to_owned(),
                    author: "alice".to_owned(),
                    head_ref_name: "feat/login".to_owned(),
                    head_repo: HeadRepo::Origin,
                },
                PrListItem {
                    number: 7,
                    title: "Fork patch".to_owned(),
                    author: "bob".to_owned(),
                    head_ref_name: "patch-1".to_owned(),
                    head_repo: HeadRepo::Fork,
                },
                PrListItem {
                    number: 3,
                    title: "Gone fork".to_owned(),
                    author: String::new(),
                    head_ref_name: "old".to_owned(),
                    head_repo: HeadRepo::Fork,
                },
            ]
        );
        assert!(http.requests()[0].url.contains("state=open"));
    }

    #[test]
    fn list_open_prs_returns_error_on_non_ok_response() {
        let exec = resolving_exec();
        let http = FakeHttp::new(vec![status_response(403, "Forbidden")]);
        let result = block_on(client(&exec, &http, gh()).list_open_prs("/repo"));
        assert_eq!(
            result.unwrap_err().to_string(),
            "GitHub API error: 403 Forbidden"
        );
    }

    #[test]
    fn list_open_prs_propagates_token_resolution_errors() {
        let exec = FakeExec::new(vec![]);
        let http = FakeHttp::new(vec![]);
        let result = block_on(client(&exec, &http, settings(false, "")).list_open_prs("/repo"));
        assert_eq!(
            result.unwrap_err().to_string(),
            "No GitHub PAT configured. Set one in Settings > GitHub."
        );
    }

    #[test]
    fn a_body_that_is_not_json_is_an_invalid_response() {
        let exec = resolving_exec();
        let http = FakeHttp::new(vec![json_response(200, "<html>")]);
        let result = block_on(client(&exec, &http, gh()).list_open_prs("/repo"));
        assert!(matches!(result, Err(GitHubError::InvalidResponse(_))));
    }

    #[test]
    fn the_trait_object_delegates_to_the_client() {
        let exec = resolving_exec();
        let http = FakeHttp::new(vec![json_response(200, "[]")]);
        let api: Arc<dyn GitHubApi> = Arc::new(client(&exec, &http, gh()));
        assert!(matches!(
            block_on(api.get_pr_info("/repo", "feature", "main")),
            Ok(PrLookup::NoPr { .. })
        ));
    }
}
