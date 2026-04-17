use std::collections::{HashMap, VecDeque};
use std::ffi::{CStr, CString};
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::unix::AsyncFd;
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use treeterm_proto::treeterm::*;

const SCROLLBACK_LINES: u16 = 10000;
const RAW_BUFFER_CAP: usize = 500 * 1024;
const RAW_BUFFER_CHUNK_CAP: usize = 1024;
/// Per-subscriber data channel capacity (~10MB at 4KB/read).
const PTY_SUBSCRIBER_CAP: usize = 2560;

#[derive(Clone)]
pub enum BufferEvent {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
}

pub struct PtySession {
    pub id: String,
    pub master_fd: RawFd,
    /// Shared AsyncFd for the master fd. The read loop and write() both call
    /// `.readable()` / `.writable()` on the same registration — tokio permits
    /// concurrent readable/writable awaits on a single AsyncFd, but registering
    /// the fd twice via `AsyncFd::new` would fail with EEXIST.
    async_fd: Option<Arc<AsyncFd<FdWrapper>>>,
    pub child_pid: libc::pid_t,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub parser: vt100::Parser,
    pub raw_buffer: VecDeque<BufferEvent>,
    pub raw_buffer_data_bytes: usize,
    pub created_at: i64,
    pub last_activity: i64,
    pub exit_code: Option<i32>,
    pub data_subscribers: Vec<mpsc::Sender<Vec<u8>>>,
    pub exit_tx: broadcast::Sender<(i32, Option<i32>)>,
    pub resize_tx: broadcast::Sender<(i32, i32)>,
    /// Handle to the background read_pty_loop task. Must be aborted before
    /// closing master_fd to prevent stale AsyncFd from corrupting tokio's I/O driver
    /// when the fd number is reused by a new PTY session.
    pub reader_handle: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Clone)]
pub struct PtyManager {
    sessions: Arc<RwLock<HashMap<String, Arc<Mutex<PtySession>>>>>,
    counter: Arc<AtomicUsize>,
}

