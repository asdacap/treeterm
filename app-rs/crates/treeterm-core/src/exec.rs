//! One-shot commands on the daemon (`ExecStream`).
//!
//! An exec is one bidirectional call. The first message starts the command; later ones feed
//! its stdin or signal it. The daemon streams stdout and stderr, then ends with the result.
//! Closing the input side is the command's stdin EOF. Dropping the event stream cancels the
//! call, and the daemon kills the command.
//!
//! When the connection manager gives the connection up (it goes to reconnecting), every
//! command still running on it ends with [`ExecError::ConnectionLost`] and is cancelled, and
//! commands started on it afterwards fail at once (Electron's `failAllForConnection`).
//!
//! [`ExecClient::run`] is the common case: no stdin, collect everything, like Electron's
//! `exec:start` (which writes the start message and ends its side of the stream at once).

use std::collections::HashMap;
use std::time::Duration;

use futures::channel::{mpsc, oneshot};
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use futures::{FutureExt, Stream, StreamExt, TryFutureExt};
use tokio::sync::watch;
use treeterm_proto::treeterm::{ExecInput, ExecSignal, ExecStart, exec_input, exec_output};

use crate::connection::ConnectionId;
use crate::daemon::ApiError;
use crate::rt::{Rt, Sub};
use crate::transport::DaemonClient;

/// The daemon SIGTERMs a command after this long unless the request says otherwise.
/// Callers pass it explicitly; long-running commands pass a larger value.
pub const DEFAULT_EXEC_TIMEOUT: Duration = Duration::from_secs(30);

/// How long past its timeout a command may go without a result before the client gives up
/// on the daemon. The daemon SIGKILLs 5s after its SIGTERM, so a healthy daemon always
/// reports first; this only fires when the result is lost.
pub const EXEC_BACKSTOP_MARGIN: Duration = Duration::from_secs(10);

/// Largest stdin write sent in one message.
pub const MAX_STDIN_CHUNK: usize = 64 * 1024;

/// When the daemon stops the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecTimeout {
    /// SIGTERM after this long (SIGKILL 5s later). Clamped to 1ms ..= `i32::MAX` ms, the
    /// range the wire carries (0 would mean "never").
    After(Duration),
    /// Run until the command exits or the call is cancelled.
    Never,
}

impl ExecTimeout {
    fn wire_millis(self) -> i32 {
        match self {
            ExecTimeout::After(duration) => {
                let millis = duration.as_millis().clamp(1, i32::MAX as u128);
                i32::try_from(millis).expect("clamped to the i32 range above")
            }
            ExecTimeout::Never => 0,
        }
    }
}

/// What to run. `env` adds to (and overrides) the daemon's own environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecRequest {
    pub cwd: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub timeout: ExecTimeout,
}

