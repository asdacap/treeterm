use std::collections::HashMap;
use std::ffi::CString;
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::unix::AsyncFd;
use tokio::sync::{broadcast, Mutex};
use treeterm_proto::treeterm::*;

const MERGE_THRESHOLD: usize = 50 * 1024; // 50KB
const COMPACTED_LIMIT: usize = 1024 * 1024; // 1MB
const SCROLLBACK_LINES: u16 = 10000;

pub struct PtySession {
    pub id: String,
    pub master_fd: RawFd,
    pub child_pid: libc::pid_t,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub buffer1: Vec<Vec<u8>>,
    pub buffer1_size: usize,
    pub buffer2: Vec<Vec<u8>>,
    pub buffer2_size: usize,
    pub created_at: i64,
    pub last_activity: i64,
    pub exit_code: Option<i32>,
    pub data_tx: broadcast::Sender<Vec<u8>>,
    pub exit_tx: broadcast::Sender<(i32, Option<i32>)>,
    pub resize_tx: broadcast::Sender<(i32, i32)>,
}

#[derive(Clone)]
pub struct PtyManager {
    sessions: Arc<Mutex<HashMap<String, PtySession>>>,
    counter: Arc<AtomicUsize>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
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

                let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
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

        let (data_tx, _) = broadcast::channel(256);
        let (exit_tx, _) = broadcast::channel(4);
        let (resize_tx, _) = broadcast::channel(16);

        let now = chrono_now_millis();
        let session = PtySession {
            id: id.clone(),
            master_fd,
            child_pid,
            cwd,
            cols,
            rows,
            buffer1: Vec::new(),
            buffer1_size: 0,
            buffer2: Vec::new(),
            buffer2_size: 0,
            created_at: now,
            last_activity: now,
            exit_code: None,
            data_tx: data_tx.clone(),
            exit_tx: exit_tx.clone(),
            resize_tx: resize_tx.clone(),
        };

        self.sessions.lock().await.insert(id.clone(), session);

        // Spawn background reader task
        let sessions = self.sessions.clone();
        let reader_id = id.clone();
        tokio::spawn(async move {
            if let Err(e) = read_pty_loop(master_fd, &reader_id, data_tx, sessions).await {
                tracing::debug!(session_id = %reader_id, error = %e, "pty reader ended");
            }
        });

        // Spawn waitpid task
        let sessions = self.sessions.clone();
        let wait_id = id.clone();
        tokio::spawn(async move {
            let (exit_code, signal) = waitpid_blocking(child_pid).await;
            tracing::info!(session_id = %wait_id, exit_code, ?signal, "pty child exited");
            let mut sessions = sessions.lock().await;
            if let Some(session) = sessions.get_mut(&wait_id) {
                session.exit_code = Some(exit_code);
                let _ = session.exit_tx.send((exit_code, signal));
            }
        });

        // Execute startup command if provided
        if let Some(cmd) = startup_command.filter(|s| !s.trim().is_empty()) {
            let fd = master_fd;
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                let data = format!("exec {}\n", cmd.trim());
                unsafe {
                    libc::write(fd, data.as_ptr() as *const libc::c_void, data.len());
                }
            });
        }

        tracing::info!(session_id = %id, "pty session created");
        Ok(id)
    }

    pub async fn get_scrollback(&self, session_id: &str) -> Result<Vec<Vec<u8>>, String> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("session {} not found", session_id))?;
        let mut scrollback = session.buffer1.clone();
        scrollback.extend(session.buffer2.iter().cloned());
        Ok(scrollback)
    }

    pub async fn write(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("session {} not found", session_id))?;
        if session.exit_code.is_some() {
            return Ok(()); // no-op if exited
        }
        let fd = session.master_fd;
        let n = unsafe { libc::write(fd, data.as_ptr() as *const libc::c_void, data.len()) };
        if n < 0 {
            return Err(format!(
                "write failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    pub async fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("session {} not found", session_id))?;
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
        session.last_activity = chrono_now_millis();
        let _ = session.resize_tx.send((cols as i32, rows as i32));
        Ok(())
    }

    pub async fn kill(&self, session_id: &str) {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.remove(session_id) {
            unsafe {
                libc::kill(session.child_pid, libc::SIGTERM);
                libc::close(session.master_fd);
            }
            tracing::info!(session_id, "pty session killed");
        } else {
            tracing::warn!(session_id, "kill: session not found");
        }
    }

    pub async fn list_sessions(&self) -> Vec<PtySessionInfo> {
        let sessions = self.sessions.lock().await;
        sessions
            .values()
            .map(|s| PtySessionInfo {
                id: s.id.clone(),
                cwd: s.cwd.clone(),
                cols: s.cols as i32,
                rows: s.rows as i32,
                created_at: s.created_at,
                last_activity: s.last_activity,
            })
            .collect()
    }

    /// Subscribe to data broadcasts for a session.
    pub async fn subscribe_data(
        &self,
        session_id: &str,
    ) -> Result<broadcast::Receiver<Vec<u8>>, String> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("session {} not found", session_id))?;
        Ok(session.data_tx.subscribe())
    }

    /// Subscribe to exit broadcasts for a session.
    pub async fn subscribe_exit(
        &self,
        session_id: &str,
    ) -> Result<broadcast::Receiver<(i32, Option<i32>)>, String> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("session {} not found", session_id))?;
        Ok(session.exit_tx.subscribe())
    }

    /// Subscribe to resize broadcasts for a session.
    pub async fn subscribe_resize(
        &self,
        session_id: &str,
    ) -> Result<broadcast::Receiver<(i32, i32)>, String> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("session {} not found", session_id))?;
        Ok(session.resize_tx.subscribe())
    }

    /// Get the exit code for a session (if already exited).
    pub async fn get_exit_code(&self, session_id: &str) -> Result<Option<i32>, String> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| format!("session {} not found", session_id))?;
        Ok(session.exit_code)
    }

    pub async fn shutdown(&self) {
        let mut sessions = self.sessions.lock().await;
        for (id, session) in sessions.drain() {
            unsafe {
                libc::kill(session.child_pid, libc::SIGTERM);
                libc::close(session.master_fd);
            }
            tracing::info!(session_id = %id, "pty session killed during shutdown");
        }
    }
}

