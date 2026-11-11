//! Attachments to daemon PTYs.
//!
//! An attachment is one `PtyStream` call: the daemon replays the PTY's current state, then
//! streams its live output, and takes writes, resizes and focus claims back. Dropping the
//! event stream detaches; it never kills the PTY ("unmount is not close", AGENTS.md).
//! Killing is the separate [`crate::daemon::DaemonConnection::kill_pty`].

use futures::channel::{mpsc, oneshot};
use futures::{Stream, StreamExt, TryFutureExt};
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

impl TryFrom<PtyOutput> for PtyEvent {
    type Error = ApiError;

    fn try_from(output: PtyOutput) -> Result<Self, ApiError> {
        use pty_output::Output;
        match output.output {
            Some(Output::Data(data)) => Ok(PtyEvent::Data(data.data)),
            Some(Output::Resize(size)) => Ok(PtyEvent::Resize {
                cols: dimension(size.cols)?,
                rows: dimension(size.rows)?,
            }),
            Some(Output::ReplayStart(Empty {})) => Ok(PtyEvent::ReplayStart),
            Some(Output::ReplayEnd(Empty {})) => Ok(PtyEvent::ReplayEnd),
            Some(Output::Exit(exit)) => Ok(PtyEvent::Exit {
                code: exit.exit_code,
                signal: exit.signal,
            }),
            None => Err(ApiError::Protocol("PtyOutput without a payload")),
        }
    }
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
}

struct Queued {
    message: PtyInput,
    /// Fired once the message is handed to tonic.
    sent: Option<oneshot::Sender<()>>,
}

/// The attachment's input queue has shut down: the stream ended or was detached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("PTY stream closed")]
pub struct PtyClosed;

impl PtyInputHandle {
    /// Send `data` to the program. Resolves once the last chunk is handed to the transport;
    /// under PTY backpressure that waits for the daemon to drain earlier writes, as
    /// Electron's `pty:write` does. Writes are delivered in call order.
    pub fn write(&self, data: &[u8]) -> impl Future<Output = Result<(), PtyClosed>> + use<> {
        let mut last = None;
        let chunks: Vec<&[u8]> = if data.is_empty() {
            Vec::new()
        } else {
            data.chunks(MAX_WRITE_CHUNK).collect()
        };
        let count = chunks.len();
        let mut queued = Ok(());
        for (index, chunk) in chunks.into_iter().enumerate() {
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
            if self.queue.unbounded_send(Queued { message, sent }).is_err() {
                queued = Err(PtyClosed);
                break;
            }
        }
        async move {
            queued?;
            match last {
                Some(rx) => rx.await.map_err(|_canceled| PtyClosed),
                None => Ok(()),
            }
        }
    }

    /// Queue `data` without waiting for it to be sent. For keystrokes and query answers:
    /// ordering is kept, and a closed stream is reported by the event stream anyway.
    pub fn send(&self, data: &[u8]) -> Result<(), PtyClosed> {
        for chunk in data.chunks(MAX_WRITE_CHUNK) {
            self.queue
                .unbounded_send(Queued {
                    message: PtyInput {
                        input: Some(pty_input::Input::Write(PtyWriteData {
                            data: chunk.to_vec(),
                        })),
                    },
                    sent: None,
                })
                .map_err(|_| PtyClosed)?;
        }
        Ok(())
    }

    /// Ask the daemon for a new size. It answers with [`PtyEvent::Resize`] carrying the
    /// size it applied, which may differ when another attachment shares the PTY.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), PtyClosed> {
        self.control(pty_input::Input::Resize(PtyResizeData {
            cols: i32::from(cols),
            rows: i32::from(rows),
        }))
    }

    /// The user is now at this attachment's terminal: of the terminal attachments, only
    /// the most recently focused may write.
    pub fn focus(&self) -> Result<(), PtyClosed> {
        self.control(pty_input::Input::Focus(Empty {}))
    }

    fn control(&self, input: pty_input::Input) -> Result<(), PtyClosed> {
        self.queue
            .unbounded_send(Queued {
                message: PtyInput { input: Some(input) },
                sent: None,
            })
            .map_err(|_| PtyClosed)
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
    let events = rt.subscribe(PumpGuarded {
        stream: call
            .map_ok(tonic::Response::into_inner)
            .try_flatten_stream()
            .map(|item| {
                item.map_err(ApiError::from)
                    .and_then(PtyEvent::try_from)
            })
            .boxed(),
        _pump: pump,
    });

    PtyAttachment {
        input: PtyInputHandle { queue: queue_tx },
        events,
    }
}

/// Keeps the input pump alive exactly as long as the event stream: detaching aborts both.
struct PumpGuarded<S> {
    stream: S,
    _pump: crate::rt::Fut<()>,
}

impl<S: Stream + Unpin> Stream for PumpGuarded<S> {
    type Item = S::Item;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<S::Item>> {
        self.stream.poll_next_unpin(cx)
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(output: pty_output::Output) -> PtyOutput {
        PtyOutput {
            output: Some(output),
        }
    }

    #[test]
    fn converts_every_output_kind() {
        use pty_output::Output;
        assert_eq!(
            PtyEvent::try_from(output(Output::Data(treeterm_proto::treeterm::PtyData {
                data: b"hi".to_vec()
            })))
            .unwrap(),
            PtyEvent::Data(b"hi".to_vec())
        );
        assert_eq!(
            PtyEvent::try_from(output(Output::Resize(PtyResizeData { cols: 80, rows: 24 })))
                .unwrap(),
            PtyEvent::Resize { cols: 80, rows: 24 }
        );
        assert_eq!(
            PtyEvent::try_from(output(Output::ReplayStart(Empty {}))).unwrap(),
            PtyEvent::ReplayStart
        );
        assert_eq!(
            PtyEvent::try_from(output(Output::ReplayEnd(Empty {}))).unwrap(),
            PtyEvent::ReplayEnd
        );
        assert_eq!(
            PtyEvent::try_from(output(Output::Exit(treeterm_proto::treeterm::PtyExit {
                exit_code: 3,
                signal: Some(9)
            })))
            .unwrap(),
            PtyEvent::Exit {
                code: 3,
                signal: Some(9)
            }
        );
    }

    #[test]
    fn rejects_empty_output_and_bad_sizes() {
        assert!(PtyEvent::try_from(PtyOutput { output: None }).is_err());
        assert!(
            PtyEvent::try_from(output(pty_output::Output::Resize(PtyResizeData {
                cols: 0,
                rows: 24
            })))
            .is_err()
        );
        assert!(
            PtyEvent::try_from(output(pty_output::Output::Resize(PtyResizeData {
                cols: 70_000,
                rows: 24
            })))
            .is_err()
        );
    }

    #[test]
    fn writes_split_into_ordered_chunks_and_ack_after_the_last() {
        let (queue, mut rx) = mpsc::unbounded();
        let handle = PtyInputHandle { queue };
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
        let (queue, rx) = mpsc::unbounded();
        let handle = PtyInputHandle { queue };
        drop(rx);
        assert_eq!(
            futures::executor::block_on(handle.write(b"x")),
            Err(PtyClosed)
        );
        assert_eq!(handle.resize(10, 10), Err(PtyClosed));
        assert_eq!(handle.send(b"x"), Err(PtyClosed));
        assert_eq!(handle.focus(), Err(PtyClosed));
    }
}
