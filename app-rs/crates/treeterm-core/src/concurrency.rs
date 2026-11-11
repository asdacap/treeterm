//! Bounded fan-out and per-key coalescing of async work (port of `renderer/lib/concurrency.ts`).
//!
//! Both are executor-agnostic: they only combine futures, so they run wherever they are polled.

use std::collections::HashMap;
use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

use futures::future::{BoxFuture, FutureExt, Shared, WeakShared};
use futures::{StreamExt, TryStreamExt};

/// Map `items` through `f` with at most `limit` calls in flight, preserving input order in
/// the result. Fails with the first error; no new item is started after a failure.
///
/// The limit is a `NonZeroUsize`, so the invalid limits the TS version rejects at run time
/// cannot be expressed.
pub async fn map_with_concurrency<T, R, E, F, Fut>(
    items: impl IntoIterator<Item = T>,
    limit: NonZeroUsize,
    f: F,
) -> Result<Vec<R>, E>
where
    F: FnMut(T) -> Fut,
    Fut: Future<Output = Result<R, E>>,
{
    futures::stream::iter(items)
        .map(f)
        .buffered(limit.get())
        .try_collect()
        .await
}

/// A run's future, shareable between every caller that coalesced onto it.
pub type SharedRun<R> = Shared<BoxFuture<'static, R>>;

/// Per key, at most one run in flight and at most one more waiting behind it. Callers
/// arriving while a run is in flight all share the single queued follow-up run, so every
/// caller gets a result computed after it asked — never a stale in-flight one — without
/// piling up work.
///
/// The decision is made when [`Self::call`] is called, not when the returned future is
/// first polled. Runs are held weakly: when every caller drops a run before it finishes,
/// the run is dropped (cancelled) and the next call starts afresh.
pub struct CoalesceByKey<R> {
    runs: Arc<Mutex<Runs<R>>>,
}

struct Runs<R> {
    next_id: u64,
    by_key: HashMap<String, KeyedRun<R>>,
}

struct KeyedRun<R> {
    running_id: u64,
    running: WeakShared<BoxFuture<'static, R>>,
    queued: Queued<R>,
}

enum Queued<R> {
    Nothing,
    Waiting(WeakShared<BoxFuture<'static, R>>),
}

impl<R> Queued<R> {
    fn live(&self) -> Option<SharedRun<R>> {
        match self {
            Queued::Nothing => None,
            Queued::Waiting(weak) => weak.upgrade(),
        }
    }
}

impl<R: Clone + Send + Sync + 'static> Default for CoalesceByKey<R> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: Clone + Send + Sync + 'static> CoalesceByKey<R> {
    pub fn new() -> Self {
        Self {
            runs: Arc::new(Mutex::new(Runs {
                next_id: 0,
                by_key: HashMap::new(),
            })),
        }
    }

    /// Run `start()` for `key`, or join the run that will produce a fresh result for this
    /// caller. `start` is invoked now when nothing runs for `key`, later (once the running
    /// call settles) when this caller creates the queued run, and not at all when this
    /// caller joins an existing queued run.
    pub fn call(
        &self,
        key: &str,
        start: impl FnOnce() -> BoxFuture<'static, R> + Send + 'static,
    ) -> SharedRun<R> {
        let mut runs = self.runs.lock().expect("coalescer lock poisoned");
        if let Some(existing) = runs.by_key.get(key) {
            if let Some(queued) = existing.queued.live() {
                return queued;
            }
            if let Some(running) = existing.running.upgrade() {
                let queued = queue_behind(&self.runs, key.to_owned(), running, start);
                let weak = queued
                    .downgrade()
                    .expect("a run that was never polled has not completed");
                runs.by_key
                    .get_mut(key)
                    .expect("entry looked up above under the same lock")
                    .queued = Queued::Waiting(weak);
                return queued;
            }
        }
        start_run(&self.runs, &mut runs, key.to_owned(), start())
    }
}

/// Register `fut` as the running call for `key`. Must be called with the lock held.
fn start_run<R: Clone + Send + Sync + 'static>(
    shared: &Arc<Mutex<Runs<R>>>,
    runs: &mut Runs<R>,
    key: String,
    fut: BoxFuture<'static, R>,
) -> SharedRun<R> {
    let id = runs.next_id;
    runs.next_id += 1;
    let run = track(Arc::clone(shared), key.clone(), id, fut);
    let running = run
        .downgrade()
        .expect("a run that was never polled has not completed");
    runs.by_key.insert(
        key,
        KeyedRun {
            running_id: id,
            running,
            queued: Queued::Nothing,
        },
    );
    run
}