impl ExecRequest {
    /// The command line as shown to people: `command arg1 arg2`.
    pub fn display(&self) -> String {
        std::iter::once(self.command.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// One message of a command's output, in stream order. `Exit` is always the last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    /// The command exited. `code` is -1 when it was killed by a signal (including the
    /// timeout's SIGTERM): the daemon reports no signal number.
    Exit {
        code: i32,
    },
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ExecError {
    #[error(transparent)]
    Api(#[from] ApiError),
    /// The daemon could not run the command at all (spawn failure, missing cwd, ...). This
    /// is not an exit code of the command.
    #[error("{0}")]
    System(String),
    /// The daemon closed the stream without a result (e.g. it restarted).
    #[error("exec stream ended without result")]
    EndedWithoutResult,
    /// The timeout plus [`EXEC_BACKSTOP_MARGIN`] passed without a result.
    #[error("{command} timed out after {}ms", .after.as_millis())]
    NoResult { command: String, after: Duration },
    /// The connection was lost (it went to reconnecting) while the command ran, or before it
    /// started. The call is cancelled, so the daemon kills the command.
    #[error("connection {0} lost while command was running")]
    ConnectionLost(ConnectionId),
}

/// Whether the connection an [`ExecClient`] runs on is lost: `Some` once the connection
/// manager gave it up. Set by [`crate::daemon::DaemonConnection::fail_running_execs`].
pub(crate) type ConnectionLoss = watch::Receiver<Option<ConnectionId>>;

/// Everything a finished command printed, and how it exited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// The input side of a running command has shut down: the command finished, the call was
/// cancelled, or stdin was closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("exec input closed")]
pub struct ExecClosed;

/// Feeds a running command. Not `Clone`: [`Self::close_stdin`] is final.
pub struct ExecInputHandle {
    queue: mpsc::UnboundedSender<ExecInput>,
}

impl ExecInputHandle {
    /// Queue `data` for the command's stdin. Writes are delivered in call order.
    pub fn write_stdin(&self, data: &[u8]) -> Result<(), ExecClosed> {
        for chunk in data.chunks(MAX_STDIN_CHUNK) {
            self.send(exec_input::Input::Stdin(chunk.to_vec()))?;
        }
        Ok(())
    }

    /// Send `signal` (e.g. `libc::SIGTERM`) to the command.
    pub fn signal(&self, signal: i32) -> Result<(), ExecClosed> {
        self.send(exec_input::Input::Signal(ExecSignal { signal }))
    }

    /// End the input side: the command reads EOF on stdin. Signals travel on the same side,
    /// so none can be sent afterwards; to stop the command then, drop its events.
    pub fn close_stdin(self) {
        self.queue.close_channel();
    }

    fn send(&self, input: exec_input::Input) -> Result<(), ExecClosed> {
        self.queue
            .unbounded_send(ExecInput { input: Some(input) })
            .map_err(|_| ExecClosed)
    }
}

/// A running command.
#[must_use = "dropping the events cancels the command"]
pub struct ExecProcess {
    pub input: ExecInputHandle,
    /// Stdout and stderr in order, then exactly one terminal item: `Ok(Exit)` or an error.
    /// Dropping it cancels the call and the daemon kills the command.
    pub events: Sub<Result<ExecEvent, ExecError>>,
}

/// `ExecStream` calls on one daemon connection. Cheap to clone.
#[derive(Clone)]
pub struct ExecClient {
    rt: Rt,
    client: DaemonClient,
    loss: ConnectionLoss,
}

impl ExecClient {
    /// A client on `client` alone, whose connection is never reported lost.
    pub fn new(rt: Rt, client: DaemonClient) -> Self {
        let (_never_lost, loss) = watch::channel(None);
        Self::on_connection(rt, client, loss)
    }

    /// A client whose commands fail with [`ExecError::ConnectionLost`] once `loss` says so.
    pub(crate) fn on_connection(rt: Rt, client: DaemonClient, loss: ConnectionLoss) -> Self {
        Self { rt, client, loss }
    }

    /// Start `request` with stdin open.
    pub fn exec(&self, request: ExecRequest) -> ExecProcess {
        let (queue, requests) = mpsc::unbounded::<ExecInput>();
        let lost = self.loss.borrow().clone();
        if let Some(connection) = lost {
            // Nothing is sent: the input side is closed and the only event is the loss.
            drop(requests);
            let failed =
                futures::stream::once(async move { Err(ExecError::ConnectionLost(connection)) });
            return ExecProcess {
                input: ExecInputHandle { queue },
                events: self.rt.subscribe(failed),
            };
        }
        let backstop = backstop(&request);
        let start = ExecInput {
            input: Some(exec_input::Input::Start(ExecStart {
                cwd: request.cwd,
                command: request.command,
                args: request.args,
                env: request.env,
                timeout_ms: Some(request.timeout.wire_millis()),
            })),
        };
        queue
            .unbounded_send(start)
            .expect("receiver is alive: it was created above");

        // h2 resets (cancels) the call only once both of its sides are gone: the request side
        // ends with the events, even while the input handle is still held.
        let (input_guard, events_gone) = oneshot::channel::<()>();
        let requests = requests.take_until(events_gone);
        let mut client = self.client.clone();
        let outputs = async move { client.exec_stream(requests).await }
            .map_ok(tonic::Response::into_inner)
            .try_flatten_stream()
            .map(|item| item.map_err(ApiError::from))
            .boxed();
        let call = Call {
            outputs,
            _input_guard: input_guard,
        };
        ExecProcess {
            input: ExecInputHandle { queue },
            events: self
                .rt
                .subscribe(exec_events(call, backstop, lost_signal(self.loss.clone()))),
        }
    }

    /// Run `request` with stdin closed and collect its output. Dropping the future cancels
    /// the command.
    pub fn run(
        &self,
        request: ExecRequest,
    ) -> impl Future<Output = Result<CommandOutput, ExecError>> + Send + 'static + use<> {
        let ExecProcess { input, events } = self.exec(request);
        input.close_stdin();
        collect_output(events)
    }
}

/// The capability to start commands, as the git client needs it. Implemented by
/// [`ExecClient`] and by test fakes.
pub trait ExecApi: Send + Sync {
    /// Start `request` with stdin closed. The stream carries the output, then exactly one
    /// terminal item; dropping it cancels the command.
    fn start(&self, request: ExecRequest) -> BoxStream<'static, Result<ExecEvent, ExecError>>;
}

impl ExecApi for ExecClient {
    fn start(&self, request: ExecRequest) -> BoxStream<'static, Result<ExecEvent, ExecError>> {
        let ExecProcess { input, events } = self.exec(request);
        input.close_stdin();
        events.boxed()
    }
}

/// Drain `events` into a [`CommandOutput`].
pub async fn collect_output(
    events: impl Stream<Item = Result<ExecEvent, ExecError>>,
) -> Result<CommandOutput, ExecError> {
    let mut events = std::pin::pin!(events);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    while let Some(event) = events.next().await {
        match event? {
            ExecEvent::Stdout(data) => stdout.extend_from_slice(&data),
            ExecEvent::Stderr(data) => stderr.extend_from_slice(&data),
            ExecEvent::Exit { code } => {
                return Ok(CommandOutput {
                    exit_code: code,
                    stdout,
                    stderr,
                });
            }
        }
    }
    Err(ExecError::EndedWithoutResult)
}

/// Resolves to the error to report once the result is overdue. Built lazily: tokio timers
/// can only be created on the runtime, which is where the stream is polled.
fn backstop(request: &ExecRequest) -> BoxFuture<'static, ExecError> {
    match request.timeout {
        ExecTimeout::After(timeout) => {
            let after = timeout + EXEC_BACKSTOP_MARGIN;
            let command = request.display();
            async move {
                tokio::time::sleep(after).await;
                ExecError::NoResult { command, after }
            }
            .boxed()
        }
        ExecTimeout::Never => futures::future::pending().boxed(),
    }
}

/// Resolves with the connection once it is lost. Never resolves while it is not, or once
/// nothing can report it lost any more (every [`crate::daemon::DaemonConnection`] of it is gone).
fn lost_signal(mut loss: ConnectionLoss) -> BoxFuture<'static, ConnectionId> {
    async move {
        let lost = match loss.wait_for(Option::is_some).await {
            Ok(lost) => lost.clone(),
            Err(_closed) => None,
        };
        match lost {
            Some(connection) => connection,
            None => futures::future::pending().await,
        }
    }
    .boxed()
}

