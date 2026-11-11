//! Git operations (port of `renderer/lib/gitClient.ts`).
//!
//! Git runs on the daemon's host through [`ExecApi`] (the same invocations and arguments as
//! the TS client) and working-tree files are read through [`FilesystemApi`]; all parsing
//! happens here. Both are injected, so tests drive the client with fakes.
//!
//! Where the TS client resolved `{ success: false, error }`, these methods return
//! `Err(GitError)` with the same message. The few TS methods that never fail keep doing so:
//! [`GitApi::get_info`] reports a failure as [`GitInfo::NotRepo`].

mod error;
mod parse;
mod types;

#[cfg(test)]
mod fake;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use futures::{FutureExt, StreamExt};
use indexmap::IndexMap;

pub use error::{CommandError, CommandFailure, GitError};
pub use parse::{parse_blame_porcelain, parse_status};
pub use types::{
    BranchDisposal, ConflictInfo, DiffFile, DiffResult, FileChangeStats, FileChangeStatus,
    FileDiffContents, GitInfo, GitLogCommit, GitLogResult, GitStatusEntry, LogScope, MergeMode,
    PathOrigin, Progress, Staging, UncommittedChanges, UncommittedFile, WorktreeBase, WorktreeInfo,
};

use crate::concurrency::{CoalesceByKey, SharedRun, map_with_concurrency};
use crate::exec::{DEFAULT_EXEC_TIMEOUT, ExecApi, ExecError, ExecEvent, ExecRequest, ExecTimeout};
use crate::fs::{FilesystemApi, MAX_READ_FILE_BYTES, ReadOutcome};
use parse::{
    ExecResult, ParsedNumstat, detect_language, interpret_error, is_binary_content, is_image_path,
    parse_binary_byte_change, parse_name_statuses, parse_numstat, parse_numstat_counts,
};

/// `git worktree add` checks out the whole tree (plus LFS smudge and post-checkout hooks),
/// which takes over 30s on large repos.
const WORKTREE_ADD_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// `git worktree remove` deletes the whole checkout, as slow as adding it on large repos.
const WORKTREE_REMOVE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// `git merge` updates the parent's checkout (plus LFS smudge and hooks).
const MERGE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// `git fetch` / `git pull` talk to the remote.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(60);

/// Bounds for `get_uncommitted_changes`' per-file git calls. Each untracked file needs its
/// own `git diff --no-index` for line counts; firing them all at once on a worktree with
/// thousands of untracked files (e.g. on NFS) floods the daemon, and every process blows
/// past the exec timeout together.
const UNCOMMITTED_STAT_CONCURRENCY: NonZeroUsize = NonZeroUsize::new(8).expect("non-zero");
const MAX_UNTRACKED_FILES_WITH_STATS: usize = 200;

/// `owned![a, b]` → `vec![String::from(a), String::from(b)]`.
macro_rules! owned {
    ($($arg:expr),* $(,)?) => { vec![$(String::from($arg)),*] };
}

/// Git operations against one daemon. Cheap to clone; clones share the coalescing of
/// [`Self::get_uncommitted_changes`].
#[derive(Clone)]
pub struct GitApi {
    runner: Runner,
    uncommitted: Arc<CoalesceByKey<Result<UncommittedChanges, GitError>>>,
}

/// Runs commands and reads files; everything that does I/O goes through here.
#[derive(Clone)]
struct Runner {
    exec: Arc<dyn ExecApi>,
    filesystem: Arc<dyn FilesystemApi>,
}

/// How a command runs: its daemon timeout and where its output is echoed.
#[derive(Clone, Copy)]
struct RunOptions<'a> {
    timeout: Duration,
    progress: Progress<'a>,
}

const DEFAULT_RUN: RunOptions<'static> = RunOptions {
    timeout: DEFAULT_EXEC_TIMEOUT,
    progress: Progress::Silent,
};

/// Where one side of a file diff is read from.
enum DiffSide {
    /// A `git show` object spec such as `HEAD:path` or `:path` (the index).
    Blob(String),
    WorkingTree,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DiffSideEncoding {
    Text,
    Base64,
}

/// What `--stat` compares for a binary file's byte sizes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StatSource {
    /// Tracked paths (both of a rename).
    Tracked,
    /// An untracked file against `/dev/null` (`--no-index` exits 1 when they differ).
    NoIndex,
}

impl Runner {
    async fn exec_command(
        &self,
        cwd: &str,
        command: &str,
        args: Vec<String>,
        options: RunOptions<'_>,
    ) -> Result<ExecResult, GitError> {
        let mut events = self.exec.start(ExecRequest {
            cwd: cwd.to_owned(),
            command: command.to_owned(),
            args: args.clone(),
            env: HashMap::new(),
            timeout: ExecTimeout::After(options.timeout),
        });
        // Echo the command so a progress view reads like a terminal session.
        options
            .progress
            .emit(&format!("$ {command} {}\n", args.join(" ")));

        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let (mut stdout_text, mut stderr_text) = (Utf8Stream::default(), Utf8Stream::default());
        while let Some(event) = events.next().await {
            match event? {
                ExecEvent::Stdout(data) => {
                    options.progress.emit(&stdout_text.push(&data));
                    stdout.extend_from_slice(&data);
                }
                ExecEvent::Stderr(data) => {
                    options.progress.emit(&stderr_text.push(&data));
                    stderr.extend_from_slice(&data);
                }
                ExecEvent::Exit { code } => {
                    options.progress.emit(&stdout_text.finish());
                    options.progress.emit(&stderr_text.finish());
                    return Ok(ExecResult {
                        exit_code: code,
                        stdout: String::from_utf8_lossy(&stdout).into_owned(),
                        stderr: String::from_utf8_lossy(&stderr).into_owned(),
                        args,
                    });
                }
            }
        }
        Err(ExecError::EndedWithoutResult.into())
    }

