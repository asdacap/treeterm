//! Bounded log buffers and change subscriptions (Electron's per-phase output ring buffers and
//! `watchX(cb) → { scrollback, unsubscribe }`).

use std::pin::Pin;
use std::task::{Context, Poll};

use futures::channel::mpsc;
use futures::{Stream, StreamExt};

/// Lines kept for scrollback. Once more than `max` lines are held, only the last `keep`
/// remain (Electron: `if (buffer.length > 1000) buffer = buffer.slice(-500)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLog {
    lines: Vec<String>,
    max: usize,
    keep: usize,
}

impl OutputLog {
    pub const fn new(max: usize, keep: usize) -> Self {
        Self {
            lines: Vec::new(),
            max,
            keep,
        }
    }

    /// Bootstrap, tunnel and port forward output: over 1000 lines → last 500.
    pub const fn ssh() -> Self {
        Self::new(1000, 500)
    }

    /// Daemon output: over 500 lines → last 250.
    pub const fn daemon() -> Self {
        Self::new(500, 250)
    }

    pub fn push(&mut self, line: String) {
        self.lines.push(line);
        if self.lines.len() > self.max {
            let excess = self.lines.len() - self.keep;
            self.lines.drain(..excess);
        }
    }

    /// A copy of the lines held.
    pub fn snapshot(&self) -> Vec<String> {
        self.lines.clone()
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }
}

/// A subscription's later values. Dropping it unsubscribes. The stream ends when its source
/// goes away (connection removed, port forward removed).
#[must_use = "dropping Updates unsubscribes"]
pub struct Updates<T>(mpsc::UnboundedReceiver<T>);

impl<T> Stream for Updates<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        self.0.poll_next_unpin(cx)
    }
}

impl<T> Updates<T> {
    /// Values already delivered, without waiting (for tests and synchronous consumers).
    pub fn drain_ready(&mut self) -> Vec<T> {
        let mut ready = Vec::new();
        while let Ok(value) = self.0.try_recv() {
            ready.push(value);
        }
        ready
    }
}

/// The current value (or scrollback) and the changes after it.
pub struct Watch<I, T> {
    pub initial: I,
    pub updates: Updates<T>,
}

/// Subscribers to one kind of value. A subscriber is dropped once its [`Updates`] is.
pub(crate) struct Listeners<T> {
    senders: Vec<mpsc::UnboundedSender<T>>,
}

impl<T> Default for Listeners<T> {
    fn default() -> Self {
        Self {
            senders: Vec::new(),
        }
    }
}

impl<T: Clone> Listeners<T> {
    pub(crate) fn subscribe(&mut self) -> Updates<T> {
        let (tx, rx) = mpsc::unbounded();
        self.senders.push(tx);
        Updates(rx)
    }

    pub(crate) fn send(&mut self, value: &T) {
        self.senders
            .retain(|sender| sender.unbounded_send(value.clone()).is_ok());
    }
}

/// Updates that yield `value` once, then end.
pub(crate) fn once<T>(value: T) -> Updates<T> {
    let (tx, rx) = mpsc::unbounded();
    tx.unbounded_send(value)
        .expect("the receiver is alive: it is returned below");
    Updates(rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_output_returns_copy_of_buffer() {
        let mut log = OutputLog::ssh();
        log.push("line 1".to_owned());
        log.push("line 2".to_owned());
        let mut output = log.snapshot();
        assert_eq!(output, vec!["line 1", "line 2"]);
        output.push("line 3".to_owned());
        assert_eq!(log.snapshot().len(), 2);
    }

    #[test]
    fn output_buffer_is_bounded() {
        let mut log = OutputLog::ssh();
        for i in 0..1100 {
            log.push(format!("line {i}"));
        }
        assert!(log.snapshot().len() <= 1000);
    }

    #[test]
    fn trims_to_the_last_half_once_over_the_limit() {
        let mut log = OutputLog::ssh();
        for i in 0..1000 {
            log.push(format!("line {i}"));
        }
        assert_eq!(log.snapshot().len(), 1000);
        log.push("line 1000".to_owned());
        let lines = log.snapshot();
        assert_eq!(lines.len(), 500);
        assert_eq!(lines.first().unwrap(), "line 501");
        assert_eq!(lines.last().unwrap(), "line 1000");
    }

    #[test]
    fn daemon_buffer_trims_when_it_exceeds_500_lines() {
        let mut log = OutputLog::daemon();
        for i in 0..501 {
            log.push(format!("line {i}"));
        }
        assert_eq!(log.snapshot().len(), 250);
    }

    #[test]
    fn listeners_are_notified_until_they_unsubscribe() {
        let mut listeners = Listeners::default();
        let mut first = listeners.subscribe();
        let second = listeners.subscribe();
        listeners.send(&"test line".to_owned());
        drop(second);
        listeners.send(&"after".to_owned());
        assert_eq!(first.drain_ready(), vec!["test line", "after"]);
        assert_eq!(listeners.senders.len(), 1);
    }

    #[test]
    fn once_yields_its_value_then_ends() {
        let updates = once(7);
        let values: Vec<i32> = futures::executor::block_on(updates.collect());
        assert_eq!(values, vec![7]);
    }
}