/// One `ExecStream` call. Dropping it cancels the call, and the daemon kills the command.
struct Call {
    outputs: BoxStream<'static, Result<treeterm_proto::treeterm::ExecOutput, ApiError>>,
    /// Dropped with the call: ends its request side.
    _input_guard: oneshot::Sender<()>,
}

/// The daemon's outputs as events: data passes through; the result, a call error, the
/// stream ending early, the backstop firing or the connection being lost is the last item.
fn exec_events(
    call: Call,
    backstop: BoxFuture<'static, ExecError>,
    lost: BoxFuture<'static, ConnectionId>,
) -> impl Stream<Item = Result<ExecEvent, ExecError>> + Send + 'static {
    struct Watch {
        call: Call,
        backstop: BoxFuture<'static, ExecError>,
        lost: BoxFuture<'static, ConnectionId>,
    }

    enum Step {
        Data(ExecEvent),
        Last(Result<ExecEvent, ExecError>),
    }

    let watch = Watch {
        call,
        backstop,
        lost,
    };
    futures::stream::unfold(Some(watch), |state| async move {
        let mut watch = state?;
        let step = tokio::select! {
            // The loss goes first: it beats whatever the dying transport reports.
            biased;
            connection = &mut watch.lost => Step::Last(Err(ExecError::ConnectionLost(connection))),
            output = watch.call.outputs.next() => match output {
                Some(Ok(output)) => match output.output {
                    Some(exec_output::Output::Stdout(out)) => Step::Data(ExecEvent::Stdout(out.data)),
                    Some(exec_output::Output::Stderr(err)) => Step::Data(ExecEvent::Stderr(err.data)),
                    Some(exec_output::Output::Result(result)) => Step::Last(match result.error {
                        Some(error) => Err(ExecError::System(error)),
                        None => Ok(ExecEvent::Exit {
                            code: result.exit_code,
                        }),
                    }),
                    None => Step::Last(Err(
                        ApiError::Protocol("ExecOutput without a payload").into()
                    )),
                },
                Some(Err(error)) => Step::Last(Err(error.into())),
                None => Step::Last(Err(ExecError::EndedWithoutResult)),
            },
            error = &mut watch.backstop => Step::Last(Err(error)),
        };
        match step {
            Step::Data(event) => Some((Ok(event), Some(watch))),
            // Dropping the watch cancels the call: nothing more is wanted from it, and the
            // daemon kills the command if it still runs.
            Step::Last(item) => Some((item, None)),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use treeterm_proto::treeterm::{ExecOutput, ExecResult, ExecStderr, ExecStdout};

    fn output(output: exec_output::Output) -> Result<ExecOutput, ApiError> {
        Ok(ExecOutput {
            output: Some(output),
        })
    }

    fn call(outputs: BoxStream<'static, Result<ExecOutput, ApiError>>) -> Call {
        Call {
            outputs,
            _input_guard: oneshot::channel().0,
        }
    }

    fn events_of(outputs: Vec<Result<ExecOutput, ApiError>>) -> Vec<Result<ExecEvent, ExecError>> {
        // A connection nobody can report lost any more: the signal never fires.
        let (_gone, loss) = watch::channel(None);
        let stream = exec_events(
            call(futures::stream::iter(outputs).boxed()),
            futures::future::pending().boxed(),
            lost_signal(loss),
        );
        futures::executor::block_on(stream.collect())
    }

    #[test]
    fn data_passes_through_and_the_result_ends_the_stream() {
        let events = events_of(vec![
            output(exec_output::Output::Stdout(ExecStdout {
                data: b"out".to_vec(),
            })),
            output(exec_output::Output::Stderr(ExecStderr {
                data: b"err".to_vec(),
            })),
            output(exec_output::Output::Result(ExecResult {
                exit_code: 3,
                error: None,
            })),
            output(exec_output::Output::Stdout(ExecStdout {
                data: b"after the end".to_vec(),
            })),
        ]);
        let events: Vec<ExecEvent> = events.into_iter().map(Result::unwrap).collect();
        assert_eq!(
            events,
            vec![
                ExecEvent::Stdout(b"out".to_vec()),
                ExecEvent::Stderr(b"err".to_vec()),
                ExecEvent::Exit { code: 3 },
            ]
        );
    }

    #[test]
    fn a_system_error_is_an_error_not_an_exit() {
        let events = events_of(vec![output(exec_output::Output::Result(ExecResult {
            exit_code: 1,
            error: Some("spawn error: nope".to_owned()),
        }))]);
        assert!(
            matches!(&events[..], [Err(ExecError::System(message))] if message == "spawn error: nope")
        );
    }

    #[test]
    fn a_stream_without_a_result_fails_loudly() {
        let events = events_of(vec![output(exec_output::Output::Stdout(ExecStdout {
            data: b"partial".to_vec(),
        }))]);
        assert!(matches!(events[0], Ok(ExecEvent::Stdout(_))));
        assert!(matches!(events[1], Err(ExecError::EndedWithoutResult)));
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn call_errors_and_empty_outputs_end_the_stream() {
        let events = events_of(vec![Ok(ExecOutput { output: None })]);
        assert!(matches!(
            &events[..],
            [Err(ExecError::Api(ApiError::Protocol(_)))]
        ));

        let events = events_of(vec![Err(ApiError::from(tonic::Status::unavailable(
            "gone",
        )))]);
        assert!(matches!(
            &events[..],
            [Err(ExecError::Api(ApiError::Status(_)))]
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn the_backstop_fires_when_the_result_never_comes() {
        let request = ExecRequest {
            cwd: "/".to_owned(),
            command: "git".to_owned(),
            args: vec!["fetch".to_owned()],
            env: HashMap::new(),
            timeout: ExecTimeout::After(Duration::from_secs(1)),
        };
        let stream = exec_events(
            call(futures::stream::pending().boxed()),
            backstop(&request),
            futures::future::pending().boxed(),
        );
        let events: Vec<_> = stream.collect().await;
        match &events[..] {
            [Err(error @ ExecError::NoResult { after, .. })] => {
                assert_eq!(*after, Duration::from_secs(1) + EXEC_BACKSTOP_MARGIN);
                assert_eq!(error.to_string(), "git fetch timed out after 11000ms");
            }
            other => panic!("expected the backstop error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_lost_connection_ends_the_stream_and_cancels_the_call() {
        let (outputs, rx) = mpsc::unbounded();
        let (input_guard, mut events_gone) = oneshot::channel();
        let (loss, lost) = watch::channel(None);
        let mut events = std::pin::pin!(exec_events(
            Call {
                outputs: rx.boxed(),
                _input_guard: input_guard,
            },
            futures::future::pending().boxed(),
            lost_signal(lost),
        ));
        outputs
            .unbounded_send(output(exec_output::Output::Stdout(ExecStdout {
                data: b"partial".to_vec(),
            })))
            .unwrap();
        assert_eq!(
            events.next().await.unwrap().unwrap(),
            ExecEvent::Stdout(b"partial".to_vec())
        );

        loss.send_replace(Some(ConnectionId::new("local-1")));
        match events.next().await {
            Some(Err(error @ ExecError::ConnectionLost(_))) => assert_eq!(
                error.to_string(),
                "connection local-1 lost while command was running"
            ),
            other => panic!("expected the loss, got {other:?}"),
        }
        assert!(events.next().await.is_none());
        // Both sides of the call are gone, so h2 resets it and the daemon kills the command.
        assert!(outputs.is_closed());
        assert_eq!(events_gone.try_recv(), Err(oneshot::Canceled));
    }

    #[tokio::test]
    async fn the_loss_beats_what_the_dying_transport_reports() {
        let (_loss, lost) = watch::channel(Some(ConnectionId::new("remote-1")));
        let events: Vec<_> = exec_events(
            call(
                futures::stream::iter(vec![Err(ApiError::from(tonic::Status::unavailable(
                    "gone",
                )))])
                .boxed(),
            ),
            futures::future::pending().boxed(),
            lost_signal(lost),
        )
        .collect()
        .await;
        assert!(
            matches!(&events[..], [Err(ExecError::ConnectionLost(id))] if id.as_str() == "remote-1"),
            "{events:?}"
        );
    }

    #[test]
    fn timeouts_map_to_the_wire_range() {
        assert_eq!(ExecTimeout::Never.wire_millis(), 0);
        assert_eq!(
            ExecTimeout::After(Duration::from_secs(30)).wire_millis(),
            30_000
        );
        assert_eq!(ExecTimeout::After(Duration::ZERO).wire_millis(), 1);
        assert_eq!(
            ExecTimeout::After(Duration::from_secs(u64::MAX)).wire_millis(),
            i32::MAX
        );
    }

    #[test]
    fn collect_output_gathers_both_streams_until_the_exit() {
        let events = futures::stream::iter(vec![
            Ok(ExecEvent::Stdout(b"a".to_vec())),
            Ok(ExecEvent::Stderr(b"x".to_vec())),
            Ok(ExecEvent::Stdout(b"b".to_vec())),
            Ok(ExecEvent::Exit { code: 0 }),
        ]);
        let output = futures::executor::block_on(collect_output(events)).unwrap();
        assert_eq!(
            output,
            CommandOutput {
                exit_code: 0,
                stdout: b"ab".to_vec(),
                stderr: b"x".to_vec(),
            }
        );

        let truncated = futures::stream::iter(vec![Ok(ExecEvent::Stdout(b"a".to_vec()))]);
        assert!(matches!(
            futures::executor::block_on(collect_output(truncated)),
            Err(ExecError::EndedWithoutResult)
        ));
    }

    #[test]
    fn stdin_is_chunked_and_closing_ends_the_input() {
        let (queue, mut rx) = mpsc::unbounded();
        let handle = ExecInputHandle { queue };
        handle
            .write_stdin(&vec![b'x'; MAX_STDIN_CHUNK + 1])
            .unwrap();
        handle.signal(15).unwrap();
        let mut sizes = Vec::new();
        while let Ok(message) = rx.try_recv() {
            match message.input {
                Some(exec_input::Input::Stdin(data)) => sizes.push(data.len()),
                Some(exec_input::Input::Signal(signal)) => assert_eq!(signal.signal, 15),
                other => panic!("unexpected input {other:?}"),
            }
            if sizes.len() == 2 {
                break;
            }
        }
        assert_eq!(sizes, vec![MAX_STDIN_CHUNK, 1]);
        handle.close_stdin();
        assert!(rx.try_recv().is_ok(), "the signal is still queued");
        assert!(
            matches!(rx.try_recv(), Err(mpsc::TryRecvError::Closed)),
            "closed after the queued input"
        );
    }
}
