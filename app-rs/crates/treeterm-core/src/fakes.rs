//! Scripted stand-ins for the daemon and the network, shared by the tests of the modules
//! ported from the renderer (the TS tests' `createMockExec`, `createMockFilesystem`, and
//! `vi.spyOn(globalThis, 'fetch')`).

use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures::channel::mpsc;
use futures::future::{BoxFuture, FutureExt};
use futures::stream::{BoxStream, Stream, StreamExt};
use treeterm_proto::treeterm::{DirectoryContents, FileEntry, FileReadHeader};

use crate::daemon::ApiError;
use crate::exec::{ExecApi, ExecError, ExecEvent, ExecRequest};
use crate::fs::{FileRead, FilesystemApi, FsError, ReadOutcome, WriteGuard, WriteOutcome};
use crate::fs_ext::{DirectoryApi, FileWriteApi};
use crate::http::{HttpClient, HttpError, HttpRequest, HttpResponse};

pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    futures::executor::block_on(future)
}

/// Pending once, then ready: lets other futures run in between, like the TS mocks resolving
/// on a later tick.
pub(crate) async fn yield_once() {
    let mut yielded = false;
    futures::future::poll_fn(|cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await;
}

/// The error a call that never reached the daemon fails with.
pub(crate) fn unreachable_daemon(message: &str) -> ApiError {
    ApiError::from(tonic::Status::unavailable(message.to_owned()))
}

// === exec ==================================================================================

type ExecItem = Result<ExecEvent, ExecError>;

/// How one exec call plays out.
pub(crate) enum FakeRun {
    /// These events, then the stream ends.
    Events(Vec<ExecItem>),
    /// Whatever the test sends through the paired sender.
    Controlled(mpsc::UnboundedReceiver<ExecItem>),
}

/// Prints `stdout` (if any), then exits with `code`.
pub(crate) fn exits(stdout: &str, code: i32) -> FakeRun {
    let mut events = Vec::new();
    if !stdout.is_empty() {
        events.push(Ok(ExecEvent::Stdout(stdout.as_bytes().to_vec())));
    }
    events.push(Ok(ExecEvent::Exit { code }));
    FakeRun::Events(events)
}

/// The call fails before the command runs (TS: `exec.start` resolved `success: false`).
pub(crate) fn start_fails(message: &str) -> FakeRun {
    FakeRun::Events(vec![Err(ExecError::Api(unreachable_daemon(message)))])
}

struct ExecState {
    queue: VecDeque<FakeRun>,
    /// Played for every call once the queue is empty; without it such a call fails the test.
    always: Option<Vec<ExecItem>>,
    calls: Vec<ExecRequest>,
    /// Per call: dropped before its terminal item (TS: `exec.kill`).
    killed: Vec<Arc<AtomicBool>>,
}

pub(crate) struct FakeExec {
    state: Mutex<ExecState>,
}

impl FakeExec {
    pub(crate) fn new(runs: Vec<FakeRun>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(ExecState {
                queue: runs.into(),
                always: None,
                calls: Vec::new(),
                killed: Vec::new(),
            }),
        })
    }

    /// Every call prints `stdout` and exits 0.
    pub(crate) fn always(stdout: &str) -> Arc<Self> {
        let exec = Self::new(Vec::new());
        let FakeRun::Events(events) = exits(stdout, 0) else {
            unreachable!("exits returns events")
        };
        exec.state.lock().unwrap().always = Some(events);
        exec
    }

    /// The next unscripted call is driven through the returned sender.
    pub(crate) fn push_controlled(&self) -> mpsc::UnboundedSender<ExecItem> {
        let (tx, rx) = mpsc::unbounded();
        self.state
            .lock()
            .unwrap()
            .queue
            .push_back(FakeRun::Controlled(rx));
        tx
    }

    pub(crate) fn calls(&self) -> Vec<ExecRequest> {
        self.state.lock().unwrap().calls.clone()
    }

    pub(crate) fn call(&self, n: usize) -> ExecRequest {
        self.calls()
            .get(n - 1)
            .cloned()
            .unwrap_or_else(|| panic!("no exec call #{n}"))
    }

    /// Whether call `n` (1-based) was cancelled before it finished.
    pub(crate) fn killed(&self, n: usize) -> bool {
        self.state.lock().unwrap().killed[n - 1].load(Ordering::SeqCst)
    }
}