/// Forget the key once its run settles, unless a newer run or a queued one took over.
fn track<R: Clone + Send + Sync + 'static>(
    shared: Arc<Mutex<Runs<R>>>,
    key: String,
    id: u64,
    fut: BoxFuture<'static, R>,
) -> SharedRun<R> {
    async move {
        let output = fut.await;
        let mut runs = shared.lock().expect("coalescer lock poisoned");
        let settled = runs
            .by_key
            .get(&key)
            .is_some_and(|run| run.running_id == id && run.queued.live().is_none());
        if settled {
            runs.by_key.remove(&key);
        }
        output
    }
    .boxed()
    .shared()
}

/// The follow-up run: waits for `running` to settle (its outcome belongs to its own
/// callers), then starts and becomes the running call for `key`.
fn queue_behind<R: Clone + Send + Sync + 'static>(
    shared: &Arc<Mutex<Runs<R>>>,
    key: String,
    running: SharedRun<R>,
    start: impl FnOnce() -> BoxFuture<'static, R> + Send + 'static,
) -> SharedRun<R> {
    let shared = Arc::clone(shared);
    async move {
        drop(running.await);
        let next = {
            let mut runs = shared.lock().expect("coalescer lock poisoned");
            start_run(&shared, &mut runs, key, start())
        };
        next.await
    }
    .boxed()
    .shared()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures::channel::oneshot;
    use futures::executor::block_on;

    fn limit(n: usize) -> NonZeroUsize {
        NonZeroUsize::new(n).unwrap()
    }

    /// Completes on its second poll, like awaiting a macrotask in the TS tests.
    fn flush() -> impl Future<Output = ()> {
        let mut polled = false;
        futures::future::poll_fn(move |cx| {
            if polled {
                std::task::Poll::Ready(())
            } else {
                polled = true;
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
        })
    }

    mod map_with_concurrency {
        use super::*;

        #[test]
        fn never_exceeds_the_limit_and_preserves_order() {
            let active = AtomicUsize::new(0);
            let peak = AtomicUsize::new(0);
            let result: Result<Vec<i32>, ()> =
                block_on(map_with_concurrency([1, 2, 3, 4, 5, 6, 7], limit(3), |n| {
                    let (active, peak) = (&active, &peak);
                    async move {
                        let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(now, Ordering::SeqCst);
                        flush().await;
                        active.fetch_sub(1, Ordering::SeqCst);
                        Ok(n * 10)
                    }
                }));
            assert_eq!(result.unwrap(), vec![10, 20, 30, 40, 50, 60, 70]);
            assert_eq!(peak.load(Ordering::SeqCst), 3);
        }

        #[test]
        fn handles_an_empty_list() {
            let result: Result<Vec<i32>, ()> = block_on(map_with_concurrency(
                Vec::<i32>::new(),
                limit(4),
                |_| async { Ok(1) },
            ));
            assert_eq!(result.unwrap(), Vec::<i32>::new());
        }

        #[test]
        fn rejects_with_the_error_and_stops_starting_new_items() {
            let started = Mutex::new(Vec::new());
            let result = block_on(map_with_concurrency([1, 2, 3, 4, 5], limit(1), |n| {
                started.lock().unwrap().push(n);
                async move { if n == 2 { Err("boom") } else { Ok(n) } }
            }));
            assert_eq!(result, Err("boom"));
            assert_eq!(*started.lock().unwrap(), vec![1, 2]);
        }
    }

    mod coalesce_by_key {
        use super::*;

        #[test]
        fn runs_once_per_key_while_idle() {
            let calls = Arc::new(AtomicUsize::new(0));
            let coalescer = CoalesceByKey::<&'static str>::new();
            let call = |key: &'static str| {
                let calls = Arc::clone(&calls);
                coalescer.call(key, move || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    async move { key }.boxed()
                })
            };
            assert_eq!(block_on(call("a")), "a");
            assert_eq!(block_on(call("a")), "a");
            assert_eq!(calls.load(Ordering::SeqCst), 2);
        }

        type Settle = oneshot::Sender<Result<i32, String>>;

        /// Each started run waits on its own oneshot, resolved by the test.
        struct Deferred {
            senders: Arc<Mutex<Vec<Settle>>>,
            coalescer: CoalesceByKey<Result<i32, String>>,
        }

        impl Deferred {
            fn new() -> Self {
                Self {
                    senders: Arc::new(Mutex::new(Vec::new())),
                    coalescer: CoalesceByKey::new(),
                }
            }

            fn call(&self, key: &str) -> SharedRun<Result<i32, String>> {
                let senders = Arc::clone(&self.senders);
                self.coalescer.call(key, move || {
                    let (tx, rx) = oneshot::channel();
                    senders.lock().unwrap().push(tx);
                    async move { rx.await.expect("test resolves every run") }.boxed()
                })
            }

            fn pending(&self) -> usize {
                self.senders.lock().unwrap().len()
            }

            fn settle(&self, index: usize, value: Result<i32, String>) {
                let sender = std::mem::replace(
                    &mut self.senders.lock().unwrap()[index],
                    oneshot::channel().0,
                );
                sender.send(value).expect("run is still waiting");
            }
        }

        #[test]
        fn queues_a_single_follow_up_run_for_callers_that_arrive_mid_flight() {
            let fake = Deferred::new();
            let first = fake.call("repo");
            let second = fake.call("repo");
            let third = fake.call("repo");
            assert_eq!(fake.pending(), 1);
            assert!(second.ptr_eq(&third));

            fake.settle(0, Ok(1));
            assert_eq!(block_on(first), Ok(1));
            let mut second = second;
            // Drive the queued run until it has started its own call.
            assert!((&mut second).now_or_never().is_none());
            assert_eq!(fake.pending(), 2);

            fake.settle(1, Ok(2));
            assert_eq!(block_on(second), Ok(2));
            assert_eq!(block_on(third), Ok(2));
        }

        #[test]
        fn keeps_keys_independent() {
            let keys = Arc::new(Mutex::new(Vec::new()));
            let coalescer = CoalesceByKey::<()>::new();
            let call = |key: &'static str| {
                let keys = Arc::clone(&keys);
                coalescer.call(key, move || {
                    keys.lock().unwrap().push(key);
                    futures::future::pending().boxed()
                })
            };
            let a = call("a");
            let b = call("b");
            assert_eq!(*keys.lock().unwrap(), vec!["a", "b"]);
            drop((a, b));
        }

        #[test]
        fn still_runs_the_queued_call_after_the_running_one_rejects() {
            let fake = Deferred::new();
            let first = fake.call("repo");
            let mut second = fake.call("repo");
            fake.settle(0, Err("fail".to_owned()));
            assert_eq!(block_on(first), Err("fail".to_owned()));
            assert!((&mut second).now_or_never().is_none());
            fake.settle(1, Ok(5));
            assert_eq!(block_on(second), Ok(5));
        }

        #[test]
        fn starts_fresh_after_everything_settles() {
            let calls = Arc::new(AtomicUsize::new(0));
            let coalescer = CoalesceByKey::<usize>::new();
            let call = || {
                let calls = Arc::clone(&calls);
                coalescer.call("repo", move || {
                    let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
                    async move { n }.boxed()
                })
            };
            assert_eq!(block_on(call()), 1);
            assert_eq!(block_on(call()), 2);
        }

        #[test]
        fn a_run_every_caller_dropped_is_cancelled_and_the_next_call_starts_afresh() {
            let fake = Deferred::new();
            let first = fake.call("repo");
            drop(first);
            let second = fake.call("repo");
            assert_eq!(fake.pending(), 2);
            fake.settle(1, Ok(7));
            assert_eq!(block_on(second), Ok(7));
        }
    }
}
