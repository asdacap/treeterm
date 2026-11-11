//! File I/O through the daemon: ReadFile, WriteFile (compare-and-swap), ReadDirectory,
//! SearchFiles, DeleteFile and WatchFileSignal.
//!
//! Paths are a `workspace_path` scope plus a `file_path` relative to it; the daemon refuses
//! anything that resolves outside the scope. Failures the daemon reports in its responses
//! (`success: false`) are [`FsError::Daemon`]; failed calls are [`FsError::Api`].

use futures::future::BoxFuture;
use futures::{FutureExt, StreamExt, TryFutureExt};
use sha2::{Digest, Sha256};
use treeterm_proto::treeterm::{
    DeleteFileRequest, DirectoryContents, FileEntry, FileReadHeader, FileWriteChunk, FileWriteData,
    FileWriteEnd, FileWriteHeader, ReadDirectoryRequest, ReadFileRequest, SearchFilesRequest,
    WatchFileRequest, file_read_chunk, file_signal_event, file_write_chunk,
};

use crate::daemon::{ApiError, UNARY_DEADLINE};
use crate::rt::{Fut, Rt, Sub};
use crate::transport::DaemonClient;

/// Largest data message of a write; matches Electron's chunking.
pub const WRITE_CHUNK: usize = 64 * 1024;