impl ExecApi for FakeExec {
    fn start(&self, request: ExecRequest) -> BoxStream<'static, ExecItem> {
        let mut state = self.state.lock().unwrap();
        let call_number = state.calls.len() + 1;
        let run = match (state.queue.pop_front(), &state.always) {
            (Some(run), _) => run,
            (None, Some(events)) => FakeRun::Events(events.clone()),
            (None, None) => panic!("unexpected exec #{call_number}: {}", request.display()),
        };
        state.calls.push(request);
        let killed = Arc::new(AtomicBool::new(false));
        state.killed.push(Arc::clone(&killed));
        let events = match run {
            FakeRun::Events(events) => futures::stream::iter(events).boxed(),
            FakeRun::Controlled(rx) => rx.boxed(),
        };
        RunStream {
            events,
            finished: false,
            killed,
        }
        .boxed()
    }
}

struct RunStream {
    events: BoxStream<'static, ExecItem>,
    finished: bool,
    killed: Arc<AtomicBool>,
}

impl Stream for RunStream {
    type Item = ExecItem;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<ExecItem>> {
        let item = self.events.poll_next_unpin(cx);
        if let Poll::Ready(Some(Ok(ExecEvent::Exit { .. }) | Err(_)) | None) = &item {
            self.finished = true;
        }
        item
    }
}

impl Drop for RunStream {
    fn drop(&mut self) {
        if !self.finished {
            self.killed.store(true, Ordering::SeqCst);
        }
    }
}

// === filesystem ============================================================================

/// What a read of one path gives.
#[derive(Debug, Clone)]
pub(crate) enum FakeRead {
    Content(String),
    TooLarge(u64),
    /// The daemon reports a failure (TS: `{ success: false, error }`).
    Fails(String),
    /// The call itself fails (TS: `readFile` rejects).
    Unreachable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WriteCall {
    pub workspace_path: String,
    pub file_path: String,
    pub content: String,
    pub guard: WriteGuard,
}

#[derive(Debug, Clone)]
pub(crate) enum FakeWrite {
    Written,
    Conflict,
    Fails(String),
}

#[derive(Default)]
struct FsState {
    /// Reads by file path; anything else is `No such file or directory`.
    files: HashMap<String, FakeRead>,
    /// Answers for the next reads, ahead of `files`.
    read_queue: VecDeque<FakeRead>,
    reads: Vec<(String, String, u64)>,
    write_queue: VecDeque<FakeWrite>,
    /// Successful writes update `files`, so later reads see them.
    writes: Vec<WriteCall>,
    directories: HashMap<String, Result<Vec<FileEntry>, String>>,
    directory_reads: Vec<String>,
    /// Directory reads that never answer (to cancel a walk mid-read).
    hanging_directories: Vec<String>,
}

#[derive(Default)]
pub(crate) struct FakeFs {
    state: Mutex<FsState>,
}

impl FakeFs {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub(crate) fn set(&self, file_path: &str, read: FakeRead) {
        self.state
            .lock()
            .unwrap()
            .files
            .insert(file_path.to_owned(), read);
    }

    /// Every read answers `read` until another is set (TS: `mockResolvedValue`).
    pub(crate) fn set_all(&self, read: FakeRead) {
        self.state.lock().unwrap().files.insert(String::new(), read);
    }

    pub(crate) fn queue_read(&self, read: FakeRead) {
        self.state.lock().unwrap().read_queue.push_back(read);
    }

    pub(crate) fn queue_write(&self, write: FakeWrite) {
        self.state.lock().unwrap().write_queue.push_back(write);
    }

    pub(crate) fn reads(&self) -> Vec<(String, String, u64)> {
        self.state.lock().unwrap().reads.clone()
    }

    pub(crate) fn writes(&self) -> Vec<WriteCall> {
        self.state.lock().unwrap().writes.clone()
    }

    pub(crate) fn set_directory(&self, path: &str, entries: Vec<FileEntry>) {
        self.state
            .lock()
            .unwrap()
            .directories
            .insert(path.to_owned(), Ok(entries));
    }

    pub(crate) fn fail_directory(&self, path: &str, error: &str) {
        self.state
            .lock()
            .unwrap()
            .directories
            .insert(path.to_owned(), Err(error.to_owned()));
    }

    pub(crate) fn hang_directory(&self, path: &str) {
        self.state
            .lock()
            .unwrap()
            .hanging_directories
            .push(path.to_owned());
    }

