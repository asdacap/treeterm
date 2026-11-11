//! Port of `store/createReviewViewedFilesStore.test.ts`, case by case. The TS helper `file()`
//! builds untagged text stats; here every file carries `FileChangeStats`, so new marks are
//! always written tagged (`{"kind":"text",...}`), which Electron reads the same way.

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::*;

#[derive(Default)]
struct Deps {
    metadata: RefCell<BTreeMap<String, String>>,
    updates: RefCell<Vec<(String, String)>>,
}

impl WorkspaceMetadata for Deps {
    fn metadata_value(&self, key: &str) -> Option<String> {
        self.metadata.borrow().get(key).cloned()
    }

    fn update_metadata(&self, key: &str, value: String, reason: &str) {
        self.updates
            .borrow_mut()
            .push((key.to_owned(), reason.to_owned()));
        self.metadata.borrow_mut().insert(key.to_owned(), value);
    }
}

fn setup(initial: Value) -> ViewedFilesStore<Deps> {
    let deps = Deps::default();
    if initial.as_object().is_some_and(|object| !object.is_empty()) {
        deps.metadata
            .borrow_mut()
            .insert(REVIEW_VIEWED_FILES_KEY.to_owned(), initial.to_string());
    }
    ViewedFilesStore::new(deps)
}

fn viewed(store: &ViewedFilesStore<Deps>) -> IndexMap<String, ViewedFileStats> {
    ViewedFiles::parse(
        store
            .deps()
            .metadata_value(REVIEW_VIEWED_FILES_KEY)
            .as_deref(),
    )
    .unwrap()
    .stats()
}

fn updates(store: &ViewedFilesStore<Deps>) -> usize {
    store.deps().updates.borrow().len()
}

fn file(path: &str, additions: u64, deletions: u64) -> ViewedFile<'_> {
    ViewedFile {
        path,
        stats: FileChangeStats::Text {
            additions,
            deletions,
        },
    }
}

fn binary(path: &str, byte_change: i64) -> ViewedFile<'_> {
    ViewedFile {
        path,
        stats: FileChangeStats::Binary { byte_change },
    }
}

fn unknown(path: &str) -> ViewedFile<'_> {
    ViewedFile {
        path,
        stats: FileChangeStats::Unknown,
    }
}

fn text(additions: u64, deletions: u64) -> ViewedFileStats {
    ViewedFileStats::Text {
        additions,
        deletions,
    }
}

fn map(entries: &[(&str, ViewedFileStats)]) -> IndexMap<String, ViewedFileStats> {
    entries
        .iter()
        .map(|(path, stats)| ((*path).to_owned(), *stats))
        .collect()
}

mod parse_viewed_files {
    use super::*;

    #[test]
    fn returns_empty_when_key_absent() {
        assert!(ViewedFiles::parse(None).unwrap().is_empty());
    }

