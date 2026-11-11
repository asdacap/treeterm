//! Compare-and-swap writes of workspace body files (`<workspace_data_dir>/<id>.json`).
//!
//! The session store is the only writer of these files in this process. Each write names
//! the sha it expects to replace, so two windows (or two apps) editing one workspace never
//! silently overwrite each other: the loser gets [`BodyWrite::Conflict`] and rewrites on
//! top of the winner.

use treeterm_proto::treeterm::{
    DeleteFileRequest, FileWriteChunk, FileWriteData, FileWriteEnd, FileWriteHeader,
    file_write_chunk,
};

use crate::daemon::{ApiError, DaemonConnection, unary};
use crate::rt::Fut;

/// Bytes per data message; well under the message size limit.
const CHUNK: usize = 1024 * 1024;

/// The current file the write must replace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expected {
    /// The file must not exist yet (first write of a new workspace).
    Absent,
    /// The file's current sha256.
    Sha(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyWrite {
    Written,
    /// The file changed since the expected version; nothing was written.
    Conflict,
    /// The daemon could not write the file.
    Failed(String),
}

impl DaemonConnection {
    /// CAS-write `content` to `file_name` under `data_dir`.
    pub fn write_body(
        &self,
        data_dir: &str,
        file_name: &str,
        content: String,
        expected: Expected,
    ) -> Fut<Result<BodyWrite, ApiError>> {
        let mut client = self.main.clone();
        let header = FileWriteHeader {
            workspace_path: data_dir.to_owned(),
            file_path: file_name.to_owned(),
            expected_sha256: Some(match expected {
                Expected::Absent => String::new(),
                Expected::Sha(sha) => sha,
            }),
        };
        let mut messages = vec![FileWriteChunk {
            chunk: Some(file_write_chunk::Chunk::Header(header)),
        }];
        for piece in content.as_bytes().chunks(CHUNK) {
            messages.push(FileWriteChunk {
                chunk: Some(file_write_chunk::Chunk::Data(FileWriteData {
                    data: piece.to_vec(),
                })),
            });
        }
        messages.push(FileWriteChunk {
            chunk: Some(file_write_chunk::Chunk::End(FileWriteEnd {})),
        });
        self.rt.run(async move {
            let response = unary(client.write_file(futures::stream::iter(messages))).await?;
            Ok(if response.success {
                BodyWrite::Written
            } else if response.conflict {
                BodyWrite::Conflict
            } else {
                BodyWrite::Failed(response.error.unwrap_or_else(|| "unknown error".to_owned()))
            })
        })
    }

    /// Delete a body file (after its workspace left the session).
    pub fn delete_body(&self, data_dir: &str, file_name: &str) -> Fut<Result<(), ApiError>> {
        let mut client = self.main.clone();
        let request = DeleteFileRequest {
            workspace_path: data_dir.to_owned(),
            file_path: file_name.to_owned(),
        };
        self.rt.run(async move {
            let response = unary(client.delete_file(request)).await?;
            if response.success {
                Ok(())
            } else {
                Err(ApiError::Operation(response.error.unwrap_or_else(|| "delete failed".to_owned())))
            }
        })
    }
}