/// The transfer budget for reading one file for display (Electron's `MAX_READ_FILE_BYTES`).
pub const MAX_READ_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, thiserror::Error)]
pub enum FsError {
    #[error(transparent)]
    Api(#[from] ApiError),
    /// The daemon refused or failed the operation (not found, access denied, I/O error, ...).
    #[error("{0}")]
    Daemon(String),
}

/// A file read in full.
#[derive(Debug, Clone, PartialEq)]
pub struct FileRead {
    /// Resolved absolute path, size and detected language, as the daemon reported them.
    pub header: FileReadHeader,
    pub content: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReadOutcome {
    Read(FileRead),
    /// The file is bigger than the caller's budget. Nothing past the header was transferred;
    /// `size` lets the caller retry with a budget that admits it.
    TooLarge {
        size: u64,
    },
}

/// Compare-and-swap guard of a write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteGuard {
    /// Write whatever is there.
    Unconditional,
    /// The file must not exist yet.
    MustNotExist,
    /// The file's current bytes must have this lowercase-hex SHA-256 (as reported by a
    /// read, a watch or a previous write).
    Matches(String),
}

impl WriteGuard {
    fn wire(self) -> Option<String> {
        match self {
            WriteGuard::Unconditional => None,
            WriteGuard::MustNotExist => Some(String::new()),
            WriteGuard::Matches(sha256) => Some(sha256),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOutcome {
    /// The content is on disk. `sha256` is its lowercase-hex SHA-256: the guard for the
    /// next write.
    Written { sha256: String },
    /// The guard did not match: the file changed since it was read. Re-read, re-apply the
    /// change and retry.
    Conflict,
}

/// Whether a watched file exists, and its hash when it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileSignal {
    Present { sha256: String },
    Absent,
}

/// Lowercase-hex SHA-256, the form the daemon uses for CAS guards.
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Filesystem calls on one daemon connection. Cheap to clone.
#[derive(Clone)]
pub struct FsClient {
    rt: Rt,
    client: DaemonClient,
}

impl FsClient {
    pub fn new(rt: Rt, client: DaemonClient) -> Self {
        Self { rt, client }
    }

    /// Read a whole file. A file over `max_bytes` is refused on its header, before any of
    /// its content crosses the wire.
    pub fn read_file(
        &self,
        workspace_path: &str,
        file_path: &str,
        max_bytes: u64,
    ) -> Fut<Result<ReadOutcome, FsError>> {
        let mut client = self.client.clone();
        let request = ReadFileRequest {
            workspace_path: workspace_path.to_owned(),
            file_path: file_path.to_owned(),
        };
        self.rt.run(async move {
            let chunks = client
                .read_file(request)
                .await
                .map_err(ApiError::from)?
                .into_inner();
            collect_read(chunks.map(|chunk| chunk.map_err(ApiError::from)), max_bytes).await
        })
    }

    /// Write `content`, replacing the file atomically (creating parent directories), if
    /// `guard` holds.
    pub fn write_file(
        &self,
        workspace_path: &str,
        file_path: &str,
        content: Vec<u8>,
        guard: WriteGuard,
    ) -> Fut<Result<WriteOutcome, FsError>> {
        let mut client = self.client.clone();
        let sha256 = sha256_hex(&content);
        let messages = write_messages(workspace_path, file_path, content, guard);
        self.rt.run(async move {
            let response = unary(client.write_file(futures::stream::iter(messages))).await?;
            match (response.success, response.conflict) {
                (true, _) => Ok(WriteOutcome::Written { sha256 }),
                (false, true) => Ok(WriteOutcome::Conflict),
                (false, false) => Err(FsError::Daemon(
                    response.error.unwrap_or_else(|| "write failed".to_owned()),
                )),
            }
        })
    }

    /// Remove a file. Removing a file that does not exist succeeds.
    pub fn delete_file(&self, workspace_path: &str, file_path: &str) -> Fut<Result<(), FsError>> {
        let mut client = self.client.clone();
        let request = DeleteFileRequest {
            workspace_path: workspace_path.to_owned(),
            file_path: file_path.to_owned(),
        };
        self.rt.run(async move {
            let response = unary(client.delete_file(request)).await?;
            if response.success {
                Ok(())
            } else {
                Err(FsError::Daemon(
                    response.error.unwrap_or_else(|| "delete failed".to_owned()),
                ))
            }
        })
    }

    /// List a directory (relative to the scope; `""` is the scope itself). Hidden entries are
    /// skipped; directories come first, then names in order.
    pub fn read_directory(
        &self,
        workspace_path: &str,
        dir_path: &str,
    ) -> Fut<Result<DirectoryContents, FsError>> {
        let mut client = self.client.clone();
        let request = ReadDirectoryRequest {
            workspace_path: workspace_path.to_owned(),
            dir_path: dir_path.to_owned(),
        };
        self.rt.run(async move {
            let response = unary(client.read_directory(request)).await?;
            match (response.success, response.contents) {
                (true, Some(contents)) => Ok(contents),
                (true, None) => {
                    Err(ApiError::Protocol("ReadDirectory succeeded without contents").into())
                }
                (false, _) => Err(FsError::Daemon(
                    response
                        .error
                        .unwrap_or_else(|| "read directory failed".to_owned()),
                )),
            }
        })
    }

    /// Entries under the scope whose name contains `query` (case-insensitive), skipping
    /// hidden entries and `node_modules`. An empty query finds nothing.
    pub fn search_files(
        &self,
        workspace_path: &str,
        query: &str,
    ) -> Fut<Result<Vec<FileEntry>, FsError>> {
        let mut client = self.client.clone();
        let request = SearchFilesRequest {
            workspace_path: workspace_path.to_owned(),
            query: query.to_owned(),
        };
        self.rt.run(async move {
            let response = unary(client.search_files(request)).await?;
            if response.success {
                Ok(response.entries)
            } else {
                Err(FsError::Daemon(
                    response.error.unwrap_or_else(|| "search failed".to_owned()),
                ))
            }
        })
    }

    /// Whether a file exists and its hash; the first event is the current state. Lighter
    /// than [`crate::daemon::DaemonConnection::watch_file`], which carries the content.
    pub fn watch_file_signal(
        &self,
        workspace_path: &str,
        file_path: &str,
    ) -> Sub<Result<FileSignal, ApiError>> {
        let mut client = self.client.clone();
        let request = WatchFileRequest {
            workspace_path: workspace_path.to_owned(),
            file_path: file_path.to_owned(),
            watcher_id: uuid::Uuid::new_v4().to_string(),
        };
        let call = async move { client.watch_file_signal(request).await };
        self.rt.subscribe(
            call.map_ok(tonic::Response::into_inner)
                .try_flatten_stream()
                .map(|item| item.map_err(ApiError::from).and_then(file_signal)),
        )
    }
}

/// The part of the filesystem client the git client needs. Implemented by [`FsClient`] and
/// by test fakes.
pub trait FilesystemApi: Send + Sync {
    /// See [`FsClient::read_file`].
    fn read_file(
        &self,
        workspace_path: &str,
        file_path: &str,
        max_bytes: u64,
    ) -> BoxFuture<'static, Result<ReadOutcome, FsError>>;
}

impl FilesystemApi for FsClient {
    fn read_file(
        &self,
        workspace_path: &str,
        file_path: &str,
        max_bytes: u64,
    ) -> BoxFuture<'static, Result<ReadOutcome, FsError>> {
        FsClient::read_file(self, workspace_path, file_path, max_bytes)
            .map(|result| match result {
                Ok(outcome) => outcome,
                Err(lost) => Err(ApiError::from(lost).into()),
            })
            .boxed()
    }
}

/// Run a unary call with [`UNARY_DEADLINE`], as every other unary call does.
async fn unary<T>(
    call: impl Future<Output = Result<tonic::Response<T>, tonic::Status>>,
) -> Result<T, FsError> {
    match tokio::time::timeout(UNARY_DEADLINE, call).await {
        Ok(response) => Ok(response.map_err(ApiError::from)?.into_inner()),
        Err(_elapsed) => Err(ApiError::DeadlineExceeded(UNARY_DEADLINE).into()),
    }
}

fn write_messages(
    workspace_path: &str,
    file_path: &str,
    content: Vec<u8>,
    guard: WriteGuard,
) -> Vec<FileWriteChunk> {
    let header = file_write_chunk::Chunk::Header(FileWriteHeader {
        workspace_path: workspace_path.to_owned(),
        file_path: file_path.to_owned(),
        expected_sha256: guard.wire(),
    });
    let data = content.chunks(WRITE_CHUNK).map(|chunk| {
        file_write_chunk::Chunk::Data(FileWriteData {
            data: chunk.to_vec(),
        })
    });
    std::iter::once(header)
        .chain(data)
        .chain(std::iter::once(file_write_chunk::Chunk::End(
            FileWriteEnd {},
        )))
        .map(|chunk| FileWriteChunk { chunk: Some(chunk) })
        .collect()
}

/// Header, data chunks, then an end that says whether the read worked. Dropping the stream
/// (on a file over budget) cancels the call.
async fn collect_read(
    chunks: impl futures::Stream<Item = Result<treeterm_proto::treeterm::FileReadChunk, ApiError>>,
    max_bytes: u64,
) -> Result<ReadOutcome, FsError> {
    let mut chunks = std::pin::pin!(chunks);
    let mut header = None;
    let mut content = Vec::new();
    while let Some(chunk) = chunks.next().await {
        match (chunk?.chunk, &header) {
            (Some(file_read_chunk::Chunk::Header(read)), None) => {
                let size = u64::try_from(read.size)
                    .map_err(|_| ApiError::Protocol("ReadFile header with a negative size"))?;
                if size > max_bytes {
                    return Ok(ReadOutcome::TooLarge { size });
                }
                header = Some(read);
            }
            (Some(file_read_chunk::Chunk::Header(_)), Some(_)) => {
                return Err(ApiError::Protocol("ReadFile sent a second header").into());
            }
            (Some(file_read_chunk::Chunk::Data(data)), Some(_)) => {
                content.extend_from_slice(&data.data);
            }
            (Some(file_read_chunk::Chunk::Data(_)), None) => {
                return Err(ApiError::Protocol("ReadFile sent data before its header").into());
            }
            (Some(file_read_chunk::Chunk::End(end)), _) if !end.success => {
                return Err(FsError::Daemon(
                    end.error.unwrap_or_else(|| "Unknown error".to_owned()),
                ));
            }
            (Some(file_read_chunk::Chunk::End(_)), Some(_)) => {
                let header = header.take().expect("matched Some above");
                return Ok(ReadOutcome::Read(FileRead { header, content }));
            }
            (Some(file_read_chunk::Chunk::End(_)), None) => {
                return Err(ApiError::Protocol("ReadFile succeeded without a header").into());
            }
            (None, _) => return Err(ApiError::Protocol("FileReadChunk without a payload").into()),
        }
    }
    Err(ApiError::Protocol("ReadFile stream ended without an end message").into())
}

fn file_signal(event: treeterm_proto::treeterm::FileSignalEvent) -> Result<FileSignal, ApiError> {
    match event.state {
        Some(file_signal_event::State::Present(present)) => Ok(FileSignal::Present {
            sha256: present.sha256,
        }),
        Some(file_signal_event::State::Absent(_)) => Ok(FileSignal::Absent),
        None => Err(ApiError::Protocol("FileSignalEvent without a state")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use treeterm_proto::treeterm::{FileReadChunk, FileReadData, FileReadEnd};

    fn header(size: i64) -> Result<FileReadChunk, ApiError> {
        Ok(FileReadChunk {
            chunk: Some(file_read_chunk::Chunk::Header(FileReadHeader {
                path: "/ws/a.txt".to_owned(),
                size,
                language: "plaintext".to_owned(),
            })),
        })
    }

    fn data(bytes: &[u8]) -> Result<FileReadChunk, ApiError> {
        Ok(FileReadChunk {
            chunk: Some(file_read_chunk::Chunk::Data(FileReadData {
                data: bytes.to_vec(),
            })),
        })
    }

    fn end(error: Option<&str>) -> Result<FileReadChunk, ApiError> {
        Ok(FileReadChunk {
            chunk: Some(file_read_chunk::Chunk::End(FileReadEnd {
                success: error.is_none(),
                error: error.map(str::to_owned),
            })),
        })
    }

    fn read(
        chunks: Vec<Result<FileReadChunk, ApiError>>,
        max_bytes: u64,
    ) -> Result<ReadOutcome, FsError> {
        futures::executor::block_on(collect_read(futures::stream::iter(chunks), max_bytes))
    }

    #[test]
    fn collects_the_header_and_every_chunk() {
        let outcome = read(vec![header(5), data(b"he"), data(b"llo"), end(None)], 5).unwrap();
        let ReadOutcome::Read(file) = outcome else {
            panic!("expected a read, got {outcome:?}");
        };
        assert_eq!(file.content, b"hello");
        assert_eq!(file.header.size, 5);
        assert_eq!(file.header.language, "plaintext");
    }

    #[test]
    fn refuses_a_file_over_budget_on_its_header() {
        // The data after the header must never be looked at.
        let outcome = read(vec![header(6), Err(ApiError::Protocol("unreachable"))], 5).unwrap();
        assert_eq!(outcome, ReadOutcome::TooLarge { size: 6 });
    }

    #[test]
    fn reports_the_daemons_failure() {
        let error = read(vec![end(Some("No such file or directory (os error 2)"))], 5);
        assert!(matches!(error, Err(FsError::Daemon(message)) if message.contains("No such file")));
    }

    #[test]
    fn rejects_malformed_streams() {
        for chunks in [
            vec![data(b"x")],
            vec![header(1), header(1)],
            vec![end(None)],
            vec![header(1), data(b"x")],
            vec![Ok(FileReadChunk { chunk: None })],
            vec![header(-1)],
        ] {
            assert!(
                matches!(read(chunks, 10), Err(FsError::Api(ApiError::Protocol(_)))),
                "malformed stream accepted"
            );
        }
    }

    #[test]
    fn writes_are_a_header_then_chunks_then_an_end() {
        let content = vec![b'x'; WRITE_CHUNK + 1];
        let messages = write_messages("/ws", "a.txt", content, WriteGuard::MustNotExist);
        let kinds: Vec<_> = messages
            .into_iter()
            .map(|message| match message.chunk {
                Some(file_write_chunk::Chunk::Header(header)) => {
                    assert_eq!(header.expected_sha256.as_deref(), Some(""));
                    "header".to_owned()
                }
                Some(file_write_chunk::Chunk::Data(data)) => format!("data {}", data.data.len()),
                Some(file_write_chunk::Chunk::End(_)) => "end".to_owned(),
                None => panic!("empty chunk"),
            })
            .collect();
        assert_eq!(kinds, vec!["header", "data 65536", "data 1", "end"]);
    }

    #[test]
    fn guards_map_to_the_wire_form() {
        assert_eq!(WriteGuard::Unconditional.wire(), None);
        assert_eq!(WriteGuard::MustNotExist.wire(), Some(String::new()));
        assert_eq!(
            WriteGuard::Matches("abc".to_owned()).wire(),
            Some("abc".to_owned())
        );
    }

    #[test]
    fn sha256_is_lowercase_hex() {
        assert_eq!(
            sha256_hex(b"hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