    /// `git <args>` in `cwd`.
    async fn git(&self, cwd: &str, args: Vec<String>) -> Result<ExecResult, GitError> {
        self.exec_command(cwd, "git", args, DEFAULT_RUN).await
    }

    async fn git_with(
        &self,
        cwd: &str,
        args: Vec<String>,
        options: RunOptions<'_>,
    ) -> Result<ExecResult, GitError> {
        self.exec_command(cwd, "git", args, options).await
    }

    /// A side that does not exist (added/deleted file) reads as empty.
    async fn read_diff_side(
        &self,
        repo_path: &str,
        file_path: &str,
        side: &DiffSide,
        encoding: DiffSideEncoding,
    ) -> Result<String, GitError> {
        let spec = match side {
            DiffSide::WorkingTree => {
                let read = self
                    .filesystem
                    .read_file(repo_path, file_path, MAX_READ_FILE_BYTES)
                    .await;
                return Ok(match read {
                    Ok(ReadOutcome::Read(file)) => match encoding {
                        DiffSideEncoding::Text => {
                            String::from_utf8_lossy(&file.content).into_owned()
                        }
                        DiffSideEncoding::Base64 => {
                            base64::engine::general_purpose::STANDARD.encode(&file.content)
                        }
                    },
                    // The file might not exist in the working tree (deleted), or be too big
                    // to show: either way that side shows as empty.
                    Ok(ReadOutcome::TooLarge { size }) => {
                        tracing::debug!(file_path, size, "[git] working tree side too large");
                        String::new()
                    }
                    Err(error) => {
                        tracing::debug!(file_path, %error, "[git] working tree side unreadable");
                        String::new()
                    }
                });
            }
            DiffSide::Blob(spec) => spec,
        };
        match encoding {
            DiffSideEncoding::Text => {
                let result = self.git(repo_path, owned!["show", spec]).await?;
                Ok(if result.exit_code == 0 {
                    result.stdout
                } else {
                    String::new()
                })
            }
            DiffSideEncoding::Base64 => {
                // Mirrors the TS client, where exec output reached the renderer as UTF-8
                // text that would mangle raw image bytes. The spec is a positional argument
                // rather than interpolated, so no quoting is needed.
                let result = self
                    .exec_command(
                        repo_path,
                        "sh",
                        owned!["-c", "git show \"$1\" | base64", "sh", spec],
                        DEFAULT_RUN,
                    )
                    .await?;
                Ok(if result.exit_code == 0 {
                    result
                        .stdout
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .collect()
                } else {
                    String::new()
                })
            }
        }
    }

    async fn load_diff_contents(
        &self,
        repo_path: &str,
        file_path: &str,
        original: DiffSide,
        modified: DiffSide,
    ) -> Result<FileDiffContents, GitError> {
        if is_image_path(file_path) {
            let (original_base64, modified_base64) = futures::future::try_join(
                self.read_diff_side(repo_path, file_path, &original, DiffSideEncoding::Base64),
                self.read_diff_side(repo_path, file_path, &modified, DiffSideEncoding::Base64),
            )
            .await?;
            return Ok(FileDiffContents::Image {
                original_base64,
                modified_base64,
            });
        }
        let (original_content, modified_content) = futures::future::try_join(
            self.read_diff_side(repo_path, file_path, &original, DiffSideEncoding::Text),
            self.read_diff_side(repo_path, file_path, &modified, DiffSideEncoding::Text),
        )
        .await?;
        if is_binary_content(&original_content) || is_binary_content(&modified_content) {
            return Ok(FileDiffContents::Binary);
        }
        Ok(FileDiffContents::Text {
            original_content,
            modified_content,
            language: detect_language(file_path).to_owned(),
        })
    }

    /// Text counts pass through; a binary file's byte change is looked up with `args`.
    async fn file_stats(
        &self,
        repo_path: &str,
        parsed: ParsedNumstat,
        mut args: Vec<String>,
        (path, origin): (&str, &PathOrigin),
        source: StatSource,
    ) -> Result<FileChangeStats, GitError> {
        match parsed {
            ParsedNumstat::Text {
                additions,
                deletions,
            } => Ok(FileChangeStats::Text {
                additions,
                deletions,
            }),
            ParsedNumstat::Binary => {
                args.push("--".to_owned());
                match (source, origin) {
                    (StatSource::NoIndex, _) => args.extend(owned!["/dev/null", path]),
                    (StatSource::Tracked, PathOrigin::RenamedFrom(original)) => {
                        args.extend(owned![original, path]);
                    }
                    (StatSource::Tracked, PathOrigin::Same) => args.push(path.to_owned()),
                }
                let result = self.git(repo_path, args).await?;
                let differs = source == StatSource::NoIndex && result.exit_code == 1;
                if result.exit_code != 0 && !differs {
                    return Err(interpret_error(&result).into());
                }
                Ok(FileChangeStats::Binary {
                    byte_change: parse_binary_byte_change(&result.stdout, path)?,
                })
            }
        }
    }

    async fn is_worktrees_dir_in_gitignore(&self, root_path: &str) -> bool {
        match self
            .git(root_path, owned!["check-ignore", ".worktrees"])
            .await
        {
            Ok(result) => result.exit_code == 0,
            Err(error) => {
                tracing::warn!(%error, "[git] gitignore check failed");
                false
            }
        }
    }