fn get_login_shell() -> String {
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if !pw.is_null() {
            if let Ok(s) = CStr::from_ptr((*pw).pw_shell).to_str() {
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
    }
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub async fn create_pty(
        &self,
        cwd: String,
        env: HashMap<String, String>,
        cols: u16,
        rows: u16,
        startup_command: Option<String>,
    ) -> Result<String, String> {
        let id = format!("pty-{}", self.counter.fetch_add(1, Ordering::Relaxed) + 1);

        let (master_fd, child_pid) = unsafe {
            let mut master: libc::c_int = 0;
            let mut ws = libc::winsize {
                ws_row: rows,
                ws_col: cols,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            let pid = libc::forkpty(
                &mut master,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut ws,
            );
            if pid < 0 {
                return Err(format!("forkpty failed: {}", std::io::Error::last_os_error()));
            }
            if pid == 0 {
                // Child process
                for (k, v) in &env {
                    std::env::set_var(k, v);
                }
                std::env::set_var("TERM", "xterm-256color");
                let _ = std::env::set_current_dir(&cwd);

                let shell = get_login_shell();
                let c_shell = CString::new(shell).unwrap();
                let c_arg = CString::new("-l").unwrap();
                libc::execvp(
                    c_shell.as_ptr(),
                    [c_shell.as_ptr(), c_arg.as_ptr(), std::ptr::null()].as_ptr(),
                );
                libc::_exit(1);
            }
            (master, pid)
        };

        // Set master fd to non-blocking
        unsafe {
            let flags = libc::fcntl(master_fd, libc::F_GETFL);
            libc::fcntl(master_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }

        // Register the non-blocking fd with tokio's I/O driver once. Both the
        // reader loop and PtyManager::write share this registration via Arc —
        // a second AsyncFd::new on the same fd would fail with EEXIST.
        let async_fd = Arc::new(
            AsyncFd::new(FdWrapper(master_fd))
                .map_err(|e| format!("AsyncFd::new failed: {}", e))?,
        );

        let (exit_tx, _) = broadcast::channel(4);
        let (resize_tx, _) = broadcast::channel(16);

        let now = chrono_now_millis();
        let session = PtySession {
            id: id.clone(),
            master_fd,
            async_fd: Some(Arc::clone(&async_fd)),
            child_pid,
            cwd,
            cols,
            rows,
            parser: vt100::Parser::new(rows, cols, SCROLLBACK_LINES as usize),
            raw_buffer: VecDeque::new(),
            raw_buffer_data_bytes: 0,
            created_at: now,
            last_activity: now,
            exit_code: None,
            data_subscribers: vec![],
            exit_tx: exit_tx.clone(),
            resize_tx: resize_tx.clone(),
            reader_handle: None,
        };

        let session = Arc::new(Mutex::new(session));
        self.sessions.write().await.insert(id.clone(), Arc::clone(&session));

        // Subscribe for startup command detection
        let startup_rx = if startup_command.as_deref().map_or(false, |s| !s.trim().is_empty()) {
            let (tx, rx) = mpsc::channel(256);
            session.lock().await.data_subscribers.push(tx);
            Some(rx)
        } else {
            None
        };

        // Spawn background reader task
        let reader_session = Arc::clone(&session);
        let reader_id = id.clone();
        let reader_fd = Arc::clone(&async_fd);
        let reader_handle = tokio::spawn(async move {
            if let Err(e) = read_pty_loop(reader_fd, reader_session).await {
                tracing::debug!(session_id = %reader_id, error = %e, "pty reader ended");
            }
        });
        session.lock().await.reader_handle = Some(reader_handle);

        // Spawn waitpid task
        let wait_session = Arc::clone(&session);
        let wait_id = id.clone();
        tokio::spawn(async move {
            let (exit_code, signal) = waitpid_blocking(child_pid).await;
            tracing::info!(session_id = %wait_id, exit_code, ?signal, "pty child exited");
            let mut session = wait_session.lock().await;
            session.exit_code = Some(exit_code);
            let _ = session.exit_tx.send((exit_code, signal));
        });

        // Execute startup command once the shell is ready to accept input.
        // Uses PtyManager::write() instead of raw libc::write() to avoid writing
        // to a closed/reused fd if the PTY was killed during the wait.
        if let Some((cmd, mut rx)) = startup_command
            .filter(|s| !s.trim().is_empty())
            .zip(startup_rx)
        {
            let startup_mgr = self.clone();
            let startup_id = id.clone();
            tokio::spawn(async move {
                wait_for_shell_ready(&mut rx).await;
                let data = format!("exec {}\n", cmd.trim());
                let _ = startup_mgr.write(&startup_id, data.as_bytes()).await;
            });
        }

        tracing::info!(session_id = %id, "pty session created");
        Ok(id)
    }

    pub async fn get_initial_state(&self, session_id: &str) -> Result<Vec<BufferEvent>, String> {
        let session = {
            let sessions = self.sessions.read().await;
            Arc::clone(sessions
                .get(session_id)
                .ok_or_else(|| format!("session {} not found", session_id))?)
        };
        let session = session.lock().await;

        let mut events = Vec::new();

        // Compacted old state from vt100
        let state = session.parser.screen().state_formatted();
        if !state.is_empty() {
            events.push(BufferEvent::Data(state));
        }

        // Parser's current size — the size at which state_formatted() was rendered
        let (rows, cols) = session.parser.screen().size();
        events.push(BufferEvent::Resize { cols, rows });

        // Buffered recent events (data + resize interleaved)
        for event in &session.raw_buffer {
            events.push(event.clone());
        }

        Ok(events)
    }

    pub async fn write(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        let async_fd = {
            let session = {
                let sessions = self.sessions.read().await;
                Arc::clone(sessions
                    .get(session_id)
                    .ok_or_else(|| format!("session {} not found", session_id))?)
            };
            let session = session.lock().await;
            if session.exit_code.is_some() {
                return Ok(());
            }
            match &session.async_fd {
                Some(afd) => Arc::clone(afd),
                None => return Err("pty master fd already released".to_string()),
            }
        };

        let mut offset = 0;
        while offset < data.len() {
            let mut guard = async_fd
                .writable()
                .await
                .map_err(|e| format!("writable await failed: {}", e))?;

            match guard.try_io(|fd| {
                let slice = &data[offset..];
                let n = unsafe {
                    libc::write(
                        fd.as_raw_fd(),
                        slice.as_ptr() as *const libc::c_void,
                        slice.len(),
                    )
                };
                if n < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            }) {
                Ok(Ok(n)) => offset += n,
                Ok(Err(e)) => {
                    // EIO on the master side means the child has closed its
                    // slave — i.e. the process exited just before/between the
                    // exit_code check and this write. Treat it as a benign
                    // race: the remaining bytes have nowhere to go, so return
                    // success (mirrors the behaviour of the early exit_code
                    // short-circuit above, and matches how the read loop
                    // treats EIO).
                    if e.raw_os_error() == Some(libc::EIO) {
                        return Ok(());
                    }
                    return Err(format!("write failed: {}", e));
                }
                // try_io observed WouldBlock — loop and re-await writability.
                Err(_would_block) => continue,
            }
        }
        Ok(())
    }

    pub async fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let session = {
            let sessions = self.sessions.read().await;
            Arc::clone(sessions
                .get(session_id)
                .ok_or_else(|| format!("session {} not found", session_id))?)
        };
        let mut session = session.lock().await;
        if session.exit_code.is_some() {
            return Ok(());
        }
        let ws = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let ret = unsafe { libc::ioctl(session.master_fd, libc::TIOCSWINSZ, &ws) };
        if ret < 0 {
            return Err(format!(
                "ioctl TIOCSWINSZ failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        session.cols = cols;
        session.rows = rows;
        session.raw_buffer.push_back(BufferEvent::Resize { cols, rows });
        session.last_activity = chrono_now_millis();
        let _ = session.resize_tx.send((cols as i32, rows as i32));
        Ok(())
    }

    pub async fn kill(&self, session_id: &str) {
        let removed = self.sessions.write().await.remove(session_id);
        if let Some(session) = removed {
            let mut session = session.lock().await;
            // Abort reader task and AWAIT it — abort() only schedules cancellation;
            // the task (and its AsyncFd) may still be alive until tokio processes it.
            // Awaiting ensures the AsyncFd is fully dropped and deregistered from epoll
            // BEFORE we close the fd, preventing fd-reuse corruption in tokio's I/O driver.
            if let Some(handle) = session.reader_handle.take() {
                handle.abort();
                let _ = handle.await;
            }
            // Drop the session's AsyncFd ref so tokio deregisters before close.
            // In-flight writes that still hold their own Arc will observe EBADF.
            let _ = session.async_fd.take();
            let pid = session.child_pid;
            let mut exit_rx = session.exit_tx.subscribe();
            unsafe {
                libc::kill(pid, libc::SIGTERM);
                libc::close(session.master_fd);
            }
            // Release lock so the waitpid task can access the session to broadcast exit
            drop(session);
            // SIGKILL fallback: if process doesn't exit within 5s after SIGTERM,
            // send SIGKILL to prevent zombie processes from SIGTERM-immune processes.
            // Safe because waitpid hasn't reaped the child yet, so PID is still ours.
            tokio::spawn(async move {
                if tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    exit_rx.recv(),
                )
                .await
                .is_err()
                {
                    tracing::warn!(pid, "process did not exit after SIGTERM, sending SIGKILL");
                    unsafe { libc::kill(pid, libc::SIGKILL); }
                }
            });
            tracing::info!(session_id, "pty session killed");
        } else {
            tracing::warn!(session_id, "kill: session not found");
        }
    }

    pub async fn list_sessions(&self) -> Vec<PtySessionInfo> {
        let sessions = self.sessions.read().await;
        let mut result = Vec::with_capacity(sessions.len());
        for session in sessions.values() {
            let s = session.lock().await;
            result.push(PtySessionInfo {
                id: s.id.clone(),
                cwd: s.cwd.clone(),
                cols: s.cols as i32,
                rows: s.rows as i32,
                created_at: s.created_at,
                last_activity: s.last_activity,
            });
        }
        result
    }

    /// Atomically subscribe to live data AND snapshot initial state under one lock.
    /// This eliminates the race where data arriving between get_initial_state()
    /// and subscribe_data() would be lost, corrupting the terminal.
    ///
    /// Each subscriber gets its own ~10MB mpsc buffer. The read loop fans out
    /// directly to all subscribers via try_send() — no intermediate broadcast
    /// or bridge tasks. A slow gRPC client whose buffer fills is dropped.
    pub async fn subscribe_with_initial_state(
        &self,
        session_id: &str,
    ) -> Result<(Vec<BufferEvent>, mpsc::Receiver<Vec<u8>>), String> {
        let session = self.get_session(session_id).await?;
        let mut session = session.lock().await;

        // Register subscriber first — any data arriving after this point
        // goes into the mpsc buffer via the read loop.
        let (tx, rx) = mpsc::channel(PTY_SUBSCRIBER_CAP);
        session.data_subscribers.push(tx);

        // Snapshot state — guaranteed no gap between snapshot and subscription
        let mut events = Vec::new();

        let state = session.parser.screen().state_formatted();
        if !state.is_empty() {
            events.push(BufferEvent::Data(state));
        }

        let (rows, cols) = session.parser.screen().size();
        events.push(BufferEvent::Resize { cols, rows });

        for event in &session.raw_buffer {
            events.push(event.clone());
        }

        Ok((events, rx))
    }

    /// Subscribe to exit broadcasts for a session.
    pub async fn subscribe_exit(
        &self,
        session_id: &str,
    ) -> Result<broadcast::Receiver<(i32, Option<i32>)>, String> {
        let session = self.get_session(session_id).await?;
        let session = session.lock().await;
        Ok(session.exit_tx.subscribe())
    }

    /// Subscribe to resize broadcasts for a session.
    pub async fn subscribe_resize(
        &self,
        session_id: &str,
    ) -> Result<broadcast::Receiver<(i32, i32)>, String> {
        let session = self.get_session(session_id).await?;
        let session = session.lock().await;
        Ok(session.resize_tx.subscribe())
    }

    /// Get the current terminal size for a session.
    pub async fn get_size(&self, session_id: &str) -> Result<(i32, i32), String> {
        let session = self.get_session(session_id).await?;
        let session = session.lock().await;
        Ok((session.cols as i32, session.rows as i32))
    }

    /// Get the exit code for a session (if already exited).
    pub async fn get_exit_code(&self, session_id: &str) -> Result<Option<i32>, String> {
        let session = self.get_session(session_id).await?;
        let session = session.lock().await;
        Ok(session.exit_code)
    }

    /// Look up a session by ID (read-locks the map briefly, returns Arc to per-session mutex).
    async fn get_session(&self, session_id: &str) -> Result<Arc<Mutex<PtySession>>, String> {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .cloned()
            .ok_or_else(|| format!("session {} not found", session_id))
    }

    pub async fn shutdown(&self) {
        // Drain sessions from map first, releasing the write lock to avoid
        // blocking all other operations during per-session cleanup.
        let drained: Vec<_> = self.sessions.write().await.drain().collect();

        let mut sigkill_tasks = Vec::new();
        for (id, session) in drained {
            let mut session = session.lock().await;
            if let Some(handle) = session.reader_handle.take() {
                handle.abort();
                let _ = handle.await;
            }
            let _ = session.async_fd.take();
            let pid = session.child_pid;
            let mut exit_rx = session.exit_tx.subscribe();
            unsafe {
                libc::kill(pid, libc::SIGTERM);
                libc::close(session.master_fd);
            }
            drop(session);
            sigkill_tasks.push(tokio::spawn(async move {
                if tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    exit_rx.recv(),
                )
                .await
                .is_err()
                {
                    tracing::warn!(pid, "process did not exit after SIGTERM during shutdown, sending SIGKILL");
                    unsafe { libc::kill(pid, libc::SIGKILL); }
                }
            }));
            tracing::info!(session_id = %id, "pty session killed during shutdown");
        }

        // Wait for all SIGKILL fallback tasks (max 5s total since they run concurrently)
        for task in sigkill_tasks {
            let _ = task.await;
        }
    }
}

/// Wait until the shell is ready to accept input by detecting silence after output.
///
/// Phase 1: wait for the first PTY output chunk (shell has started). 5s timeout.
/// Phase 2: wait for 200ms of silence after the last output (shell prompt drawn, init done). 30s timeout.
async fn wait_for_shell_ready(rx: &mut mpsc::Receiver<Vec<u8>>) {
    // Phase 1: wait for first output (5s timeout)
    let phase1_timeout = tokio::time::sleep(std::time::Duration::from_secs(5));
    tokio::pin!(phase1_timeout);

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Some(_) => break,
                    None => return,
                }
            }
            _ = &mut phase1_timeout => {
                tracing::warn!("wait_for_shell_ready: timed out waiting for first output");
                return;
            }
        }
    }

    // Phase 2: wait for 200ms of silence (30s timeout)
    let phase2_timeout = tokio::time::sleep(std::time::Duration::from_secs(30));
    tokio::pin!(phase2_timeout);

    loop {
        let silence = tokio::time::sleep(std::time::Duration::from_millis(200));
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Some(_) => continue,
                    None => return,
                }
            }
            _ = silence => return,
            _ = &mut phase2_timeout => {
                tracing::warn!("wait_for_shell_ready: timed out waiting for silence");
                return;
            }
        }
    }
}

