//! Attachments to daemon PTYs.
//!
//! An attachment is one `PtyStream` call: the daemon replays the PTY's current state, then
//! streams its live output, and takes writes, resizes and focus claims back. Dropping the
//! event stream detaches; it never kills the PTY ("unmount is not close", AGENTS.md).
//! Killing is the separate [`crate::daemon::DaemonConnection::kill_pty`].

use futures::channel::{mpsc, oneshot};
use futures::future::{FutureExt, Shared};
use futures::{Stream, StreamExt, TryFutureExt};
use serde_json::Value;
use tokio_stream::wrappers::ReceiverStream;
use treeterm_proto::treeterm::{
    Empty, PtyInput, PtyOutput, PtyResizeData, PtyStartData, PtyWriteData, pty_input,
    pty_output,
};

use crate::daemon::ApiError;
use crate::rt::{Rt, Sub};
use crate::transport::DaemonClient;

/// Largest write sent in one message. Pastes are split so one huge paste neither hits the
/// message limit nor holds the stream's flow-control window in one piece.
pub const MAX_WRITE_CHUNK: usize = 64 * 1024;

/// Messages the client may have handed to tonic before a write waits for the daemon.
const REQUEST_BUFFER: usize = 64;

/// What an attachment does with the PTY. Mirrors `PtyAttachKind` in the proto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachKind {
    /// Writes programmatically or only reads. The daemon never gates its writes.
    Background,
    /// Renders the output and answers the program's queries. Of these attachments only the
    /// most recently focused one may write (see [`PtyInputHandle::focus`]).
    Terminal,
}

impl AttachKind {
    fn proto(self) -> treeterm_proto::treeterm::PtyAttachKind {
        use treeterm_proto::treeterm::PtyAttachKind as P;
        match self {
            AttachKind::Background => P::Background,
            AttachKind::Terminal => P::Terminal,
        }
    }
}

/// One message of a PTY's output, in stream order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PtyEvent {
    Data(Vec<u8>),
    /// The size the daemon applied. The daemon is the size authority: a terminal resizes
    /// only when this arrives, never on its own request.
    Resize { cols: u16, rows: u16 },
    /// The initial state (snapshot + raw buffer) follows. It holds the program's original
    /// output verbatim, including queries that were answered when they first ran.
    ReplayStart,
    ReplayEnd,
    Exit { code: i32, signal: Option<i32> },
}

/// The event `output` carries; `None` for an output kind this client does not know (one a
/// newer daemon added, which prost decodes as no payload). Electron's `PtyStream` skips
/// those too.
fn event(output: PtyOutput) -> Result<Option<PtyEvent>, ApiError> {
    use pty_output::Output;
    let Some(output) = output.output else {
        return Ok(None);
    };
    let event = match output {
        Output::Data(data) => PtyEvent::Data(data.data),
        Output::Resize(size) => PtyEvent::Resize {
            cols: dimension(size.cols)?,
            rows: dimension(size.rows)?,
        },
        Output::ReplayStart(Empty {}) => PtyEvent::ReplayStart,
        Output::ReplayEnd(Empty {}) => PtyEvent::ReplayEnd,
        Output::Exit(exit) => PtyEvent::Exit {
            code: exit.exit_code,
            signal: exit.signal,
        },
    };
    Ok(Some(event))
}

fn dimension(value: i32) -> Result<u16, ApiError> {
    u16::try_from(value)
        .ok()
        .filter(|v| *v > 0)
        .ok_or(ApiError::Protocol("PTY size out of range"))
}

/// The attachment's input side: ordered writes, resizes and focus claims.
///
/// Cloneable and `Send`, so a terminal's parser thread can answer queries through the
/// same ordered queue the keyboard uses.
#[derive(Clone)]
pub struct PtyInputHandle {
    queue: mpsc::UnboundedSender<Queued>,
    /// Why the attachment ended, once it has (sent by [`PumpGuarded`]).
    closed: Shared<oneshot::Receiver<String>>,
}

