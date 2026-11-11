//! Port of `store/createReviewCommentStore.test.ts`, case by case.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use futures::FutureExt;
use serde_json::{Value, json};

use super::*;
use crate::review::WorkspaceMetadata;
use crate::fakes::block_on;

type Metadata = Rc<RefCell<BTreeMap<String, String>>>;

enum Blame {
    Lines(HashMap<String, Vec<String>>),
    Fails(String),
    /// Rewrites the comments while blame runs, then answers these lines.
    Rewrites {
        comments: String,
        lines: Vec<String>,
    },
}

struct Deps {
    metadata: Metadata,
    blame: Blame,
    blame_calls: Cell<usize>,
}

impl ReviewCommentDeps for Deps {
    fn get_line_commits(
        &self,
        file_path: &str,
    ) -> LocalBoxFuture<'static, Result<Vec<String>, GitError>> {
        self.blame_calls.set(self.blame_calls.get() + 1);
        let result = match &self.blame {
            Blame::Lines(blames) => Ok(blames.get(file_path).cloned().unwrap_or_default()),
            Blame::Fails(message) => Err(GitError::PullFailed(message.clone())),
            Blame::Rewrites { comments, lines } => {
                self.update_metadata(REVIEW_COMMENTS_KEY, comments.clone(), "test");
                Ok(lines.clone())
            }
        };
        async move { result }.boxed_local()
    }
}

impl WorkspaceMetadata for Deps {
    fn metadata_value(&self, key: &str) -> Option<String> {
        self.metadata.borrow().get(key).cloned()
    }

    fn update_metadata(&self, key: &str, value: String, _reason: &str) {
        self.metadata.borrow_mut().insert(key.to_owned(), value);
    }
}

fn make_deps(comments: Option<Value>, blames: &[(&str, &[&str])]) -> Deps {
    let metadata: BTreeMap<String, String> = comments
        .map(|comments| (REVIEW_COMMENTS_KEY.to_owned(), comments.to_string()))
        .into_iter()
        .collect();
    Deps {
        metadata: Rc::new(RefCell::new(metadata)),
        blame: Blame::Lines(
            blames
                .iter()
                .map(|(path, lines)| {
                    (
                        (*path).to_owned(),
                        lines.iter().map(|line| (*line).to_owned()).collect(),
                    )
                })
                .collect(),
        ),
        blame_calls: Cell::new(0),
    }
}

fn store(deps: Deps) -> ReviewCommentStore<Deps> {
    ReviewCommentStore::new(deps)
}

fn stored(deps: &Deps) -> Value {
    serde_json::from_str(
        &deps
            .metadata_value(REVIEW_COMMENTS_KEY)
            .expect("comments were written"),
    )
    .unwrap()
}

/// A stored comment as the TS tests' `make()` builds it, with `over` applied.
fn make(over: Value) -> Value {
    let mut comment = json!({
        "id": "c", "filePath": "a.ts", "lineNumber": 1, "text": "T", "commitHash": "old-head",
        "lineCommitHash": "L1", "createdAt": 1, "isOutdated": false, "addressed": false, "side": "modified",
    });
    for (key, value) in over.as_object().unwrap() {
        comment[key] = value.clone();
    }
    comment
}

fn outdated_flags(deps: &Deps) -> BTreeMap<String, bool> {
    stored(deps)
        .as_array()
        .unwrap()
        .iter()
        .map(|comment| {
            (
                comment["id"].as_str().unwrap().to_owned(),
                comment["isOutdated"].as_bool().unwrap(),
            )
        })
        .collect()
}

fn new_comment(side: Side, commit: CommitAnchor) -> NewReviewComment {
    NewReviewComment {
        file_path: "a.ts".to_owned(),
        line_number: 2,
        text: "T".to_owned(),
        commit,
        side,
    }
}

fn head() -> CommitAnchor {
    CommitAnchor::Head("head".to_owned())
}

mod parse_review_comments {
    use super::*;

    #[test]
    fn returns_empty_array_when_no_review_comments_key() {
        assert!(ReviewComments::parse(None).unwrap().is_empty());
        assert!(ReviewComments::parse(Some("")).unwrap().is_empty());
    }

