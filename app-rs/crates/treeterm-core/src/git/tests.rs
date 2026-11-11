//! Port of `renderer/lib/gitClient.test.ts`, case by case: one module per `describe`, one
//! test per `it`, same names in snake_case. The exec fake replaces the TS `ExecApi` mock;
//! its scripted completions are consumed in call order, as `autoComplete` did.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::fake::{Completion, FakeExec, FakeFile, FakeFs, err, out, out_code, reject};
use super::*;
use crate::exec::{DEFAULT_EXEC_TIMEOUT, ExecTimeout};

fn api(exec: &Arc<FakeExec>, fs: Arc<FakeFs>) -> GitApi {
    GitApi::new(exec.clone(), fs)
}

fn run<F: std::future::Future>(future: F) -> F::Output {
    futures::executor::block_on(future)
}

/// The message of an expected failure.
fn error_of<T: std::fmt::Debug>(result: Result<T, GitError>) -> String {
    result.expect_err("expected a failure").to_string()
}

fn owned(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

/// `(cwd, command, args, timeout)` of a call.
fn call_parts(request: &ExecRequest) -> (&str, &str, Vec<&str>, ExecTimeout) {
    (
        request.cwd.as_str(),
        request.command.as_str(),
        request.args.iter().map(String::as_str).collect(),
        request.timeout,
    )
}

/// `toHaveBeenNthCalledWith(n, 'conn-1', cwd, command, args, DEFAULT_EXEC_TIMEOUT_MS)`.
fn assert_nth_call(exec: &FakeExec, n: usize, cwd: &str, command: &str, args: &[&str]) {
    let calls = exec.calls();
    let call = calls
        .get(n - 1)
        .unwrap_or_else(|| panic!("no call #{n}: {calls:?}"));
    assert_eq!(
        call_parts(call),
        (
            cwd,
            command,
            args.to_vec(),
            ExecTimeout::After(DEFAULT_EXEC_TIMEOUT)
        ),
        "call #{n}"
    );
}

/// `toHaveBeenCalledWith('conn-1', cwd, command, args, DEFAULT_EXEC_TIMEOUT_MS)`.
fn assert_called_with(exec: &FakeExec, cwd: &str, command: &str, args: &[&str]) {
    let calls = exec.calls();
    let expected = (
        cwd,
        command,
        args.to_vec(),
        ExecTimeout::After(DEFAULT_EXEC_TIMEOUT),
    );
    assert!(
        calls.iter().any(|call| call_parts(call) == expected),
        "no call {expected:?} in {calls:?}"
    );
}

fn text(additions: u64, deletions: u64) -> FileChangeStats {
    FileChangeStats::Text {
        additions,
        deletions,
    }
}

fn binary(byte_change: i64) -> FileChangeStats {
    FileChangeStats::Binary { byte_change }
}

fn diff_file(path: &str, status: FileChangeStatus, stats: FileChangeStats) -> DiffFile {
    DiffFile {
        path: path.to_owned(),
        status,
        stats,
    }
}

fn find<'a>(files: &'a [DiffFile], path: &str) -> &'a DiffFile {
    files
        .iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| panic!("no {path} in {files:?}"))
}

/// The text sides of a diff, failing on any other kind.
fn text_sides(contents: &FileDiffContents) -> (&str, &str, &str) {
    match contents {
        FileDiffContents::Text {
            original_content,
            modified_content,
            language,
        } => (original_content, modified_content, language),
        other => panic!("expected text contents, got {other:?}"),
    }
}

/// Collects progress output; `report()` is what the git calls take.
struct ProgressLog(Mutex<Vec<String>>);

impl ProgressLog {
    fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }

    fn record(&self, text: &str) {
        self.0.lock().unwrap().push(text.to_owned());
    }

    fn calls(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

/// `describe(<method> (catch branch))` + `it('catches thrown errors')`: every exec call
/// fails with `$message`, which the method must report.
macro_rules! catches_thrown_errors {
    ($module:ident, $message:literal, |$git:ident| $call:expr) => {
        mod $module {
            use super::*;

            #[test]
            fn catches_thrown_errors() {
                let exec = FakeExec::always(reject($message));
                let $git = api(&exec, FakeFs::missing());
                let error = error_of(run($call));
                assert!(error.contains($message), "{error}");
            }
        }
    };
}

mod parse_status_via_get_uncommitted_changes {
    use super::*;

    #[test]
    fn parses_modified_unstaged_file() {
        let entries = parse_status(" M src/app.ts\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "src/app.ts");
        assert_eq!(entries[0].status, FileChangeStatus::Modified);
        assert_eq!(entries[0].staged, Staging::Unstaged);
    }

    #[test]
    fn parses_added_staged_file() {
        let entries = parse_status("A  src/new.ts");
        assert_eq!(entries[0].status, FileChangeStatus::Added);
        assert_eq!(entries[0].staged, Staging::Staged);
    }

    #[test]
    fn parses_deleted_file() {
        let entries = parse_status("D  src/old.ts");
        assert_eq!(entries[0].status, FileChangeStatus::Deleted);
    }

    #[test]
    fn parses_untracked_file() {
        let entries = parse_status("?? src/new.ts");
        assert_eq!(entries[0].status, FileChangeStatus::Untracked);
        assert_eq!(entries[0].staged, Staging::Unstaged);
    }

    #[test]
    fn parses_renamed_file() {
        let entries = parse_status("R  new.ts -> old.ts");
        assert_eq!(entries[0].status, FileChangeStatus::Renamed);
        assert_eq!(entries[0].path, "old.ts");
        assert_eq!(
            entries[0].origin,
            PathOrigin::RenamedFrom("new.ts".to_owned())
        );
    }

    #[test]
    fn parses_file_with_both_staged_and_unstaged_changes() {
        let entries = parse_status("MM src/app.ts");
        assert_eq!(entries.len(), 2);
        let staged = entries
            .iter()
            .find(|e| e.path == "src/app.ts" && e.staged == Staging::Staged)
            .expect("staged entry");
        let unstaged = entries
            .iter()
            .find(|e| e.path == "src/app.ts" && e.staged == Staging::Unstaged)
            .expect("unstaged entry");
        assert_eq!(staged.status, FileChangeStatus::Modified);
        assert_eq!(unstaged.status, FileChangeStatus::Modified);
    }

    #[test]
    fn parses_file_with_only_staged_changes() {
        let entries = parse_status("M  src/app.ts");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].staged, Staging::Staged);
        assert_eq!(entries[0].status, FileChangeStatus::Modified);
    }

    #[test]
    fn returns_empty_array_for_empty_status_output() {
        assert!(parse_status("").is_empty());
    }
}

mod interpret_error_via_has_uncommitted_changes_and_stage_file {
    use super::*;

    fn stage_file_error(stderr: Completion) -> String {
        let exec = FakeExec::new(vec![stderr]);
        let git = api(&exec, FakeFs::missing());
        error_of(run(git.stage_file("/repo", "file.ts")))
    }

    #[test]
    fn detects_not_a_git_repository_error() {
        let exec = FakeExec::new(vec![err("not a git repository", 1)]);
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.stage_file("/not-a-repo", "file.ts")));
        assert!(error.contains("Not a git repository"), "{error}");
    }

    #[test]
    fn detects_merge_conflict_error() {
        let error = stage_file_error(err("merge conflict detected", 1));
        assert!(error.contains("Merge conflict detected"), "{error}");
    }

    #[test]
    fn detects_already_exists_error() {
        let error = stage_file_error(err("already exists", 1));
        assert!(error.contains("Already exists"), "{error}");
    }

    #[test]
    fn detects_pathspec_error() {
        let exec = FakeExec::new(vec![err("pathspec did not match any files", 1)]);
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.stage_file("/repo", "missing.ts")));
        assert!(error.contains("File not found"), "{error}");
    }

    #[test]
    fn detects_failed_to_merge_error() {
        let error = stage_file_error(err("failed to merge", 1));
        assert!(error.contains("Merge failed"), "{error}");
    }

    #[test]
    fn detects_could_not_resolve_error() {
        let error = stage_file_error(err("could not resolve reference", 1));
        assert!(error.contains("Could not resolve reference"), "{error}");
    }

    #[test]
    fn falls_back_to_generic_git_error_with_stderr() {
        let error = stage_file_error(err("some unknown error message", 1));
        assert!(error.contains("Git error [git add file.ts]:"), "{error}");
    }

    #[test]
    fn detects_nothing_to_commit_in_stderr() {
        let exec = FakeExec::new(vec![err("nothing to commit, working tree clean", 1)]);
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.commit_staged("/repo", "msg")));
        assert!(error.contains("No changes to commit"), "{error}");
    }

    #[test]
    fn falls_back_to_exit_code_message_when_stderr_is_empty() {
        let error = stage_file_error(out_code("", 128));
        assert!(
            error.contains("Git command failed [git add file.ts] with exit code 128"),
            "{error}"
        );
    }
}

mod detect_language_via_get_file_contents_for_diff {
    use super::*;

    fn language_of(file_path: &str, original: &str, modified: &str) -> String {
        // Stream sequence: rev-parse HEAD, merge-base, show original, show HEAD (modified)
        let exec = FakeExec::new(vec![
            out("feature-branch"),
            out("abc123"),
            out(original),
            out(modified),
        ]);
        let git = api(&exec, FakeFs::missing());
        let contents = run(git.get_file_contents_for_diff("/repo", "main", file_path)).unwrap();
        text_sides(&contents).2.to_owned()
    }

    #[test]
    fn detects_typescript_from_ts_extension() {
        assert_eq!(
            language_of("src/app.ts", "original content", "modified content"),
            "typescript"
        );
    }

    #[test]
    fn detects_python_from_py_extension() {
        assert_eq!(
            language_of("script.py", "print(\"hi\")", "print(\"world\")"),
            "python"
        );
    }

    #[test]
    fn returns_plaintext_for_unknown_extension() {
        assert_eq!(language_of("data.xyz", "data", "data"), "plaintext");
    }
}

mod list_worktrees {
    use super::*;

    #[test]
    fn parses_porcelain_worktree_output() {
        let output = [
            "worktree /repo",
            "HEAD abc123",
            "branch refs/heads/main",
            "",
            "worktree /repo/.worktrees/feature",
            "HEAD def456",
            "branch refs/heads/feature",
            "",
        ]
        .join("\n");
        let exec = FakeExec::new(vec![out(output)]);
        let git = api(&exec, FakeFs::missing());
        let worktrees = run(git.list_worktrees("/repo")).unwrap();
        assert_eq!(worktrees.len(), 2);
        assert_eq!(worktrees[0].path, "/repo");
        assert_eq!(worktrees[0].branch, "main");
        assert_eq!(worktrees[1].path, "/repo/.worktrees/feature");
        assert_eq!(worktrees[1].branch, "feature");
    }

