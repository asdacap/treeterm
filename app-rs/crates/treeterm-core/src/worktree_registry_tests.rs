//! Port of `lib/worktreeRegistry.test.ts`, case by case.

use serde_json::json;

use super::*;
use crate::fakes::{FakeExec, FakeFs, FakeRead, FakeWrite, block_on};

fn setup() -> (Arc<FakeFs>, Arc<FakeExec>) {
    (FakeFs::new(), FakeExec::always("/home/user"))
}

fn input(
    path: &str,
    branch: &str,
    display_name: Option<&str>,
    description: Option<&str>,
) -> RegistryEntryInput {
    RegistryEntryInput {
        path: path.to_owned(),
        branch: branch.to_owned(),
        display_name: display_name.map(str::to_owned),
        description: description.map(str::to_owned),
    }
}

fn entry(path: &str, branch: &str, last_used_at: i64) -> WorktreeRegistryEntry {
    WorktreeRegistryEntry {
        path: path.to_owned(),
        branch: branch.to_owned(),
        display_name: None,
        description: None,
        last_used_at,
        extra: Map::new(),
    }
}

fn written(fs: &FakeFs, n: usize) -> Value {
    serde_json::from_str(&fs.writes()[n].content).unwrap()
}

mod load_registry {
    use super::*;

    #[test]
    fn returns_empty_when_file_is_missing() {
        let (fs, exec) = setup();
        fs.set_all(FakeRead::Fails(
            "No such file or directory (os error 2)".to_owned(),
        ));
        assert_eq!(
            block_on(load_registry(fs.as_ref(), exec.as_ref())).unwrap(),
            vec![]
        );
        assert_eq!(
            fs.reads(),
            vec![(
                "/home/user/.treeterm".to_owned(),
                "worktrees.json".to_owned(),
                MAX_READ_FILE_BYTES
            )]
        );
        let home = exec.call(1);
        assert_eq!((home.cwd.as_str(), home.command.as_str()), ("/", "sh"));
        assert_eq!(home.args, vec!["-c", "echo $HOME"]);
    }

    #[test]
    fn parses_valid_registry_file() {
        let (fs, exec) = setup();
        let file = json!({
            "version": 1,
            "entries": [{ "path": "/wt/a", "branch": "a", "displayName": "A", "description": null, "lastUsedAt": 100 }],
        });
        fs.set_all(FakeRead::Content(file.to_string()));
        let entries = block_on(load_registry(fs.as_ref(), exec.as_ref())).unwrap();
        assert_eq!(
            entries,
            vec![WorktreeRegistryEntry {
                display_name: Some("A".to_owned()),
                ..entry("/wt/a", "a", 100)
            }]
        );
    }

    #[test]
    fn throws_on_malformed_json() {
        let (fs, exec) = setup();
        fs.set_all(FakeRead::Content("not json {".to_owned()));
        assert!(matches!(
            block_on(load_registry(fs.as_ref(), exec.as_ref())),
            Err(RegistryError::InvalidJson(_))
        ));
    }

    #[test]
    fn throws_on_unsupported_version() {
        let (fs, exec) = setup();
        fs.set_all(FakeRead::Content(
            json!({ "version": 2, "entries": [] }).to_string(),
        ));
        let error = block_on(load_registry(fs.as_ref(), exec.as_ref())).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Unsupported worktree registry version: 2"
        );

        fs.set_all(FakeRead::Content(json!({ "entries": [] }).to_string()));
        let error = block_on(load_registry(fs.as_ref(), exec.as_ref())).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Unsupported worktree registry version: undefined"
        );

        fs.set_all(FakeRead::Content("[1]".to_owned()));
        let error = block_on(load_registry(fs.as_ref(), exec.as_ref())).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Worktree registry file has invalid shape"
        );
    }

    #[test]
    fn throws_on_non_enoent_read_failure() {
        let (fs, exec) = setup();
        fs.set_all(FakeRead::Fails("permission denied".to_owned()));
        let error = block_on(load_registry(fs.as_ref(), exec.as_ref())).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Failed to read worktree registry: permission denied"
        );
    }

    #[test]
    fn refuses_a_file_over_the_read_limit() {
        let (fs, exec) = setup();
        fs.set_all(FakeRead::TooLarge(MAX_READ_FILE_BYTES + 1));
        assert!(matches!(
            block_on(load_registry(fs.as_ref(), exec.as_ref())),
            Err(RegistryError::TooLarge { .. })
        ));
    }

    #[test]
    fn fails_when_the_home_directory_cannot_be_resolved() {
        let fs = FakeFs::new();
        let exec = FakeExec::always("");
        let error = block_on(load_registry(fs.as_ref(), exec.as_ref())).unwrap_err();
        assert_eq!(error.to_string(), "Failed to resolve remote home directory");
    }
}

mod upsert_registry_entry {
    use super::*;