/// Wrapper so AsyncFd can own something with AsRawFd.
struct FdWrapper(RawFd);

impl AsRawFd for FdWrapper {
    fn as_raw_fd(&self) -> RawFd {
        self.0
    }
}

async fn read_pty_loop(
    async_fd: Arc<AsyncFd<FdWrapper>>,
    session: Arc<Mutex<PtySession>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    loop {
        let mut guard = async_fd.readable().await?;
        let mut buf = [0u8; 4096];
        match guard.try_io(|fd| {
            let n = unsafe {
                libc::read(
                    fd.as_raw_fd(),
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                )
            };
            if n < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(n as usize)
            }
        }) {
            Ok(Ok(0)) => break, // EOF
            Ok(Ok(n)) => {
                let chunk = buf[..n].to_vec();

                // Fan out to subscribers + update raw_buffer under lock
                {
                    let mut session = session.lock().await;
                    session.last_activity = chrono_now_millis();

                    // Fan out directly to per-subscriber mpsc channels (non-blocking)
                    session.data_subscribers.retain(|tx| {
                        tx.try_send(chunk.clone()).is_ok()
                    });

                    // Append to raw buffer in ≤1KB chunks for fine-grained eviction
                    for sub in chunk.chunks(RAW_BUFFER_CHUNK_CAP) {
                        session.raw_buffer.push_back(BufferEvent::Data(sub.to_vec()));
                    }
                    session.raw_buffer_data_bytes += n;

                    // Flush oldest events to vt100 parser when cap exceeded
                    while session.raw_buffer_data_bytes > RAW_BUFFER_CAP {
                        match session.raw_buffer.pop_front() {
                            Some(BufferEvent::Data(data)) => {
                                session.raw_buffer_data_bytes -= data.len();
                                session.parser.process(&data);
                            }
                            Some(BufferEvent::Resize { cols, rows }) => {
                                session.parser.set_size(rows, cols);
                            }
                            None => break,
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                // EIO is normal when child exits
                if e.raw_os_error() == Some(libc::EIO) {
                    break;
                }
                return Err(e.into());
            }
            Err(_would_block) => continue,
        }
    }
    Ok(())
}


async fn waitpid_blocking(child_pid: libc::pid_t) -> (i32, Option<i32>) {
    tokio::task::spawn_blocking(move || {
        let mut status: libc::c_int = 0;
        unsafe {
            libc::waitpid(child_pid, &mut status, 0);
        }
        if libc::WIFEXITED(status) {
            (libc::WEXITSTATUS(status), None)
        } else if libc::WIFSIGNALED(status) {
            (-1, Some(libc::WTERMSIG(status)))
        } else {
            (-1, None)
        }
    })
    .await
    .unwrap_or((-1, None))
}

fn chrono_now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_login_shell_returns_nonempty() {
        let shell = get_login_shell();
        assert!(!shell.is_empty());
        // Should be a valid path
        assert!(shell.starts_with('/'));
    }

    #[test]
    fn chrono_now_millis_returns_positive() {
        let ms = chrono_now_millis();
        assert!(ms > 0);
    }

    #[tokio::test]
    async fn pty_manager_new_has_no_sessions() {
        let mgr = PtyManager::new();
        let sessions = mgr.list_sessions().await;
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn create_pty_returns_id() {
        let mgr = PtyManager::new();
        let id = mgr
            .create_pty("/tmp".into(), HashMap::new(), 80, 24, None)
            .await
            .unwrap();

        assert!(id.starts_with("pty-"));

        let sessions = mgr.list_sessions().await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, id);
        assert_eq!(sessions[0].cols, 80);
        assert_eq!(sessions[0].rows, 24);

        mgr.kill(&id).await;
    }

    #[tokio::test]
    async fn create_pty_sequential_ids() {
        let mgr = PtyManager::new();
        let id1 = mgr.create_pty("/tmp".into(), HashMap::new(), 80, 24, None).await.unwrap();
        let id2 = mgr.create_pty("/tmp".into(), HashMap::new(), 80, 24, None).await.unwrap();

        assert_eq!(id1, "pty-1");
        assert_eq!(id2, "pty-2");

        mgr.kill(&id1).await;
        mgr.kill(&id2).await;
    }

    #[tokio::test]
    async fn write_to_nonexistent_session_errors() {
        let mgr = PtyManager::new();
        let result = mgr.write("nonexistent", b"hello").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn resize_nonexistent_session_errors() {
        let mgr = PtyManager::new();
        let result = mgr.resize("nonexistent", 120, 40).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_exit_code_nonexistent_errors() {
        let mgr = PtyManager::new();
        let result = mgr.get_exit_code("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_size_returns_initial_dimensions() {
        let mgr = PtyManager::new();
        let id = mgr.create_pty("/tmp".into(), HashMap::new(), 100, 50, None).await.unwrap();

        let (cols, rows) = mgr.get_size(&id).await.unwrap();
        assert_eq!(cols, 100);
        assert_eq!(rows, 50);

        mgr.kill(&id).await;
    }

    #[tokio::test]
    async fn write_large_payload_completes_via_backpressure() {
        // Regression guard for the macOS paste hang: writes larger than the
        // PTY slave input buffer (~1KB on macOS, ~4KB on Linux) must loop on
        // EAGAIN via AsyncFd::writable() until every byte lands. The shell
        // echoes input back, which drains the slave buffer through the reader
        // task — so write() completes only if the loop is correctly yielding
        // back to the runtime.
        let mgr = PtyManager::new();
        let id = mgr
            .create_pty("/tmp".into(), HashMap::new(), 80, 24, None)
            .await
            .unwrap();

        // 64KB of printable bytes terminated by a newline — well past
        // MAX_CANON on both platforms, so EAGAIN is exercised.
        let mut payload = vec![b'a'; 64 * 1024];
        payload.push(b'\n');

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            mgr.write(&id, &payload),
        )
        .await;

        assert!(result.is_ok(), "write did not complete within 10s");
        assert!(result.unwrap().is_ok(), "write returned an error");

        mgr.kill(&id).await;
    }

    #[tokio::test]
    async fn write_and_get_initial_state() {
        let mgr = PtyManager::new();
        let id = mgr.create_pty("/tmp".into(), HashMap::new(), 80, 24, None).await.unwrap();

        // Write something to the pty
        let _ = mgr.write(&id, b"echo hello\n").await;

        // Give the pty a moment to process
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let events = mgr.get_initial_state(&id).await;
        assert!(events.is_ok());
        let events = events.unwrap();
        assert!(!events.is_empty());

        mgr.kill(&id).await;
    }

    #[tokio::test]
    async fn resize_pty_updates_dimensions() {
        let mgr = PtyManager::new();
        let id = mgr.create_pty("/tmp".into(), HashMap::new(), 80, 24, None).await.unwrap();

        mgr.resize(&id, 120, 40).await.unwrap();
        let (cols, rows) = mgr.get_size(&id).await.unwrap();
        assert_eq!(cols, 120);
        assert_eq!(rows, 40);

        mgr.kill(&id).await;
    }

    #[tokio::test]
    async fn kill_removes_session() {
        let mgr = PtyManager::new();
        let id = mgr.create_pty("/tmp".into(), HashMap::new(), 80, 24, None).await.unwrap();

        mgr.kill(&id).await;

        let sessions = mgr.list_sessions().await;
        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn kill_nonexistent_is_noop() {
        let mgr = PtyManager::new();
        mgr.kill("nonexistent").await; // should not panic
    }

    #[tokio::test]
    async fn subscribe_with_initial_state_nonexistent_errors() {
        let mgr = PtyManager::new();
        assert!(mgr.subscribe_with_initial_state("nonexistent").await.is_err());
    }

    #[tokio::test]
    async fn subscribe_exit_nonexistent_errors() {
        let mgr = PtyManager::new();
        assert!(mgr.subscribe_exit("nonexistent").await.is_err());
    }

    #[tokio::test]
    async fn subscribe_resize_nonexistent_errors() {
        let mgr = PtyManager::new();
        assert!(mgr.subscribe_resize("nonexistent").await.is_err());
    }

    #[tokio::test]
    async fn get_size_nonexistent_errors() {
        let mgr = PtyManager::new();
        assert!(mgr.get_size("nonexistent").await.is_err());
    }

    #[tokio::test]
    async fn shutdown_clears_all_sessions() {
        let mgr = PtyManager::new();
        mgr.create_pty("/tmp".into(), HashMap::new(), 80, 24, None).await.unwrap();
        mgr.create_pty("/tmp".into(), HashMap::new(), 80, 24, None).await.unwrap();

        assert_eq!(mgr.list_sessions().await.len(), 2);

        mgr.shutdown().await;
        assert!(mgr.list_sessions().await.is_empty());
    }

    #[tokio::test]
    async fn get_exit_code_initially_none() {
        let mgr = PtyManager::new();
        let id = mgr.create_pty("/tmp".into(), HashMap::new(), 80, 24, None).await.unwrap();

        let exit_code = mgr.get_exit_code(&id).await.unwrap();
        assert!(exit_code.is_none());

        mgr.kill(&id).await;
    }

    #[tokio::test]
    async fn raw_buffer_starts_empty() {
        let mgr = PtyManager::new();
        let id = mgr.create_pty("/tmp".into(), HashMap::new(), 80, 24, None).await.unwrap();

        let sessions = mgr.sessions.read().await;
        let session = sessions.get(&id).unwrap().lock().await;
        assert!(session.raw_buffer.is_empty());
        assert_eq!(session.raw_buffer_data_bytes, 0);
        drop(session);
        drop(sessions);

        mgr.kill(&id).await;
    }

    #[tokio::test]
    async fn raw_buffer_accumulates_data() {
        let mgr = PtyManager::new();
        let id = mgr.create_pty("/tmp".into(), HashMap::new(), 80, 24, None).await.unwrap();

        // Write data and wait for read loop to process
        mgr.write(&id, b"echo test\n").await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let sessions = mgr.sessions.read().await;
        let session = sessions.get(&id).unwrap().lock().await;
        // Buffer should have accumulated some data events
        assert!(session.raw_buffer_data_bytes > 0);
        assert!(!session.raw_buffer.is_empty());
        drop(session);
        drop(sessions);

        mgr.kill(&id).await;
    }

    #[tokio::test]
    async fn raw_buffer_flush_cap() {
        let mgr = PtyManager::new();
        let id = mgr.create_pty("/tmp".into(), HashMap::new(), 80, 24, None).await.unwrap();

        {
            let sessions = mgr.sessions.read().await;
            let mut session = sessions.get(&id).unwrap().lock().await;

            // Manually push data exceeding cap
            let big_chunk = vec![b'A'; RAW_BUFFER_CAP + 100];
            for sub in big_chunk.chunks(RAW_BUFFER_CHUNK_CAP) {
                session.raw_buffer.push_back(BufferEvent::Data(sub.to_vec()));
            }
            session.raw_buffer_data_bytes += big_chunk.len();

            // Simulate the flush logic from read_pty_loop
            while session.raw_buffer_data_bytes > RAW_BUFFER_CAP {
                match session.raw_buffer.pop_front() {
                    Some(BufferEvent::Data(data)) => {
                        session.raw_buffer_data_bytes -= data.len();
                        session.parser.process(&data);
                    }
                    Some(BufferEvent::Resize { cols, rows }) => {
                        session.parser.set_size(rows, cols);
                    }
                    None => break,
                }
            }

            // After flush, buffer should be within cap
            assert!(session.raw_buffer_data_bytes <= RAW_BUFFER_CAP);
        }

        mgr.kill(&id).await;
    }

    #[tokio::test]
    async fn resize_adds_buffer_event() {
        let mgr = PtyManager::new();
        let id = mgr.create_pty("/tmp".into(), HashMap::new(), 80, 24, None).await.unwrap();

        mgr.resize(&id, 120, 40).await.unwrap();

        let sessions = mgr.sessions.read().await;
        let session = sessions.get(&id).unwrap().lock().await;

        // Should have at least one resize event in buffer
        let has_resize = session.raw_buffer.iter().any(|e| {
            matches!(e, BufferEvent::Resize { cols: 120, rows: 40 })
        });
        assert!(has_resize);
        drop(session);
        drop(sessions);

        mgr.kill(&id).await;
    }

    #[tokio::test]
    async fn initial_state_includes_parser_size() {
        let mgr = PtyManager::new();
        let id = mgr.create_pty("/tmp".into(), HashMap::new(), 80, 24, None).await.unwrap();

        let events = mgr.get_initial_state(&id).await.unwrap();

        // Should contain a resize event with the parser's size
        let has_resize = events.iter().any(|e| {
            matches!(e, BufferEvent::Resize { cols: 80, rows: 24 })
        });
        assert!(has_resize);

        mgr.kill(&id).await;
    }

    #[tokio::test]
    async fn initial_state_includes_buffer_events_after_parser_size() {
        let mgr = PtyManager::new();
        let id = mgr.create_pty("/tmp".into(), HashMap::new(), 80, 24, None).await.unwrap();

        // Resize to new dimensions — this goes into the buffer
        mgr.resize(&id, 100, 30).await.unwrap();

        let events = mgr.get_initial_state(&id).await.unwrap();

        // Find the parser size resize (80x24) and the buffer resize (100x30)
        let resize_events: Vec<_> = events.iter().filter(|e| {
            matches!(e, BufferEvent::Resize { .. })
        }).collect();

        // Should have at least 2 resizes: parser size + buffer resize
        assert!(resize_events.len() >= 2);

        // The buffer resize (100x30) should come after the parser size (80x24)
        let parser_size_idx = events.iter().position(|e| {
            matches!(e, BufferEvent::Resize { cols: 80, rows: 24 })
        }).unwrap();
        let buffer_resize_idx = events.iter().position(|e| {
            matches!(e, BufferEvent::Resize { cols: 100, rows: 30 })
        }).unwrap();
        assert!(buffer_resize_idx > parser_size_idx);

        mgr.kill(&id).await;
    }

    #[tokio::test]
    async fn get_initial_state_nonexistent_errors() {
        let mgr = PtyManager::new();
        assert!(mgr.get_initial_state("nonexistent").await.is_err());
    }

    #[tokio::test]
    async fn wait_for_shell_ready_detects_silence() {
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(16);

        let start = std::time::Instant::now();
        let handle = tokio::spawn(async move {
            wait_for_shell_ready(&mut rx).await;
        });

        // Simulate shell producing output for 100ms
        tx.send(b"some output".to_vec()).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        tx.send(b"more output".to_vec()).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        tx.send(b"prompt: ".to_vec()).await.unwrap();
        // Then silence — wait_for_shell_ready should return after 200ms of silence

        handle.await.unwrap();
        let elapsed = start.elapsed();

        // Should have returned after ~300ms (100ms output + 200ms silence), well under 5s
        assert!(elapsed < std::time::Duration::from_secs(2));
        // Should have waited at least 200ms for the silence window
        assert!(elapsed >= std::time::Duration::from_millis(200));
    }

    #[tokio::test]
    async fn wait_for_shell_ready_channel_closed() {
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(16);

        // Send one chunk then drop sender (simulates channel close)
        tx.send(b"output".to_vec()).await.unwrap();
        drop(tx);

        let start = std::time::Instant::now();
        wait_for_shell_ready(&mut rx).await;
        // Should return quickly once channel closes
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
    }

    #[tokio::test]
    async fn wait_for_shell_ready_no_output_times_out() {
        let (_tx, mut rx) = mpsc::channel::<Vec<u8>>(16);

        let start = std::time::Instant::now();
        wait_for_shell_ready(&mut rx).await;
        let elapsed = start.elapsed();

        // Should time out after 5s (Phase 1 timeout)
        assert!(elapsed >= std::time::Duration::from_secs(5));
        assert!(elapsed < std::time::Duration::from_secs(7));
    }

    #[tokio::test]
    async fn wait_for_shell_ready_continuous_output_waits_for_silence() {
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(16);

        let start = std::time::Instant::now();
        let handle = tokio::spawn(async move {
            wait_for_shell_ready(&mut rx).await;
        });

        // Simulate shell outputting continuously for 6 seconds (beyond old 5s shared timeout)
        for _ in 0..60 {
            tx.send(b"loading...".to_vec()).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        // Stop sending — silence should be detected after 200ms

        handle.await.unwrap();
        let elapsed = start.elapsed();

        // Should wait for all output (~6s) plus 200ms silence, not bail at 5s
        assert!(elapsed >= std::time::Duration::from_secs(6));
        assert!(elapsed < std::time::Duration::from_secs(8));
    }
}