    #[test]
    fn throws_when_worktree_list_fails() {
        let exec = FakeExec::new(vec![err("not a git repository", 1)]);
        let git = api(&exec, FakeFs::missing());
        assert!(run(git.list_worktrees("/not-repo")).is_err());
    }
}

mod get_diff {
    use super::*;

    #[test]
    fn parses_numstat_and_name_status_output() {
        let exec = FakeExec::new(vec![
            out("feature"),                             // current branch
            out("base123"),                             // merge base
            out("10\t5\tsrc/app.ts\n3\t0\tsrc/new.ts"), // numstat
            out("M\tsrc/app.ts\nA\tsrc/new.ts"),        // name-status
        ]);
        let git = api(&exec, FakeFs::missing());
        let diff = run(git.get_diff("/repo", "main")).unwrap();
        assert_eq!(diff.head_branch, "feature");
        assert_eq!(diff.base_branch, "main");
        assert_eq!(diff.total_additions, 13);
        assert_eq!(diff.total_deletions, 5);
        assert_eq!(diff.files.len(), 2);

        let app_file = find(&diff.files, "src/app.ts");
        assert_eq!(app_file.status, FileChangeStatus::Modified);
        assert_eq!(app_file.stats, text(10, 5));

        let new_file = find(&diff.files, "src/new.ts");
        assert_eq!(new_file.status, FileChangeStatus::Added);
        assert!(matches!(
            new_file.stats,
            FileChangeStats::Text { additions: 3, .. }
        ));
    }

    #[test]
    fn handles_binary_files_in_numstat() {
        let exec = FakeExec::new(vec![
            out("feature"),
            out("base123"),
            out("-\t-\timage.png"),
            out("M\timage.png"),
            out(" image.png | Bin 100 -> 125 bytes\n 1 file changed"),
        ]);
        let git = api(&exec, FakeFs::missing());
        let diff = run(git.get_diff("/repo", "main")).unwrap();
        assert_eq!(find(&diff.files, "image.png").stats, binary(25));
    }
}

mod binary_branch_comparison {
    use super::*;

    #[test]
    fn keeps_text_only_totals_and_computes_negative_and_zero_byte_changes_by_path() {
        let exec = FakeExec::new(vec![
            out("feature"),
            out("base123"),
            out("3\t1\tsrc/app.ts\n-\t-\tfiles/shrunk image.png\n-\t-\tfiles/same.png"),
            out("M\tsrc/app.ts\nM\tfiles/shrunk image.png\nM\tfiles/same.png"),
            out(" files/shrunk image.png | Bin 125 -> 80 bytes\n"),
            out(" files/same.png | Bin 80 -> 80 bytes\n"),
        ]);
        let diff = run(api(&exec, FakeFs::missing()).get_diff("/repo", "main")).unwrap();
        assert_eq!(diff.total_additions, 3);
        assert_eq!(diff.total_deletions, 1);
        assert_eq!(
            diff.files,
            vec![
                diff_file("src/app.ts", FileChangeStatus::Modified, text(3, 1)),
                diff_file(
                    "files/shrunk image.png",
                    FileChangeStatus::Modified,
                    binary(-45)
                ),
                diff_file("files/same.png", FileChangeStatus::Modified, binary(0)),
            ]
        );
        assert_nth_call(
            &exec,
            5,
            "/repo",
            "git",
            &[
                "diff",
                "--stat",
                "base123",
                "feature",
                "--",
                "files/shrunk image.png",
            ],
        );
    }

    #[test]
    fn resolves_a_renamed_binary_using_both_original_and_new_paths() {
        let exec = FakeExec::new(vec![
            out("feature"),
            out("base123"),
            out("-\t-\tdir/{old => new}.png"),
            out("R099\tdir/old.png\tdir/new.png"),
            out(" dir/{old => new}.png | Bin 100 -> 110 bytes\n"),
        ]);
        let diff = run(api(&exec, FakeFs::missing()).get_diff("/repo", "main")).unwrap();
        assert_eq!(
            diff.files,
            vec![diff_file(
                "dir/new.png",
                FileChangeStatus::Renamed,
                binary(10)
            )]
        );
        assert_nth_call(
            &exec,
            5,
            "/repo",
            "git",
            &[
                "diff",
                "--stat",
                "base123",
                "feature",
                "--",
                "dir/old.png",
                "dir/new.png",
            ],
        );
    }

    #[test]
    fn preserves_a_literal_arrow_in_a_binary_filename() {
        let exec = FakeExec::new(vec![
            out("feature"),
            out("base123"),
            out("-\t-\ticon => cover.png"),
            out("M\ticon => cover.png"),
            out(" icon => cover.png | Bin 20 -> 21 bytes\n"),
        ]);
        let diff = run(api(&exec, FakeFs::missing()).get_diff("/repo", "main")).unwrap();
        assert_eq!(diff.files[0].path, "icon => cover.png");
        assert_eq!(diff.files[0].stats, binary(1));
        assert_called_with(
            &exec,
            "/repo",
            "git",
            &[
                "diff",
                "--stat",
                "base123",
                "feature",
                "--",
                "icon => cover.png",
            ],
        );
    }

    #[test]
    fn rejects_a_missing_binary_stat_instead_of_inventing_zero_bytes() {
        let exec = FakeExec::new(vec![
            out("feature"),
            out("base123"),
            out("-\t-\timage.png"),
            out("M\timage.png"),
            out(" 1 file changed, 0 insertions(+), 0 deletions(-)\n"),
        ]);
        let error = error_of(run(api(&exec, FakeFs::missing()).get_diff("/repo", "main")));
        assert!(
            error.contains("Missing or ambiguous binary byte sizes"),
            "{error}"
        );
    }

    #[test]
    fn fails_when_git_has_no_binary_byte_sizes_or_reports_rounded_sizes() {
        let exec = FakeExec::new(vec![
            out("feature"),
            out("base123"),
            out("-\t-\timage.png"),
            out("M\timage.png"),
            out(" image.png | Bin 100 -> 0.1 KiB\n"),
        ]);
        let error = error_of(run(api(&exec, FakeFs::missing()).get_diff("/repo", "main")));
        assert!(error.contains("Invalid binary byte sizes"), "{error}");
    }

    #[test]
    fn fails_if_the_size_lookup_command_fails() {
        let exec = FakeExec::new(vec![
            out("feature"),
            out("base123"),
            out("-\t-\timage.png"),
            out("M\timage.png"),
            err("fatal: invalid path", 128),
        ]);
        let error = error_of(run(api(&exec, FakeFs::missing()).get_diff("/repo", "main")));
        assert!(error.contains("fatal: invalid path"), "{error}");
    }
}

mod get_info {
    use super::*;

    #[test]
    fn returns_non_repo_info_on_failure() {
        let exec = FakeExec::new(vec![err("not a git repository", 1)]);
        let git = api(&exec, FakeFs::missing());
        assert_eq!(run(git.get_info("/not-repo")), GitInfo::NotRepo);
    }

    #[test]
    fn returns_repo_info_on_success() {
        let exec = FakeExec::new(vec![
            out("true"),             // is-inside-work-tree
            out("main"),             // abbrev-ref HEAD (parallel)
            out("worktree /repo\n"), // worktree list --porcelain (resolveRepoRoot, parallel)
            out("/repo"),            // show-toplevel (parallel)
        ]);
        let git = api(&exec, FakeFs::missing());
        assert_eq!(
            run(git.get_info("/repo")),
            GitInfo::Repo {
                branch: "main".to_owned(),
                root_path: "/repo".to_owned()
            }
        );
    }
}

mod has_uncommitted_changes {
    use super::*;

    #[test]
    fn returns_true_when_there_are_uncommitted_changes() {
        let exec = FakeExec::new(vec![out("M  src/app.ts")]);
        let git = api(&exec, FakeFs::missing());
        assert!(run(git.has_uncommitted_changes("/repo")).unwrap());
    }

    #[test]
    fn returns_false_when_working_tree_is_clean() {
        let exec = FakeExec::new(vec![out("")]);
        let git = api(&exec, FakeFs::missing());
        assert!(!run(git.has_uncommitted_changes("/repo")).unwrap());
    }
}

/// `describe(<method>)` with `it('returns success on success')` and
/// `it('returns failure on error')` (not a git repository).
macro_rules! succeeds_or_fails {
    ($module:ident, |$git:ident, $path:ident| $call:expr) => {
        mod $module {
            use super::*;

            #[test]
            fn returns_success_on_success() {
                let exec = FakeExec::new(vec![out("")]);
                let $git = api(&exec, FakeFs::missing());
                let $path = "/repo";
                run($call).unwrap();
            }

            #[test]
            fn returns_failure_on_error() {
                let exec = FakeExec::new(vec![err("not a git repository", 1)]);
                let $git = api(&exec, FakeFs::missing());
                let $path = "/not-repo";
                let error = error_of(run($call));
                assert!(error.contains("Not a git repository"), "{error}");
            }
        }
    };
}

mod stage_file {
    use super::*;

    #[test]
    fn returns_success_on_success() {
        let exec = FakeExec::new(vec![out("")]);
        let git = api(&exec, FakeFs::missing());
        run(git.stage_file("/repo", "src/app.ts")).unwrap();
    }

    #[test]
    fn returns_failure_on_error() {
        let exec = FakeExec::new(vec![err("pathspec did not match any files", 1)]);
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.stage_file("/repo", "missing.ts")));
        assert!(error.contains("File not found"), "{error}");
    }
}

succeeds_or_fails!(unstage_file, |git, path| git
    .unstage_file(path, "src/app.ts"));
succeeds_or_fails!(stage_all, |git, path| git.stage_all(path));
succeeds_or_fails!(unstage_all, |git, path| git.unstage_all(path));

mod commit_staged {
    use super::*;

    #[test]
    fn returns_success_on_commit() {
        let exec = FakeExec::new(vec![out("[main abc1234] add feature\n 1 file changed")]);
        let git = api(&exec, FakeFs::missing());
        run(git.commit_staged("/repo", "add feature")).unwrap();
    }