    /// The MAIN worktree root for a repo, even when `dir_path` is itself a linked worktree.
    /// `git rev-parse --show-toplevel` returns the linked worktree's own path, which would
    /// anchor new worktrees inside a child workspace instead of at the repo root. The first
    /// entry of `git worktree list --porcelain` is always the main worktree.
    async fn resolve_repo_root(&self, dir_path: &str) -> Result<Option<String>, GitError> {
        let result = self
            .git(dir_path, owned!["worktree", "list", "--porcelain"])
            .await?;
        if result.exit_code != 0 {
            return Ok(None);
        }
        Ok(result
            .stdout
            .split('\n')
            .find_map(|line| line.strip_prefix("worktree "))
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_owned))
    }

    async fn resolve_worktree_path(
        &self,
        root_path: &str,
        worktree_name: &str,
    ) -> Result<String, GitError> {
        if self.is_worktrees_dir_in_gitignore(root_path).await {
            return Ok(format!("{root_path}/.worktrees/{worktree_name}"));
        }
        let repo_name = root_path.rsplit('/').next().unwrap_or(root_path);
        let home = self.resolve_homedir().await?;
        Ok(format!(
            "{home}/.treeterm/worktrees/{repo_name}/{worktree_name}"
        ))
    }

    /// `$HOME` on the daemon's host (port of `renderer/lib/homedir.ts`).
    async fn resolve_homedir(&self) -> Result<String, GitError> {
        let result = self
            .exec_command("/", "sh", owned!["-c", "echo $HOME"], DEFAULT_RUN)
            .await?;
        let home = result.stdout.trim();
        if result.exit_code == 0 && !home.is_empty() {
            Ok(home.to_owned())
        } else {
            Err(GitError::HomeDirUnresolved)
        }
    }

    async fn current_branch(&self, worktree_path: &str) -> Result<String, GitError> {
        let result = self
            .git(worktree_path, owned!["rev-parse", "--abbrev-ref", "HEAD"])
            .await?;
        if result.exit_code != 0 {
            return Err(interpret_error(&result).into());
        }
        Ok(result.stdout.trim().to_owned())
    }

    async fn merge_base(
        &self,
        worktree_path: &str,
        parent_branch: &str,
        current_branch: &str,
    ) -> Result<String, GitError> {
        let result = self
            .git(
                worktree_path,
                owned!["merge-base", parent_branch, current_branch],
            )
            .await?;
        if result.exit_code != 0 {
            return Err(interpret_error(&result).into());
        }
        Ok(result.stdout.trim().to_owned())
    }

    /// `numstat` and `name-status` of the same comparison, as diff files. Binary byte
    /// changes are looked up with `stat_args` (`... --stat <comparison>`).
    async fn diff_files(
        &self,
        repo_path: &str,
        numstat: &ExecResult,
        name_status: &ExecResult,
        stat_args: &[String],
    ) -> Result<Vec<DiffFile>, GitError> {
        if numstat.exit_code != 0 {
            return Err(interpret_error(numstat).into());
        }
        if name_status.exit_code != 0 {
            return Err(interpret_error(name_status).into());
        }
        let status_map = parse_name_statuses(&name_status.stdout);
        let known: HashSet<&str> = status_map.keys().map(String::as_str).collect();
        let mut files = Vec::new();
        for (path, parsed) in parse_numstat(&numstat.stdout, &known)? {
            let (status, origin) = match status_map.get(&path) {
                Some(change) => (change.status, change.origin.clone()),
                None => (FileChangeStatus::Modified, PathOrigin::Same),
            };
            let stats = self
                .file_stats(
                    repo_path,
                    parsed,
                    stat_args.to_vec(),
                    (&path, &origin),
                    StatSource::Tracked,
                )
                .await?;
            files.push(DiffFile {
                path,
                status,
                stats,
            });
        }
        Ok(files)
    }

    async fn load_uncommitted_changes(
        &self,
        repo_path: &str,
    ) -> Result<UncommittedChanges, GitError> {
        // `--untracked-files=all` is required, not a nicety: git's default (`normal`)
        // collapses a wholly-untracked directory into one `dir/` entry. That entry is a
        // directory, so it has no blob to diff against and cannot be read as a file.
        // Listing every untracked file individually is what makes new files inside new
        // directories reviewable at all.
        let status_result = self
            .git(
                repo_path,
                owned!["status", "--porcelain", "--untracked-files=all"],
            )
            .await?;
        if status_result.exit_code != 0 {
            return Err(interpret_error(&status_result).into());
        }
        let status = parse_status(&status_result.stdout);
        let known: HashSet<&str> = status.iter().map(|entry| entry.path.as_str()).collect();

        let staged_result = self
            .git(repo_path, owned!["diff", "--cached", "--numstat"])
            .await?;
        if staged_result.exit_code != 0 {
            return Err(interpret_error(&staged_result).into());
        }
        let staged_stats = parse_numstat(&staged_result.stdout, &known)?;

        let unstaged_result = self.git(repo_path, owned!["diff", "--numstat"]).await?;
        if unstaged_result.exit_code != 0 {
            return Err(interpret_error(&unstaged_result).into());
        }
        let unstaged_stats = parse_numstat(&unstaged_result.stdout, &known)?;

        // `git diff` omits untracked files. Diff each against /dev/null; exit 1 means they
        // differ, while exit codes above 1 are errors. Past the cap, untracked files are
        // listed without stats rather than spawning a git process per file.
        // Owned items below: closures over borrowed items trip the higher-ranked `Send`
        // check of the boxed (coalesced) future.
        let untracked: Vec<String> = status
            .iter()
            .filter(|entry| entry.status == FileChangeStatus::Untracked)
            .map(|entry| entry.path.clone())
            .collect();
        let untracked_stats_skipped = untracked.len() > MAX_UNTRACKED_FILES_WITH_STATS;
        let mut untracked_stats = IndexMap::new();
        if !untracked_stats_skipped {
            let counted =
                map_with_concurrency(untracked, UNCOMMITTED_STAT_CONCURRENCY, |path| async move {
                    let result = self
                        .git(
                            repo_path,
                            owned!["diff", "--numstat", "--no-index", "--", "/dev/null", &path],
                        )
                        .await?;
                    if result.exit_code > 1 {
                        return Err(GitError::from(interpret_error(&result)));
                    }
                    match result
                        .stdout
                        .trim()
                        .split('\n')
                        .find(|line| !line.is_empty())
                    {
                        Some(line) => Ok(Some((path, parse_numstat_counts(line)?))),
                        None => Ok(None),
                    }
                })
                .await?;
            untracked_stats.extend(counted.into_iter().flatten());
        }

        let files = map_with_concurrency(status, UNCOMMITTED_STAT_CONCURRENCY, |entry| {
            let untracked = entry.status == FileChangeStatus::Untracked;
            let (stats_by_path, args, source) = match (untracked, entry.staged) {
                (true, _) => (
                    &untracked_stats,
                    owned!["diff", "--stat", "--no-index"],
                    StatSource::NoIndex,
                ),
                (false, Staging::Staged) => (
                    &staged_stats,
                    owned!["diff", "--cached", "--stat"],
                    StatSource::Tracked,
                ),
                (false, Staging::Unstaged) => (
                    &unstaged_stats,
                    owned!["diff", "--stat"],
                    StatSource::Tracked,
                ),
            };
            async move {
                if untracked && untracked_stats_skipped {
                    return Ok(UncommittedFile {
                        entry,
                        stats: FileChangeStats::Unknown,
                    });
                }
                let parsed =
                    stats_by_path
                        .get(&entry.path)
                        .copied()
                        .unwrap_or(ParsedNumstat::Text {
                            additions: 0,
                            deletions: 0,
                        });
                let stats = self
                    .file_stats(
                        repo_path,
                        parsed,
                        args,
                        (&entry.path, &entry.origin),
                        source,
                    )
                    .await?;
                Ok::<_, GitError>(UncommittedFile { entry, stats })
            }
        })
        .await?;

        let (total_additions, total_deletions) = text_totals(files.iter().map(|file| file.stats));
        Ok(UncommittedChanges {
            files,
            total_additions,
            total_deletions,
        })
    }
}