    /// TS: "returns empty array for invalid JSON". The native app fails loudly instead, so
    /// the next write cannot silently replace what was stored.
    #[test]
    fn rejects_invalid_json() {
        let error = ReviewComments::parse(Some("not-json{{")).unwrap_err();
        assert_eq!(error.key, REVIEW_COMMENTS_KEY);
    }

    #[test]
    fn parses_valid_json_comments_defaulting_a_missing_line_commit_hash_to_null() {
        let mut legacy = make(json!({ "id": "c1" }));
        legacy.as_object_mut().unwrap().remove("lineCommitHash");
        let tracked = make(json!({ "id": "c2", "lineCommitHash": "L1" }));
        let comments = ReviewComments::parse(Some(&json!([legacy, tracked]).to_string()))
            .unwrap()
            .comments();
        let lines: Vec<_> = comments
            .iter()
            .map(|comment| comment.line.clone())
            .collect();
        assert_eq!(
            lines,
            vec![LineAnchor::Untracked, LineAnchor::Blamed("L1".to_owned())]
        );
    }

    #[test]
    fn a_comment_missing_addressed_reads_as_open() {
        let mut comment = make(json!({}));
        comment.as_object_mut().unwrap().remove("addressed");
        let comments = ReviewComments::parse(Some(&json!([comment]).to_string()))
            .unwrap()
            .comments();
        assert_eq!(comments[0].resolution, Resolution::Open);
    }
}

mod create_review_comment_store {
    use super::*;

    #[test]
    fn get_review_comments_returns_parsed_comments_from_metadata() {
        let deps = make_deps(
            Some(json!([make(json!({ "id": "c1", "commitHash": "h1" }))])),
            &[],
        );
        let comments = store(deps).get_review_comments().unwrap();
        assert_eq!(
            comments,
            vec![ReviewComment {
                id: "c1".to_owned(),
                file_path: "a.ts".to_owned(),
                line_number: 1,
                text: "T".to_owned(),
                commit: CommitAnchor::Head("h1".to_owned()),
                line: LineAnchor::Blamed("L1".to_owned()),
                created_at: 1,
                freshness: Freshness::Current,
                resolution: Resolution::Open,
                side: Side::Modified,
            }]
        );
    }

    #[test]
    fn get_review_comments_returns_empty_when_no_metadata() {
        assert_eq!(
            store(make_deps(None, &[])).get_review_comments().unwrap(),
            vec![]
        );
    }

    #[test]
    fn mark_review_comments_addressed_marks_only_the_given_ids() {
        let deps = make_deps(
            Some(json!([
                make(json!({ "id": "c1" })),
                make(json!({ "id": "c2" }))
            ])),
            &[],
        );
        let store = store(deps);
        store
            .mark_review_comments_addressed(&["c1".to_owned()])
            .unwrap();
        let result = stored(store.deps());
        assert_eq!(result[0]["addressed"], json!(true));
        assert_eq!(result[1]["addressed"], json!(false));
    }

    #[test]
    fn mark_review_comments_addressed_with_empty_ids_is_a_no_op() {
        let deps = make_deps(Some(json!([make(json!({ "id": "c1" }))])), &[]);
        let before = deps.metadata_value(REVIEW_COMMENTS_KEY);
        let store = store(deps);
        store.mark_review_comments_addressed(&[]).unwrap();
        assert_eq!(store.deps().metadata_value(REVIEW_COMMENTS_KEY), before);
    }

    #[test]
    fn toggle_and_delete_rewrite_only_what_they_touch() {
        let deps = make_deps(
            Some(json!([
                make(json!({ "id": "c1" })),
                make(json!({ "id": "c2" }))
            ])),
            &[],
        );
        let store = store(deps);
        store.toggle_review_comment_addressed("c1").unwrap();
        assert_eq!(stored(store.deps())[0]["addressed"], json!(true));
        store.toggle_review_comment_addressed("c1").unwrap();
        assert_eq!(stored(store.deps())[0]["addressed"], json!(false));
        store.delete_review_comment("c1").unwrap();
        assert_eq!(stored(store.deps()), json!([make(json!({ "id": "c2" }))]));
        store.clear_review_comments();
        assert_eq!(
            store.deps().metadata_value(REVIEW_COMMENTS_KEY).as_deref(),
            Some("[]")
        );
    }