    #[test]
    fn returns_failure_on_nothing_to_commit() {
        let exec = FakeExec::new(vec![err("nothing to commit, working tree clean", 1)]);
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.commit_staged("/repo", "msg")));
        assert!(error.contains("No changes to commit"), "{error}");
    }
}

mod commit_all {
    use super::*;

    #[test]
    fn streams_the_commit_to_on_progress() {
        let exec = FakeExec::new(vec![out("[feature def5678] fix bug\n")]);
        let git = api(&exec, FakeFs::missing());
        let log = ProgressLog::new();
        let report = |text: &str| log.record(text);
        run(git.commit_all("/repo", "fix bug", Progress::Report(&report))).unwrap();
        assert_eq!(
            log.calls(),
            vec!["$ git commit -am fix bug\n", "[feature def5678] fix bug\n"]
        );
    }

    #[test]
    fn returns_success_on_commit() {
        let exec = FakeExec::new(vec![out("[feature def5678] fix bug\n 2 files changed")]);
        let git = api(&exec, FakeFs::missing());
        run(git.commit_all("/repo", "fix bug", Progress::Silent)).unwrap();
    }

    #[test]
    fn returns_failure_on_nothing_to_commit() {
        let exec = FakeExec::new(vec![err("nothing to commit", 1)]);
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.commit_all("/repo", "msg", Progress::Silent)));
        assert!(error.contains("No changes to commit"), "{error}");
    }
}

mod list_local_branches {
    use super::*;

    #[test]
    fn returns_list_of_local_branches() {
        let exec = FakeExec::new(vec![out("main\nfeature\ndevelop")]);
        let git = api(&exec, FakeFs::missing());
        assert_eq!(
            run(git.list_local_branches("/repo")).unwrap(),
            owned(&["main", "feature", "develop"])
        );
    }

    #[test]
    fn returns_empty_array_when_no_branches() {
        let exec = FakeExec::new(vec![out("")]);
        let git = api(&exec, FakeFs::missing());
        assert!(run(git.list_local_branches("/repo")).unwrap().is_empty());
    }

    #[test]
    fn throws_on_failure() {
        let exec = FakeExec::new(vec![err("not a git repository", 1)]);
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.list_local_branches("/not-repo")));
        assert!(error.contains("Not a git repository"), "{error}");
    }
}

mod list_remote_branches {
    use super::*;

    #[test]
    fn returns_list_of_remote_branches() {
        let exec = FakeExec::new(vec![out("origin/main\norigin/feature")]);
        let git = api(&exec, FakeFs::missing());
        assert_eq!(
            run(git.list_remote_branches("/repo")).unwrap(),
            owned(&["origin/main", "origin/feature"])
        );
    }

    #[test]
    fn throws_on_failure() {
        let exec = FakeExec::new(vec![err("not a git repository", 1)]);
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.list_remote_branches("/not-repo")));
        assert!(error.contains("Not a git repository"), "{error}");
    }
}

mod delete_branch {
    use super::*;

    #[test]
    fn returns_success_on_deletion() {
        let exec = FakeExec::new(vec![out("")]);
        let git = api(&exec, FakeFs::missing());
        run(git.delete_branch("/repo", "feature", Progress::Silent)).unwrap();
    }

    #[test]
    fn returns_failure_on_error() {
        let exec = FakeExec::new(vec![err("not a git repository", 1)]);
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.delete_branch(
            "/not-repo",
            "feature",
            Progress::Silent,
        )));
        assert!(error.contains("Not a git repository"), "{error}");
    }
}

mod get_head_commit_hash {
    use super::*;

    #[test]
    fn returns_trimmed_commit_hash() {
        let exec = FakeExec::new(vec![out("abc123def456\n")]);
        let git = api(&exec, FakeFs::missing());
        assert_eq!(
            run(git.get_head_commit_hash("/repo")).unwrap(),
            "abc123def456"
        );
    }

    #[test]
    fn returns_failure_on_error() {
        let exec = FakeExec::new(vec![err("not a git repository", 1)]);
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.get_head_commit_hash("/not-repo")));
        assert!(error.contains("Not a git repository"), "{error}");
    }
}

mod get_line_commits {
    use super::*;

    fn porcelain() -> (String, String, String) {
        let h1 = "a".repeat(40);
        let h2 = "b".repeat(40);
        let porcelain = [
            format!("{h1} 1 1 2"),
            "author A".to_owned(),
            "summary s".to_owned(),
            "filename f.ts".to_owned(),
            "\tline one".to_owned(),
            format!("{h1} 2 2"),
            "\tline two".to_owned(),
            format!("{h2} 7 3 1"),
            format!("previous {h1} f.ts"),
            "filename f.ts".to_owned(),
            "\tline three".to_owned(),
        ]
        .join("\n");
        (porcelain, h1, h2)
    }

    #[test]
    fn maps_each_line_to_the_commit_that_last_changed_it() {
        let (porcelain, h1, h2) = porcelain();
        let exec = FakeExec::new(vec![out(porcelain)]);
        let git = api(&exec, FakeFs::missing());
        assert_eq!(
            run(git.get_line_commits("/repo", "f.ts")).unwrap(),
            vec![h1.clone(), h1, h2]
        );
    }

    #[test]
    fn returns_no_lines_for_a_path_git_does_not_know() {
        let exec = FakeExec::new(vec![err("fatal: no such path 'f.ts' in HEAD", 128)]);
        let git = api(&exec, FakeFs::missing());
        assert!(
            run(git.get_line_commits("/repo", "f.ts"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn returns_failure_on_other_errors() {
        let exec = FakeExec::new(vec![err("not a git repository", 128)]);
        let git = api(&exec, FakeFs::missing());
        assert!(run(git.get_line_commits("/repo", "f.ts")).is_err());
    }
}

mod get_uncommitted_changes {
    use super::*;

    #[test]
    fn returns_files_with_add_delete_stats() {
        let exec = FakeExec::new(vec![
            out("M  src/app.ts"),    // status (staged)
            out("5\t2\tsrc/app.ts"), // staged numstat
            out(""),                 // unstaged numstat
        ]);
        let git = api(&exec, FakeFs::missing());
        let changes = run(git.get_uncommitted_changes("/repo")).unwrap();
        assert_eq!(changes.files.len(), 1);
        assert_eq!(changes.files[0].entry.path, "src/app.ts");
        assert_eq!(changes.files[0].stats, text(5, 2));
        assert_eq!(changes.total_additions, 5);
        assert_eq!(changes.total_deletions, 2);
    }

    // Without --untracked-files=all git reports a wholly-new directory as a single `newdir/`
    // entry, which has no blob to diff and cannot be read as a file — the review pane
    // renders it blank. Every untracked file must be listed on its own.
    #[test]
    fn asks_git_to_list_every_untracked_file_not_just_the_containing_directory() {
        let exec = FakeExec::new(vec![
            out("?? newdir/inner.ts\n"),                         // status
            out(""),                                             // staged numstat
            out(""),                                             // unstaged numstat
            out_code("1\t0\t/dev/null => newdir/inner.ts\n", 1), // untracked line count
        ]);
        let git = api(&exec, FakeFs::missing());
        let changes = run(git.get_uncommitted_changes("/repo")).unwrap();
        assert_nth_call(
            &exec,
            1,
            "/repo",
            "git",
            &["status", "--porcelain", "--untracked-files=all"],
        );
        assert_eq!(changes.files.len(), 1);
        assert_eq!(changes.files[0].entry.path, "newdir/inner.ts");
        assert_eq!(changes.files[0].entry.status, FileChangeStatus::Untracked);
    }

    // `git diff --numstat` never reports untracked paths, so without the extra --no-index
    // pass every new file would show +0/-0.
    #[test]
    fn counts_lines_in_untracked_files_by_diffing_them_against_an_empty_blob() {
        let exec = FakeExec::new(vec![
            out("?? new.ts\n"), // status
            out(""),            // staged numstat
            out(""),            // unstaged numstat
            // --no-index exits 1 whenever the inputs differ, and names the pair rather than
            // the path alone.
            out_code("7\t0\t/dev/null => new.ts\n", 1),
        ]);
        let git = api(&exec, FakeFs::missing());
        let changes = run(git.get_uncommitted_changes("/repo")).unwrap();
        assert_nth_call(
            &exec,
            4,
            "/repo",
            "git",
            &[
                "diff",
                "--numstat",
                "--no-index",
                "--",
                "/dev/null",
                "new.ts",
            ],
        );
        assert_eq!(changes.files[0].stats, text(7, 0));
        assert_eq!(changes.total_additions, 7);
    }

    // One `git diff --no-index` per untracked file, all at once, flooded the daemon on a
    // worktree with thousands of untracked files over NFS.
    #[test]
    fn skips_per_file_stats_past_the_untracked_file_cap_instead_of_spawning_a_git_per_file() {
        let paths: Vec<String> = (0..201).map(|i| format!("gen/f{i}.ts")).collect();
        let status: String = paths.iter().map(|path| format!("?? {path}\n")).collect();
        let exec = FakeExec::new(vec![out(status), out(""), out("")]);
        let git = api(&exec, FakeFs::missing());
        let changes = run(git.get_uncommitted_changes("/repo")).unwrap();
        assert_eq!(exec.calls().len(), 3);
        assert_eq!(changes.files.len(), 201);
        assert!(
            changes
                .files
                .iter()
                .all(|file| file.stats == FileChangeStats::Unknown)
        );
        assert_eq!(changes.total_additions, 0);
    }

    #[test]
    fn runs_at_most_8_untracked_line_counts_at_a_time() {
        let paths: Vec<String> = (0..20).map(|i| format!("n{i}.ts")).collect();
        let status: String = paths.iter().map(|path| format!("?? {path}\n")).collect();
        let mut completions = vec![out(status), out(""), out("")];
        completions.extend((0..20).map(|_| out_code("1\t0\t/dev/null => x\n", 1)));
        let exec = FakeExec::new(completions);
        let git = api(&exec, FakeFs::missing());
        run(git.get_uncommitted_changes("/repo")).unwrap();
        assert_eq!(exec.calls().len(), 23);
        assert_eq!(exec.peak(), 8);
    }

    #[test]
    fn coalesces_overlapping_requests_for_the_same_repo_into_one_follow_up_scan() {
        let exec = FakeExec::new(vec![
            out("M  a.ts"),
            out("1\t0\ta.ts"),
            out(""),
            out("M  a.ts"),
            out("2\t0\ta.ts"),
            out(""),
        ]);
        let git = api(&exec, FakeFs::missing());
        let first = git.get_uncommitted_changes("/repo");
        let second = git.get_uncommitted_changes("/repo");
        let third = git.get_uncommitted_changes("/repo");
        assert!(second.ptr_eq(&third));
        let (r1, r2, r3) = run(futures::future::join3(first, second, third));
        assert_eq!(exec.calls().len(), 6);
        assert_eq!(r1.unwrap().total_additions, 1);
        let r2 = r2.unwrap();
        assert_eq!(r2.total_additions, 2);
        assert_eq!(r3.unwrap(), r2);
    }

    #[test]
    fn counts_a_binary_untracked_file_as_zero_changed_lines() {
        let exec = FakeExec::new(vec![
            out("?? logo.png\n"),
            out(""),
            out(""),
            out_code("-\t-\t/dev/null => logo.png\n", 1),
            out_code(" /dev/null => logo.png | Bin 0 -> 23 bytes\n", 1),
        ]);
        let git = api(&exec, FakeFs::missing());
        let changes = run(git.get_uncommitted_changes("/repo")).unwrap();
        assert_eq!(changes.files[0].stats, binary(23));
    }

    #[test]
    fn fails_loudly_when_the_untracked_line_count_errors_out() {
        let exec = FakeExec::new(vec![
            out("?? new.ts\n"),
            out(""),
            out(""),
            // exitCode 2+ is a real failure, unlike the 1 that merely means "differs"
            err("fatal: could not resolve", 2),
        ]);
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.get_uncommitted_changes("/repo")));
        assert!(error.contains("Could not resolve reference"), "{error}");
    }

    #[test]
    fn keeps_numstat_stats_for_tracked_files_keyed_by_path() {
        let exec = FakeExec::new(vec![
            out(" M src/app.ts\n?? new.ts\n"),
            out(""),
            out("3\t4\tsrc/app.ts\n"),
            out_code("1\t0\t/dev/null => new.ts\n", 1),
        ]);
        let git = api(&exec, FakeFs::missing());
        let changes = run(git.get_uncommitted_changes("/repo")).unwrap();
        let tracked = changes
            .files
            .iter()
            .find(|file| file.entry.path == "src/app.ts")
            .expect("tracked file");
        assert_eq!(tracked.stats, text(3, 4));
        assert_eq!(changes.total_additions, 4);
        assert_eq!(changes.total_deletions, 4);
    }

    #[test]
    fn returns_empty_result_for_clean_repo() {
        let exec = FakeExec::new(vec![
            out(""), // status
            out(""), // staged numstat
            out(""), // unstaged numstat
        ]);
        let git = api(&exec, FakeFs::missing());
        let changes = run(git.get_uncommitted_changes("/repo")).unwrap();
        assert!(changes.files.is_empty());
        assert_eq!(changes.total_additions, 0);
        assert_eq!(changes.total_deletions, 0);
    }

    #[test]
    fn returns_failure_when_status_fails() {
        let exec = FakeExec::new(vec![err("not a git repository", 1)]);
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.get_uncommitted_changes("/not-repo")));
        assert!(error.contains("Not a git repository"), "{error}");
    }
}

