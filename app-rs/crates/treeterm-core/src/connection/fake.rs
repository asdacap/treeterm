//! A scripted [`ProcessSpawner`] for tests (the TS tests' mocked `child_process.spawn`).
//!
//! Every spawn is recorded as a [`FakeProcess`] the test drives: print lines, exit, fail.
//! A responder decides how each spawn starts — kept running for the test to drive, exited
//! at once with scripted output, or refused.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use futures::channel::mpsc;
use tokio::sync::Notify;

use super::process::{
    ExitStatus, Process, ProcessCommand, ProcessEvent, ProcessSpawner, SpawnError,
};

/// How a spawn starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeResponse {
    /// Keeps running until the test drives it through its [`FakeProcess`].
    Run,
    /// Prints `stdout` then `stderr` (split into lines), then exits with `code`.
    Exit {
        stdout: String,
        stderr: String,
        code: i32,
    },
    /// `spawn` fails with this message.
    Refuse(String),
}

impl FakeResponse {
    pub fn exit(code: i32) -> Self {
        FakeResponse::Exit {
            stdout: String::new(),
            stderr: String::new(),
            code,
        }
    }

    pub fn stdout(stdout: impl Into<String>) -> Self {
        FakeResponse::Exit {
            stdout: stdout.into(),
            stderr: String::new(),
            code: 0,
        }
    }
}

type Responder = Box<dyn FnMut(&ProcessCommand) -> FakeResponse + Send>;

/// Records spawns; see the module docs.
#[derive(Clone)]
pub struct FakeSpawner {
    inner: Arc<Inner>,
}

struct Inner {
    state: Mutex<State>,
    spawned: Notify,
}

struct State {
    processes: Vec<FakeProcess>,
    refused: Vec<ProcessCommand>,
    responder: Responder,
}

impl Default for FakeSpawner {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeSpawner {
    /// Every spawn keeps running ([`FakeResponse::Run`]).
    pub fn new() -> Self {
        Self::with_responder(|_| FakeResponse::Run)
    }

    pub fn with_responder(
        responder: impl FnMut(&ProcessCommand) -> FakeResponse + Send + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    processes: Vec::new(),
                    refused: Vec::new(),
                    responder: Box::new(responder),
                }),
                spawned: Notify::new(),
            }),
        }
    }

    /// Replace the responder for later spawns.
    pub fn respond(&self, responder: impl FnMut(&ProcessCommand) -> FakeResponse + Send + 'static) {
        self.state().responder = Box::new(responder);
    }

    /// Every started process, in spawn order.
    pub fn processes(&self) -> Vec<FakeProcess> {
        self.state().processes.clone()
    }

    /// Commands whose spawn was refused.
    pub fn refused(&self) -> Vec<ProcessCommand> {
        self.state().refused.clone()
    }

    /// Commands of every spawn attempt that started a process, in order.
    pub fn commands(&self) -> Vec<ProcessCommand> {
        self.state()
            .processes
            .iter()
            .map(|process| process.command.clone())
            .collect()
    }

    /// The `index`th started process, waiting for it to be spawned.
    pub async fn process(&self, index: usize) -> FakeProcess {
        loop {
            let spawned = self.inner.spawned.notified();
            if let Some(process) = self.state().processes.get(index) {
                return process.clone();
            }
            spawned.await;
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner.state.lock().expect("fake spawner poisoned")
    }
}

impl ProcessSpawner for FakeSpawner {
    fn spawn(&self, command: &ProcessCommand) -> Result<Process, SpawnError> {
        let mut state = self.state();
        let response = (state.responder)(command);
        let (tx, rx) = mpsc::unbounded();
        let process = FakeProcess {
            command: command.clone(),
            events: tx,
            ended: Arc::new(AtomicBool::new(false)),
            killed: Arc::new(AtomicBool::new(false)),
        };
        match response {
            FakeResponse::Refuse(message) => {
                state.refused.push(command.clone());
                return Err(SpawnError {
                    program: command.program.clone(),
                    message,
                });
            }
            FakeResponse::Run => {}
            FakeResponse::Exit {
                stdout,
                stderr,
                code,
            } => {
                process.stdout(&stdout);
                process.stderr(&stderr);
                process.exit(code);
            }
        }
        state.processes.push(process.clone());
        drop(state);
        self.inner.spawned.notify_waiters();
        let guard = KillGuard(process);
        Ok(Process::from_events(rx.map(move |event| {
            let _owner = &guard;
            event
        })))
    }
}

/// Marks the fake killed when its [`Process`] is dropped before it ended.
struct KillGuard(FakeProcess);

impl Drop for KillGuard {
    fn drop(&mut self) {
        if !self.0.ended.load(Ordering::SeqCst) {
            self.0.killed.store(true, Ordering::SeqCst);
        }
    }
}

/// The test's side of a spawned fake.
#[derive(Clone)]
pub struct FakeProcess {
    command: ProcessCommand,
    events: mpsc::UnboundedSender<ProcessEvent>,
    ended: Arc<AtomicBool>,
    killed: Arc<AtomicBool>,
}

impl FakeProcess {
    pub fn command(&self) -> &ProcessCommand {
        &self.command
    }

    pub fn args(&self) -> &[String] {
        &self.command.args
    }

    /// Print `text` on stdout; each non-empty line becomes one event (like Electron's
    /// `data.toString().split('\n').filter(Boolean)`).
    pub fn stdout(&self, text: &str) {
        for line in text.split('\n').filter(|line| !line.is_empty()) {
            self.send(ProcessEvent::Stdout(line.to_owned()));
        }
    }

    pub fn stderr(&self, text: &str) {
        for line in text.split('\n').filter(|line| !line.is_empty()) {
            self.send(ProcessEvent::Stderr(line.to_owned()));
        }
    }

    pub fn exit(&self, code: i32) {
        self.end(ProcessEvent::Exited(ExitStatus::Code(code)));
    }

    pub fn kill_by_signal(&self, signal: i32) {
        self.end(ProcessEvent::Exited(ExitStatus::Signal(signal)));
    }

    /// Node's `error` event on a running child.
    pub fn fail(&self, message: &str) {
        self.end(ProcessEvent::Failed(message.to_owned()));
    }

    /// Its owner dropped it while it was still running.
    pub fn is_killed(&self) -> bool {
        self.killed.load(Ordering::SeqCst)
    }

    /// It exited or failed (by the test's hand) before being dropped.
    pub fn has_ended(&self) -> bool {
        self.ended.load(Ordering::SeqCst)
    }

    fn end(&self, event: ProcessEvent) {
        self.send(event);
        self.ended.store(true, Ordering::SeqCst);
        self.events.close_channel();
    }

    fn send(&self, event: ProcessEvent) {
        if self.events.unbounded_send(event).is_err() {
            // The owner already dropped (killed) it; a dead process prints nothing.
            tracing::trace!("fake process event after kill dropped");
        }
    }
}