/// Sum of the text files' line counts; binary and unknown changes have none.
fn text_totals(stats: impl Iterator<Item = FileChangeStats>) -> (u64, u64) {
    stats.fold((0, 0), |(additions, deletions), stats| match stats {
        FileChangeStats::Text {
            additions: a,
            deletions: d,
        } => (additions + a, deletions + d),
        FileChangeStats::Binary { .. } | FileChangeStats::Unknown => (additions, deletions),
    })
}

/// `Err` unless the command exited 0.
fn succeeded(result: ExecResult) -> Result<ExecResult, GitError> {
    if result.exit_code == 0 {
        Ok(result)
    } else {
        Err(interpret_error(&result).into())
    }
}

/// The non-empty lines of `output`, trimmed as a whole.
fn lines(output: &str) -> Vec<String> {
    output
        .trim()
        .split('\n')
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A git object id as `merge-tree --write-tree` prints it (SHA-1 or SHA-256).
fn is_tree_oid(text: &str) -> bool {
    (40..=64).contains(&text.len()) && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `parseInt(text.trim(), 10) || 0` for a count.
fn leading_count(text: &str) -> u64 {
    let text = text.trim();
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    text[..end].parse().unwrap_or(0)
}

impl GitApi {
    pub fn new(exec: Arc<dyn ExecApi>, filesystem: Arc<dyn FilesystemApi>) -> Self {
        Self {
            runner: Runner { exec, filesystem },
            uncommitted: Arc::new(CoalesceByKey::new()),
        }
    }

    /// Whether `dir_path` is in a git work tree, and its branch and main root. Any failure
    /// (including the daemon being unreachable) reads as [`GitInfo::NotRepo`], as in TS.
    pub async fn get_info(&self, dir_path: &str) -> GitInfo {
        match self.try_get_info(dir_path).await {
            Ok(info) => info,
            Err(error) => {
                tracing::warn!(%error, "[git] getGitInfo failed, treating as non-repo");
                GitInfo::NotRepo
            }
        }
    }

    async fn try_get_info(&self, dir_path: &str) -> Result<GitInfo, GitError> {
        let r = &self.runner;
        let result = r
            .git(dir_path, owned!["rev-parse", "--is-inside-work-tree"])
            .await?;
        if result.exit_code != 0 {
            return Ok(GitInfo::NotRepo);
        }
        let (branch_result, repo_root, top_level_result) = futures::future::try_join3(
            r.git(dir_path, owned!["rev-parse", "--abbrev-ref", "HEAD"]),
            r.resolve_repo_root(dir_path),
            r.git(dir_path, owned!["rev-parse", "--show-toplevel"]),
        )
        .await?;
        let branch = match branch_result.stdout.trim() {
            "" => "HEAD".to_owned(),
            branch => branch.to_owned(),
        };
        let root_path = match (repo_root, top_level_result.stdout.trim()) {
            (Some(root), _) => root,
            (None, "") => dir_path.to_owned(),
            (None, top_level) => top_level.to_owned(),
        };
        Ok(GitInfo::Repo { branch, root_path })
    }

    /// A new branch `name` checked out in a new worktree, placed under `<root>/.worktrees`
    /// when that is gitignored, else under `~/.treeterm/worktrees/<repo>/`.
    pub async fn create_worktree(
        &self,
        repo_path: &str,
        name: &str,
        base: WorktreeBase,
        progress: Progress<'_>,
    ) -> Result<WorktreeInfo, GitError> {
        let r = &self.runner;
        // Resolve to the MAIN worktree root so forking a child workspace creates the new
        // worktree at the repo root, not nested inside the child.
        let root_path = r
            .resolve_repo_root(repo_path)
            .await?
            .ok_or(GitError::RepoRootUnresolved)?;
        let worktree_path = r.resolve_worktree_path(&root_path, name).await?;
        let mut args = owned!["worktree", "add", "-b", name, &worktree_path];
        match base {
            WorktreeBase::Head => {}
            WorktreeBase::Ref(base) => args.push(base),
        }
        let options = RunOptions {
            timeout: WORKTREE_ADD_TIMEOUT,
            progress,
        };
        succeeded(r.git_with(repo_path, args, options).await?)?;
        Ok(WorktreeInfo {
            path: worktree_path,
            branch: name.to_owned(),
        })
    }

    /// Remove a worktree (forcefully), optionally deleting its branch afterwards.
    pub async fn remove_worktree(
        &self,
        repo_path: &str,
        worktree_path: &str,
        branch: BranchDisposal,
        progress: Progress<'_>,
    ) -> Result<(), GitError> {
        let r = &self.runner;
        let step = RunOptions {
            timeout: DEFAULT_EXEC_TIMEOUT,
            progress,
        };
        let branch_name = match branch {
            BranchDisposal::Keep => None,
            BranchDisposal::Delete => {
                let head = r
                    .git_with(
                        worktree_path,
                        owned!["rev-parse", "--abbrev-ref", "HEAD"],
                        step,
                    )
                    .await;
                match head {
                    Ok(result) if result.exit_code == 0 => Some(result.stdout.trim().to_owned()),
                    Ok(result) => {
                        tracing::warn!(stderr = %result.stderr, "[git] could not get branch name before worktree removal");
                        None
                    }
                    Err(error) => {
                        tracing::warn!(%error, "[git] could not get branch name before worktree removal");
                        None
                    }
                }
            }
        };

        let remove = RunOptions {
            timeout: WORKTREE_REMOVE_TIMEOUT,
            progress,
        };
        succeeded(
            r.git_with(
                repo_path,
                owned!["worktree", "remove", worktree_path, "--force"],
                remove,
            )
            .await?,
        )?;

        if let Some(branch_name) = branch_name.filter(|name| !name.is_empty()) {
            match r
                .git_with(repo_path, owned!["branch", "-D", branch_name], step)
                .await
            {
                Ok(result) if result.exit_code == 0 => {}
                Ok(result) => {
                    tracing::warn!(stderr = %result.stderr, "[git] branch deletion after worktree removal failed");
                }
                Err(error) => {
                    tracing::warn!(%error, "[git] branch deletion after worktree removal failed");
                }
            }
        }
        Ok(())
    }

    /// Worktrees with a branch checked out (detached ones are skipped).
    pub async fn list_worktrees(&self, repo_path: &str) -> Result<Vec<WorktreeInfo>, GitError> {
        let result = succeeded(
            self.runner
                .git(repo_path, owned!["worktree", "list", "--porcelain"])
                .await?,
        )?;
        let mut worktrees = Vec::new();
        let (mut current_path, mut current_branch) = (String::new(), String::new());
        for line in result.stdout.split('\n') {
            if let Some(path) = line.strip_prefix("worktree ") {
                current_path = path.to_owned();
            } else if let Some(branch) = line.strip_prefix("branch ") {
                current_branch = branch.replacen("refs/heads/", "", 1);
            } else if line.is_empty() && !current_path.is_empty() && !current_branch.is_empty() {
                worktrees.push(WorktreeInfo {
                    path: std::mem::take(&mut current_path),
                    branch: std::mem::take(&mut current_branch),
                });
            }
        }
        Ok(worktrees)
    }

    pub async fn list_local_branches(&self, repo_path: &str) -> Result<Vec<String>, GitError> {
        let result = succeeded(
            self.runner
                .git(repo_path, owned!["branch", "--format=%(refname:short)"])
                .await?,
        )?;
        Ok(lines(&result.stdout))
    }

    pub async fn list_remote_branches(&self, repo_path: &str) -> Result<Vec<String>, GitError> {
        let result = succeeded(
            self.runner
                .git(
                    repo_path,
                    owned!["branch", "-r", "--format=%(refname:short)"],
                )
                .await?,
        )?;
        Ok(lines(&result.stdout))
    }

    pub async fn get_branches_in_worktrees(
        &self,
        repo_path: &str,
    ) -> Result<Vec<String>, GitError> {
        let worktrees = self.list_worktrees(repo_path).await?;
        Ok(worktrees
            .into_iter()
            .map(|worktree| worktree.branch)
            .collect())
    }

    /// An existing local `branch` checked out in a new worktree.
    pub async fn create_worktree_from_branch(
        &self,
        repo_path: &str,
        branch: &str,
        worktree_name: &str,
        progress: Progress<'_>,
    ) -> Result<WorktreeInfo, GitError> {
        let r = &self.runner;
        let root_path = r
            .resolve_repo_root(repo_path)
            .await?
            .ok_or(GitError::RepoRootUnresolved)?;
        let worktree_path = r.resolve_worktree_path(&root_path, worktree_name).await?;
        let options = RunOptions {
            timeout: WORKTREE_ADD_TIMEOUT,
            progress,
        };
        succeeded(
            r.git_with(
                repo_path,
                owned!["worktree", "add", &worktree_path, branch],
                options,
            )
            .await?,
        )?;
        Ok(WorktreeInfo {
            path: worktree_path,
            branch: branch.to_owned(),
        })
    }

    /// A new local branch tracking `remote_branch` (`origin/feature` → `feature`) checked
    /// out in a new worktree.
    pub async fn create_worktree_from_remote(
        &self,
        repo_path: &str,
        remote_branch: &str,
        worktree_name: &str,
        progress: Progress<'_>,
    ) -> Result<WorktreeInfo, GitError> {
        let r = &self.runner;
        let branch_name = match remote_branch.split_once('/') {
            Some((remote, branch)) if !remote.is_empty() => branch,
            _ => remote_branch,
        };
        let root_path = r
            .resolve_repo_root(repo_path)
            .await?
            .ok_or(GitError::RepoRootUnresolved)?;
        let worktree_path = r.resolve_worktree_path(&root_path, worktree_name).await?;
        let options = RunOptions {
            timeout: WORKTREE_ADD_TIMEOUT,
            progress,
        };
        succeeded(
            r.git_with(
                repo_path,
                owned![
                    "worktree",
                    "add",
                    "-b",
                    branch_name,
                    &worktree_path,
                    remote_branch
                ],
                options,
            )
            .await?,
        )?;
        Ok(WorktreeInfo {
            path: worktree_path,
            branch: branch_name.to_owned(),
        })
    }

    /// Changes on the worktree's branch since it left `parent_branch` (from the merge base).
    pub async fn get_diff(
        &self,
        worktree_path: &str,
        parent_branch: &str,
    ) -> Result<DiffResult, GitError> {
        let r = &self.runner;
        let current_branch = r.current_branch(worktree_path).await?;
        let merge_base = r
            .merge_base(worktree_path, parent_branch, &current_branch)
            .await?;
        let (numstat, name_status) = futures::future::try_join(
            r.git(
                worktree_path,
                owned!["diff", "--numstat", &merge_base, &current_branch],
            ),
            r.git(
                worktree_path,
                owned!["diff", "--name-status", &merge_base, &current_branch],
            ),
        )
        .await?;
        let stat_args = owned!["diff", "--stat", &merge_base, &current_branch];
        let files = r
            .diff_files(worktree_path, &numstat, &name_status, &stat_args)
            .await?;
        let (total_additions, total_deletions) = text_totals(files.iter().map(|file| file.stats));
        Ok(DiffResult {
            files,
            total_additions,
            total_deletions,
            base_branch: parent_branch.to_owned(),
            head_branch: current_branch,
        })
    }

    /// The unified diff of one file since the branch left `parent_branch`.
    pub async fn get_file_diff(
        &self,
        worktree_path: &str,
        parent_branch: &str,
        file_path: &str,
    ) -> Result<String, GitError> {
        let r = &self.runner;
        let current_branch = r.current_branch(worktree_path).await?;
        let merge_base = r
            .merge_base(worktree_path, parent_branch, &current_branch)
            .await?;
        let result = succeeded(
            r.git(
                worktree_path,
                owned!["diff", &merge_base, &current_branch, "--", file_path],
            )
            .await?,
        )?;
        Ok(result.stdout)
    }

    /// Merge `worktree_branch` into the branch checked out at `target_worktree_path`.
    pub async fn merge(
        &self,
        target_worktree_path: &str,
        worktree_branch: &str,
        mode: MergeMode,
        progress: Progress<'_>,
    ) -> Result<(), GitError> {
        let args = match mode {
            MergeMode::Merge => owned!["merge", worktree_branch],
            MergeMode::Squash => owned!["merge", "--squash", worktree_branch],
        };
        let options = RunOptions {
            timeout: MERGE_TIMEOUT,
            progress,
        };
        succeeded(
            self.runner
                .git_with(target_worktree_path, args, options)
                .await?,
        )?;
        Ok(())
    }

    /// Whether merging `source_branch` into `target_branch` would conflict, without touching
    /// any worktree (`git merge-tree --write-tree`).
    pub async fn check_merge_conflicts(
        &self,
        repo_path: &str,
        source_branch: &str,
        target_branch: &str,
    ) -> Result<ConflictInfo, GitError> {
        // Machine-readable output: "<tree-oid>\0" then one "<path>\0" per conflicted file.
        // Exit 0 = clean, 1 = conflicts; git also exits 1 (with no tree) on bad refs.
        let result = self
            .runner
            .git(
                repo_path,
                owned![
                    "merge-tree",
                    "--write-tree",
                    "--name-only",
                    "--no-messages",
                    "-z",
                    target_branch,
                    source_branch
                ],
            )
            .await?;
        let mut fields = result.stdout.split('\0');
        let tree_oid = fields.next().unwrap_or_default();
        if !(result.exit_code == 0 || result.exit_code == 1) || !is_tree_oid(tree_oid) {
            return Err(interpret_error(&result).into());
        }
        let mut conflicted_files: Vec<String> = Vec::new();
        if result.exit_code == 1 {
            for path in fields.filter(|path| !path.is_empty()) {
                if !conflicted_files.iter().any(|seen| seen == path) {
                    conflicted_files.push(path.to_owned());
                }
            }
        }
        let has_conflicts = !conflicted_files.is_empty();
        let messages = if has_conflicts {
            vec![format!("{} conflicting files", conflicted_files.len())]
        } else {
            Vec::new()
        };
        Ok(ConflictInfo {
            has_conflicts,
            conflicted_files,
            messages,
        })
    }

    /// `false` too when git fails (e.g. not a repository).
    pub async fn has_uncommitted_changes(&self, repo_path: &str) -> Result<bool, GitError> {
        let result = self
            .runner
            .git(repo_path, owned!["status", "--porcelain"])
            .await?;
        Ok(result.exit_code == 0 && !result.stdout.trim().is_empty())
    }

    pub async fn commit_all(
        &self,
        repo_path: &str,
        message: &str,
        progress: Progress<'_>,
    ) -> Result<(), GitError> {
        let options = RunOptions {
            timeout: DEFAULT_EXEC_TIMEOUT,
            progress,
        };
        succeeded(
            self.runner
                .git_with(repo_path, owned!["commit", "-am", message], options)
                .await?,
        )?;
        Ok(())
    }

    pub async fn delete_branch(
        &self,
        repo_path: &str,
        branch_name: &str,
        progress: Progress<'_>,
    ) -> Result<(), GitError> {
        let options = RunOptions {
            timeout: DEFAULT_EXEC_TIMEOUT,
            progress,
        };
        succeeded(
            self.runner
                .git_with(repo_path, owned!["branch", "-D", branch_name], options)
                .await?,
        )?;
        Ok(())
    }

    pub async fn rename_branch(
        &self,
        repo_path: &str,
        old_name: &str,
        new_name: &str,
    ) -> Result<(), GitError> {
        succeeded(
            self.runner
                .git(repo_path, owned!["branch", "-m", old_name, new_name])
                .await?,
        )?;
        Ok(())
    }

    /// Every staged, unstaged and untracked change with its size. Review refreshes on
    /// mount, on focus and after every stage/commit; a slow repo must not accumulate
    /// overlapping scans, so concurrent requests for the same repo share one follow-up run
    /// (see [`CoalesceByKey`]). The run is decided now, not when the future is first polled.
    pub fn get_uncommitted_changes(
        &self,
        repo_path: &str,
    ) -> SharedRun<Result<UncommittedChanges, GitError>> {
        let runner = self.runner.clone();
        let repo = repo_path.to_owned();
        self.uncommitted.call(repo_path, move || {
            async move { runner.load_uncommitted_changes(&repo).await }.boxed()
        })
    }

    pub async fn get_uncommitted_file_diff(
        &self,
        repo_path: &str,
        file_path: &str,
        staging: Staging,
    ) -> Result<String, GitError> {
        let args = match staging {
            Staging::Staged => owned!["diff", "--cached", "--", file_path],
            Staging::Unstaged => owned!["diff", "--", file_path],
        };
        Ok(succeeded(self.runner.git(repo_path, args).await?)?.stdout)
    }

    pub async fn stage_file(&self, repo_path: &str, file_path: &str) -> Result<(), GitError> {
        succeeded(self.runner.git(repo_path, owned!["add", file_path]).await?)?;
        Ok(())
    }

    pub async fn unstage_file(&self, repo_path: &str, file_path: &str) -> Result<(), GitError> {
        succeeded(
            self.runner
                .git(repo_path, owned!["reset", "HEAD", file_path])
                .await?,
        )?;
        Ok(())
    }

    pub async fn stage_all(&self, repo_path: &str) -> Result<(), GitError> {
        succeeded(self.runner.git(repo_path, owned!["add", "."]).await?)?;
        Ok(())
    }

    pub async fn unstage_all(&self, repo_path: &str) -> Result<(), GitError> {
        succeeded(self.runner.git(repo_path, owned!["reset", "HEAD"]).await?)?;
        Ok(())
    }

    pub async fn commit_staged(&self, repo_path: &str, message: &str) -> Result<(), GitError> {
        succeeded(
            self.runner
                .git(repo_path, owned!["commit", "-m", message])
                .await?,
        )?;
        Ok(())
    }

    /// One file at the merge base with `parent_branch` and at `HEAD`.
    pub async fn get_file_contents_for_diff(
        &self,
        worktree_path: &str,
        parent_branch: &str,
        file_path: &str,
    ) -> Result<FileDiffContents, GitError> {
        let r = &self.runner;
        let current_branch = r.current_branch(worktree_path).await?;
        let merge_base = r
            .merge_base(worktree_path, parent_branch, &current_branch)
            .await?;
        r.load_diff_contents(
            worktree_path,
            file_path,
            DiffSide::Blob(format!("{merge_base}:{file_path}")),
            DiffSide::Blob(format!("HEAD:{file_path}")),
        )
        .await
    }

    /// Staged: `HEAD` against the index. Unstaged: the index against the working tree.
    pub async fn get_uncommitted_file_contents_for_diff(
        &self,
        repo_path: &str,
        file_path: &str,
        staging: Staging,
    ) -> Result<FileDiffContents, GitError> {
        let index = DiffSide::Blob(format!(":{file_path}"));
        let (original, modified) = match staging {
            Staging::Staged => (DiffSide::Blob(format!("HEAD:{file_path}")), index),
            Staging::Unstaged => (index, DiffSide::WorkingTree),
        };
        self.runner
            .load_diff_contents(repo_path, file_path, original, modified)
            .await
    }

    pub async fn get_head_commit_hash(&self, repo_path: &str) -> Result<String, GitError> {
        let result = succeeded(
            self.runner
                .git(repo_path, owned!["rev-parse", "HEAD"])
                .await?,
        )?;
        Ok(result.stdout.trim().to_owned())
    }

    /// `git blame` of the working-tree file: index `i` holds the hash of the commit that
    /// last changed line `i + 1` (all zeros while the line is uncommitted). A path git does
    /// not know has no lines rather than being an error.
    pub async fn get_line_commits(
        &self,
        repo_path: &str,
        file_path: &str,
    ) -> Result<Vec<String>, GitError> {
        let result = self
            .runner
            .git(repo_path, owned!["blame", "--porcelain", "--", file_path])
            .await?;
        if result.exit_code != 0 {
            if result.stderr.to_lowercase().contains("no such path") {
                return Ok(Vec::new());
            }
            return Err(interpret_error(&result).into());
        }
        Ok(parse_blame_porcelain(&result.stdout))
    }

    /// One page of history: `limit` commits after skipping `skip`.
    pub async fn get_log(
        &self,
        repo_path: &str,
        scope: LogScope,
        skip: usize,
        limit: usize,
    ) -> Result<GitLogResult, GitError> {
        let mut args = owned![
            "log",
            "--format=%H%x1e%h%x1e%an%x1e%aI%x1e%s%x1e%P",
            format!("--skip={skip}"),
            format!("--max-count={}", limit + 1)
        ];
        match scope {
            LogScope::All => {}
            LogScope::Since(parent_branch) => args.push(format!("{parent_branch}..HEAD")),
        }
        let result = succeeded(self.runner.git(repo_path, args).await?)?;
        let lines = lines(&result.stdout);
        let has_more = lines.len() > limit;
        let commits = lines
            .iter()
            .take(limit)
            .map(|line| {
                let mut fields = line.split('\x1e');
                let mut field = || fields.next().unwrap_or_default().to_owned();
                let (hash, short_hash, author, date, message, parents) =
                    (field(), field(), field(), field(), field(), field());
                GitLogCommit {
                    hash,
                    short_hash,
                    author,
                    date,
                    message,
                    parent_hashes: parents
                        .split(' ')
                        .filter(|parent| !parent.is_empty())
                        .map(str::to_owned)
                        .collect(),
                }
            })
            .collect();
        Ok(GitLogResult { commits, has_more })
    }

    /// The files one commit changed (against its first parent; a root commit against the
    /// empty tree).
    pub async fn get_commit_diff(
        &self,
        repo_path: &str,
        commit_hash: &str,
    ) -> Result<Vec<DiffFile>, GitError> {
        let r = &self.runner;
        let (numstat, name_status) = futures::future::try_join(
            r.git(
                repo_path,
                owned![
                    "diff-tree",
                    "--no-commit-id",
                    "--root",
                    "-r",
                    "--numstat",
                    commit_hash
                ],
            ),
            r.git(
                repo_path,
                owned![
                    "diff-tree",
                    "--no-commit-id",
                    "--root",
                    "-r",
                    "--name-status",
                    commit_hash
                ],
            ),
        )
        .await?;
        let stat_args = owned![
            "diff-tree",
            "--no-commit-id",
            "--root",
            "-r",
            "--stat",
            commit_hash
        ];
        r.diff_files(repo_path, &numstat, &name_status, &stat_args)
            .await
    }

    /// One file before and after a commit.
    pub async fn get_commit_file_diff(
        &self,
        repo_path: &str,
        commit_hash: &str,
        file_path: &str,
    ) -> Result<FileDiffContents, GitError> {
        self.runner
            .load_diff_contents(
                repo_path,
                file_path,
                DiffSide::Blob(format!("{commit_hash}~1:{file_path}")),
                DiffSide::Blob(format!("{commit_hash}:{file_path}")),
            )
            .await
    }

    pub async fn fetch(&self, repo_path: &str) -> Result<(), GitError> {
        let options = RunOptions {
            timeout: NETWORK_TIMEOUT,
            progress: Progress::Silent,
        };
        let result = self
            .runner
            .git_with(repo_path, owned!["fetch"], options)
            .await?;
        if result.exit_code != 0 {
            return Err(GitError::FetchFailed {
                stderr: result.stderr,
            });
        }
        Ok(())
    }

    pub async fn pull(&self, repo_path: &str) -> Result<(), GitError> {
        let options = RunOptions {
            timeout: NETWORK_TIMEOUT,
            progress: Progress::Silent,
        };
        let result = self
            .runner
            .git_with(repo_path, owned!["pull"], options)
            .await?;
        if result.exit_code != 0 {
            return Err(GitError::PullFailed(match result.stderr.trim() {
                "" => "git pull failed".to_owned(),
                stderr => stderr.to_owned(),
            }));
        }
        Ok(())
    }

    /// Commits the upstream has that `HEAD` lacks; 0 without an upstream.
    pub async fn get_behind_count(&self, repo_path: &str) -> Result<u64, GitError> {
        let result = self
            .runner
            .git(
                repo_path,
                owned!["rev-list", "--count", "HEAD..@{upstream}"],
            )
            .await?;
        if result.exit_code != 0 {
            return Ok(0);
        }
        Ok(leading_count(&result.stdout))
    }

    pub async fn get_remote_url(&self, repo_path: &str) -> Result<String, GitError> {
        let result = self
            .runner
            .git(repo_path, owned!["remote", "get-url", "origin"])
            .await?;
        if result.exit_code != 0 {
            return Err(GitError::RemoteUrlUnavailable {
                stderr: result.stderr,
            });
        }
        Ok(result.stdout.trim().to_owned())
    }

    /// Exit 0 = ancestor; 1 = not; other codes (bad ref) also read as not an ancestor.
    pub async fn is_ancestor(
        &self,
        repo_path: &str,
        ancestor_ref: &str,
        descendant_ref: &str,
    ) -> Result<bool, GitError> {
        let result = self
            .runner
            .git(
                repo_path,
                owned!["merge-base", "--is-ancestor", ancestor_ref, descendant_ref],
            )
            .await?;
        Ok(result.exit_code == 0)
    }
}

/// Incremental UTF-8 decoding for progress output: a character split across chunks is held
/// back until its remaining bytes arrive.
#[derive(Default)]
struct Utf8Stream {
    pending: Vec<u8>,
}

impl Utf8Stream {
    fn push(&mut self, data: &[u8]) -> String {
        self.pending.extend_from_slice(data);
        let complete = self.pending.len() - incomplete_tail_len(&self.pending);
        let text = String::from_utf8_lossy(&self.pending[..complete]).into_owned();
        self.pending.drain(..complete);
        text
    }

    fn finish(&mut self) -> String {
        let text = String::from_utf8_lossy(&self.pending).into_owned();
        self.pending.clear();
        text
    }
}

/// Bytes at the end of `bytes` that start a UTF-8 sequence without finishing it.
fn incomplete_tail_len(bytes: &[u8]) -> usize {
    for back in 1..=bytes.len().min(4) {
        let byte = bytes[bytes.len() - back];
        if byte & 0b1100_0000 == 0b1000_0000 {
            continue; // a continuation byte: keep looking for the sequence's lead
        }
        let width = match byte {
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => 1,
        };
        return if width > back { back } else { 0 };
    }
    0
}