mod binary_uncommitted_comparisons {
    use super::*;

    fn uncommitted(path: &str, staged: Staging, stats: FileChangeStats) -> UncommittedFile {
        UncommittedFile {
            entry: GitStatusEntry {
                path: path.to_owned(),
                status: FileChangeStatus::Modified,
                staged,
                origin: PathOrigin::Same,
            },
            stats,
        }
    }

    #[test]
    fn tracks_staged_growth_separately_from_unstaged_shrinkage_of_the_same_path() {
        let exec = FakeExec::new(vec![
            out("MM asset.png\n M src/app.ts"),
            out("-\t-\tasset.png"),
            out("-\t-\tasset.png\n4\t2\tsrc/app.ts"),
            out(" asset.png | Bin 100 -> 150 bytes\n"),
            out(" asset.png | Bin 150 -> 120 bytes\n"),
        ]);
        let changes = run(api(&exec, FakeFs::missing()).get_uncommitted_changes("/repo")).unwrap();
        assert_eq!(
            changes.files,
            vec![
                uncommitted("asset.png", Staging::Staged, binary(50)),
                uncommitted("asset.png", Staging::Unstaged, binary(-30)),
                uncommitted("src/app.ts", Staging::Unstaged, text(4, 2)),
            ]
        );
        assert_eq!(changes.total_additions, 4);
        assert_eq!(changes.total_deletions, 2);
        assert_called_with(
            &exec,
            "/repo",
            "git",
            &["diff", "--cached", "--stat", "--", "asset.png"],
        );
        assert_called_with(
            &exec,
            "/repo",
            "git",
            &["diff", "--stat", "--", "asset.png"],
        );
    }

    #[test]
    fn compares_a_renamed_staged_binary_using_its_original_path() {
        let exec = FakeExec::new(vec![
            out("R  old name.png -> new name.png"),
            out("-\t-\told name.png => new name.png"),
            out(""),
            out(" old name.png => new name.png | Bin 100 -> 90 bytes\n"),
        ]);
        let changes = run(api(&exec, FakeFs::missing()).get_uncommitted_changes("/repo")).unwrap();
        assert_eq!(changes.files[0].entry.path, "new name.png");
        assert_eq!(changes.files[0].stats, binary(-10));
        assert_called_with(
            &exec,
            "/repo",
            "git",
            &[
                "diff",
                "--cached",
                "--stat",
                "--",
                "old name.png",
                "new name.png",
            ],
        );
    }
}

mod get_uncommitted_file_diff {
    use super::*;

    #[test]
    fn returns_diff_for_staged_file() {
        let exec = FakeExec::new(vec![out("@@ -1 +1 @@\n-old\n+new")]);
        let git = api(&exec, FakeFs::missing());
        let diff =
            run(git.get_uncommitted_file_diff("/repo", "src/app.ts", Staging::Staged)).unwrap();
        assert!(diff.contains("@@ -1 +1 @@"));
        assert_nth_call(
            &exec,
            1,
            "/repo",
            "git",
            &["diff", "--cached", "--", "src/app.ts"],
        );
    }

    #[test]
    fn returns_diff_for_unstaged_file() {
        let exec = FakeExec::new(vec![out("@@ -1 +1 @@\n-old\n+new")]);
        let git = api(&exec, FakeFs::missing());
        let diff =
            run(git.get_uncommitted_file_diff("/repo", "src/app.ts", Staging::Unstaged)).unwrap();
        assert!(diff.contains("@@ -1 +1 @@"));
        assert_nth_call(&exec, 1, "/repo", "git", &["diff", "--", "src/app.ts"]);
    }

    #[test]
    fn returns_failure_on_error() {
        let exec = FakeExec::new(vec![err("not a git repository", 1)]);
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.get_uncommitted_file_diff(
            "/not-repo",
            "src/app.ts",
            Staging::Staged,
        )));
        assert!(error.contains("Not a git repository"), "{error}");
    }
}

mod get_file_diff {
    use super::*;

    #[test]
    fn returns_diff_output_for_file() {
        let exec = FakeExec::new(vec![
            out("feature"),                 // current branch
            out("base123"),                 // merge base
            out("@@ -1 +1 @@\n-old\n+new"), // diff output
        ]);
        let git = api(&exec, FakeFs::missing());
        let diff = run(git.get_file_diff("/repo", "main", "src/app.ts")).unwrap();
        assert!(diff.contains("@@ -1 +1 @@"));
    }

    #[test]
    fn returns_failure_when_current_branch_fails() {
        let exec = FakeExec::new(vec![err("not a git repository", 1)]);
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.get_file_diff("/not-repo", "main", "src/app.ts")));
        assert!(error.contains("Not a git repository"), "{error}");
    }
}

mod fetch {
    use super::*;

    #[test]
    fn returns_success_on_success() {
        let exec = FakeExec::new(vec![out("")]);
        let git = api(&exec, FakeFs::missing());
        run(git.fetch("/repo")).unwrap();
        assert_eq!(
            exec.calls()[0].timeout,
            ExecTimeout::After(Duration::from_secs(60))
        );
    }

    #[test]
    fn returns_failure_on_error() {
        let exec = FakeExec::new(vec![err("could not resolve host", 1)]);
        let git = api(&exec, FakeFs::missing());
        assert_eq!(
            error_of(run(git.fetch("/repo"))),
            "git fetch failed: could not resolve host"
        );
    }
}

mod pull {
    use super::*;

    #[test]
    fn returns_success_on_exit_code_0() {
        let exec = FakeExec::new(vec![out("Already up to date.")]);
        let git = api(&exec, FakeFs::missing());
        run(git.pull("/repo")).unwrap();
    }

    #[test]
    fn returns_failure_with_error_message_on_non_zero_exit() {
        let exec = FakeExec::new(vec![err("merge conflict", 1)]);
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.pull("/repo")));
        assert!(error.contains("merge conflict"), "{error}");
    }

    #[test]
    fn uses_fallback_message_when_stderr_is_empty() {
        let exec = FakeExec::new(vec![out_code("", 1)]);
        let git = api(&exec, FakeFs::missing());
        assert_eq!(error_of(run(git.pull("/repo"))), "git pull failed");
    }
}

mod get_behind_count {
    use super::*;

    #[test]
    fn returns_parsed_count_on_success() {
        let exec = FakeExec::new(vec![out("5\n")]);
        let git = api(&exec, FakeFs::missing());
        assert_eq!(run(git.get_behind_count("/repo")).unwrap(), 5);
    }

    #[test]
    fn returns_0_on_failure() {
        let exec = FakeExec::new(vec![err("no upstream", 1)]);
        let git = api(&exec, FakeFs::missing());
        assert_eq!(run(git.get_behind_count("/repo")).unwrap(), 0);
    }

    #[test]
    fn returns_0_for_nan_output() {
        let exec = FakeExec::new(vec![out("not-a-number\n")]);
        let git = api(&exec, FakeFs::missing());
        assert_eq!(run(git.get_behind_count("/repo")).unwrap(), 0);
    }
}

mod get_remote_url {
    use super::*;

    #[test]
    fn returns_trimmed_url_on_success() {
        let exec = FakeExec::new(vec![out("git@github.com:user/repo.git\n")]);
        let git = api(&exec, FakeFs::missing());
        assert_eq!(
            run(git.get_remote_url("/repo")).unwrap(),
            "git@github.com:user/repo.git"
        );
    }