    #[test]
    fn appends_new_entry_when_path_not_present() {
        let (fs, exec) = setup();
        block_on(upsert_registry_entry(
            fs.as_ref(),
            exec.as_ref(),
            &input("/wt/a", "a", Some("A"), Some("desc")),
        ))
        .unwrap();

        let writes = fs.writes();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].workspace_path, "/home/user/.treeterm");
        assert_eq!(writes[0].file_path, "worktrees.json");
        let file = written(&fs, 0);
        assert_eq!(file["version"], 1);
        assert_eq!(file["entries"].as_array().unwrap().len(), 1);
        assert_eq!(file["entries"][0]["path"], "/wt/a");
        assert!(file["entries"][0]["lastUsedAt"].is_i64());
    }

    #[test]
    fn writes_the_file_as_electron_does() {
        let (fs, exec) = setup();
        block_on(upsert_registry_entry(
            fs.as_ref(),
            exec.as_ref(),
            &input("/wt/a", "a", None, Some("d")),
        ))
        .unwrap();
        let file = written(&fs, 0);
        let stamp = file["entries"][0]["lastUsedAt"].as_i64().unwrap();
        // `JSON.stringify(file, null, 2)`.
        assert_eq!(
            fs.writes()[0].content,
            format!(
                "{{\n  \"version\": 1,\n  \"entries\": [\n    {{\n      \"path\": \"/wt/a\",\n      \"branch\": \"a\",\n      \"displayName\": null,\n      \"description\": \"d\",\n      \"lastUsedAt\": {stamp}\n    }}\n  ]\n}}"
            )
        );
    }

    #[test]
    fn replaces_existing_entry_by_path_and_bumps_last_used_at() {
        let (fs, exec) = setup();
        let existing = json!({
            "version": 1,
            "entries": [{ "path": "/wt/a", "branch": "a", "displayName": null, "description": null, "lastUsedAt": 100 }],
        });
        fs.set_all(FakeRead::Content(existing.to_string()));
        block_on(upsert_registry_entry(
            fs.as_ref(),
            exec.as_ref(),
            &input("/wt/a", "a", Some("NewName"), Some("new")),
        ))
        .unwrap();

        let file = written(&fs, 0);
        assert_eq!(file["entries"].as_array().unwrap().len(), 1);
        assert_eq!(file["entries"][0]["displayName"], "NewName");
        assert!(file["entries"][0]["lastUsedAt"].as_i64().unwrap() > 100);
    }

    #[test]
    fn keeps_fields_it_does_not_know_on_other_entries() {
        let (fs, exec) = setup();
        let existing = json!({
            "version": 1,
            "entries": [{ "path": "/wt/b", "branch": "b", "displayName": null, "description": null, "lastUsedAt": 1, "pinned": true }],
        });
        fs.set_all(FakeRead::Content(existing.to_string()));
        block_on(upsert_registry_entry(
            fs.as_ref(),
            exec.as_ref(),
            &input("/wt/a", "a", None, None),
        ))
        .unwrap();
        assert_eq!(written(&fs, 0)["entries"][0]["pinned"], true);
    }

    #[test]
    fn throws_when_write_fails() {
        let (fs, exec) = setup();
        fs.set_all(FakeRead::Fails("not found".to_owned()));
        fs.queue_write(FakeWrite::Fails("disk full".to_owned()));
        let error = block_on(upsert_registry_entry(
            fs.as_ref(),
            exec.as_ref(),
            &input("/wt/a", "a", None, None),
        ))
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Failed to write worktree registry: disk full"
        );
    }
}

mod remove_registry_entry {
    use super::*;

    #[test]
    fn filters_out_matching_path_and_writes() {
        let (fs, exec) = setup();
        let existing = json!({
            "version": 1,
            "entries": [
                { "path": "/wt/a", "branch": "a", "displayName": null, "description": null, "lastUsedAt": 100 },
                { "path": "/wt/b", "branch": "b", "displayName": null, "description": null, "lastUsedAt": 200 },
            ],
        });
        fs.set_all(FakeRead::Content(existing.to_string()));
        block_on(remove_registry_entry(fs.as_ref(), exec.as_ref(), "/wt/a")).unwrap();
        assert_eq!(written(&fs, 0)["entries"], json!([existing["entries"][1]]));
    }

    #[test]
    fn no_ops_when_path_is_absent_no_write() {
        let (fs, exec) = setup();
        block_on(remove_registry_entry(fs.as_ref(), exec.as_ref(), "/wt/a")).unwrap();
        assert!(fs.writes().is_empty());
    }
}

mod compare_and_swap {
    use super::*;

    #[test]
    fn passes_empty_expected_hash_when_the_file_is_missing() {
        let (fs, exec) = setup();
        block_on(upsert_registry_entry(
            fs.as_ref(),
            exec.as_ref(),
            &input("/wt/a", "a", None, None),
        ))
        .unwrap();
        assert_eq!(fs.writes()[0].guard, WriteGuard::MustNotExist);
    }

