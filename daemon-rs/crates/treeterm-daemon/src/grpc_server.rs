use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};
use treeterm_proto::treeterm::*;
use treeterm_proto::treeterm::tree_term_daemon_server::TreeTermDaemon;

use crate::exec_manager;
use crate::filesystem;
use crate::pty_manager::PtyManager;
use crate::session_store::SessionStore;

pub struct DaemonService {
    session_store: SessionStore,
    pty_manager: PtyManager,
    client_counter: AtomicUsize,
}

impl DaemonService {
    pub fn new(session_store: SessionStore, pty_manager: PtyManager) -> Self {
        Self {
            session_store,
            pty_manager,
            client_counter: AtomicUsize::new(0),
        }
    }

    fn get_client_id(&self, metadata: &tonic::metadata::MetadataMap) -> String {
        metadata
            .get("client-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                format!(
                    "client-{}",
                    self.client_counter.fetch_add(1, Ordering::Relaxed)
                )
            })
    }
}

#[tonic::async_trait]
impl TreeTermDaemon for DaemonService {
    // ---- PTY Management ----

    async fn create_pty(
        &self,
        req: Request<CreatePtyRequest>,
    ) -> Result<Response<CreatePtyResponse>, Status> {
        let r = req.into_inner();
        let env: HashMap<String, String> = r.env.into_iter().collect();
        let cols = r.cols.unwrap_or(80) as u16;
        let rows = r.rows.unwrap_or(24) as u16;

        let session_id = self
            .pty_manager
            .create_pty(r.cwd, env, cols, rows, r.startup_command)
            .await
            .map_err(|e| Status::internal(e))?;

        Ok(Response::new(CreatePtyResponse { session_id }))
    }

    async fn kill_pty(&self, req: Request<KillPtyRequest>) -> Result<Response<Empty>, Status> {
        self.pty_manager.kill(&req.into_inner().session_id).await;
        Ok(Response::new(Empty {}))
    }

    async fn list_pty_sessions(
        &self,
        _req: Request<Empty>,
    ) -> Result<Response<ListPtySessionsResponse>, Status> {
        let sessions = self.pty_manager.list_sessions().await;
        Ok(Response::new(ListPtySessionsResponse { sessions }))
    }

    type PtyStreamStream = ReceiverStream<Result<PtyOutput, Status>>;

