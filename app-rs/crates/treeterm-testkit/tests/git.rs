//! The git client against real git, run through a real in-process daemon.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use treeterm_core::exec::ExecClient;
use treeterm_core::fs::FsClient;
use treeterm_core::git::{
    BranchDisposal, FileChangeStats, FileChangeStatus, FileDiffContents, GitApi, GitInfo, LogScope,
    Progress, Staging, WorktreeBase,
};
use treeterm_core::rt::Rt;
use treeterm_core::transport::{connect_unix, daemon_client};
use treeterm_testkit::TestDaemon;

async fn git_api(daemon: &TestDaemon) -> GitApi {
    let rt = Rt::new(tokio::runtime::Handle::current());
    let client = daemon_client(connect_unix(daemon.socket_path()).await.unwrap());
    GitApi::new(
        Arc::new(ExecClient::new(rt.clone(), client.clone())),
        Arc::new(FsClient::new(rt, client)),
    )
}

/// Set-up only: the code under test runs git through the daemon.
fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A repository on `main` with one commit, `.worktrees` ignored. Returns its canonical path
/// (git reports `/private/tmp/...` for `/tmp/...` on macOS).
fn init_repo(daemon: &TestDaemon) -> PathBuf {
    let repo = daemon.root().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "TreeTerm Test"]);
    git(&repo, &["config", "user.email", "test@example.invalid"]);
    git(&repo, &["config", "commit.gpgsign", "false"]);
    std::fs::write(repo.join("README.md"), "hello\n").unwrap();
    std::fs::write(repo.join(".gitignore"), ".worktrees\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "initial"]);
    repo.canonicalize().unwrap()
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("utf-8 temp path")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn get_info_reports_the_branch_and_the_main_root() {
    let daemon = TestDaemon::start();
    let repo = init_repo(&daemon);
    let api = git_api(&daemon).await;

    assert_eq!(
        api.get_info(path_str(&repo)).await,
        GitInfo::Repo {
            branch: "main".to_owned(),
            root_path: path_str(&repo).to_owned(),
        }
    );
    assert_eq!(
        api.get_info(path_str(daemon.workspaces_dir())).await,
        GitInfo::NotRepo
    );

    // A linked worktree reports its own branch but the main worktree's root.
    let worktree = api
        .create_worktree(
            path_str(&repo),
            "child",
            WorktreeBase::Head,
            Progress::Silent,
        )
        .await
        .unwrap();
    assert_eq!(
        api.get_info(&worktree.path).await,
        GitInfo::Repo {
            branch: "child".to_owned(),
            root_path: path_str(&repo).to_owned(),
        }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worktrees_and_branches_round_trip() {
    let daemon = TestDaemon::start();
    let repo = init_repo(&daemon);
    let repo = path_str(&repo);
    let api = git_api(&daemon).await;

    let worktree = api
        .create_worktree(
            repo,
            "feature",
            WorktreeBase::Ref("main".to_owned()),
            Progress::Silent,
        )
        .await
        .unwrap();
    assert_eq!(worktree.path, format!("{repo}/.worktrees/feature"));
    assert_eq!(worktree.branch, "feature");
    assert!(Path::new(&worktree.path).join("README.md").exists());

    assert_eq!(
        api.list_local_branches(repo).await.unwrap(),
        vec!["feature", "main"]
    );
    assert_eq!(
        api.get_branches_in_worktrees(repo).await.unwrap(),
        vec!["main", "feature"]
    );
    let worktrees = api.list_worktrees(repo).await.unwrap();
    assert_eq!(worktrees[0].path, repo);
    assert_eq!(worktrees[1].path, worktree.path);
    assert!(api.list_remote_branches(repo).await.unwrap().is_empty());
    assert!(api.is_ancestor(repo, "main", "feature").await.unwrap());

    api.remove_worktree(
        repo,
        &worktree.path,
        BranchDisposal::Delete,
        Progress::Silent,
    )
    .await
    .unwrap();
    assert!(!Path::new(&worktree.path).exists());
    assert_eq!(api.list_local_branches(repo).await.unwrap(), vec!["main"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uncommitted_changes_are_reviewed_staged_and_committed() {
    let daemon = TestDaemon::start();
    let repo = init_repo(&daemon);
    std::fs::write(repo.join("README.md"), "hello\nmore\nlines\n").unwrap();
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/new.txt"), "a\nb\nc\n").unwrap();
    let repo = path_str(&repo);
    let api = git_api(&daemon).await;

    assert!(api.has_uncommitted_changes(repo).await.unwrap());
    let changes = api.get_uncommitted_changes(repo).await.unwrap();
    let summary: Vec<_> = changes
        .files
        .iter()
        .map(|file| {
            (
                file.entry.path.as_str(),
                file.entry.status,
                file.entry.staged,
                file.stats,
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            (
                "README.md",
                FileChangeStatus::Modified,
                Staging::Unstaged,
                FileChangeStats::Text {
                    additions: 2,
                    deletions: 0
                }
            ),
            (
                "src/new.txt",
                FileChangeStatus::Untracked,
                Staging::Unstaged,
                FileChangeStats::Text {
                    additions: 3,
                    deletions: 0
                }
            ),
        ]
    );
    assert_eq!((changes.total_additions, changes.total_deletions), (5, 0));

    // The working-tree side is read through the daemon's ReadFile.
    assert_eq!(
        api.get_uncommitted_file_contents_for_diff(repo, "README.md", Staging::Unstaged)
            .await
            .unwrap(),
        FileDiffContents::Text {
            original_content: "hello\n".to_owned(),
            modified_content: "hello\nmore\nlines\n".to_owned(),
            language: "markdown".to_owned(),
        }
    );
    let diff = api
        .get_uncommitted_file_diff(repo, "README.md", Staging::Unstaged)
        .await
        .unwrap();
    assert!(diff.contains("+more"), "{diff}");

    api.stage_all(repo).await.unwrap();
    api.commit_staged(repo, "second").await.unwrap();
    assert!(!api.has_uncommitted_changes(repo).await.unwrap());

    let log = api.get_log(repo, LogScope::All, 0, 10).await.unwrap();
    assert!(!log.has_more);
    let messages: Vec<&str> = log
        .commits
        .iter()
        .map(|commit| commit.message.as_str())
        .collect();
    assert_eq!(messages, vec!["second", "initial"]);
    assert_eq!(
        log.commits[0].parent_hashes,
        vec![log.commits[1].hash.clone()]
    );
    assert_eq!(
        api.get_head_commit_hash(repo).await.unwrap(),
        log.commits[0].hash
    );

    let files = api
        .get_commit_diff(repo, &log.commits[0].hash)
        .await
        .unwrap();
    let paths: Vec<(&str, FileChangeStatus)> = files
        .iter()
        .map(|file| (file.path.as_str(), file.status))
        .collect();
    assert_eq!(
        paths,
        vec![
            ("README.md", FileChangeStatus::Modified),
            ("src/new.txt", FileChangeStatus::Added)
        ]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn merge_conflicts_are_found_without_touching_the_worktree() {
    let daemon = TestDaemon::start();
    let repo = init_repo(&daemon);
    git(&repo, &["checkout", "-q", "-b", "topic"]);
    std::fs::write(repo.join("README.md"), "topic\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "topic change"]);
    git(&repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("README.md"), "main\n").unwrap();
    git(&repo, &["commit", "-q", "-am", "main change"]);
    let repo = path_str(&repo);
    let api = git_api(&daemon).await;

    let conflicts = api
        .check_merge_conflicts(repo, "topic", "main")
        .await
        .unwrap();
    assert!(conflicts.has_conflicts);
    assert_eq!(conflicts.conflicted_files, vec!["README.md"]);
    assert!(!api.has_uncommitted_changes(repo).await.unwrap());

    let error = api
        .check_merge_conflicts(repo, "no-such-branch", "main")
        .await
        .expect_err("an unknown ref cannot be merged");
    assert!(error.to_string().contains("merge-tree"), "{error}");
}