    /// TS: "returns empty on malformed JSON". The native app fails loudly instead.
    #[test]
    fn rejects_malformed_json() {
        assert!(ViewedFiles::parse(Some("{not json")).is_err());
        assert!(ViewedFiles::parse(Some(r#"{"a.ts":{"kind":"weird"}}"#)).is_err());
        assert!(ViewedFiles::parse(Some(r#"{"a.ts":{"additions":-1,"deletions":0}}"#)).is_err());
    }

    #[test]
    fn parses_stored_entries() {
        let stored = json!({ "a.ts": { "additions": 3, "deletions": 4 } }).to_string();
        assert_eq!(
            ViewedFiles::parse(Some(&stored)).unwrap().stats(),
            map(&[("a.ts", text(3, 4))])
        );
    }
}

mod toggle_viewed_file {
    use super::*;

    #[test]
    fn marks_an_unviewed_file_viewed_with_its_current_stats() {
        let store = setup(json!({}));
        store.toggle_viewed_file(file("a.ts", 5, 6)).unwrap();
        assert_eq!(viewed(&store), map(&[("a.ts", text(5, 6))]));
    }

    #[test]
    fn unmarks_an_already_viewed_file() {
        let store = setup(json!({ "a.ts": { "additions": 5, "deletions": 6 } }));
        store.toggle_viewed_file(file("a.ts", 5, 6)).unwrap();
        assert_eq!(viewed(&store), map(&[]));
    }

    #[test]
    fn leaves_other_files_untouched() {
        let store = setup(json!({ "a.ts": { "additions": 1, "deletions": 1 } }));
        store.toggle_viewed_file(file("b.ts", 2, 2)).unwrap();
        assert_eq!(
            viewed(&store),
            map(&[("a.ts", text(1, 1)), ("b.ts", text(2, 2))])
        );
        // The untouched legacy entry is written back as it was stored.
        assert_eq!(
            store
                .deps()
                .metadata_value(REVIEW_VIEWED_FILES_KEY)
                .unwrap(),
            r#"{"a.ts":{"additions":1,"deletions":1},"b.ts":{"kind":"text","additions":2,"deletions":2}}"#
        );
    }

    #[test]
    fn persists_to_workspace_metadata_not_tab_state() {
        let store = setup(json!({}));
        store.toggle_viewed_file(file("a.ts", 1, 2)).unwrap();
        assert_eq!(
            *store.deps().updates.borrow(),
            vec![(
                REVIEW_VIEWED_FILES_KEY.to_owned(),
                "toggleViewedFile".to_owned()
            )]
        );
    }
}

mod mark_files_viewed {
    use super::*;

    #[test]
    fn adds_every_unviewed_file() {
        let store = setup(json!({}));
        store
            .mark_files_viewed(&[file("a.ts", 1, 1), file("b.ts", 2, 2)])
            .unwrap();
        assert_eq!(
            viewed(&store).keys().collect::<Vec<_>>(),
            vec!["a.ts", "b.ts"]
        );
    }

    #[test]
    fn keeps_the_original_stats_of_already_viewed_files() {
        let store = setup(json!({ "a.ts": { "additions": 9, "deletions": 9 } }));
        store.mark_files_viewed(&[file("a.ts", 1, 1)]).unwrap();
        assert_eq!(viewed(&store)["a.ts"], text(9, 9));
    }

    #[test]
    fn does_not_write_when_nothing_changes() {
        let store = setup(json!({ "a.ts": { "additions": 1, "deletions": 1 } }));
        store.mark_files_viewed(&[file("a.ts", 1, 1)]).unwrap();
        assert_eq!(updates(&store), 0);
    }

    #[test]
    fn does_not_write_for_an_empty_list() {
        let store = setup(json!({}));
        store.mark_files_viewed(&[]).unwrap();
        assert_eq!(updates(&store), 0);
    }
}

mod unmark_files_viewed {
    use super::*;

    #[test]
    fn removes_only_the_given_paths() {
        let store = setup(
            json!({ "a.ts": { "additions": 1, "deletions": 1 }, "b.ts": { "additions": 2, "deletions": 2 } }),
        );
        store.unmark_files_viewed(&["a.ts", "missing.ts"]).unwrap();
        assert_eq!(viewed(&store).keys().collect::<Vec<_>>(), vec!["b.ts"]);
    }

    #[test]
    fn does_not_write_when_none_of_the_paths_are_viewed() {
        let store = setup(json!({ "a.ts": { "additions": 1, "deletions": 1 } }));
        store.unmark_files_viewed(&["b.ts"]).unwrap();
        assert_eq!(updates(&store), 0);
    }
}

mod reconcile_viewed_files {
    use super::*;

    #[test]
    fn drops_entries_whose_stats_changed() {
        let store = setup(json!({ "a.ts": { "additions": 1, "deletions": 1 } }));
        store.reconcile_viewed_files(&[file("a.ts", 2, 1)]).unwrap();
        assert_eq!(viewed(&store), map(&[]));
    }

    #[test]
    fn keeps_entries_whose_stats_match() {
        let store = setup(json!({ "a.ts": { "additions": 1, "deletions": 1 } }));
        store.reconcile_viewed_files(&[file("a.ts", 1, 1)]).unwrap();
        assert_eq!(updates(&store), 0);
    }

    #[test]
    fn preserves_entries_absent_from_the_list_other_view_mode() {
        let store = setup(
            json!({ "a.ts": { "additions": 1, "deletions": 1 }, "b.ts": { "additions": 3, "deletions": 3 } }),
        );
        store.reconcile_viewed_files(&[file("a.ts", 2, 2)]).unwrap();
        assert_eq!(viewed(&store), map(&[("b.ts", text(3, 3))]));
    }

    #[test]
    fn does_not_write_when_nothing_is_invalidated() {
        let store = setup(json!({ "b.ts": { "additions": 3, "deletions": 3 } }));
        store.reconcile_viewed_files(&[file("a.ts", 1, 1)]).unwrap();
        assert_eq!(updates(&store), 0);
    }
}

mod binary_viewed_file_stats {
    use super::*;

    #[test]
    fn stores_a_binary_byte_delta_without_line_counts() {
        let store = setup(json!({}));
        store.toggle_viewed_file(binary("asset.png", 40)).unwrap();
        assert_eq!(
            viewed(&store)["asset.png"],
            ViewedFileStats::Binary { byte_change: 40 }
        );
        assert_eq!(
            store
                .deps()
                .metadata_value(REVIEW_VIEWED_FILES_KEY)
                .unwrap(),
            r#"{"asset.png":{"kind":"binary","byteChange":40}}"#
        );
    }

    #[test]
    fn invalidates_an_existing_viewed_binary_when_its_byte_delta_changes() {
        let store = setup(json!({ "asset.png": { "kind": "binary", "byteChange": 40 } }));
        store
            .reconcile_viewed_files(&[binary("asset.png", -10)])
            .unwrap();
        assert_eq!(viewed(&store), map(&[]));
    }

    #[test]
    fn keeps_a_legacy_viewed_text_file_when_counts_match() {
        let store = setup(json!({ "a.ts": { "additions": 1, "deletions": 2 } }));
        store.reconcile_viewed_files(&[file("a.ts", 1, 2)]).unwrap();
        assert_eq!(updates(&store), 0);
    }
}

mod get_viewed_files {
    use super::*;

    #[test]
    fn reads_through_to_the_latest_metadata() {
        let store = setup(json!({}));
        assert_eq!(store.get_viewed_files().unwrap(), map(&[]));
        store.toggle_viewed_file(file("a.ts", 7, 8)).unwrap();
        assert_eq!(
            store.get_viewed_files().unwrap(),
            map(&[("a.ts", text(7, 8))])
        );
    }
}

mod unknown_viewed_file_stats {
    use super::*;

    #[test]
    fn stores_an_unknown_stats_file_as_viewed_without_counts() {
        let store = setup(json!({}));
        store.toggle_viewed_file(unknown("gen/a.ts")).unwrap();
        assert_eq!(viewed(&store)["gen/a.ts"], ViewedFileStats::Unknown);
        assert_eq!(
            store
                .deps()
                .metadata_value(REVIEW_VIEWED_FILES_KEY)
                .unwrap(),
            r#"{"gen/a.ts":{"kind":"unknown"}}"#
        );
    }

    #[test]
    fn keeps_the_viewed_mark_while_counts_remain_unknown() {
        let store = setup(json!({ "gen/a.ts": { "kind": "unknown" } }));
        store
            .reconcile_viewed_files(&[unknown("gen/a.ts")])
            .unwrap();
        assert_eq!(updates(&store), 0);
    }

    #[test]
    fn drops_the_viewed_mark_once_real_counts_arrive() {
        let store = setup(json!({ "gen/a.ts": { "kind": "unknown" } }));
        store
            .reconcile_viewed_files(&[file("gen/a.ts", 3, 0)])
            .unwrap();
        assert_eq!(viewed(&store), map(&[]));
    }
}
