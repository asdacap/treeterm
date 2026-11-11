//! Download metering (`createTrafficMeterStore.ts`): byte counters fed by the daemon socket
//! and by PTY streams, and the rolling rate the traffic badges show.

use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Rolling window the download rate is averaged over.
pub const TRAFFIC_WINDOW: Duration = Duration::from_secs(10);
/// How often totals are sampled. Counting a chunk never touches UI state.
pub const TRAFFIC_TICK: Duration = Duration::from_secs(1);

/// A running byte total, shared between whatever receives bytes and whatever samples it.
#[derive(Debug, Clone, Default)]
pub struct ByteCounter(Arc<AtomicU64>);

impl ByteCounter {
    pub fn add(&self, bytes: u64) {
        self.0.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn total(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TrafficRate {
    pub total_bytes: u64,
    pub bytes_per_sec: f64,
}

/// Samples of a running total, and the rate across those still inside [`TRAFFIC_WINDOW`].
#[derive(Debug, Default)]
pub struct RateWindow {
    samples: VecDeque<(Instant, u64)>,
}

impl RateWindow {
    /// Add `total` read at `time`, and the rate across the window ending there.
    pub fn sample(&mut self, time: Instant, total: u64) -> TrafficRate {
        let window_start = time.checked_sub(TRAFFIC_WINDOW);
        while let (Some(&(oldest, _)), Some(start)) = (self.samples.front(), window_start) {
            if oldest >= start {
                break;
            }
            self.samples.pop_front();
        }
        let (since, from) = self.samples.front().copied().unwrap_or((time, total));
        let elapsed = time.saturating_duration_since(since).as_secs_f64();
        self.samples.push_back((time, total));
        TrafficRate {
            total_bytes: total,
            bytes_per_sec: if elapsed > 0.0 {
                total.saturating_sub(from) as f64 / elapsed
            } else {
                0.0
            },
        }
    }
}

/// `formatBytes.ts`.
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    match bytes {
        b if b < KB => format!("{b} B"),
        b if b < MB => format!("{:.1} KB", b as f64 / KB as f64),
        b if b < GB => format!("{:.1} MB", b as f64 / MB as f64),
        b => format!("{:.1} GB", b as f64 / GB as f64),
    }
}

/// A rate is an average, so whole bytes are rounded before formatting.
pub fn format_rate(bytes_per_sec: f64) -> String {
    format!("{}/s", format_bytes(bytes_per_sec.round().max(0.0) as u64))
}

/// A stream that adds every byte read from it to a [`ByteCounter`].
#[derive(Debug)]
pub struct CountingStream<S> {
    inner: S,
    received: ByteCounter,
}

impl<S> CountingStream<S> {
    pub fn new(inner: S, received: ByteCounter) -> Self {
        Self { inner, received }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for CountingStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &poll {
            self.received.add((buf.filled().len() - before) as u64);
        }
        poll
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for CountingStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn first_sample_has_no_rate() {
        let start = Instant::now();
        let rate = RateWindow::default().sample(start, 500);
        assert_eq!(rate, TrafficRate { total_bytes: 500, bytes_per_sec: 0.0 });
    }

    #[test]
    fn rate_averages_across_the_window() {
        let start = Instant::now();
        let mut window = RateWindow::default();
        window.sample(start, 0);
        window.sample(at(start, 1_000), 1_000);
        let rate = window.sample(at(start, 2_000), 4_000);
        assert_eq!(rate, TrafficRate { total_bytes: 4_000, bytes_per_sec: 2_000.0 });
    }

    #[test]
    fn samples_older_than_the_window_drop_out() {
        let start = Instant::now();
        let mut window = RateWindow::default();
        window.sample(start, 0);
        window.sample(at(start, 5_000), 100_000);
        // At 12s the 0s sample is outside the 10s window; the rate runs from the 5s sample.
        let rate = window.sample(at(start, 12_000), 100_000);
        assert_eq!(rate.bytes_per_sec, 0.0);
        assert_eq!(rate.total_bytes, 100_000);
    }

    #[test]
    fn a_sample_exactly_at_the_window_edge_is_kept() {
        let start = Instant::now();
        let mut window = RateWindow::default();
        window.sample(start, 0);
        let rate = window.sample(at(start, 10_000), 10_000);
        assert_eq!(rate.bytes_per_sec, 1_000.0);
    }

    #[test]
    fn formats_like_electron() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
        assert_eq!(format_rate(1023.6), "1.0 KB/s");
        assert_eq!(format_rate(12.4), "12 B/s");
    }

    #[tokio::test]
    async fn counting_stream_counts_bytes_read_not_written() {
        let (ours, mut theirs) = tokio::io::duplex(64);
        let counter = ByteCounter::default();
        let mut stream = CountingStream::new(ours, counter.clone());
        stream.write_all(b"outgoing").await.unwrap();
        theirs.write_all(b"hello").await.unwrap();
        let mut buf = [0u8; 5];
        stream.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");
        assert_eq!(counter.total(), 5);
    }
}