    #[test]
    fn operations_on_invalid_metadata_fail_and_write_nothing() {
        let deps = make_deps(None, &[]);
        deps.update_metadata(REVIEW_COMMENTS_KEY, "{oops".to_owned(), "test");
        let store = store(deps);
        assert!(store.delete_review_comment("c1").is_err());
        assert!(block_on(store.add_review_comment(new_comment(Side::Original, head()))).is_err());
        assert_eq!(
            store.deps().metadata_value(REVIEW_COMMENTS_KEY).as_deref(),
            Some("{oops")
        );
    }

    mod add_review_comment {
        use super::*;

        #[test]
        fn records_the_blamed_commit_of_a_modified_side_line() {
            let store = store(make_deps(None, &[("a.ts", &["L1", "L2"])]));
            block_on(store.add_review_comment(new_comment(Side::Modified, head()))).unwrap();
            assert_eq!(
                store.get_review_comments().unwrap()[0].line,
                LineAnchor::Blamed("L2".to_owned())
            );
        }

        #[test]
        fn records_null_when_the_line_is_beyond_the_blamed_file() {
            let store = store(make_deps(None, &[("a.ts", &["L1"])]));
            block_on(store.add_review_comment(new_comment(Side::Modified, head()))).unwrap();
            assert_eq!(
                store.get_review_comments().unwrap()[0].line,
                LineAnchor::Untracked
            );
        }

        #[test]
        fn does_not_blame_original_side_or_non_git_comments() {
            let store = store(make_deps(None, &[]));
            block_on(store.add_review_comment(new_comment(Side::Original, head()))).unwrap();
            block_on(
                store.add_review_comment(new_comment(Side::Modified, CommitAnchor::NotInRepo)),
            )
            .unwrap();
            assert_eq!(store.deps().blame_calls.get(), 0);
            let lines: Vec<_> = store
                .get_review_comments()
                .unwrap()
                .into_iter()
                .map(|comment| comment.line)
                .collect();
            assert_eq!(lines, vec![LineAnchor::Untracked, LineAnchor::Untracked]);
        }

        #[test]
        fn throws_and_adds_nothing_when_blame_fails() {
            let mut deps = make_deps(None, &[]);
            deps.blame = Blame::Fails("boom".to_owned());
            let store = store(deps);
            let error = block_on(store.add_review_comment(new_comment(Side::Modified, head())))
                .unwrap_err();
            assert_eq!(error.to_string(), "Failed to blame a.ts: boom");
            assert_eq!(store.get_review_comments().unwrap(), vec![]);
        }

        #[test]
        fn writes_new_comments_in_electrons_key_order() {
            let store = store(make_deps(None, &[("a.ts", &["L1", "L2"])]));
            block_on(store.add_review_comment(new_comment(Side::Modified, head()))).unwrap();
            let comment = &store.get_review_comments().unwrap()[0];
            assert_eq!(
                store.deps().metadata_value(REVIEW_COMMENTS_KEY).unwrap(),
                format!(
                    r#"[{{"filePath":"a.ts","lineNumber":2,"text":"T","commitHash":"head","isOutdated":false,"addressed":false,"side":"modified","lineCommitHash":"L2","id":"{}","createdAt":{}}}]"#,
                    comment.id, comment.created_at
                )
            );
            assert!(comment.created_at > 1_700_000_000_000);
            assert_eq!(comment.id.len(), 36);
        }

        #[test]
        fn keeps_existing_comments_byte_for_byte() {
            // A legacy comment (no lineCommitHash, no addressed) with a field this app does
            // not know: Electron appends `lineCommitHash: null` on read and keeps the rest.
            let legacy = r#"{"id":"old","filePath":"b.ts","lineNumber":3,"text":"x","commitHash":null,"createdAt":5,"isOutdated":false,"side":"original","extra":[1,{"k":2}]}"#;
            let deps = make_deps(None, &[]);
            deps.update_metadata(REVIEW_COMMENTS_KEY, format!("[{legacy}]"), "test");
            let store = store(deps);
            block_on(store.add_review_comment(new_comment(Side::Original, head()))).unwrap();
            let written = store.deps().metadata_value(REVIEW_COMMENTS_KEY).unwrap();
            let expected_legacy =
                legacy.replace(r#"[1,{"k":2}]}"#, r#"[1,{"k":2}],"lineCommitHash":null}"#);
            assert!(
                written.starts_with(&format!("[{expected_legacy},")),
                "{written}"
            );
        }
    }

