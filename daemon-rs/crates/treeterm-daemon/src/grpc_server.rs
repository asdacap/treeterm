use std::collections::HashMap;
use std::path::Path;

use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};
use treeterm_proto::treeterm::*;
use treeterm_proto::treeterm::tree_term_daemon_server::TreeTermDaemon;

use crate::connection_id::ConnectionId;
use crate::exec_manager;
use crate::file_watcher::FileWatcher;
use crate::filesystem;
use crate::pty_manager::BufferEvent;
use crate::session_store::SessionStore;

fn connection_id<T>(req: &Request<T>) -> Result<String, Status> {
    req.extensions()
        .get::<ConnectionId>()
        .map(|c| c.0.clone())
        .ok_or_else(|| Status::internal("missing connection ID"))
}

/// Frame the attach-time initial state for the wire, bracketed by ReplayStart/ReplayEnd.
/// The buffered events are the program's output verbatim — terminal queries included — so
/// the client needs to know which output is a replay and must not be answered again.
fn replay_messages(events: Vec<BufferEvent>) -> Vec<PtyOutput> {
    let start = PtyOutput {
        output: Some(pty_output::Output::ReplayStart(Empty {})),
    };
    let end = PtyOutput {
        output: Some(pty_output::Output::ReplayEnd(Empty {})),
    };
    let body = events.into_iter().map(|event| match event {
        BufferEvent::Data(data) => PtyOutput {
            output: Some(pty_output::Output::Data(PtyData { data })),
        },
        BufferEvent::Resize { cols, rows } => PtyOutput {
            output: Some(pty_output::Output::Resize(PtyResizeData {
                cols: cols as i32,
                rows: rows as i32,
            })),
        },
    });
    std::iter::once(start).chain(body).chain(std::iter::once(end)).collect()
}

fn exit_output(exit_code: i32, signal: Option<i32>) -> PtyOutput {
    PtyOutput {
        output: Some(pty_output::Output::Exit(PtyExit { exit_code, signal })),
    }
}