    #[test]
    fn returns_failure_on_error() {
        let exec = FakeExec::new(vec![err("No such remote", 1)]);
        let git = api(&exec, FakeFs::missing());
        assert_eq!(
            error_of(run(git.get_remote_url("/repo"))),
            "Failed to get remote URL: No such remote"
        );
    }
}

mod rename_branch {
    use super::*;

    #[test]
    fn returns_success_on_success() {
        let exec = FakeExec::new(vec![out("")]);
        let git = api(&exec, FakeFs::missing());
        run(git.rename_branch("/repo", "old", "new")).unwrap();
        assert_nth_call(&exec, 1, "/repo", "git", &["branch", "-m", "old", "new"]);
    }

    #[test]
    fn returns_failure_on_error() {
        let exec = FakeExec::new(vec![err("branch not found", 1)]);
        let git = api(&exec, FakeFs::missing());
        assert!(run(git.rename_branch("/repo", "old", "new")).is_err());
    }
}

mod get_log {
    use super::*;

    #[test]
    fn parses_commits_with_parent_branch() {
        let log_output = "abc123\x1eabc\x1eAuthor\x1e2024-01-01T00:00:00Z\x1ecommit msg\x1edef456";
        let exec = FakeExec::new(vec![out(log_output)]);
        let git = api(&exec, FakeFs::missing());
        let result = run(git.get_log("/repo", LogScope::Since("main".to_owned()), 0, 10)).unwrap();
        assert_eq!(result.commits.len(), 1);
        let commit = &result.commits[0];
        assert_eq!(commit.hash, "abc123");
        assert_eq!(commit.short_hash, "abc");
        assert_eq!(commit.author, "Author");
        assert_eq!(commit.message, "commit msg");
        assert_eq!(commit.parent_hashes, owned(&["def456"]));
        assert!(!result.has_more);
        assert_nth_call(
            &exec,
            1,
            "/repo",
            "git",
            &[
                "log",
                "--format=%H%x1e%h%x1e%an%x1e%aI%x1e%s%x1e%P",
                "--skip=0",
                "--max-count=11",
                "main..HEAD",
            ],
        );
    }

    #[test]
    fn parses_commits_without_parent_branch_null() {
        let exec = FakeExec::new(vec![out("abc\x1ea\x1eAuthor\x1e2024-01-01\x1emsg\x1e")]);
        let git = api(&exec, FakeFs::missing());
        let result = run(git.get_log("/repo", LogScope::All, 0, 10)).unwrap();
        assert_eq!(result.commits.len(), 1);
        assert!(result.commits[0].parent_hashes.is_empty());
        assert_eq!(exec.calls()[0].args.len(), 4, "no range argument");
    }

    #[test]
    fn detects_has_more_when_results_exceed_limit() {
        let lines = (0..3)
            .map(|i| format!("hash{i}\x1eh{i}\x1eAuthor\x1e2024-01-01\x1emsg{i}\x1e"))
            .collect::<Vec<_>>()
            .join("\n");
        let exec = FakeExec::new(vec![out(lines)]);
        let git = api(&exec, FakeFs::missing());
        let result = run(git.get_log("/repo", LogScope::All, 0, 2)).unwrap();
        assert!(result.has_more);
        assert_eq!(result.commits.len(), 2);
    }

    #[test]
    fn returns_failure_on_error() {
        let exec = FakeExec::new(vec![err("not a git repository", 1)]);
        let git = api(&exec, FakeFs::missing());
        assert!(run(git.get_log("/repo", LogScope::Since("main".to_owned()), 0, 10)).is_err());
    }
}

mod binary_commit_comparisons {
    use super::*;

    #[test]
    fn uses_root_size_stats_for_a_binary_in_the_initial_commit() {
        let exec = FakeExec::new(vec![
            out("-\t-\tnew.png"),
            out("A\tnew.png"),
            out(" new.png | Bin 0 -> 62 bytes\n"),
        ]);
        let files = run(api(&exec, FakeFs::missing()).get_commit_diff("/repo", "root123")).unwrap();
        assert_eq!(
            files,
            vec![diff_file("new.png", FileChangeStatus::Added, binary(62))]
        );
        assert_called_with(
            &exec,
            "/repo",
            "git",
            &[
                "diff-tree",
                "--no-commit-id",
                "--root",
                "-r",
                "--stat",
                "root123",
                "--",
                "new.png",
            ],
        );
    }

    #[test]
    fn counts_removed_binary_bytes_in_a_subsequent_commit() {
        let exec = FakeExec::new(vec![
            out("-\t-\told.png"),
            out("D\told.png"),
            out(" old.png | Bin 73 -> 0 bytes\n"),
        ]);
        let files =
            run(api(&exec, FakeFs::missing()).get_commit_diff("/repo", "commit123")).unwrap();
        assert_eq!(
            files,
            vec![diff_file("old.png", FileChangeStatus::Deleted, binary(-73))]
        );
    }
}

mod get_commit_diff {
    use super::*;

    #[test]
    fn parses_a_m_d_r_status_types() {
        let name_status = "A\tnew.ts\nM\tmod.ts\nD\tdel.ts\nR100\trenamed.ts";
        let numstat = "10\t0\tnew.ts\n5\t3\tmod.ts\n0\t10\tdel.ts\n2\t1\trenamed.ts";
        let exec = FakeExec::new(vec![out(numstat), out(name_status)]);
        let git = api(&exec, FakeFs::missing());
        let files = run(git.get_commit_diff("/repo", "abc123")).unwrap();
        assert_eq!(files.len(), 4);
        assert_eq!(find(&files, "new.ts").status, FileChangeStatus::Added);
        assert_eq!(find(&files, "mod.ts").status, FileChangeStatus::Modified);
        assert_eq!(find(&files, "del.ts").status, FileChangeStatus::Deleted);
        assert_eq!(find(&files, "renamed.ts").status, FileChangeStatus::Renamed);
    }

    #[test]
    fn handles_binary_files() {
        let exec = FakeExec::new(vec![
            out("-\t-\timage.png"),
            out("M\timage.png"),
            out(" image.png | Bin 100 -> 125 bytes\n 1 file changed"),
        ]);
        let git = api(&exec, FakeFs::missing());
        let files = run(git.get_commit_diff("/repo", "abc123")).unwrap();
        assert_eq!(files[0].stats, binary(25));
    }

    #[test]
    fn returns_failure_on_error() {
        let exec = FakeExec::new(vec![err("bad object", 1), err("bad object", 1)]);
        let git = api(&exec, FakeFs::missing());
        assert!(run(git.get_commit_diff("/repo", "bad")).is_err());
    }
}

mod get_commit_file_diff {
    use super::*;

    #[test]
    fn returns_file_contents_from_commit_and_parent() {
        let exec = FakeExec::new(vec![out("original content"), out("modified content")]);
        let git = api(&exec, FakeFs::missing());
        let contents = run(git.get_commit_file_diff("/repo", "abc123", "file.ts")).unwrap();
        assert_eq!(
            text_sides(&contents),
            ("original content", "modified content", "typescript")
        );
        assert_nth_call(&exec, 1, "/repo", "git", &["show", "abc123~1:file.ts"]);
        assert_nth_call(&exec, 2, "/repo", "git", &["show", "abc123:file.ts"]);
    }

    #[test]
    fn returns_empty_content_when_show_fails() {
        let exec = FakeExec::new(vec![err("path not found", 128), err("path not found", 128)]);
        let git = api(&exec, FakeFs::missing());
        let contents = run(git.get_commit_file_diff("/repo", "abc123", "file.ts")).unwrap();
        let (original, modified, _) = text_sides(&contents);
        assert_eq!((original, modified), ("", ""));
    }
}

mod binary_and_image_diff_contents {
    use super::*;

    #[test]
    fn loads_image_blobs_base64_encoded_through_sh() {
        let exec = FakeExec::new(vec![out("AAAA\nBBBB\n"), out_code("", 1)]);
        let git = api(&exec, FakeFs::missing());
        let contents = run(git.get_commit_file_diff("/repo", "abc123", "img/logo.png")).unwrap();
        assert_eq!(
            contents,
            FileDiffContents::Image {
                original_base64: "AAAABBBB".to_owned(),
                modified_base64: String::new(),
            }
        );
        assert_called_with(
            &exec,
            "/repo",
            "sh",
            &[
                "-c",
                "git show \"$1\" | base64",
                "sh",
                "abc123~1:img/logo.png",
            ],
        );
    }

    // The TS filesystem mock returned the base64 Electron's main sends images as; the native
    // client reads raw bytes and encodes them itself, with the same result.
    #[test]
    fn reads_the_working_tree_image_from_the_filesystem() {
        let exec = FakeExec::new(vec![out("SU5ERVg=\n")]);
        let git = api(&exec, FakeFs::with_content(b"WORK"));
        let contents =
            run(git.get_uncommitted_file_contents_for_diff("/repo", "logo.png", Staging::Unstaged))
                .unwrap();
        assert_eq!(
            contents,
            FileDiffContents::Image {
                original_base64: "SU5ERVg=".to_owned(),
                modified_base64: "V09SSw==".to_owned(),
            }
        );
    }

    // There is no base64 hop in the native client: the model's raw bytes are read as text.
    #[test]
    fn diffs_a_working_tree_3d_model_as_text_decoding_the_base64_main_sends_it_as() {
        let exec = FakeExec::new(vec![out("v 0 0 0\n")]);
        let git = api(&exec, FakeFs::with_content(b"v 0 0 1\n"));
        let contents =
            run(git.get_uncommitted_file_contents_for_diff("/repo", "mesh.obj", Staging::Unstaged))
                .unwrap();
        assert_eq!(
            contents,
            FileDiffContents::Text {
                original_content: "v 0 0 0\n".to_owned(),
                modified_content: "v 0 0 1\n".to_owned(),
                language: "plaintext".to_owned(),
            }
        );
    }

    #[test]
    fn reports_a_binary_3d_model_in_the_working_tree_as_binary() {
        let exec = FakeExec::new(vec![out("stl\0\x01")]);
        let git = api(&exec, FakeFs::with_content(b"stl\0\0"));
        let contents =
            run(git.get_uncommitted_file_contents_for_diff("/repo", "part.stl", Staging::Unstaged))
                .unwrap();
        assert_eq!(contents, FileDiffContents::Binary);
    }

    #[test]
    fn reports_binary_content_without_returning_it_as_text() {
        let exec = FakeExec::new(vec![out("text"), out("ELF\0\0")]);
        let git = api(&exec, FakeFs::missing());
        let contents =
            run(git.get_uncommitted_file_contents_for_diff("/repo", "app.bin", Staging::Staged))
                .unwrap();
        assert_eq!(contents, FileDiffContents::Binary);
    }
}

