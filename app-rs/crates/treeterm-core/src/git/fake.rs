//! Scripted stand-ins for the daemon (the TS tests' `createMockExec` + `autoComplete` and
//! `createMockFilesystem`).
//!
//! Every exec completes on its second poll, like the TS mock completing on a macrotask, so
//! commands started together are all in flight before any finishes.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures::Stream;
use futures::future::{BoxFuture, FutureExt};
use futures::stream::{BoxStream, StreamExt};
use treeterm_proto::treeterm::FileReadHeader;

use crate::daemon::ApiError;
use crate::exec::{ExecApi, ExecError, ExecEvent, ExecRequest};
use crate::fs::{FileRead, FilesystemApi, FsError, ReadOutcome};

/// How one exec call ends.
#[derive(Debug, Clone)]
pub(super) enum Completion {
    /// Prints `stdout` (if any), then exits.
    Stdout { stdout: String, exit_code: i32 },
    /// Prints `stderr` (if any), then exits.
    Stderr { stderr: String, exit_code: i32 },
    /// The call fails before the command runs (TS: `exec.start` rejected or returned
    /// `success: false`).
    Reject(String),
    /// The daemon reports an error instead of a result (TS: an `Error` exec event).
    ErrorEvent(String),
}

pub(super) fn out(stdout: impl Into<String>) -> Completion {
    out_code(stdout, 0)
}

pub(super) fn out_code(stdout: impl Into<String>, exit_code: i32) -> Completion {
    Completion::Stdout {
        stdout: stdout.into(),
        exit_code,
    }
}

pub(super) fn err(stderr: impl Into<String>, exit_code: i32) -> Completion {
    Completion::Stderr {
        stderr: stderr.into(),
        exit_code,
    }
}

pub(super) fn reject(message: &str) -> Completion {
    Completion::Reject(message.to_owned())
}

impl Completion {
    fn events(self) -> VecDeque<Result<ExecEvent, ExecError>> {
        let mut events = VecDeque::new();
        match self {
            Completion::Stdout { stdout, exit_code } => {
                if !stdout.is_empty() {
                    events.push_back(Ok(ExecEvent::Stdout(stdout.into_bytes())));
                }
                events.push_back(Ok(ExecEvent::Exit { code: exit_code }));
            }
            Completion::Stderr { stderr, exit_code } => {
                if !stderr.is_empty() {
                    events.push_back(Ok(ExecEvent::Stderr(stderr.into_bytes())));
                }
                events.push_back(Ok(ExecEvent::Exit { code: exit_code }));
            }
            Completion::Reject(message) => events.push_back(Err(ExecError::Api(ApiError::from(
                tonic::Status::unavailable(message),
            )))),
            Completion::ErrorEvent(message) => events.push_back(Err(ExecError::System(message))),
        }
        events
    }
}

enum Script {
    /// The Nth call gets the Nth completion; a call past the end fails the test.
    InOrder(VecDeque<Completion>),
    Always(Completion),
}

struct State {
    script: Script,
    calls: Vec<ExecRequest>,
    active: usize,
    peak: usize,
}

pub(super) struct FakeExec {
    state: Arc<Mutex<State>>,
}

impl FakeExec {
    pub(super) fn new(completions: Vec<Completion>) -> Arc<Self> {
        Self::with(Script::InOrder(completions.into()))
    }

    pub(super) fn always(completion: Completion) -> Arc<Self> {
        Self::with(Script::Always(completion))
    }

    fn with(script: Script) -> Arc<Self> {
        Arc::new(Self {
            state: Arc::new(Mutex::new(State {
                script,
                calls: Vec::new(),
                active: 0,
                peak: 0,
            })),
        })
    }

    pub(super) fn calls(&self) -> Vec<ExecRequest> {
        self.state.lock().unwrap().calls.clone()
    }

    /// The most commands that were running at once.
    pub(super) fn peak(&self) -> usize {
        self.state.lock().unwrap().peak
    }
}

impl ExecApi for FakeExec {
    fn start(&self, request: ExecRequest) -> BoxStream<'static, Result<ExecEvent, ExecError>> {
        let mut state = self.state.lock().unwrap();
        let call_number = state.calls.len() + 1;
        let completion = match &mut state.script {
            Script::InOrder(completions) => completions
                .pop_front()
                .unwrap_or_else(|| panic!("unexpected exec #{call_number}: {}", request.display())),
            Script::Always(completion) => completion.clone(),
        };
        state.calls.push(request);
        state.active += 1;
        state.peak = state.peak.max(state.active);
        FakeRun {
            state: Arc::clone(&self.state),
            events: completion.events(),
            started: false,
            running: true,
        }
        .boxed()
    }
}

struct FakeRun {
    state: Arc<Mutex<State>>,
    events: VecDeque<Result<ExecEvent, ExecError>>,
    started: bool,
    running: bool,
}

impl FakeRun {
    fn finish(&mut self) {
        if self.running {
            self.running = false;
            self.state.lock().unwrap().active -= 1;
        }
    }
}

impl Stream for FakeRun {
    type Item = Result<ExecEvent, ExecError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if !this.started {
            this.started = true;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        let event = this.events.pop_front();
        if this.events.is_empty() {
            this.finish();
        }
        Poll::Ready(event)
    }
}

impl Drop for FakeRun {
    fn drop(&mut self) {
        self.finish();
    }
}

/// What reading any working-tree file gives.
#[derive(Debug, Clone)]
pub(super) enum FakeFile {
    Content(Vec<u8>),
    TooLarge(u64),
    /// The daemon reports a failure (TS: `{ success: false, error }`).
    Fails(String),
    /// The call itself fails (TS: `readFile` rejects).
    Unreachable(String),
}

pub(super) struct FakeFs {
    file: FakeFile,
}

impl FakeFs {
    pub(super) fn new(file: FakeFile) -> Arc<Self> {
        Arc::new(Self { file })
    }

    /// The TS mock's default: every read fails with `not found`.
    pub(super) fn missing() -> Arc<Self> {
        Self::new(FakeFile::Fails("not found".to_owned()))
    }

    pub(super) fn with_content(content: &[u8]) -> Arc<Self> {
        Self::new(FakeFile::Content(content.to_vec()))
    }
}

impl FilesystemApi for FakeFs {
    fn read_file(
        &self,
        workspace_path: &str,
        file_path: &str,
        _max_bytes: u64,
    ) -> BoxFuture<'static, Result<ReadOutcome, FsError>> {
        let outcome = match &self.file {
            FakeFile::Content(content) => Ok(ReadOutcome::Read(FileRead {
                header: FileReadHeader {
                    path: format!("{workspace_path}/{file_path}"),
                    size: i64::try_from(content.len()).unwrap(),
                    language: "plaintext".to_owned(),
                },
                content: content.clone(),
            })),
            FakeFile::TooLarge(size) => Ok(ReadOutcome::TooLarge { size: *size }),
            FakeFile::Fails(message) => Err(FsError::Daemon(message.clone())),
            FakeFile::Unreachable(message) => Err(FsError::Api(ApiError::from(
                tonic::Status::unavailable(message.clone()),
            ))),
        };
        async move { outcome }.boxed()
    }
}