struct Queued {
    message: PtyInput,
    /// Fired once the message is handed to tonic.
    sent: Option<oneshot::Sender<()>>,
}

/// The attachment's input has shut down. Carries why, in the words of Electron's
/// `PtyStream`: the stream's own error, [`STREAM_ENDED`] or [`STREAM_DETACHED`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PtyClosed(pub String);

/// The daemon closed the stream.
pub const STREAM_ENDED: &str = "pty stream ended";
/// The attachment was dropped on this side.
pub const STREAM_DETACHED: &str = "pty stream detached";
/// The input is gone and no reason was recorded.
const STREAM_CLOSED: &str = "pty stream closed";

impl PtyClosed {
    fn from_reason(reason: Result<String, oneshot::Canceled>) -> Self {
        match reason {
            Ok(reason) => Self(reason),
            Err(oneshot::Canceled) => Self(STREAM_CLOSED.to_owned()),
        }
    }
}

impl PtyInputHandle {
    /// Send `data` to the program. Resolves once the last chunk is handed to the transport;
    /// under PTY backpressure that waits for the daemon to drain earlier writes, as
    /// Electron's `pty:write` does. Writes are delivered in call order. A write the
    /// attachment drops fails with why it ended, as Electron's pending writes do.
    pub fn write(&self, data: &[u8]) -> impl Future<Output = Result<(), PtyClosed>> + use<> {
        let mut last = None;
        let chunks: Vec<&[u8]> = if data.is_empty() {
            Vec::new()
        } else {
            data.chunks(MAX_WRITE_CHUNK).collect()
        };
        let count = chunks.len();
        let mut queued = self.ended().is_none();
        for (index, chunk) in chunks.into_iter().enumerate() {
            if !queued {
                break;
            }
            let sent = if index + 1 == count {
                let (tx, rx) = oneshot::channel();
                last = Some(rx);
                Some(tx)
            } else {
                None
            };
            let message = PtyInput {
                input: Some(pty_input::Input::Write(PtyWriteData {
                    data: chunk.to_vec(),
                })),
            };
            queued = self.queue.unbounded_send(Queued { message, sent }).is_ok();
        }
        let closed = self.closed();
        async move {
            let sent = match (queued, last) {
                (false, _) => false,
                (true, Some(rx)) => match rx.await {
                    Ok(()) => true,
                    Err(oneshot::Canceled) => false,
                },
                (true, None) => true,
            };
            // The reason is awaited: the input side can shut down before the event side
            // has recorded why.
            if sent { Ok(()) } else { Err(closed.await) }
        }
    }

    /// Queue `data` without waiting for it to be sent. For keystrokes and query answers:
    /// ordering is kept, and a closed stream is reported by the event stream anyway.
    pub fn send(&self, data: &[u8]) -> Result<(), PtyClosed> {
        for chunk in data.chunks(MAX_WRITE_CHUNK) {
            self.enqueue(pty_input::Input::Write(PtyWriteData {
                data: chunk.to_vec(),
            }))?;
        }
        Ok(())
    }

    /// Ask the daemon for a new size. It answers with [`PtyEvent::Resize`] carrying the
    /// size it applied, which may differ when another attachment shares the PTY.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), PtyClosed> {
        self.enqueue(pty_input::Input::Resize(PtyResizeData {
            cols: i32::from(cols),
            rows: i32::from(rows),
        }))
    }

    /// The user is now at this attachment's terminal: of the terminal attachments, only
    /// the most recently focused may write.
    pub fn focus(&self) -> Result<(), PtyClosed> {
        self.enqueue(pty_input::Input::Focus(Empty {}))
    }

    /// Resolves once the attachment has ended, with why.
    pub fn closed(&self) -> impl Future<Output = PtyClosed> + use<> {
        self.closed.clone().map(PtyClosed::from_reason)
    }

    /// Why the attachment ended, if it has. Nothing is queued after that, as Electron's
    /// `PtyStream` refuses writes once closed.
    fn ended(&self) -> Option<PtyClosed> {
        self.closed().now_or_never()
    }

    fn enqueue(&self, input: pty_input::Input) -> Result<(), PtyClosed> {
        if let Some(ended) = self.ended() {
            return Err(ended);
        }
        self.queue
            .unbounded_send(Queued {
                message: PtyInput { input: Some(input) },
                sent: None,
            })
            // The input shut down before the event side recorded why.
            .map_err(|_| {
                self.ended()
                    .unwrap_or_else(|| PtyClosed(STREAM_CLOSED.to_owned()))
            })
    }
}

