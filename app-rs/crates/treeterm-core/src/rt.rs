//! Bridge from tokio to any executor.
//!
//! tonic, child processes and HTTP all need a tokio reactor, while GPUI runs its own
//! executors. All such work is spawned on one tokio runtime and handed back as plain
//! futures ([`Fut`]) and streams ([`Sub`]) that any executor can poll. Dropping either
//! aborts the tokio task — for an RPC that cancels the call — so they are the
//! disposables of this codebase: hold one for as long as the work should run.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::channel::{mpsc, oneshot};
use futures::{FutureExt, SinkExt, Stream, StreamExt};

/// Items a subscription may buffer before the producer waits for the consumer.
const SUB_BUFFER: usize = 64;

/// Handle to the tokio runtime that drives all I/O. Created once at the top level and
/// injected; nothing below the binary reaches for a global runtime.
#[derive(Clone)]
pub struct Rt {
    handle: tokio::runtime::Handle,
}

impl Rt {
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self { handle }
    }

    /// Enter the runtime's context, for constructors that must run inside it (reqwest).
    pub fn enter(&self) -> tokio::runtime::EnterGuard<'_> {
        self.handle.enter()
    }

    /// Run `fut` on tokio. The returned future resolves with its output on any executor.
    pub fn run<T: Send + 'static>(&self, fut: impl Future<Output = T> + Send + 'static) -> Fut<T> {
        let (tx, rx) = oneshot::channel();
        let task = self.handle.spawn(async move {
            let output = fut.await;
            if tx.send(output).is_err() {
                tracing::trace!("rt: caller dropped the result");
            }
        });
        Fut {
            rx,
            _guard: AbortOnDrop(task.abort_handle()),
        }
    }

    /// Pump `stream` on tokio into a bounded channel readable from any executor.
    pub fn subscribe<T: Send + 'static>(
        &self,
        stream: impl Stream<Item = T> + Send + 'static,
    ) -> Sub<T> {
        let (mut tx, rx) = mpsc::channel(SUB_BUFFER);
        let task = self.handle.spawn(async move {
            let mut stream = std::pin::pin!(stream);
            while let Some(item) = stream.next().await {
                if tx.send(item).await.is_err() {
                    break;
                }
            }
        });
        Sub {
            rx,
            _guard: AbortOnDrop(task.abort_handle()),
        }
    }
}

/// Aborts a tokio task when dropped.
#[must_use = "dropping it immediately aborts the task"]
pub struct AbortOnDrop(tokio::task::AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// The tokio task behind a [`Fut`] ended without a result: it panicked, or the runtime
/// is shutting down.
#[derive(Debug, Clone, thiserror::Error)]
#[error("background task ended without a result (panicked or runtime shut down)")]
pub struct TaskLost;

/// Output of [`Rt::run`]. Dropping it aborts the task.
#[must_use = "dropping a Fut aborts the underlying task"]
pub struct Fut<T> {
    rx: oneshot::Receiver<T>,
    _guard: AbortOnDrop,
}

impl<T> Future for Fut<T> {
    type Output = Result<T, TaskLost>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.rx
            .poll_unpin(cx)
            .map(|result| result.map_err(|_canceled| TaskLost))
    }
}

/// Output of [`Rt::subscribe`]. Ends when the source stream ends. Dropping it aborts the pump.
#[must_use = "dropping a Sub cancels the subscription"]
pub struct Sub<T> {
    rx: mpsc::Receiver<T>,
    _guard: AbortOnDrop,
}

impl<T> Stream for Sub<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        self.rx.poll_next_unpin(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn run_delivers_output_to_a_foreign_executor() {
        let tokio = runtime();
        let rt = Rt::new(tokio.handle().clone());
        let fut = rt.run(async {
            tokio::time::sleep(Duration::from_millis(5)).await;
            42
        });
        assert_eq!(futures::executor::block_on(fut).unwrap(), 42);
    }

    #[test]
    fn subscribe_streams_items_in_order_then_ends() {
        let tokio = runtime();
        let rt = Rt::new(tokio.handle().clone());
        let sub = rt.subscribe(futures::stream::iter(1..=3));
        let items: Vec<i32> = futures::executor::block_on(sub.collect());
        assert_eq!(items, vec![1, 2, 3]);
    }

    #[test]
    fn dropping_a_fut_aborts_the_task() {
        let tokio = runtime();
        let rt = Rt::new(tokio.handle().clone());
        let finished = Arc::new(AtomicBool::new(false));
        let flag = finished.clone();
        let fut = rt.run(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            flag.store(true, Ordering::SeqCst);
        });
        drop(fut);
        std::thread::sleep(Duration::from_millis(120));
        assert!(!finished.load(Ordering::SeqCst));
    }

    #[test]
    fn a_panicking_task_reports_task_lost() {
        let tokio = runtime();
        let rt = Rt::new(tokio.handle().clone());
        let fut = rt.run(async { panic!("boom") });
        assert!(matches!(futures::executor::block_on(fut), Err(TaskLost)));
    }
}
