//! Git result types (ports of `GitInfo`, `DiffResult`, `UncommittedChanges`, … from
//! `renderer/types/index.ts` and `FileChangeStatus` from `shared/types.ts`).

/// What `get_info` found at a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitInfo {
    NotRepo,
    Repo {
        /// `HEAD` when detached.
        branch: String,
        /// The main worktree's root, even for a linked worktree.
        root_path: String,
    },
}

/// A worktree: where it is checked out and which branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
}

/// Whether a change is in the index or only in the working tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Staging {
    Staged,
    Unstaged,
}

/// Where a changed path came from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathOrigin {
    Same,
    RenamedFrom(String),
}

/// One line of `git status --porcelain`, split per side: a file changed in both the index
/// and the working tree yields two entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStatusEntry {
    pub path: String,
    pub status: FileChangeStatus,
    pub staged: Staging,
    pub origin: PathOrigin,
}

/// Size of a change. A binary change has a signed net byte delta, never line counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeStats {
    Text {
        additions: u64,
        deletions: u64,
    },
    Binary {
        byte_change: i64,
    },
    /// Not computed (too many untracked files to diff individually).
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFile {
    pub path: String,
    pub status: FileChangeStatus,
    pub stats: FileChangeStats,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffResult {
    pub files: Vec<DiffFile>,
    pub total_additions: u64,
    pub total_deletions: u64,
    pub base_branch: String,
    pub head_branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictInfo {
    pub has_conflicts: bool,
    pub conflicted_files: Vec<String>,
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UncommittedFile {
    pub entry: GitStatusEntry,
    pub stats: FileChangeStats,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UncommittedChanges {
    pub files: Vec<UncommittedFile>,
    pub total_additions: u64,
    pub total_deletions: u64,
}

/// Both sides of one file's diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileDiffContents {
    Text {
        original_content: String,
        modified_content: String,
        language: String,
    },
    /// Each side is base64 image bytes; empty when the file does not exist on that side.
    Image {
        original_base64: String,
        modified_base64: String,
    },
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitLogCommit {
    pub hash: String,
    pub short_hash: String,
    pub author: String,
    /// Strict ISO 8601 (`%aI`).
    pub date: String,
    pub message: String,
    pub parent_hashes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitLogResult {
    pub commits: Vec<GitLogCommit>,
    pub has_more: bool,
}

/// What a new worktree's branch starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeBase {
    /// The current `HEAD` of the repository the command runs in.
    Head,
    Ref(String),
}

/// What happens to a removed worktree's branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchDisposal {
    Keep,
    /// Best effort: a failure to find or delete the branch is logged, not reported.
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMode {
    Merge,
    /// `git merge --squash`: stages the result without committing.
    Squash,
}

/// Which commits `get_log` lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogScope {
    /// Everything reachable from `HEAD`.
    All,
    /// `<parent>..HEAD`: commits on this branch since it left `parent`.
    Since(String),
}

/// Where a running command's output is echoed as it arrives (the command line first, as in
/// a terminal).
#[derive(Clone, Copy)]
pub enum Progress<'a> {
    Silent,
    Report(&'a (dyn Fn(&str) + Send + Sync)),
}

impl Progress<'_> {
    /// Report `text`; there is nothing to show for an empty one.
    pub(crate) fn emit(&self, text: &str) {
        match self {
            Progress::Report(report) if !text.is_empty() => report(text),
            Progress::Report(_) | Progress::Silent => {}
        }
    }
}