/// A live attachment. Dropping `events` detaches: the RPC is cancelled and the PTY keeps
/// running for its other attachments.
pub struct PtyAttachment {
    pub input: PtyInputHandle,
    /// Ends after an exit, when the daemon closes the stream (e.g. the PTY does not exist),
    /// or after an error.
    pub events: Sub<Result<PtyEvent, ApiError>>,
}

pub(crate) fn attach(
    rt: &Rt,
    mut client: DaemonClient,
    session_id: &str,
    kind: AttachKind,
) -> PtyAttachment {
    let (queue_tx, mut queue_rx) = mpsc::unbounded::<Queued>();
    let (request_tx, request_rx) = tokio::sync::mpsc::channel::<PtyInput>(REQUEST_BUFFER);

    let start = PtyInput {
        input: Some(pty_input::Input::Start(PtyStartData {
            session_id: session_id.to_owned(),
            kind: kind.proto().into(),
        })),
    };
    queue_tx
        .unbounded_send(Queued {
            message: start,
            sent: None,
        })
        .expect("receiver is alive: it was created above");

    // The pump preserves call order between the unbounded queue (callable from anywhere,
    // synchronously) and the bounded channel tonic reads from. It ends once the call is
    // gone (the request channel closes) or every input handle is dropped.
    let pump = rt.run(async move {
        while let Some(queued) = queue_rx.next().await {
            if request_tx.send(queued.message).await.is_err() {
                break;
            }
            if let Some(sent) = queued.sent {
                // The writer may have stopped waiting; nothing to report then.
                sent.send(()).ok();
            }
        }
    });

    let call = async move {
        client
            .pty_stream(ReceiverStream::new(request_rx))
            .await
    };
    let session_id = session_id.to_owned();
    let (closed_tx, closed_rx) = oneshot::channel();
    let events = rt.subscribe(PumpGuarded {
        stream: call
            .map_ok(tonic::Response::into_inner)
            .try_flatten_stream()
            .filter_map(move |item| {
                let decoded = item.map_err(ApiError::from).and_then(event).transpose();
                if decoded.is_none() {
                    tracing::warn!(
                        %session_id,
                        "ignoring PTY output of a kind this client does not know (newer daemon?)"
                    );
                }
                futures::future::ready(decoded)
            })
            .boxed(),
        closed: Some(closed_tx),
        _pump: pump,
    });

    PtyAttachment {
        input: PtyInputHandle {
            queue: queue_tx,
            closed: closed_rx.shared(),
        },
        events,
    }
}

/// Keeps the input pump alive exactly as long as the event stream: detaching aborts both.
///
/// Also tells the input handles why the attachment ended, as Electron's `PtyStream` rejects
/// its writes: with the stream's error, [`STREAM_ENDED`], or [`STREAM_DETACHED`] when it is
/// dropped first. An error ends the stream, as a stream `error` closes Electron's.
struct PumpGuarded<S> {
    stream: S,
    /// Taken when the reason is sent.
    closed: Option<oneshot::Sender<String>>,
    _pump: crate::rt::Fut<()>,
}

