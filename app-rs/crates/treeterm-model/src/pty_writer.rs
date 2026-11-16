//! Programmatic writes to a daemon PTY: Electron's `TtyWriter` (`getTtyWriter` in
//! `createWorkspaceStore.ts`).
//!
//! A writer is its own `Background` attachment, whose writes the daemon never gates on
//! focus. So an AI-harness prompt or an auto-approve lands even while another terminal
//! attachment of the PTY holds its input claim (another window, Electron showing the same
//! tab, the Active Processes viewer); through the tab's own terminal attachment the daemon
//! would silently drop it.
//!
//! The output is read only to learn that the attachment is up and when it ends. Dropping
//! the writer detaches; it never kills the PTY.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::StreamExt;
use futures::channel::oneshot;
use futures::future::{FutureExt, Shared};
use gpui_kit::{BackgroundExecutor, Task};
use treeterm_core::daemon::DaemonConnection;
use treeterm_core::pty::{AttachKind, PtyClosed, PtyInputHandle};

pub struct PtyWriter {
    input: PtyInputHandle,
    /// Resolves once the daemon answered the attach (it starts every attachment with its
    /// replay); cancelled when the stream ended before that (no such PTY).
    attached: Shared<oneshot::Receiver<()>>,
    /// The stream ended or a write failed: the writer is spent.
    closed: Arc<AtomicBool>,
    _events: Task<()>,
}

impl PtyWriter {
    pub fn open(conn: &DaemonConnection, pty_id: &str, executor: &BackgroundExecutor) -> Self {
        let attachment = conn.attach_pty(pty_id, AttachKind::Background);
        let (attached_tx, attached_rx) = oneshot::channel();
        let closed = Arc::new(AtomicBool::new(false));
        let ended = closed.clone();
        let pty = pty_id.to_owned();
        let mut events = attachment.events;
        let drain = executor.spawn(async move {
            let mut attached = Some(attached_tx);
            while let Some(event) = events.next().await {
                match event {
                    Ok(_) => {
                        if let Some(attached) = attached.take() {
                            match attached.send(()) {
                                Ok(()) | Err(()) => {} // Err: the writer is being dropped
                            }
                        }
                    }
                    Err(error) => {
                        tracing::debug!(%pty, %error, "pty writer stream failed");
                        break;
                    }
                }
            }
            // Before `attached` drops, so a write that sees the attach cancelled also sees
            // the writer spent.
            ended.store(true, Ordering::SeqCst);
        });
        Self {
            input: attachment.input,
            attached: attached_rx.shared(),
            closed,
            _events: drain,
        }
    }

    /// Send `data` to the program. Writes are queued in call order; each resolves once
    /// handed to the transport after the daemon accepted the attach. Fails, and spends the
    /// writer, when the attachment is gone or never came up.
    pub fn write(&self, data: &[u8]) -> impl Future<Output = Result<(), PtyClosed>> + use<> {
        let closed = self.closed.clone();
        let sent = (!closed.load(Ordering::SeqCst)).then(|| self.input.write(data));
        let attached = self.attached.clone();
        async move {
            let result = match sent {
                None => Err(PtyClosed),
                Some(sent) => match attached.await {
                    Ok(()) => sent.await,
                    Err(_canceled) => Err(PtyClosed),
                },
            };
            if result.is_err() {
                closed.store(true, Ordering::SeqCst);
            }
            result
        }
    }

    /// The writer is spent: a fresh one must be opened.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}