    #[test]
    fn passes_the_sha256_of_the_content_that_was_read() {
        let (fs, exec) = setup();
        let content = json!({ "version": 1, "entries": [] }).to_string();
        fs.set_all(FakeRead::Content(content.clone()));
        block_on(upsert_registry_entry(
            fs.as_ref(),
            exec.as_ref(),
            &input("/wt/a", "a", None, None),
        ))
        .unwrap();
        assert_eq!(
            fs.writes()[0].guard,
            WriteGuard::Matches(sha256_hex(content.as_bytes()))
        );
    }

    #[test]
    fn retries_the_read_modify_write_on_conflict_and_succeeds() {
        // First read: empty registry. Second read (after the conflict): another window added /wt/b.
        let (fs, exec) = setup();
        fs.queue_read(FakeRead::Fails("No such file".to_owned()));
        fs.queue_read(FakeRead::Content(
            json!({ "version": 1, "entries": [{ "path": "/wt/b", "branch": "b", "displayName": null, "description": null, "lastUsedAt": 1 }] })
                .to_string(),
        ));
        fs.queue_write(FakeWrite::Conflict);
        fs.queue_write(FakeWrite::Written);

        block_on(upsert_registry_entry(
            fs.as_ref(),
            exec.as_ref(),
            &input("/wt/a", "a", None, None),
        ))
        .unwrap();

        assert_eq!(fs.writes().len(), 2);
        // The retry re-applied the change on top of the other window's entry: nothing lost.
        let mut paths: Vec<String> = written(&fs, 1)["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["path"].as_str().unwrap().to_owned())
            .collect();
        paths.sort();
        assert_eq!(paths, vec!["/wt/a", "/wt/b"]);
    }

    #[test]
    fn fails_loudly_after_exhausting_conflict_retries() {
        let (fs, exec) = setup();
        for _ in 0..MAX_CAS_RETRIES {
            fs.queue_write(FakeWrite::Conflict);
        }
        let error = block_on(upsert_registry_entry(
            fs.as_ref(),
            exec.as_ref(),
            &input("/wt/a", "a", None, None),
        ))
        .unwrap_err();
        assert!(error.to_string().contains("after 5 attempts"));
        assert_eq!(fs.writes().len(), 5);
    }
}

mod build_entry_from_workspace {
    use super::*;

    fn workspace(git_branch: Option<&str>, metadata: Value) -> WorkspaceFile {
        let mut file = json!({
            "name": "x", "status": "active", "isGitRepo": true, "isWorktree": true,
            "appStates": {}, "metadata": metadata, "createdAt": 0, "lastActivity": 0,
        });
        if let Some(branch) = git_branch {
            file["gitBranch"] = json!(branch);
        }
        serde_json::from_value(file).unwrap()
    }

    #[test]
    fn pulls_metadata_display_name_and_metadata_description() {
        let file = workspace(
            Some("feat/x"),
            json!({ "displayName": "X custom", "description": "a desc" }),
        );
        assert_eq!(
            build_entry_from_workspace("/wt/x", &file),
            input("/wt/x", "feat/x", Some("X custom"), Some("a desc"))
        );
    }

    #[test]
    fn returns_null_for_missing_metadata_fields() {
        let file = workspace(None, json!({}));
        assert_eq!(
            build_entry_from_workspace("/wt/y", &file),
            input("/wt/y", "", None, None)
        );
    }
}

mod create_worktree_registry_api {
    use super::*;

    #[test]
    fn exposes_list_upsert_remove_bound_to_connection() {
        let (fs, exec) = setup();
        let api = WorktreeRegistry::new(fs.clone(), exec);
        assert_eq!(block_on(api.list()).unwrap(), vec![]);
        block_on(api.upsert(&input("/wt/a", "a", None, None))).unwrap();
        assert_eq!(fs.writes().len(), 1);
        block_on(api.remove("/wt/a")).unwrap();
        assert_eq!(block_on(api.list()).unwrap(), vec![]);
    }

    #[test]
    fn serializes_concurrent_upserts_so_later_calls_observe_earlier_writes() {
        // Each read reflects the last write. Unserialized, both upserts would read the empty
        // baseline (reads yield before answering) and the second write would drop the first.
        let (fs, exec) = setup();
        let api = WorktreeRegistry::new(fs.clone(), exec);
        let (first, second) = block_on(futures::future::join(
            api.upsert(&input("/wt/a", "a", None, None)),
            api.upsert(&input("/wt/b", "b", None, None)),
        ));
        first.unwrap();
        second.unwrap();

        let mut paths: Vec<String> = block_on(api.list())
            .unwrap()
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        paths.sort();
        assert_eq!(paths, vec!["/wt/a", "/wt/b"]);
        assert!(
            fs.writes()
                .iter()
                .all(|write| write.guard != WriteGuard::Unconditional)
        );
    }
}