    mod update_outdated_review_comments {
        use super::*;

        fn run(deps: Deps, head: &str) -> (Deps, Result<(), ReviewCommentError>) {
            let store = store(deps);
            let result = block_on(store.update_outdated_review_comments(head));
            (store.deps, result)
        }

        #[test]
        fn is_a_no_op_without_comments() {
            let (deps, result) = run(make_deps(None, &[]), "head");
            result.unwrap();
            assert_eq!(deps.metadata_value(REVIEW_COMMENTS_KEY), None);
        }

        #[test]
        fn keeps_a_comment_current_across_new_commits_while_its_line_is_unchanged() {
            let (deps, result) = run(
                make_deps(
                    Some(json!([make(json!({ "id": "c1" }))])),
                    &[("a.ts", &["L1"])],
                ),
                "new-head",
            );
            result.unwrap();
            assert_eq!(
                outdated_flags(&deps),
                BTreeMap::from([("c1".to_owned(), false)])
            );
        }

        #[test]
        fn marks_outdated_when_the_line_blame_changed_or_the_line_is_gone_and_clears_when_restored()
        {
            let comments = json!([
                make(json!({ "id": "changed", "lineNumber": 1 })),
                make(json!({ "id": "gone", "lineNumber": 5 })),
                make(
                    json!({ "id": "restored", "filePath": "b.ts", "lineCommitHash": "B1", "isOutdated": true })
                ),
            ]);
            let (deps, result) = run(
                make_deps(Some(comments), &[("a.ts", &["L9"]), ("b.ts", &["B1"])]),
                "new-head",
            );
            result.unwrap();
            assert_eq!(
                outdated_flags(&deps),
                BTreeMap::from([
                    ("changed".to_owned(), true),
                    ("gone".to_owned(), true),
                    ("restored".to_owned(), false)
                ])
            );
        }

        #[test]
        fn falls_back_to_head_comparison_for_untracked_comments() {
            let comments = json!([
                make(json!({ "id": "stale", "lineCommitHash": null, "commitHash": "old-head" })),
                make(json!({ "id": "fresh", "lineCommitHash": null, "commitHash": "new-head" })),
                make(json!({ "id": "nogit", "lineCommitHash": null, "commitHash": null })),
            ]);
            let (deps, result) = run(make_deps(Some(comments), &[]), "new-head");
            result.unwrap();
            assert_eq!(
                outdated_flags(&deps),
                BTreeMap::from([
                    ("stale".to_owned(), true),
                    ("fresh".to_owned(), false),
                    ("nogit".to_owned(), false)
                ])
            );
            assert_eq!(deps.blame_calls.get(), 0);
        }

        #[test]
        fn blames_each_file_once() {
            let comments = json!([
                make(json!({ "id": "c1" })),
                make(json!({ "id": "c2", "lineNumber": 2 }))
            ]);
            let (deps, result) = run(
                make_deps(Some(comments), &[("a.ts", &["L1", "L2"])]),
                "new-head",
            );
            result.unwrap();
            assert_eq!(deps.blame_calls.get(), 1);
        }

        #[test]
        fn applies_results_to_the_latest_list_keeping_comments_added_during_blame() {
            let mut deps = make_deps(Some(json!([make(json!({ "id": "c1" }))])), &[]);
            deps.blame = Blame::Rewrites {
                comments: json!([make(json!({ "id": "c1" })), make(json!({ "id": "added" }))])
                    .to_string(),
                lines: vec!["L2".to_owned()],
            };
            let (deps, result) = run(deps, "new-head");
            result.unwrap();
            assert_eq!(
                outdated_flags(&deps),
                BTreeMap::from([("c1".to_owned(), true), ("added".to_owned(), false)])
            );
        }

        #[test]
        fn throws_when_blame_fails() {
            let mut deps = make_deps(Some(json!([make(json!({ "id": "c1" }))])), &[]);
            deps.blame = Blame::Fails("boom".to_owned());
            let (_, result) = run(deps, "h");
            assert!(result.unwrap_err().to_string().contains("boom"));
        }
    }
}