impl<S> PumpGuarded<S> {
    fn close(&mut self, reason: String) {
        if let Some(closed) = self.closed.take() {
            match closed.send(reason) {
                Ok(()) | Err(_) => {} // Err: every input handle is gone; no one is left to tell.
            }
        }
    }
}

impl<S: Stream<Item = Result<PtyEvent, ApiError>> + Unpin> Stream for PumpGuarded<S> {
    type Item = S::Item;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<S::Item>> {
        if self.closed.is_none() {
            return std::task::Poll::Ready(None);
        }
        let item = std::task::ready!(self.stream.poll_next_unpin(cx));
        match &item {
            Some(Ok(_)) => {}
            Some(Err(error)) => self.close(error.to_string()),
            None => self.close(STREAM_ENDED.to_owned()),
        }
        std::task::Poll::Ready(item)
    }
}

impl<S> Drop for PumpGuarded<S> {
    fn drop(&mut self) {
        self.close(STREAM_DETACHED.to_owned());
    }
}

/// Parameters of [`crate::daemon::DaemonConnection::create_pty`].
#[derive(Debug, Clone, Default)]
pub struct CreatePty {
    pub cwd: String,
    pub env: std::collections::HashMap<String, String>,
    pub size: Option<(u16, u16)>,
    pub startup_command: Option<String>,
    /// Client-minted idempotency key (the tab's persisted `ptyHandle`): a live PTY already
    /// created for it is returned instead of a new one.
    pub handle: Option<String>,
    /// How to sandbox the program (an AI-harness tab's `sandbox`). The proto's
    /// `SandboxConfig`.
    pub sandbox: Option<Sandbox>,
}

/// `SandboxConfig` (`src/shared/types.ts`): what an AI-harness tab's PTY may reach.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sandbox {
    pub enabled: bool,
    pub allow_network: bool,
    pub allowed_paths: Vec<String>,
}