mod create_worktree {
    use super::*;

    #[test]
    fn creates_worktree_in_worktrees_when_gitignored() {
        let exec = FakeExec::new(vec![
            out("worktree /repo\n"), // worktree list --porcelain (resolveRepoRoot)
            out(""),                 // check-ignore .worktrees (exit 0 = ignored)
            out(""),                 // worktree add
        ]);
        let git = api(&exec, FakeFs::missing());
        let worktree =
            run(git.create_worktree("/repo", "feature", WorktreeBase::Head, Progress::Silent))
                .unwrap();
        assert!(
            worktree.path.contains(".worktrees/feature"),
            "{}",
            worktree.path
        );
        assert_eq!(worktree.branch, "feature");
        assert_nth_call(&exec, 2, "/repo", "git", &["check-ignore", ".worktrees"]);
    }

    // Checking out a large LFS repo takes over the default 30s exec timeout, which would
    // SIGTERM git midway through the fork.
    #[test]
    fn gives_worktree_add_a_longer_exec_timeout_than_the_default() {
        let exec = FakeExec::new(vec![out("worktree /repo\n"), out(""), out("")]);
        let git = api(&exec, FakeFs::missing());
        run(git.create_worktree("/repo", "feature", WorktreeBase::Head, Progress::Silent)).unwrap();
        let calls = exec.calls();
        let add = calls
            .iter()
            .find(|call| {
                call.args.first().map(String::as_str) == Some("worktree")
                    && call.args.get(1).map(String::as_str) == Some("add")
            })
            .expect("worktree add call");
        let ExecTimeout::After(timeout) = add.timeout else {
            panic!("worktree add without a timeout");
        };
        assert!(timeout > DEFAULT_EXEC_TIMEOUT);
    }

    #[test]
    fn creates_worktree_in_home_dir_when_not_gitignored() {
        let exec = FakeExec::new(vec![
            out("worktree /repo\n"), // worktree list --porcelain (resolveRepoRoot)
            err(".worktrees is not ignored", 1), // check-ignore (exit 1 = not ignored)
            out("/home/user"),       // resolveHomedir (echo $HOME)
            out(""),                 // worktree add
        ]);
        let git = api(&exec, FakeFs::missing());
        let worktree =
            run(git.create_worktree("/repo", "feature", WorktreeBase::Head, Progress::Silent))
                .unwrap();
        assert_eq!(worktree.path, "/home/user/.treeterm/worktrees/repo/feature");
        assert_eq!(worktree.branch, "feature");
        assert_nth_call(&exec, 3, "/", "sh", &["-c", "echo $HOME"]);
    }

    #[test]
    fn adds_base_branch_to_args_when_provided() {
        let exec = FakeExec::new(vec![
            out("worktree /repo\n"), // worktree list --porcelain (resolveRepoRoot)
            out(""),                 // gitignored
            out(""),                 // worktree add
        ]);
        let git = api(&exec, FakeFs::missing());
        run(git.create_worktree(
            "/repo",
            "feature",
            WorktreeBase::Ref("main".to_owned()),
            Progress::Silent,
        ))
        .unwrap();
        let calls = exec.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(
            calls[2].args,
            owned(&[
                "worktree",
                "add",
                "-b",
                "feature",
                "/repo/.worktrees/feature",
                "main"
            ])
        );
    }

    #[test]
    fn returns_failure_when_rev_parse_fails() {
        let exec = FakeExec::new(vec![err("not a git repository", 1)]);
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.create_worktree(
            "/repo",
            "feature",
            WorktreeBase::Head,
            Progress::Silent,
        )));
        assert_eq!(error, "Failed to resolve repository root");
    }
}

mod create_worktree_from_branch {
    use super::*;

    #[test]
    fn creates_worktree_from_existing_branch() {
        let exec = FakeExec::new(vec![
            out("worktree /repo\n"), // worktree list --porcelain (resolveRepoRoot)
            out(""),                 // check-ignore
            out(""),                 // worktree add
        ]);
        let git = api(&exec, FakeFs::missing());
        let worktree = run(git.create_worktree_from_branch(
            "/repo",
            "develop",
            "develop-wt",
            Progress::Silent,
        ))
        .unwrap();
        assert_eq!(worktree.branch, "develop");
        assert_eq!(
            exec.calls()[2].args,
            owned(&["worktree", "add", "/repo/.worktrees/develop-wt", "develop"])
        );
    }
}

mod create_worktree_from_remote {
    use super::*;

    #[test]
    fn strips_remote_prefix_from_branch_name() {
        let exec = FakeExec::new(vec![
            out("worktree /repo\n"), // worktree list --porcelain (resolveRepoRoot)
            out(""),                 // check-ignore
            out(""),                 // worktree add
        ]);
        let git = api(&exec, FakeFs::missing());
        let worktree = run(git.create_worktree_from_remote(
            "/repo",
            "origin/feature",
            "feature-wt",
            Progress::Silent,
        ))
        .unwrap();
        assert_eq!(worktree.branch, "feature");
        assert_eq!(
            exec.calls()[2].args,
            owned(&[
                "worktree",
                "add",
                "-b",
                "feature",
                "/repo/.worktrees/feature-wt",
                "origin/feature"
            ])
        );
    }
}

mod remove_worktree {
    use super::*;

    #[test]
    fn streams_every_command_with_a_10_minute_removal_timeout() {
        let exec = FakeExec::new(vec![
            out("feature\n"),                // rev-parse --abbrev-ref HEAD
            out(""),                         // worktree remove
            out("Deleted branch feature\n"), // branch -D
        ]);
        let git = api(&exec, FakeFs::missing());
        let log = ProgressLog::new();
        let report = |text: &str| log.record(text);
        run(git.remove_worktree(
            "/repo",
            "/repo/.worktrees/feature",
            BranchDisposal::Delete,
            Progress::Report(&report),
        ))
        .unwrap();
        assert_eq!(
            exec.calls()[1].timeout,
            ExecTimeout::After(Duration::from_secs(10 * 60))
        );
        assert_eq!(
            log.calls(),
            vec![
                "$ git rev-parse --abbrev-ref HEAD\n",
                "feature\n",
                "$ git worktree remove /repo/.worktrees/feature --force\n",
                "$ git branch -D feature\n",
                "Deleted branch feature\n",
            ]
        );
    }

    #[test]
    fn removes_worktree_without_branch_deletion() {
        let exec = FakeExec::new(vec![
            out(""), // worktree remove
        ]);
        let git = api(&exec, FakeFs::missing());
        run(git.remove_worktree(
            "/repo",
            "/repo/.worktrees/feature",
            BranchDisposal::Keep,
            Progress::Silent,
        ))
        .unwrap();
    }

    #[test]
    fn removes_worktree_with_branch_deletion() {
        let exec = FakeExec::new(vec![
            out("feature\n"), // rev-parse --abbrev-ref HEAD
            out(""),          // worktree remove
            out(""),          // branch -D
        ]);
        let git = api(&exec, FakeFs::missing());
        run(git.remove_worktree(
            "/repo",
            "/repo/.worktrees/feature",
            BranchDisposal::Delete,
            Progress::Silent,
        ))
        .unwrap();
        assert_nth_call(
            &exec,
            1,
            "/repo/.worktrees/feature",
            "git",
            &["rev-parse", "--abbrev-ref", "HEAD"],
        );
        assert_nth_call(&exec, 3, "/repo", "git", &["branch", "-D", "feature"]);
    }

    #[test]
    fn handles_branch_deletion_failure_gracefully() {
        let exec = FakeExec::new(vec![
            out("feature\n"),                 // rev-parse
            out(""),                          // worktree remove
            err("branch deletion failed", 1), // branch -D fails
        ]);
        let git = api(&exec, FakeFs::missing());
        // Should not fail despite branch deletion failure
        run(git.remove_worktree(
            "/repo",
            "/repo/.worktrees/feature",
            BranchDisposal::Delete,
            Progress::Silent,
        ))
        .unwrap();
    }

    #[test]
    fn handles_get_branch_failure_gracefully_when_delete_branch_is_true() {
        let exec = FakeExec::new(vec![
            err("cannot get branch", 1), // rev-parse fails
            out(""),                     // worktree remove still succeeds
        ]);
        let git = api(&exec, FakeFs::missing());
        run(git.remove_worktree(
            "/repo",
            "/repo/.worktrees/feature",
            BranchDisposal::Delete,
            Progress::Silent,
        ))
        .unwrap();
        assert_eq!(exec.calls().len(), 2);
    }
}

mod check_merge_conflicts {
    use super::*;

    const TREE_OID: &str = "b86bcd7cdc13608e0904c28f3c0876b06ef8bbd1";

    #[test]
    fn returns_no_conflicts_for_clean_merge() {
        let exec = FakeExec::new(vec![out(format!("{TREE_OID}\0"))]);
        let git = api(&exec, FakeFs::missing());
        let conflicts = run(git.check_merge_conflicts("/repo", "feature", "main")).unwrap();
        assert!(!conflicts.has_conflicts);
        assert!(conflicts.conflicted_files.is_empty());
        assert!(conflicts.messages.is_empty());
    }

    #[test]
    fn detects_conflicts_including_files_deleted_on_the_target_and_paths_without_an_extension() {
        let exec = FakeExec::new(vec![out_code(
            format!("{TREE_OID}\0Makefile\0src/app.ts\0"),
            1,
        )]);
        let git = api(&exec, FakeFs::missing());
        let conflicts = run(git.check_merge_conflicts("/repo", "feature", "main")).unwrap();
        assert_called_with(
            &exec,
            "/repo",
            "git",
            &[
                "merge-tree",
                "--write-tree",
                "--name-only",
                "--no-messages",
                "-z",
                "main",
                "feature",
            ],
        );
        assert!(conflicts.has_conflicts);
        assert_eq!(
            conflicts.conflicted_files,
            owned(&["Makefile", "src/app.ts"])
        );
        assert_eq!(conflicts.messages, owned(&["2 conflicting files"]));
    }

    #[test]
    fn fails_when_git_cannot_run_the_merge_e_g_unknown_ref() {
        let exec = FakeExec::new(vec![err(
            "merge-tree: nope - not something we can merge",
            1,
        )]);
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.check_merge_conflicts("/repo", "nope", "main")));
        assert!(error.contains("not something we can merge"), "{error}");
    }
}