    async fn pty_stream(
        &self,
        req: Request<Streaming<PtyInput>>,
    ) -> Result<Response<Self::PtyStreamStream>, Status> {
        let mut in_stream = req.into_inner();
        let (tx, rx) = mpsc::channel(256);
        let pty_mgr = self.pty_manager.clone();

        tokio::spawn(async move {
            // Wait for PtyStartData as first message
            let session_id = match in_stream.message().await {
                Ok(Some(PtyInput {
                    input: Some(pty_input::Input::Start(s)),
                })) => s.session_id,
                _ => return,
            };

            // Send scrollback
            if let Ok(scrollback) = pty_mgr.get_scrollback(&session_id).await {
                for chunk in scrollback {
                    let msg = PtyOutput {
                        output: Some(pty_output::Output::Data(PtyData { data: chunk })),
                    };
                    if tx.send(Ok(msg)).await.is_err() {
                        return;
                    }
                }
            }

            // Check if already exited
            if let Ok(Some(exit_code)) = pty_mgr.get_exit_code(&session_id).await {
                let _ = tx
                    .send(Ok(PtyOutput {
                        output: Some(pty_output::Output::Exit(PtyExit {
                            exit_code,
                            signal: None,
                        })),
                    }))
                    .await;
                return;
            }

            // Subscribe to live events
            let mut data_rx = match pty_mgr.subscribe_data(&session_id).await {
                Ok(rx) => rx,
                Err(_) => return,
            };
            let mut exit_rx = match pty_mgr.subscribe_exit(&session_id).await {
                Ok(rx) => rx,
                Err(_) => return,
            };
            let mut resize_rx = match pty_mgr.subscribe_resize(&session_id).await {
                Ok(rx) => rx,
                Err(_) => return,
            };

            // Spawn task to handle incoming writes/resizes from client
            let pty_mgr2 = pty_mgr.clone();
            let sid = session_id.clone();
            tokio::spawn(async move {
                while let Ok(Some(msg)) = in_stream.message().await {
                    match msg.input {
                        Some(pty_input::Input::Write(w)) => {
                            let _ = pty_mgr2.write(&sid, &w.data).await;
                        }
                        Some(pty_input::Input::Resize(r)) => {
                            let _ = pty_mgr2
                                .resize(&sid, r.cols as u16, r.rows as u16)
                                .await;
                        }
                        _ => {}
                    }
                }
            });

            // Forward live events to client
            loop {
                tokio::select! {
                    Ok(data) = data_rx.recv() => {
                        let msg = PtyOutput {
                            output: Some(pty_output::Output::Data(PtyData { data })),
                        };
                        if tx.send(Ok(msg)).await.is_err() { break; }
                    }
                    Ok((exit_code, signal)) = exit_rx.recv() => {
                        let _ = tx.send(Ok(PtyOutput {
                            output: Some(pty_output::Output::Exit(PtyExit { exit_code, signal })),
                        })).await;
                        break;
                    }
                    Ok((cols, rows)) = resize_rx.recv() => {
                        let _ = tx.send(Ok(PtyOutput {
                            output: Some(pty_output::Output::Resize(PtyResizeData { cols, rows })),
                        })).await;
                    }
                    else => break,
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    // ---- Exec Streaming ----

    type ExecStreamStream = ReceiverStream<Result<ExecOutput, Status>>;

    async fn exec_stream(
        &self,
        req: Request<Streaming<ExecInput>>,
    ) -> Result<Response<Self::ExecStreamStream>, Status> {
        let in_stream = req.into_inner();
        let (tx, rx) = mpsc::channel(256);

        tokio::spawn(async move {
            exec_manager::handle_exec_stream(in_stream, tx).await;
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    // ---- Session Management ----

    async fn create_session(
        &self,
        req: Request<CreateSessionRequest>,
    ) -> Result<Response<Session>, Status> {
        let client_id = self.get_client_id(req.metadata());
        let r = req.into_inner();
        let session = self
            .session_store
            .create_session(&client_id, r.workspaces)
            .await;
        Ok(Response::new(session))
    }

    async fn update_session(
        &self,
        req: Request<UpdateSessionRequest>,
    ) -> Result<Response<Session>, Status> {
        let client_id = self.get_client_id(req.metadata());
        let r = req.into_inner();

        let sender_id = r.sender_id.unwrap_or_default();
        if sender_id.is_empty() {
            return Err(Status::invalid_argument(
                "senderId is required for session updates",
            ));
        }

        let session = self
            .session_store
            .update_session(&client_id, &r.session_id, r.workspaces)
            .await
            .ok_or_else(|| Status::not_found(format!("session not found: {}", r.session_id)))?;

        // Broadcast to watchers
        self.session_store
            .broadcast_update(&r.session_id, &session, &sender_id)
            .await;

        Ok(Response::new(session))
    }

    async fn delete_session(
        &self,
        req: Request<DeleteSessionRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.session_store
            .delete_session(&req.into_inner().session_id)
            .await;
        Ok(Response::new(Empty {}))
    }

    async fn list_sessions(
        &self,
        _req: Request<Empty>,
    ) -> Result<Response<ListSessionsResponse>, Status> {
        let sessions = self.session_store.list_sessions().await;
        Ok(Response::new(ListSessionsResponse { sessions }))
    }

    async fn get_default_session_id(
        &self,
        req: Request<Empty>,
    ) -> Result<Response<GetDefaultSessionIdResponse>, Status> {
        let client_id = self.get_client_id(req.metadata());
        let session = self
            .session_store
            .get_or_create_default_session(&client_id)
            .await;
        Ok(Response::new(GetDefaultSessionIdResponse {
            session_id: session.id,
        }))
    }

    type SessionWatchStream = ReceiverStream<Result<SessionWatchEvent, Status>>;

    async fn session_watch(
        &self,
        req: Request<SessionWatchRequest>,
    ) -> Result<Response<Self::SessionWatchStream>, Status> {
        let r = req.into_inner();
        if r.listener_id.is_empty() {
            return Err(Status::invalid_argument("listenerId is required"));
        }

        // Send initial session state
        let session = self
            .session_store
            .get_session(&r.session_id)
            .await
            .ok_or_else(|| Status::not_found(format!("session not found: {}", r.session_id)))?;

        let (tx, rx) = mpsc::channel(64);
        let initial_event = SessionWatchEvent {
            session_id: r.session_id.clone(),
            session: Some(session),
            sender_id: String::new(),
        };
        let _ = tx.send(Ok(initial_event)).await;

        self.session_store
            .add_watcher(r.listener_id, r.session_id, tx)
            .await;

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    // ---- Daemon Control ----

    async fn shutdown(&self, _req: Request<Empty>) -> Result<Response<Empty>, Status> {
        tracing::info!("shutdown requested via gRPC");
        self.pty_manager.shutdown().await;
        // Schedule process exit after response is sent
        tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            std::process::exit(0);
        });
        Ok(Response::new(Empty {}))
    }

    // ---- Filesystem Operations ----

    async fn read_directory(
        &self,
        req: Request<ReadDirectoryRequest>,
    ) -> Result<Response<ReadDirectoryResponse>, Status> {
        let r = req.into_inner();
        let result = filesystem::read_directory(Path::new(&r.workspace_path), &r.dir_path).await;
        Ok(Response::new(result))
    }

    type ReadFileStream = ReceiverStream<Result<FileReadChunk, Status>>;

    async fn read_file(
        &self,
        req: Request<ReadFileRequest>,
    ) -> Result<Response<Self::ReadFileStream>, Status> {
        let r = req.into_inner();
        let (tx, rx) = mpsc::channel(16);

        tokio::spawn(async move {
            match filesystem::read_file_streaming(Path::new(&r.workspace_path), &r.file_path).await {
                Ok((header, content)) => {
                    let _ = tx
                        .send(Ok(FileReadChunk {
                            chunk: Some(file_read_chunk::Chunk::Header(header)),
                        }))
                        .await;

                    // Send in 64KB chunks
                    for chunk in content.chunks(64 * 1024) {
                        let _ = tx
                            .send(Ok(FileReadChunk {
                                chunk: Some(file_read_chunk::Chunk::Data(FileReadData {
                                    data: chunk.to_vec(),
                                })),
                            }))
                            .await;
                    }

                    let _ = tx
                        .send(Ok(FileReadChunk {
                            chunk: Some(file_read_chunk::Chunk::End(FileReadEnd {
                                success: true,
                                error: None,
                            })),
                        }))
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(Ok(FileReadChunk {
                            chunk: Some(file_read_chunk::Chunk::End(FileReadEnd {
                                success: false,
                                error: Some(e),
                            })),
                        }))
                        .await;
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn write_file(
        &self,
        req: Request<Streaming<FileWriteChunk>>,
    ) -> Result<Response<WriteFileResponse>, Status> {
        let mut stream = req.into_inner();
        let mut workspace_path = String::new();
        let mut file_path = String::new();
        let mut chunks: Vec<u8> = Vec::new();

        while let Some(msg) = stream.message().await.map_err(|e| Status::internal(e.to_string()))? {
            match msg.chunk {
                Some(file_write_chunk::Chunk::Header(h)) => {
                    workspace_path = h.workspace_path;
                    file_path = h.file_path;
                }
                Some(file_write_chunk::Chunk::Data(d)) => {
                    chunks.extend(d.data);
                }
                Some(file_write_chunk::Chunk::End(_)) => break,
                None => {}
            }
        }

        let result = filesystem::write_file_streaming(Path::new(&workspace_path), &file_path, chunks).await;
        Ok(Response::new(result))
    }

    async fn search_files(
        &self,
        req: Request<SearchFilesRequest>,
    ) -> Result<Response<SearchFilesResponse>, Status> {
        let r = req.into_inner();
        let result = filesystem::search_files(Path::new(&r.workspace_path), &r.query).await;
        Ok(Response::new(result))
    }
}