impl Sandbox {
    /// The `sandbox` object of a tab's state, as Electron sends it to `createPty`: a field
    /// it lacks is sent as the proto default (false, no paths). `None` when the state has no
    /// such object.
    pub fn from_tab_state(state: &Value) -> Option<Self> {
        let sandbox = state.get("sandbox")?.as_object()?;
        let flag = |key: &str| sandbox.get(key).and_then(Value::as_bool).unwrap_or(false);
        let allowed_paths = sandbox
            .get("allowedPaths")
            .and_then(Value::as_array)
            .map(|paths| {
                paths
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            enabled: flag("enabled"),
            allow_network: flag("allowNetwork"),
            allowed_paths,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(output: pty_output::Output) -> PtyOutput {
        PtyOutput {
            output: Some(output),
        }
    }

    /// An input handle on a bare queue, with the queue and the sender of its close reason.
    fn input() -> (
        PtyInputHandle,
        mpsc::UnboundedReceiver<Queued>,
        oneshot::Sender<String>,
    ) {
        let (queue, rx) = mpsc::unbounded();
        let (closed_tx, closed_rx) = oneshot::channel();
        let handle = PtyInputHandle {
            queue,
            closed: closed_rx.shared(),
        };
        (handle, rx, closed_tx)
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap()
    }

    type Events = futures::stream::BoxStream<'static, Result<PtyEvent, ApiError>>;

    /// A pump guard over `items`, with the receiver of its close reason and one that is
    /// cancelled once its pump has stopped.
    fn guarded(
        rt: &Rt,
        items: Vec<Result<PtyEvent, ApiError>>,
    ) -> (
        PumpGuarded<Events>,
        oneshot::Receiver<String>,
        oneshot::Receiver<()>,
    ) {
        let (closed_tx, closed_rx) = oneshot::channel();
        let (alive_tx, alive_rx) = oneshot::channel::<()>();
        let pump = rt.run(async move {
            let _alive = alive_tx;
            std::future::pending::<()>().await;
        });
        let guarded = PumpGuarded {
            stream: futures::stream::iter(items).boxed(),
            closed: Some(closed_tx),
            _pump: pump,
        };
        (guarded, closed_rx, alive_rx)
    }

    #[test]
    fn converts_every_output_kind() {
        use pty_output::Output;
        assert_eq!(
            event(output(Output::Data(treeterm_proto::treeterm::PtyData {
                data: b"hi".to_vec()
            })))
            .unwrap(),
            Some(PtyEvent::Data(b"hi".to_vec()))
        );
        assert_eq!(
            event(output(Output::Resize(PtyResizeData { cols: 80, rows: 24 }))).unwrap(),
            Some(PtyEvent::Resize { cols: 80, rows: 24 })
        );
        assert_eq!(
            event(output(Output::ReplayStart(Empty {}))).unwrap(),
            Some(PtyEvent::ReplayStart)
        );
        assert_eq!(
            event(output(Output::ReplayEnd(Empty {}))).unwrap(),
            Some(PtyEvent::ReplayEnd)
        );
        assert_eq!(
            event(output(Output::Exit(treeterm_proto::treeterm::PtyExit {
                exit_code: 3,
                signal: Some(9)
            })))
            .unwrap(),
            Some(PtyEvent::Exit {
                code: 3,
                signal: Some(9)
            })
        );
    }

    #[test]
    fn skips_an_unknown_output_kind_and_rejects_bad_sizes() {
        assert_eq!(event(PtyOutput { output: None }).unwrap(), None);
        assert!(
            event(output(pty_output::Output::Resize(PtyResizeData {
                cols: 0,
                rows: 24
            })))
            .is_err()
        );
        assert!(
            event(output(pty_output::Output::Resize(PtyResizeData {
                cols: 70_000,
                rows: 24
            })))
            .is_err()
        );
    }

    #[test]
    fn writes_split_into_ordered_chunks_and_ack_after_the_last() {
        let (handle, mut rx, _closed) = input();
        let data = vec![b'x'; MAX_WRITE_CHUNK * 2 + 5];
        let done = handle.write(&data);

        let mut sizes = Vec::new();
        let mut acks = Vec::new();
        while let Ok(queued) = rx.try_recv() {
            let Some(pty_input::Input::Write(write)) = queued.message.input else {
                panic!("expected a write");
            };
            sizes.push(write.data.len());
            acks.push(queued.sent);
        }
        assert_eq!(sizes, vec![MAX_WRITE_CHUNK, MAX_WRITE_CHUNK, 5]);
        assert!(acks[0].is_none() && acks[1].is_none());
        let last = acks.pop().unwrap().unwrap();
        last.send(()).unwrap();
        futures::executor::block_on(done).unwrap();
    }

    #[test]
    fn writes_fail_once_the_queue_is_gone() {
        let (handle, rx, closed) = input();
        drop(rx);
        drop(closed);
        let gone = Err(PtyClosed(STREAM_CLOSED.to_owned()));
        assert_eq!(futures::executor::block_on(handle.write(b"x")), gone);
        assert_eq!(handle.resize(10, 10), gone);
        assert_eq!(handle.send(b"x"), gone);
        assert_eq!(handle.focus(), gone);
    }

    #[test]
    fn once_ended_nothing_is_queued_and_writes_fail_with_the_reason() {
        let (handle, mut rx, closed) = input();
        closed.send(STREAM_ENDED.to_owned()).unwrap();
        let ended = Err(PtyClosed(STREAM_ENDED.to_owned()));
        assert_eq!(futures::executor::block_on(handle.write(b"x")), ended);
        assert_eq!(handle.resize(10, 10), ended);
        assert_eq!(handle.send(b"x"), ended);
        assert_eq!(handle.focus(), ended);
        assert!(rx.try_recv().is_err(), "nothing was queued");
    }

    #[test]
    fn a_write_the_attachment_dropped_waits_for_why() {
        let (handle, mut rx, closed) = input();
        let mut write = Box::pin(handle.write(b"x"));
        // The pump drops what it still holds as the attachment goes away.
        drop(rx.try_recv().unwrap());
        assert!((&mut write).now_or_never().is_none(), "waits for the reason");
        let reason = "14 UNAVAILABLE: Connection reset by peer";
        closed.send(reason.to_owned()).unwrap();
        assert_eq!(
            futures::executor::block_on(write),
            Err(PtyClosed(reason.to_owned()))
        );
    }

    #[test]
    fn the_stream_ending_is_why_the_attachment_closed() {
        let tokio = runtime();
        let rt = Rt::new(tokio.handle().clone());
        let (guarded, closed, pump) = guarded(&rt, vec![Ok(PtyEvent::ReplayStart)]);
        let items: Vec<_> = futures::executor::block_on(guarded.collect());
        assert!(matches!(items.as_slice(), [Ok(PtyEvent::ReplayStart)]));
        assert_eq!(futures::executor::block_on(closed), Ok(STREAM_ENDED.to_owned()));
        assert!(futures::executor::block_on(pump).is_err(), "the pump stops");
    }

    #[test]
    fn an_error_ends_the_stream_and_is_why_the_attachment_closed() {
        let tokio = runtime();
        let rt = Rt::new(tokio.handle().clone());
        let items = vec![
            Ok(PtyEvent::ReplayStart),
            Err(ApiError::Protocol("PTY size out of range")),
            Ok(PtyEvent::ReplayEnd),
        ];
        let (guarded, closed, _pump) = guarded(&rt, items);
        let items: Vec<_> = futures::executor::block_on(guarded.collect());
        assert!(matches!(
            items.as_slice(),
            [Ok(PtyEvent::ReplayStart), Err(ApiError::Protocol(_))]
        ));
        assert_eq!(
            futures::executor::block_on(closed),
            Ok("daemon protocol violation: PTY size out of range".to_owned())
        );
    }

    #[test]
    fn dropping_the_stream_first_is_a_detach() {
        let tokio = runtime();
        let rt = Rt::new(tokio.handle().clone());
        let items = vec![Ok(PtyEvent::ReplayStart), Ok(PtyEvent::ReplayEnd)];
        let (mut guarded, closed, pump) = guarded(&rt, items);
        assert!(matches!(
            futures::executor::block_on(guarded.next()),
            Some(Ok(PtyEvent::ReplayStart))
        ));
        drop(guarded);
        assert_eq!(futures::executor::block_on(closed), Ok(STREAM_DETACHED.to_owned()));
        assert!(futures::executor::block_on(pump).is_err(), "the pump stops");
    }

    #[test]
    fn a_tab_states_sandbox_object_is_its_sandbox_config() {
        let state = serde_json::json!({
            "ptyId": null,
            "sandbox": { "enabled": true, "allowNetwork": true, "allowedPaths": ["/a", "/b"] },
        });
        assert_eq!(
            Sandbox::from_tab_state(&state),
            Some(Sandbox {
                enabled: true,
                allow_network: true,
                allowed_paths: vec!["/a".to_owned(), "/b".to_owned()],
            })
        );
    }

    #[test]
    fn a_sandbox_field_the_state_lacks_is_the_proto_default() {
        let state = serde_json::json!({ "sandbox": { "enabled": true } });
        assert_eq!(
            Sandbox::from_tab_state(&state),
            Some(Sandbox {
                enabled: true,
                allow_network: false,
                allowed_paths: Vec::new(),
            })
        );
    }

    #[test]
    fn a_state_without_a_sandbox_object_has_no_sandbox() {
        assert_eq!(Sandbox::from_tab_state(&serde_json::json!({ "ptyId": "p1" })), None);
        assert_eq!(Sandbox::from_tab_state(&serde_json::json!({ "sandbox": null })), None);
        assert_eq!(Sandbox::from_tab_state(&serde_json::json!(null)), None);
    }
}