mod merge {
    use super::*;

    #[test]
    fn merges_without_squash() {
        let exec = FakeExec::new(vec![out("")]);
        let git = api(&exec, FakeFs::missing());
        run(git.merge("/repo", "feature", MergeMode::Merge, Progress::Silent)).unwrap();
        assert_eq!(exec.calls()[0].args, owned(&["merge", "feature"]));
    }

    #[test]
    fn merges_with_squash() {
        let exec = FakeExec::new(vec![out("")]);
        let git = api(&exec, FakeFs::missing());
        run(git.merge("/repo", "feature", MergeMode::Squash, Progress::Silent)).unwrap();
        assert_eq!(
            exec.calls()[0].args,
            owned(&["merge", "--squash", "feature"])
        );
    }

    #[test]
    fn returns_failure_on_error() {
        let exec = FakeExec::new(vec![err("merge conflict detected", 1)]);
        let git = api(&exec, FakeFs::missing());
        assert!(run(git.merge("/repo", "feature", MergeMode::Merge, Progress::Silent)).is_err());
    }

    #[test]
    fn allows_5_minutes_for_large_repos_and_streams_output_to_on_progress() {
        let exec = FakeExec::new(vec![out("Updating abc..def\nFast-forward\n")]);
        let git = api(&exec, FakeFs::missing());
        let log = ProgressLog::new();
        let report = |text: &str| log.record(text);
        run(git.merge(
            "/repo",
            "feature",
            MergeMode::Merge,
            Progress::Report(&report),
        ))
        .unwrap();
        assert_eq!(
            exec.calls()[0].timeout,
            ExecTimeout::After(Duration::from_secs(5 * 60))
        );
        assert_eq!(
            log.calls(),
            vec!["$ git merge feature\n", "Updating abc..def\nFast-forward\n"]
        );
    }
}

mod get_uncommitted_file_contents_for_diff {
    use super::*;

    #[test]
    fn returns_staged_file_contents() {
        let exec = FakeExec::new(vec![out("original from HEAD"), out("modified from index")]);
        let git = api(&exec, FakeFs::missing());
        let contents =
            run(git.get_uncommitted_file_contents_for_diff("/repo", "src/app.ts", Staging::Staged))
                .unwrap();
        assert_eq!(
            text_sides(&contents),
            ("original from HEAD", "modified from index", "typescript")
        );
        assert_nth_call(&exec, 1, "/repo", "git", &["show", "HEAD:src/app.ts"]);
        assert_nth_call(&exec, 2, "/repo", "git", &["show", ":src/app.ts"]);
    }

    #[test]
    fn returns_unstaged_file_contents_with_filesystem_read() {
        let exec = FakeExec::new(vec![out("index content")]);
        let git = api(&exec, FakeFs::with_content(b"working tree content"));
        let contents = run(git.get_uncommitted_file_contents_for_diff(
            "/repo",
            "src/app.ts",
            Staging::Unstaged,
        ))
        .unwrap();
        let (original, modified, _) = text_sides(&contents);
        assert_eq!(
            (original, modified),
            ("index content", "working tree content")
        );
    }

    #[test]
    fn returns_empty_content_when_unstaged_filesystem_read_fails() {
        let exec = FakeExec::new(vec![out("index content")]);
        let git = api(
            &exec,
            FakeFs::new(FakeFile::Unreachable("file not found".to_owned())),
        );
        let contents = run(git.get_uncommitted_file_contents_for_diff(
            "/repo",
            "src/deleted.ts",
            Staging::Unstaged,
        ))
        .unwrap();
        let (original, modified, _) = text_sides(&contents);
        assert_eq!((original, modified), ("index content", ""));
    }

    #[test]
    fn returns_empty_original_when_head_show_fails() {
        let exec = FakeExec::new(vec![err("path not found", 128), out("new staged content")]);
        let git = api(&exec, FakeFs::missing());
        let contents = run(git.get_uncommitted_file_contents_for_diff(
            "/repo",
            "new-file.ts",
            Staging::Staged,
        ))
        .unwrap();
        let (original, modified, _) = text_sides(&contents);
        assert_eq!((original, modified), ("", "new staged content"));
    }
}

mod get_branches_in_worktrees {
    use super::*;

    #[test]
    fn returns_branch_names_from_worktrees() {
        let output = [
            "worktree /repo",
            "HEAD abc",
            "branch refs/heads/main",
            "",
            "worktree /repo/.wt/f",
            "HEAD def",
            "branch refs/heads/feature",
            "",
        ]
        .join("\n");
        let exec = FakeExec::new(vec![out(output)]);
        let git = api(&exec, FakeFs::missing());
        assert_eq!(
            run(git.get_branches_in_worktrees("/repo")).unwrap(),
            owned(&["main", "feature"])
        );
    }
}

mod remove_worktree_failure_branches {
    use super::*;

    #[test]
    fn returns_failure_when_worktree_remove_fails() {
        let exec = FakeExec::new(vec![
            err("worktree is dirty", 1), // worktree remove fails
        ]);
        let git = api(&exec, FakeFs::missing());
        assert!(
            run(git.remove_worktree(
                "/repo",
                "/repo/.worktrees/feature",
                BranchDisposal::Keep,
                Progress::Silent
            ))
            .is_err()
        );
    }

    #[test]
    fn returns_failure_when_worktree_remove_fails_with_delete_branch() {
        let exec = FakeExec::new(vec![
            out("feature\n"),            // rev-parse
            err("worktree is dirty", 1), // worktree remove fails
        ]);
        let git = api(&exec, FakeFs::missing());
        assert!(
            run(git.remove_worktree(
                "/repo",
                "/repo/.worktrees/feature",
                BranchDisposal::Delete,
                Progress::Silent
            ))
            .is_err()
        );
        assert_eq!(
            exec.calls().len(),
            2,
            "no branch deletion after a failed removal"
        );
    }

    #[test]
    fn catches_thrown_errors() {
        let exec = FakeExec::always(reject("connection lost"));
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.remove_worktree(
            "/repo",
            "/repo/.worktrees/feature",
            BranchDisposal::Keep,
            Progress::Silent,
        )));
        assert!(error.contains("connection lost"), "{error}");
    }
}

/// `describe(<worktree creation> (failure branches))`: no repo root, a failing add, and a
/// lost daemon.
macro_rules! worktree_creation_failure_branches {
    ($module:ident, |$git:ident| $call:expr) => {
        mod $module {
            use super::*;

            #[test]
            fn returns_failure_when_rev_parse_fails() {
                let exec = FakeExec::new(vec![err("not a git repository", 1)]);
                let $git = api(&exec, FakeFs::missing());
                assert!(run($call).is_err());
            }

            #[test]
            fn returns_failure_when_worktree_add_fails() {
                let exec = FakeExec::new(vec![
                    out("worktree /repo\n"), // worktree list --porcelain (resolveRepoRoot)
                    out(""),                 // check-ignore
                    err("already exists", 1), // worktree add fails
                ]);
                let $git = api(&exec, FakeFs::missing());
                let error = error_of(run($call));
                assert!(error.contains("Already exists"), "{error}");
            }

            #[test]
            fn catches_thrown_errors() {
                let exec = FakeExec::always(reject("daemon crash"));
                let $git = api(&exec, FakeFs::missing());
                let error = error_of(run($call));
                assert!(error.contains("daemon crash"), "{error}");
            }
        }
    };
}

worktree_creation_failure_branches!(create_worktree_from_branch_failure_branches, |git| git
    .create_worktree_from_branch("/repo", "develop", "wt", Progress::Silent));
worktree_creation_failure_branches!(create_worktree_from_remote_failure_branches, |git| git
    .create_worktree_from_remote("/repo", "origin/feature", "wt", Progress::Silent));

mod get_diff_failure_branches {
    use super::*;

    #[test]
    fn returns_failure_when_current_branch_rev_parse_fails() {
        let exec = FakeExec::new(vec![err("not a git repo", 1)]);
        let git = api(&exec, FakeFs::missing());
        assert!(run(git.get_diff("/repo", "main")).is_err());
    }

    #[test]
    fn returns_failure_when_merge_base_fails() {
        let exec = FakeExec::new(vec![
            out("feature"),               // current branch
            err("no common ancestor", 1), // merge-base fails
        ]);
        let git = api(&exec, FakeFs::missing());
        assert!(run(git.get_diff("/repo", "main")).is_err());
    }

    #[test]
    fn parses_d_and_r_status_types() {
        let exec = FakeExec::new(vec![
            out("feature"),
            out("base123"),
            out("0\t10\tdel.ts\n2\t1\trenamed.ts"),
            out("D\tdel.ts\nR100\trenamed.ts"),
        ]);
        let git = api(&exec, FakeFs::missing());
        let diff = run(git.get_diff("/repo", "main")).unwrap();
        assert_eq!(
            find(&diff.files, "del.ts").status,
            FileChangeStatus::Deleted
        );
        assert_eq!(
            find(&diff.files, "renamed.ts").status,
            FileChangeStatus::Renamed
        );
    }

    #[test]
    fn catches_thrown_errors() {
        let exec = FakeExec::always(reject("connection lost"));
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.get_diff("/repo", "main")));
        assert!(error.contains("connection lost"), "{error}");
    }
}

mod get_file_diff_failure_branches {
    use super::*;

    #[test]
    fn returns_failure_when_merge_base_fails() {
        let exec = FakeExec::new(vec![out("feature"), err("no common ancestor", 1)]);
        let git = api(&exec, FakeFs::missing());
        assert!(run(git.get_file_diff("/repo", "main", "src/app.ts")).is_err());
    }

    #[test]
    fn returns_failure_when_diff_command_fails() {
        let exec = FakeExec::new(vec![out("feature"), out("base123"), err("diff failed", 1)]);
        let git = api(&exec, FakeFs::missing());
        assert!(run(git.get_file_diff("/repo", "main", "src/app.ts")).is_err());
    }

    #[test]
    fn catches_thrown_errors() {
        let exec = FakeExec::always(reject("connection lost"));
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.get_file_diff("/repo", "main", "src/app.ts")));
        assert!(error.contains("connection lost"), "{error}");
    }
}

mod get_file_contents_for_diff_failure_branches {
    use super::*;

    #[test]
    fn returns_empty_content_when_original_show_fails() {
        let exec = FakeExec::new(vec![
            out("feature"),
            out("base123"),
            err("path not found", 128), // original fails
            out("modified content"),
        ]);
        let git = api(&exec, FakeFs::missing());
        let contents = run(git.get_file_contents_for_diff("/repo", "main", "new-file.ts")).unwrap();
        let (original, modified, _) = text_sides(&contents);
        assert_eq!((original, modified), ("", "modified content"));
    }