/// Forward a PTY's live output, size changes and exit to its stream until the PTY exits, its
/// subscriber is dropped, or the client goes away.
async fn forward_live_events(
    mut data_rx: mpsc::Receiver<Vec<u8>>,
    mut exit_rx: broadcast::Receiver<(i32, Option<i32>)>,
    mut resize_rx: broadcast::Receiver<(i32, i32)>,
    tx: mpsc::Sender<Result<PtyOutput, Status>>,
) {
    loop {
        tokio::select! {
            result = data_rx.recv() => {
                match result {
                    Some(data) => {
                        let msg = PtyOutput {
                            output: Some(pty_output::Output::Data(PtyData { data })),
                        };
                        if tx.send(Ok(msg)).await.is_err() { break; }
                    }
                    None => {
                        // The subscriber's senders are gone: the read loop dropped it because
                        // its buffer filled (try_send failed), or the session itself was
                        // dropped after a kill. The waitpid task broadcasts the exit before it
                        // drops its reference to the session, so a killed PTY's exit is here
                        // already, and the select may have picked this branch over it.
                        let last = match exit_rx.try_recv() {
                            Ok((exit_code, signal)) => Ok(exit_output(exit_code, signal)),
                            Err(_still_running) => Err(Status::resource_exhausted(
                                "Stream buffer overflow: terminal output exceeded buffer capacity",
                            )),
                        };
                        let _ = tx.send(last).await;
                        break;
                    }
                }
            }
            Ok((exit_code, signal)) = exit_rx.recv() => {
                let _ = tx.send(Ok(exit_output(exit_code, signal))).await;
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
}

pub struct DaemonService {
    session_store: SessionStore,
    file_watcher: FileWatcher,
}

impl DaemonService {
    pub fn new(session_store: SessionStore, file_watcher: FileWatcher) -> Self {
        Self { session_store, file_watcher }
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
            .session_store
            .pty_manager()
            .create_pty(r.cwd, env, cols, rows, r.startup_command, r.handle)
            .await
            .map_err(|e| Status::internal(e))?;

        Ok(Response::new(CreatePtyResponse { session_id }))
    }

    async fn kill_pty(&self, req: Request<KillPtyRequest>) -> Result<Response<Empty>, Status> {
        self.session_store.pty_manager().kill(&req.into_inner().session_id).await;
        Ok(Response::new(Empty {}))
    }

    async fn list_pty_sessions(
        &self,
        _req: Request<Empty>,
    ) -> Result<Response<ListPtySessionsResponse>, Status> {
        let sessions = self.session_store.pty_manager().list_sessions().await;
        Ok(Response::new(ListPtySessionsResponse { sessions }))
    }

    type PtyStreamStream = ReceiverStream<Result<PtyOutput, Status>>;

    async fn pty_stream(
        &self,
        req: Request<Streaming<PtyInput>>,
    ) -> Result<Response<Self::PtyStreamStream>, Status> {
        let mut in_stream = req.into_inner();
        let (tx, rx) = mpsc::channel(256);
        let pty_mgr = self.session_store.pty_manager().clone();

        tokio::spawn(async move {
            // Wait for PtyStartData as first message
            let (session_id, gated) = match in_stream.message().await {
                Ok(Some(PtyInput {
                    input: Some(pty_input::Input::Start(s)),
                })) => {
                    let gated = s.kind() == PtyAttachKind::Terminal;
                    (s.session_id, gated)
                }
                _ => return,
            };

            // Atomically subscribe + snapshot initial state under one lock.
            // This prevents data loss between get_initial_state() and subscribe_data().
            let (initial_events, data_rx) = match pty_mgr.subscribe_with_initial_state(&session_id).await {
                Ok(result) => result,
                Err(_) => return,
            };

            // Send initial state (vt100 snapshot + parser size + buffered events)
            for msg in replay_messages(initial_events) {
                if tx.send(Ok(msg)).await.is_err() {
                    return;
                }
            }

            // Check if already exited
            if let Ok(Some(exit_code)) = pty_mgr.get_exit_code(&session_id).await {
                let _ = tx.send(Ok(exit_output(exit_code, None))).await;
                return;
            }
            let exit_rx = match pty_mgr.subscribe_exit(&session_id).await {
                Ok(rx) => rx,
                Err(_) => return,
            };
            let resize_rx = match pty_mgr.subscribe_resize(&session_id).await {
                Ok(rx) => rx,
                Err(_) => return,
            };

            // Spawn task to handle incoming writes/resizes from client.
            // Writes are awaited (AsyncFd-based) so tonic won't pull the next
            // PtyInput until the current write has fully landed in the PTY
            // master — that's the HTTP/2 stream-level backpressure the client
            // relies on. On write error, propagate via tx so the client sees
            // the failure (otherwise the paste would silently truncate).
            let pty_mgr2 = pty_mgr.clone();
            let sid = session_id.clone();
            let tx_write = tx.clone();
            let attachment = pty_mgr.next_attachment_id();
            tokio::spawn(async move {
                while let Ok(Some(msg)) = in_stream.message().await {
                    match msg.input {
                        Some(pty_input::Input::Write(w)) => {
                            match pty_mgr2.write_from(&sid, attachment, gated, &w.data).await {
                                Ok(true) => {}
                                Ok(false) => tracing::debug!(
                                    session_id = %sid,
                                    attachment,
                                    bytes = w.data.len(),
                                    "dropped write from an unfocused attachment"
                                ),
                                Err(e) => {
                                    let _ = tx_write.send(Err(Status::internal(e))).await;
                                    break;
                                }
                            }
                        }
                        Some(pty_input::Input::Focus(_)) => {
                            if let Err(e) = pty_mgr2.focus(&sid, attachment).await {
                                tracing::warn!(session_id = %sid, attachment, error = %e, "focus failed");
                            }
                        }
                        Some(pty_input::Input::Resize(r)) => {
                            let _ = pty_mgr2
                                .resize(&sid, r.cols as u16, r.rows as u16)
                                .await;
                        }
                        _ => {}
                    }
                }
                pty_mgr2.release(&sid, attachment).await;
            });

            forward_live_events(data_rx, exit_rx, resize_rx, tx).await;
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

    // ---- Session Management (one session per daemon) ----

    async fn update_session(
        &self,
        req: Request<UpdateSessionRequest>,
    ) -> Result<Response<Session>, Status> {
        let conn_id = connection_id(&req)?;
        let r = req.into_inner();

        let sender_id = r.sender_id.unwrap_or_default();
        if sender_id.is_empty() {
            return Err(Status::invalid_argument(
                "senderId is required for session updates",
            ));
        }

        let (session, accepted) = self
            .session_store
            .update_session(r.workspace_refs, r.expected_version, &sender_id, &conn_id)
            .await;

        // Only broadcast to other watchers if the update was accepted
        if accepted {
            self.session_store
                .broadcast_update(&session, &sender_id)
                .await;
        }

        Ok(Response::new(session))
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

        // Snapshot + register atomically so no update can land between the initial
        // state and the subscription (it would otherwise be lost until the heartbeat).
        let (tx, rx) = mpsc::channel(64);
        self.session_store
            .add_watcher_with_initial(r.listener_id, tx)
            .await;

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn lock_session(
        &self,
        req: Request<LockSessionRequest>,
    ) -> Result<Response<LockSessionResponse>, Status> {
        let conn_id = connection_id(&req)?;
        let r = req.into_inner();

        let ttl_ms = r.ttl_ms.unwrap_or(60_000);
        let (acquired, session) = self
            .session_store
            .lock_session(conn_id.clone(), ttl_ms)
            .await;

        if acquired {
            self.session_store
                .broadcast_update(&session, "")
                .await;
        }

        Ok(Response::new(LockSessionResponse {
            acquired,
            session: Some(session),
        }))
    }

    async fn unlock_session(
        &self,
        req: Request<UnlockSessionRequest>,
    ) -> Result<Response<Session>, Status> {
        let conn_id = connection_id(&req)?;

        let session = self
            .session_store
            .unlock_session(&conn_id)
            .await;

        self.session_store
            .broadcast_update(&session, "")
            .await;

        Ok(Response::new(session))
    }

    async fn force_unlock_session(
        &self,
        _req: Request<ForceUnlockSessionRequest>,
    ) -> Result<Response<Session>, Status> {
        let session = self
            .session_store
            .force_unlock_session()
            .await;

        self.session_store
            .broadcast_update(&session, "")
            .await;

        Ok(Response::new(session))
    }

    // ---- Daemon Control ----

    async fn shutdown(&self, _req: Request<Empty>) -> Result<Response<Empty>, Status> {
        tracing::info!("shutdown requested via gRPC");
        self.session_store.shutdown().await;
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
        let mut expected_sha256: Option<String> = None;
        let mut chunks: Vec<u8> = Vec::new();

        while let Some(msg) = stream.message().await.map_err(|e| Status::internal(e.to_string()))? {
            match msg.chunk {
                Some(file_write_chunk::Chunk::Header(h)) => {
                    workspace_path = h.workspace_path;
                    file_path = h.file_path;
                    expected_sha256 = h.expected_sha256;
                }
                Some(file_write_chunk::Chunk::Data(d)) => {
                    chunks.extend(d.data);
                }
                Some(file_write_chunk::Chunk::End(_)) => break,
                None => {}
            }
        }

        let result = filesystem::write_file_streaming(Path::new(&workspace_path), &file_path, &chunks, expected_sha256).await;

        // Intercept: tell watchers about our own write directly. This is what makes
        // same-daemon updates instant on NFS, where inotify never fires.
        if result.success {
            self.file_watcher
                .notify_written(&filesystem::resolve_target(Path::new(&workspace_path), &file_path), &chunks)
                .await;
        }

        Ok(Response::new(result))
    }

    async fn delete_file(
        &self,
        req: Request<DeleteFileRequest>,
    ) -> Result<Response<DeleteFileResponse>, Status> {
        let r = req.into_inner();
        let result = filesystem::delete_file(Path::new(&r.workspace_path), &r.file_path).await;

        if result.success {
            self.file_watcher
                .notify_deleted(&filesystem::resolve_target(Path::new(&r.workspace_path), &r.file_path))
                .await;
        }

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

    // ---- File Watching ----

    type WatchFileStream = ReceiverStream<Result<FileWatchEvent, Status>>;

    async fn watch_file(
        &self,
        req: Request<WatchFileRequest>,
    ) -> Result<Response<Self::WatchFileStream>, Status> {
        let r = req.into_inner();
        if r.watcher_id.is_empty() {
            return Err(Status::invalid_argument("watcherId is required"));
        }

        let (tx, rx) = mpsc::channel(64);
        self.file_watcher
            .subscribe_content(Path::new(&r.workspace_path), &r.file_path, r.watcher_id, tx)
            .await?;

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    type WatchFileSignalStream = ReceiverStream<Result<FileSignalEvent, Status>>;

    async fn watch_file_signal(
        &self,
        req: Request<WatchFileRequest>,
    ) -> Result<Response<Self::WatchFileSignalStream>, Status> {
        let r = req.into_inner();
        if r.watcher_id.is_empty() {
            return Err(Status::invalid_argument("watcherId is required"));
        }

        let (tx, rx) = mpsc::channel(64);
        self.file_watcher
            .subscribe_signal(Path::new(&r.workspace_path), &r.file_path, r.watcher_id, tx)
            .await?;

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty_manager::PtyManager;

    #[test]
    fn replay_messages_brackets_the_initial_state() {
        let messages = replay_messages(vec![
            BufferEvent::Data(b"\x1b[c".to_vec()),
            BufferEvent::Resize { cols: 80, rows: 24 },
        ]);
        let outputs: Vec<_> = messages.into_iter().map(|m| m.output).collect();
        assert_eq!(
            outputs,
            vec![
                Some(pty_output::Output::ReplayStart(Empty {})),
                Some(pty_output::Output::Data(PtyData {
                    data: b"\x1b[c".to_vec(),
                })),
                Some(pty_output::Output::Resize(PtyResizeData { cols: 80, rows: 24 })),
                Some(pty_output::Output::ReplayEnd(Empty {})),
            ]
        );
    }

    #[test]
    fn replay_messages_brackets_an_empty_initial_state() {
        let outputs: Vec<_> = replay_messages(vec![]).into_iter().map(|m| m.output).collect();
        assert_eq!(
            outputs,
            vec![
                Some(pty_output::Output::ReplayStart(Empty {})),
                Some(pty_output::Output::ReplayEnd(Empty {})),
            ]
        );
    }

    type Sent = Result<Option<pty_output::Output>, (tonic::Code, String)>;

    const OVERFLOW: &str = "Stream buffer overflow: terminal output exceeded buffer capacity";

    /// What `forward_live_events` sends until it returns.
    async fn forwarded(
        data_rx: mpsc::Receiver<Vec<u8>>,
        exit_rx: broadcast::Receiver<(i32, Option<i32>)>,
        resize_rx: broadcast::Receiver<(i32, i32)>,
    ) -> Vec<Sent> {
        let (tx, mut rx) = mpsc::channel(4);
        let forwarding = tokio::spawn(forward_live_events(data_rx, exit_rx, resize_rx, tx));
        let mut sent = Vec::new();
        while let Some(message) = rx.recv().await {
            sent.push(
                message
                    .map(|output| output.output)
                    .map_err(|status| (status.code(), status.message().to_owned())),
            );
        }
        forwarding.await.unwrap();
        sent
    }

    fn exit(exit_code: i32, signal: Option<i32>) -> Sent {
        Ok(Some(pty_output::Output::Exit(PtyExit { exit_code, signal })))
    }

    #[tokio::test]
    async fn a_killed_ptys_stream_ends_with_its_exit_although_its_output_closes_with_it() {
        // A kill drops the session (and with it the output's senders) right after its exit
        // is broadcast, so both are ready at once; the select takes either, at random.
        for _ in 0..64 {
            let (data_tx, data_rx) = mpsc::channel(4);
            let (exit_tx, exit_rx) = broadcast::channel(4);
            let (resize_tx, resize_rx) = broadcast::channel(4);
            exit_tx.send((-1, Some(libc::SIGHUP))).unwrap();
            drop((data_tx, exit_tx, resize_tx));
            assert_eq!(
                forwarded(data_rx, exit_rx, resize_rx).await,
                vec![exit(-1, Some(libc::SIGHUP))]
            );
        }
    }

    #[tokio::test]
    async fn a_running_ptys_dropped_subscriber_ends_its_stream_with_an_overflow() {
        let (data_tx, data_rx) = mpsc::channel(4);
        let (exit_tx, exit_rx) = broadcast::channel(4);
        let (resize_tx, resize_rx) = broadcast::channel(4);
        data_tx.send(b"out".to_vec()).await.unwrap();
        // The read loop drops a subscriber whose buffer is full; the PTY runs on.
        drop(data_tx);
        assert_eq!(
            forwarded(data_rx, exit_rx, resize_rx).await,
            vec![
                Ok(Some(pty_output::Output::Data(PtyData {
                    data: b"out".to_vec()
                }))),
                Err((tonic::Code::ResourceExhausted, OVERFLOW.to_owned())),
            ]
        );
        drop((exit_tx, resize_tx));
    }

    #[tokio::test]
    async fn a_pty_that_exits_ends_its_stream_with_its_exit() {
        let (data_tx, data_rx) = mpsc::channel(4);
        let (exit_tx, exit_rx) = broadcast::channel(4);
        let (resize_tx, resize_rx) = broadcast::channel(4);
        // The session lives on after its child exits: its output stays open.
        exit_tx.send((3, None)).unwrap();
        assert_eq!(
            forwarded(data_rx, exit_rx, resize_rx).await,
            vec![exit(3, None)]
        );
        drop((data_tx, exit_tx, resize_tx));
    }

    #[tokio::test]
    async fn a_killed_ptys_stream_ends_with_its_exit() {
        let mgr = PtyManager::new();
        let id = mgr
            .create_pty("/tmp".into(), HashMap::new(), 80, 24, None, None)
            .await
            .unwrap();
        let (_initial, data_rx) = mgr.subscribe_with_initial_state(&id).await.unwrap();
        let exit_rx = mgr.subscribe_exit(&id).await.unwrap();
        let resize_rx = mgr.subscribe_resize(&id).await.unwrap();
        mgr.kill(&id).await;
        let sent = forwarded(data_rx, exit_rx, resize_rx).await;
        assert!(
            matches!(sent.last(), Some(Ok(Some(pty_output::Output::Exit(_))))),
            "{sent:?}"
        );
    }
}