    pub(crate) fn directory_reads(&self) -> Vec<String> {
        self.state.lock().unwrap().directory_reads.clone()
    }
}

impl FilesystemApi for FakeFs {
    fn read_file(
        &self,
        workspace_path: &str,
        file_path: &str,
        max_bytes: u64,
    ) -> BoxFuture<'static, Result<ReadOutcome, FsError>> {
        let mut state = self.state.lock().unwrap();
        state
            .reads
            .push((workspace_path.to_owned(), file_path.to_owned(), max_bytes));
        let read = match state.read_queue.pop_front() {
            Some(read) => read,
            None => state
                .files
                .get(file_path)
                .or_else(|| state.files.get(""))
                .cloned()
                .unwrap_or_else(|| {
                    FakeRead::Fails("No such file or directory (os error 2)".to_owned())
                }),
        };
        let outcome = match read {
            FakeRead::Content(content) => Ok(ReadOutcome::Read(FileRead {
                header: FileReadHeader {
                    path: file_path.to_owned(),
                    size: i64::try_from(content.len()).unwrap(),
                    language: "plaintext".to_owned(),
                },
                content: content.into_bytes(),
            })),
            FakeRead::TooLarge(size) => Ok(ReadOutcome::TooLarge { size }),
            FakeRead::Fails(message) => Err(FsError::Daemon(message)),
            FakeRead::Unreachable(message) => Err(FsError::Api(unreachable_daemon(&message))),
        };
        async move {
            yield_once().await;
            outcome
        }
        .boxed()
    }
}

impl FileWriteApi for FakeFs {
    fn write_file(
        &self,
        workspace_path: &str,
        file_path: &str,
        content: Vec<u8>,
        guard: WriteGuard,
    ) -> BoxFuture<'static, Result<WriteOutcome, FsError>> {
        let mut state = self.state.lock().unwrap();
        let content = String::from_utf8(content).unwrap();
        state.writes.push(WriteCall {
            workspace_path: workspace_path.to_owned(),
            file_path: file_path.to_owned(),
            content: content.clone(),
            guard,
        });
        let outcome = match state.write_queue.pop_front().unwrap_or(FakeWrite::Written) {
            FakeWrite::Written => {
                let sha256 = crate::fs::sha256_hex(content.as_bytes());
                state
                    .files
                    .insert(file_path.to_owned(), FakeRead::Content(content));
                Ok(WriteOutcome::Written { sha256 })
            }
            FakeWrite::Conflict => Ok(WriteOutcome::Conflict),
            FakeWrite::Fails(message) => Err(FsError::Daemon(message)),
        };
        async move { outcome }.boxed()
    }
}

impl DirectoryApi for FakeFs {
    fn read_directory(
        &self,
        _workspace_path: &str,
        dir_path: &str,
    ) -> BoxFuture<'static, Result<DirectoryContents, FsError>> {
        let mut state = self.state.lock().unwrap();
        state.directory_reads.push(dir_path.to_owned());
        if state
            .hanging_directories
            .iter()
            .any(|path| path == dir_path)
        {
            return futures::future::pending().boxed();
        }
        let outcome = match state.directories.get(dir_path).cloned() {
            Some(Ok(entries)) => Ok(DirectoryContents {
                path: dir_path.to_owned(),
                entries,
            }),
            Some(Err(message)) => Err(FsError::Daemon(message)),
            None => Ok(DirectoryContents {
                path: dir_path.to_owned(),
                entries: Vec::new(),
            }),
        };
        async move { outcome }.boxed()
    }
}

// === http ==================================================================================

pub(crate) fn json_response(status: u16, body: &str) -> Result<HttpResponse, HttpError> {
    Ok(HttpResponse {
        status,
        status_text: String::new(),
        body: body.as_bytes().to_vec(),
    })
}

pub(crate) fn status_response(status: u16, status_text: &str) -> Result<HttpResponse, HttpError> {
    Ok(HttpResponse {
        status,
        status_text: status_text.to_owned(),
        body: Vec::new(),
    })
}

#[derive(Default)]
pub(crate) struct FakeHttp {
    responses: Mutex<VecDeque<Result<HttpResponse, HttpError>>>,
    requests: Mutex<Vec<HttpRequest>>,
}

impl FakeHttp {
    pub(crate) fn new(responses: Vec<Result<HttpResponse, HttpError>>) -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn requests(&self) -> Vec<HttpRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl HttpClient for FakeHttp {
    fn send(&self, request: HttpRequest) -> BoxFuture<'static, Result<HttpResponse, HttpError>> {
        let response = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| {
                panic!(
                    "unexpected request {} {}",
                    request.method.as_str(),
                    request.url
                )
            });
        self.requests.lock().unwrap().push(request);
        async move { response }.boxed()
    }
}