    #[test]
    fn returns_empty_content_when_modified_show_fails() {
        let exec = FakeExec::new(vec![
            out("feature"),
            out("base123"),
            out("original content"),
            err("path not found", 128), // modified fails
        ]);
        let git = api(&exec, FakeFs::missing());
        let contents =
            run(git.get_file_contents_for_diff("/repo", "main", "deleted-file.ts")).unwrap();
        let (original, modified, _) = text_sides(&contents);
        assert_eq!((original, modified), ("original content", ""));
    }

    #[test]
    fn catches_thrown_errors() {
        let exec = FakeExec::always(reject("connection lost"));
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(
            git.get_file_contents_for_diff("/repo", "main", "file.ts")
        ));
        assert!(error.contains("connection lost"), "{error}");
    }
}

mod get_info_catch_branch {
    use super::*;

    #[test]
    fn returns_non_repo_when_exec_throws() {
        let exec = FakeExec::always(reject("daemon crash"));
        let git = api(&exec, FakeFs::missing());
        assert_eq!(run(git.get_info("/repo")), GitInfo::NotRepo);
    }
}

catches_thrown_errors!(check_merge_conflicts_catch_branch, "daemon down", |git| git
    .check_merge_conflicts("/repo", "feature", "main"));
catches_thrown_errors!(merge_catch_branch, "daemon down", |git| git.merge(
    "/repo",
    "feature",
    MergeMode::Merge,
    Progress::Silent
));

mod has_uncommitted_changes_edge_cases {
    use super::*;

    #[test]
    fn returns_false_when_exit_code_is_non_zero() {
        let exec = FakeExec::new(vec![err("error", 1)]);
        let git = api(&exec, FakeFs::missing());
        assert!(!run(git.has_uncommitted_changes("/repo")).unwrap());
    }
}

mod get_uncommitted_file_contents_for_diff_edge_cases {
    use super::*;

    #[test]
    fn returns_empty_modified_when_unstaged_read_file_returns_failure() {
        let exec = FakeExec::new(vec![out("index content")]);
        let git = api(&exec, FakeFs::new(FakeFile::Fails("not found".to_owned())));
        let contents = run(git.get_uncommitted_file_contents_for_diff(
            "/repo",
            "src/app.ts",
            Staging::Unstaged,
        ))
        .unwrap();
        let (original, modified, _) = text_sides(&contents);
        assert_eq!((original, modified), ("index content", ""));
    }

    #[test]
    fn returns_empty_original_when_unstaged_show_fails() {
        let exec = FakeExec::new(vec![
            err("path not found", 128), // show :filePath fails
        ]);
        let git = api(&exec, FakeFs::with_content(b"working"));
        let contents = run(git.get_uncommitted_file_contents_for_diff(
            "/repo",
            "new-file.ts",
            Staging::Unstaged,
        ))
        .unwrap();
        let (original, modified, _) = text_sides(&contents);
        assert_eq!((original, modified), ("", "working"));
    }

    #[test]
    fn returns_empty_modified_when_staged_show_fails() {
        let exec = FakeExec::new(vec![
            out("original from HEAD"),
            err("path not found", 128), // show :filePath fails
        ]);
        let git = api(&exec, FakeFs::missing());
        let contents =
            run(git.get_uncommitted_file_contents_for_diff("/repo", "deleted.ts", Staging::Staged))
                .unwrap();
        let (original, modified, _) = text_sides(&contents);
        assert_eq!((original, modified), ("original from HEAD", ""));
    }

    // Not in the TS suite: a working-tree file over the read budget shows as empty, like
    // any other unreadable side.
    #[test]
    fn returns_empty_modified_when_the_working_tree_file_is_over_the_read_budget() {
        let exec = FakeExec::new(vec![out("index content")]);
        let git = api(
            &exec,
            FakeFs::new(FakeFile::TooLarge(MAX_READ_FILE_BYTES + 1)),
        );
        let contents =
            run(git.get_uncommitted_file_contents_for_diff("/repo", "big.ts", Staging::Unstaged))
                .unwrap();
        let (original, modified, _) = text_sides(&contents);
        assert_eq!((original, modified), ("index content", ""));
    }
}

catches_thrown_errors!(commit_all_catch_branch, "daemon down", |git| git
    .commit_all("/repo", "msg", Progress::Silent));
catches_thrown_errors!(commit_staged_catch_branch, "daemon down", |git| git
    .commit_staged("/repo", "msg"));
catches_thrown_errors!(stage_file_catch_branch, "daemon down", |git| git
    .stage_file("/repo", "file.ts"));
catches_thrown_errors!(unstage_file_catch_branch, "daemon down", |git| git
    .unstage_file("/repo", "file.ts"));
catches_thrown_errors!(stage_all_catch_branch, "daemon down", |git| git
    .stage_all("/repo"));
catches_thrown_errors!(unstage_all_catch_branch, "daemon down", |git| git
    .unstage_all("/repo"));
catches_thrown_errors!(delete_branch_catch_branch, "daemon down", |git| git
    .delete_branch("/repo", "feature", Progress::Silent));
catches_thrown_errors!(rename_branch_catch_branch, "daemon down", |git| git
    .rename_branch("/repo", "old", "new"));
catches_thrown_errors!(get_head_commit_hash_catch_branch, "daemon down", |git| git
    .get_head_commit_hash("/repo"));
catches_thrown_errors!(get_log_catch_branch, "daemon down", |git| git.get_log(
    "/repo",
    LogScope::Since("main".to_owned()),
    0,
    10
));
catches_thrown_errors!(get_commit_diff_catch_branch, "daemon down", |git| git
    .get_commit_diff("/repo", "abc123"));
catches_thrown_errors!(get_commit_file_diff_catch_branch, "daemon down", |git| git
    .get_commit_file_diff("/repo", "abc123", "file.ts"));
catches_thrown_errors!(fetch_catch_branch, "daemon down", |git| git.fetch("/repo"));
catches_thrown_errors!(pull_catch_branch, "daemon down", |git| git.pull("/repo"));
catches_thrown_errors!(get_remote_url_catch_branch, "daemon down", |git| git
    .get_remote_url("/repo"));
catches_thrown_errors!(get_uncommitted_changes_catch_branch, "daemon down", |git| {
    git.get_uncommitted_changes("/repo")
});
catches_thrown_errors!(
    get_uncommitted_file_diff_catch_branch,
    "daemon down",
    |git| git.get_uncommitted_file_diff("/repo", "file.ts", Staging::Staged)
);

mod create_worktree_catch_branch {
    use super::*;

    #[test]
    fn catches_thrown_errors_from_worktree_add() {
        // worktree list succeeds, check-ignore succeeds, but worktree add throws
        let exec = FakeExec::new(vec![out("worktree /repo\n"), out(""), reject("disk full")]);
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.create_worktree(
            "/repo",
            "feature",
            WorktreeBase::Head,
            Progress::Silent,
        )));
        assert!(error.contains("disk full"), "{error}");
    }
}

mod get_uncommitted_changes_unstaged_stats {
    use super::*;

    #[test]
    fn returns_unstaged_file_stats() {
        let exec = FakeExec::new(vec![
            out(" M src/app.ts"),    // status (unstaged)
            out(""),                 // staged numstat
            out("3\t1\tsrc/app.ts"), // unstaged numstat
        ]);
        let git = api(&exec, FakeFs::missing());
        let changes = run(git.get_uncommitted_changes("/repo")).unwrap();
        assert_eq!(changes.files.len(), 1);
        assert_eq!(changes.files[0].stats, text(3, 1));
    }

    #[test]
    fn handles_binary_files_in_stats() {
        let exec = FakeExec::new(vec![
            out("M  image.png"),
            out("-\t-\timage.png"), // binary in staged numstat
            out(""),
            out(" image.png | Bin 100 -> 125 bytes\n"),
        ]);
        let git = api(&exec, FakeFs::missing());
        let changes = run(git.get_uncommitted_changes("/repo")).unwrap();
        assert_eq!(changes.files[0].stats, binary(25));
    }
}

mod exec_edge_cases {
    use super::*;

    #[test]
    fn handles_exec_start_failure() {
        let exec = FakeExec::always(reject("daemon down"));
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.fetch("/repo")));
        assert!(error.contains("daemon down"), "{error}");
    }

    #[test]
    fn handles_error_event_from_exec() {
        let exec = FakeExec::always(Completion::ErrorEvent("stream broke".to_owned()));
        let git = api(&exec, FakeFs::missing());
        let error = error_of(run(git.fetch("/repo")));
        assert!(error.contains("stream broke"), "{error}");
    }

    // Not in the TS suite: progress decodes a character split across output chunks once its
    // last byte arrives.
    #[test]
    fn progress_holds_back_a_character_split_across_chunks() {
        let mut stream = Utf8Stream::default();
        let snowman = "\u{2603}".as_bytes();
        assert_eq!(stream.push(&[b'a', snowman[0]]), "a");
        assert_eq!(stream.push(&snowman[1..2]), "");
        assert_eq!(stream.push(&[snowman[2], b'b']), "\u{2603}b");
        assert_eq!(stream.push(&[0xE2]), "");
        assert_eq!(stream.finish(), "\u{FFFD}");
    }
}

/// Not in the TS suite: properties of the native client itself.
mod native_only {
    use super::*;

    fn assert_send<T: Send>(_: &T) {}

    fn assert_sync<T: Send + Sync>() {}

    /// Models run git work on background executors, so the client and its futures must
    /// cross threads. Building the futures runs nothing: they are lazy.
    #[test]
    fn the_client_and_its_futures_are_send() {
        assert_sync::<GitApi>();
        let exec = FakeExec::new(Vec::new());
        let git = api(&exec, FakeFs::missing());
        let report = |_: &str| {};
        assert_send(&git.get_info("/repo"));
        assert_send(&git.get_diff("/repo", "main"));
        assert_send(&git.get_uncommitted_changes("/repo"));
        assert_send(&git.get_uncommitted_file_contents_for_diff(
            "/repo",
            "a.png",
            Staging::Unstaged,
        ));
        assert_send(&git.remove_worktree(
            "/repo",
            "/wt",
            BranchDisposal::Delete,
            Progress::Report(&report),
        ));
        assert!(exec.calls().is_empty());
    }
}
