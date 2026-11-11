//! Git failures. Messages match the TS client's so both apps show the same text.

use crate::exec::ExecError;

#[derive(Debug, Clone, thiserror::Error)]
pub enum GitError {
    /// The command could not be run or its result was lost.
    #[error(transparent)]
    Exec(#[from] ExecError),
    /// The command ran and failed (`interpretError`).
    #[error(transparent)]
    Command(#[from] CommandError),
    #[error("Failed to resolve repository root")]
    RepoRootUnresolved,
    #[error("Failed to resolve remote home directory")]
    HomeDirUnresolved,
    #[error("Invalid git numstat counts: {line}")]
    InvalidNumstat { line: String },
    #[error("Missing path in git numstat: {line}")]
    NumstatWithoutPath { line: String },
    #[error("Missing or ambiguous binary byte sizes for {path}")]
    BinarySizesMissing { path: String },
    #[error("Invalid binary byte sizes for {path}")]
    BinarySizesInvalid { path: String },
    #[error("git fetch failed: {stderr}")]
    FetchFailed { stderr: String },
    /// git's trimmed stderr, or `git pull failed` when it printed nothing.
    #[error("{0}")]
    PullFailed(String),
    #[error("Failed to get remote URL: {stderr}")]
    RemoteUrlUnavailable { stderr: String },
}

/// A git command that exited unsuccessfully, classified from its stderr.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}", self.message())]
pub struct CommandError {
    /// `git <args>`.
    pub command: String,
    pub failure: CommandFailure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandFailure {
    NothingToCommit,
    MergeConflict,
    AlreadyExists,
    NotARepository,
    FileNotFound,
    MergeFailed,
    CouldNotResolve,
    /// None of the above; git's stderr verbatim.
    Stderr(String),
    /// Failed without saying why.
    ExitCode(i32),
}

impl CommandError {
    fn message(&self) -> String {
        let cmd = &self.command;
        match &self.failure {
            CommandFailure::NothingToCommit => format!("No changes to commit ({cmd})"),
            CommandFailure::MergeConflict => format!("Merge conflict detected ({cmd})"),
            CommandFailure::AlreadyExists => format!("Already exists ({cmd})"),
            CommandFailure::NotARepository => format!("Not a git repository ({cmd})"),
            CommandFailure::FileNotFound => format!("File not found ({cmd})"),
            CommandFailure::MergeFailed => format!("Merge failed ({cmd})"),
            CommandFailure::CouldNotResolve => format!("Could not resolve reference ({cmd})"),
            CommandFailure::Stderr(stderr) => format!("Git error [{cmd}]: {stderr}"),
            CommandFailure::ExitCode(code) => {
                format!("Git command failed [{cmd}] with exit code {code}")
            }
        }
    }
}