/// Count the number of terminal lines that chunks would produce
/// when processed through a terminal emulator.
fn count_terminal_lines(chunks: &[Vec<u8>], cols: u16, rows: u16) -> usize {
    let mut parser = vt100::Parser::new(rows, cols, SCROLLBACK_LINES as usize);
    for chunk in chunks {
        parser.process(chunk);
    }
    let screen = parser.screen();
    // scrollback lines + visible terminal rows
    screen.scrollback() + screen.size().0 as usize
}

fn append_scrollback(session: &mut PtySession, data: &[u8]) {
    session.buffer2.push(data.to_vec());
    session.buffer2_size += data.len();

    if session.buffer2_size > MERGE_THRESHOLD {
        compact_scrollback(session);
    }
}

fn compact_scrollback(session: &mut PtySession) {
    let cols = session.cols;
    let rows = session.rows;

    // Measure combined buffer1 + buffer2
    let combined: Vec<Vec<u8>> = session.buffer1.iter().chain(session.buffer2.iter()).cloned().collect();
    let combined_lines = count_terminal_lines(&combined, cols, rows);

    // Measure buffer2 alone
    let buffer2_lines = count_terminal_lines(&session.buffer2, cols, rows);

    if buffer2_lines < combined_lines {
        // buffer1 contributes meaningful scrollback — merge both into buffer1
        session.buffer1.extend(session.buffer2.drain(..));
        session.buffer1_size += session.buffer2_size;
    } else {
        // buffer1 is redundant — buffer2 alone captures all visible state
        session.buffer1 = std::mem::take(&mut session.buffer2);
        session.buffer1_size = session.buffer2_size;
    }

    // Clear buffer2
    session.buffer2 = Vec::new();
    session.buffer2_size = 0;

    // Truncate buffer1 if exceeds compacted limit
    while session.buffer1_size > COMPACTED_LIMIT && !session.buffer1.is_empty() {
        let removed = session.buffer1.remove(0);
        session.buffer1_size -= removed.len();
    }

    tracing::debug!(
        session_id = %session.id,
        buffer1_chunks = session.buffer1.len(),
        buffer1_size = session.buffer1_size,
        combined_lines,
        buffer2_lines,
        "scrollback compacted"
    );
}

/// Wrapper so AsyncFd can own something with AsRawFd.
struct FdWrapper(RawFd);

impl AsRawFd for FdWrapper {
    fn as_raw_fd(&self) -> RawFd {
        self.0
    }
}

async fn read_pty_loop(
    master_fd: RawFd,
    session_id: &str,
    data_tx: broadcast::Sender<Vec<u8>>,
    sessions: Arc<Mutex<HashMap<String, PtySession>>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let async_fd = AsyncFd::new(FdWrapper(master_fd))?;
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
                let _ = data_tx.send(chunk.clone());
                let mut sessions = sessions.lock().await;
                if let Some(session) = sessions.get_mut(session_id) {
                    session.last_activity = chrono_now_millis();
                    append_scrollback(session, &chunk);
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
