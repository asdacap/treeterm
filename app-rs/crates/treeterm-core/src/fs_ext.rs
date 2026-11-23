//! Filesystem capabilities beyond [`FilesystemApi`]'s read, as traits so the clients built on
//! them (worktree registry, run actions, favourites) can be driven by fakes. [`FsClient`]
//! implements all of them.

use futures::FutureExt;
use futures::future::BoxFuture;
use treeterm_proto::treeterm::DirectoryContents;

use crate::daemon::ApiError;
use crate::fs::{
    FilesystemApi, FsClient, FsError, MAX_READ_FILE_BYTES, ReadOutcome, WriteGuard, WriteOutcome,
};

/// See [`FsClient::write_file`].
pub trait FileWriteApi: Send + Sync {
    fn write_file(
        &self,
        workspace_path: &str,
        file_path: &str,
        content: Vec<u8>,
        guard: WriteGuard,
    ) -> BoxFuture<'static, Result<WriteOutcome, FsError>>;
}

/// See [`FsClient::read_directory`].
pub trait DirectoryApi: Send + Sync {
    fn read_directory(
        &self,
        workspace_path: &str,
        dir_path: &str,
    ) -> BoxFuture<'static, Result<DirectoryContents, FsError>>;
}

impl FileWriteApi for FsClient {
    fn write_file(
        &self,
        workspace_path: &str,
        file_path: &str,
        content: Vec<u8>,
        guard: WriteGuard,
    ) -> BoxFuture<'static, Result<WriteOutcome, FsError>> {
        FsClient::write_file(self, workspace_path, file_path, content, guard)
            .map(|result| result.unwrap_or_else(|lost| Err(ApiError::from(lost).into())))
            .boxed()
    }
}

impl DirectoryApi for FsClient {
    fn read_directory(
        &self,
        workspace_path: &str,
        dir_path: &str,
    ) -> BoxFuture<'static, Result<DirectoryContents, FsError>> {
        FsClient::read_directory(self, workspace_path, dir_path)
            .map(|result| result.unwrap_or_else(|lost| Err(ApiError::from(lost).into())))
            .boxed()
    }
}

/// Whether a daemon read failure means the file is not there (`isMissingFileError`).
pub fn is_missing_file_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("no such file") || lower.contains("not found") || lower.contains("os error 2")
}

/// A text file's content, or what kept it from being read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextRead {
    Read(String),
    Missing,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum TextReadError {
    #[error(transparent)]
    Fs(#[from] FsError),
    #[error("{path} is {size} bytes, over the {limit} byte read limit")]
    TooLarge { path: String, size: u64, limit: u64 },
}

/// Read a small text file (within [`MAX_READ_FILE_BYTES`]); a missing file is
/// [`TextRead::Missing`], any other failure an error. Invalid UTF-8 is replaced, as the
/// daemon's text reads do.
pub async fn read_text(
    fs: &dyn FilesystemApi,
    workspace_path: &str,
    file_path: &str,
) -> Result<TextRead, TextReadError> {
    match fs
        .read_file(workspace_path, file_path, MAX_READ_FILE_BYTES)
        .await
    {
        Ok(ReadOutcome::Read(file)) => Ok(TextRead::Read(
            String::from_utf8_lossy(&file.content).into_owned(),
        )),
        Ok(ReadOutcome::TooLarge { size }) => Err(TextReadError::TooLarge {
            path: file_path.to_owned(),
            size,
            limit: MAX_READ_FILE_BYTES,
        }),
        Err(FsError::Daemon(message)) if is_missing_file_error(&message) => Ok(TextRead::Missing),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_errors_are_recognised_by_message() {
        assert!(is_missing_file_error(
            "No such file or directory (os error 2)"
        ));
        assert!(is_missing_file_error("File not found"));
        assert!(is_missing_file_error("os error 2"));
        assert!(!is_missing_file_error("permission denied"));
    }
}
