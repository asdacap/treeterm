//! Download metering (`createTrafficMeterStore.ts`): byte counters fed by the gRPC messages
//! received from the daemon and by PTY streams, and the rolling rate the traffic badges show.

use std::collections::{HashMap, VecDeque};
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

/// `formatBytes.ts`. Its `toFixed(1)` rounds an exact half up (1280 B is "1.3 KB") where
/// `{:.1}` rounds it to even, so the tenths are rounded in integers: a byte count over a
/// power of 1024 is exact, as it is in JavaScript.
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    let (unit, name) = match bytes {
        b if b < KB => return format!("{b} B"),
        b if b < MB => (KB, "KB"),
        b if b < GB => (MB, "MB"),
        _ => (GB, "GB"),
    };
    let unit = u128::from(unit);
    let tenths = (u128::from(bytes) * 20 + unit) / (2 * unit);
    format!("{}.{} {name}", tenths / 10, tenths % 10)
}

/// A rate is an average, so whole bytes are rounded before formatting.
pub fn format_rate(bytes_per_sec: f64) -> String {
    format!("{}/s", format_bytes(bytes_per_sec.round().max(0.0) as u64))
}

/// RFC 9113 §4.1: a frame header is a 24-bit length, a type, flags and a 31-bit stream id.
const FRAME_HEADER_LEN: usize = 9;
const FRAME_DATA: u8 = 0x0;
const FRAME_HEADERS: u8 = 0x1;
const FRAME_RST_STREAM: u8 = 0x3;
/// On DATA and HEADERS frames.
const FLAG_END_STREAM: u8 = 0x1;
/// On DATA frames: a pad length byte leads the payload, and that much padding ends it.
const FLAG_PADDED: u8 = 0x8;
/// gRPC's message prefix: a compressed flag and the message's big-endian u32 length.
const MESSAGE_PREFIX_LEN: usize = 5;

/// Sizes the gRPC messages in what a client reads from its HTTP/2 connection to a server,
/// as grpc-js's `responseDeserialize` sees them (`grpcByteCounter.ts`): each message once it
/// is complete, without its prefix, the frames around it, padding, or any other frame
/// (HEADERS and trailers, SETTINGS, PING, WINDOW_UPDATE). A read may end anywhere. A server
/// sends no connection preface, so the first byte is a frame header.
#[derive(Debug, Default)]
pub struct GrpcMessageMeter {
    frame: FrameState,
    /// Streams in the middle of a message; a stream between messages has no entry.
    messages: HashMap<u32, MessageState>,
}

#[derive(Debug)]
enum FrameState {
    /// `have` bytes of a frame header read.
    Header {
        header: [u8; FRAME_HEADER_LEN],
        have: usize,
    },
    /// A padded DATA frame's pad length is next, and `len` bytes of the frame after it.
    PadLength { frame: DataFrame, len: usize },
    /// In a DATA frame: `data` bytes of the stream's messages are left, then `padding`.
    Data {
        frame: DataFrame,
        data: usize,
        padding: usize,
    },
    /// In any other frame, `left` bytes from its end.
    Skip { left: usize },
}

impl Default for FrameState {
    fn default() -> Self {
        Self::Header {
            header: [0; FRAME_HEADER_LEN],
            have: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct DataFrame {
    stream: u32,
    end_stream: bool,
}

#[derive(Debug)]
enum MessageState {
    /// `have` bytes of a message prefix read.
    Prefix {
        prefix: [u8; MESSAGE_PREFIX_LEN],
        have: usize,
    },
    /// In a message of `len` bytes, `left` bytes from its end.
    Body { len: usize, left: usize },
}

impl Default for MessageState {
    fn default() -> Self {
        Self::Prefix {
            prefix: [0; MESSAGE_PREFIX_LEN],
            have: 0,
        }
    }
}

impl MessageState {
    fn is_between_messages(&self) -> bool {
        matches!(self, Self::Prefix { have: 0, .. })
    }
}

impl GrpcMessageMeter {
    /// Read the next `bytes` from the server; returns the size of the messages they complete.
    pub fn feed(&mut self, mut bytes: &[u8]) -> u64 {
        let mut completed = 0;
        while !bytes.is_empty() {
            let (next, used) = match std::mem::take(&mut self.frame) {
                FrameState::Header { mut header, have } => {
                    let used = (FRAME_HEADER_LEN - have).min(bytes.len());
                    header[have..have + used].copy_from_slice(&bytes[..used]);
                    let have = have + used;
                    let next = if have == FRAME_HEADER_LEN {
                        self.start_frame(header)
                    } else {
                        FrameState::Header { header, have }
                    };
                    (next, used)
                }
                FrameState::PadLength { frame, len } => {
                    let padding = usize::from(bytes[0]).min(len);
                    (self.in_data(frame, len - padding, padding), 1)
                }
                FrameState::Data {
                    frame,
                    data,
                    padding,
                } if data > 0 => {
                    let used = data.min(bytes.len());
                    completed += self.feed_stream(frame.stream, &bytes[..used]);
                    (self.in_data(frame, data - used, padding), used)
                }
                FrameState::Data {
                    frame,
                    data,
                    padding,
                } => {
                    let used = padding.min(bytes.len());
                    (self.in_data(frame, data, padding - used), used)
                }
                FrameState::Skip { left } => {
                    let used = left.min(bytes.len());
                    (skipping(left - used), used)
                }
            };
            self.frame = next;
            bytes = &bytes[used..];
        }
        completed
    }

    fn start_frame(&mut self, header: [u8; FRAME_HEADER_LEN]) -> FrameState {
        let len = usize::from(header[0]) << 16 | usize::from(header[1]) << 8;
        let len = len | usize::from(header[2]);
        let (kind, flags) = (header[3], header[4]);
        let stream = u32::from_be_bytes([header[5], header[6], header[7], header[8]]) & 0x7fff_ffff;
        let end_stream = flags & FLAG_END_STREAM != 0;
        match kind {
            FRAME_DATA => {
                let frame = DataFrame { stream, end_stream };
                if flags & FLAG_PADDED != 0 && len > 0 {
                    FrameState::PadLength {
                        frame,
                        len: len - 1,
                    }
                } else {
                    self.in_data(frame, len, 0)
                }
            }
            FRAME_HEADERS if end_stream => {
                self.messages.remove(&stream);
                skipping(len)
            }
            FRAME_RST_STREAM => {
                self.messages.remove(&stream);
                skipping(len)
            }
            _ => skipping(len),
        }
    }

    /// Where a DATA frame with `data` and then `padding` bytes left is: at the next frame
    /// once nothing is left, with the stream forgotten if the frame ended it.
    fn in_data(&mut self, frame: DataFrame, data: usize, padding: usize) -> FrameState {
        if data > 0 || padding > 0 {
            return FrameState::Data {
                frame,
                data,
                padding,
            };
        }
        if frame.end_stream {
            self.messages.remove(&frame.stream);
        }
        FrameState::default()
    }

    /// Read the next `bytes` of `stream`'s messages; returns the size of those they complete.
    fn feed_stream(&mut self, stream: u32, mut bytes: &[u8]) -> u64 {
        let mut completed = 0;
        let mut message = self.messages.remove(&stream).unwrap_or_default();
        while !bytes.is_empty() {
            let used;
            (message, used) = match message {
                MessageState::Prefix { mut prefix, have } => {
                    let used = (MESSAGE_PREFIX_LEN - have).min(bytes.len());
                    prefix[have..have + used].copy_from_slice(&bytes[..used]);
                    let have = have + used;
                    if have < MESSAGE_PREFIX_LEN {
                        (MessageState::Prefix { prefix, have }, used)
                    } else {
                        let len = u32::from_be_bytes([prefix[1], prefix[2], prefix[3], prefix[4]]);
                        (body(len as usize), used)
                    }
                }
                MessageState::Body { len, left } => {
                    let used = left.min(bytes.len());
                    if used == left {
                        completed += len as u64;
                        (MessageState::default(), used)
                    } else {
                        let left = left - used;
                        (MessageState::Body { len, left }, used)
                    }
                }
            };
            bytes = &bytes[used..];
        }
        if !message.is_between_messages() {
            self.messages.insert(stream, message);
        }
        completed
    }
}

/// In a frame, `left` bytes from its end: at the next frame if that is none.
fn skipping(left: usize) -> FrameState {
    if left == 0 {
        FrameState::default()
    } else {
        FrameState::Skip { left }
    }
}

/// A message of `len` bytes is next. An empty one is complete already, and counts nothing.
fn body(len: usize) -> MessageState {
    if len == 0 {
        MessageState::default()
    } else {
        MessageState::Body { len, left: len }
    }
}

/// The client end of a gRPC connection, which adds the size of every message read from it to
/// a [`ByteCounter`] (see [`GrpcMessageMeter`]). What is read passes through unchanged.
#[derive(Debug)]
pub struct CountingStream<S> {
    inner: S,
    meter: GrpcMessageMeter,
    received: ByteCounter,
}

impl<S> CountingStream<S> {
    pub fn new(inner: S, received: ByteCounter) -> Self {
        Self {
            inner,
            meter: GrpcMessageMeter::default(),
            received,
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for CountingStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let poll = Pin::new(&mut this.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &poll {
            this.received.add(this.meter.feed(&buf.filled()[before..]));
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

    #[test]
    fn an_exact_half_rounds_up_as_to_fixed_does() {
        // Each checked against `formatBytes` in node.
        assert_eq!(format_bytes(1229), "1.2 KB");
        assert_eq!(format_bytes(1280), "1.3 KB");
        assert_eq!(format_bytes(2304), "2.3 KB");
        assert_eq!(format_bytes(1_048_524), "1023.9 KB");
        assert_eq!(format_bytes(1_048_575), "1024.0 KB");
        assert_eq!(format_bytes(1_310_720), "1.3 MB");
        assert_eq!(format_bytes(1_073_741_823), "1024.0 MB");
        assert_eq!(format_bytes(1_342_177_280), "1.3 GB");
        assert_eq!(format_bytes(3_758_096_384), "3.5 GB");
        assert_eq!(format_bytes(5_497_826_574_336), "5120.3 GB");
        assert_eq!(format_rate(1279.5), "1.3 KB/s");
    }

    const SETTINGS: u8 = 0x4;
    const PING: u8 = 0x6;
    const WINDOW_UPDATE: u8 = 0x8;
    const END_HEADERS: u8 = 0x4;
    const ACK: u8 = 0x1;
    const TRAILERS: u8 = END_HEADERS | FLAG_END_STREAM;

    fn frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
        let len = u32::try_from(payload.len()).unwrap();
        let mut out = len.to_be_bytes()[1..].to_vec();
        out.extend([kind, flags]);
        out.extend(stream.to_be_bytes());
        out.extend(payload);
        out
    }

    /// A gRPC message of `len` bytes, with its prefix.
    fn message(len: usize) -> Vec<u8> {
        let mut out = vec![0];
        out.extend(u32::try_from(len).unwrap().to_be_bytes());
        out.extend(std::iter::repeat_n(b'm', len));
        out
    }

    fn padded(pad: u8, data: &[u8]) -> Vec<u8> {
        let mut out = vec![pad];
        out.extend(data);
        out.extend(std::iter::repeat_n(0, usize::from(pad)));
        out
    }

    /// What a server might send on one connection, and the size of the messages in it.
    fn server_bytes() -> (Vec<u8>, u64) {
        let mut out = Vec::new();
        // The connection's settings, and the ack of the client's.
        out.extend(frame(SETTINGS, 0, 0, &[0, 4, 0, 0x10, 0, 0]));
        out.extend(frame(SETTINGS, ACK, 0, &[]));
        // Stream 1: response headers, then two messages in one DATA frame.
        out.extend(frame(FRAME_HEADERS, END_HEADERS, 1, b"response headers"));
        let two = [message(10), message(300)].concat();
        out.extend(frame(FRAME_DATA, 0, 1, &two));
        // Stream 3 starts a 50-byte message whose prefix and body span three frames, with
        // stream 1's frames in between.
        let fifty = message(50);
        out.extend(frame(FRAME_HEADERS, END_HEADERS, 3, b"headers"));
        out.extend(frame(FRAME_DATA, 0, 3, &fifty[..3]));
        out.extend(frame(FRAME_DATA, FLAG_PADDED, 1, &padded(7, &message(20))));
        out.extend(frame(FRAME_DATA, 0, 3, &fifty[3..30]));
        out.extend(frame(PING, 0, 0, &[1; 8]));
        out.extend(frame(WINDOW_UPDATE, 0, 0, &[0, 1, 0, 0]));
        out.extend(frame(FRAME_DATA, 0, 3, &fifty[30..]));
        // Trailers end stream 1; stream 3 sends an empty message and ends with its DATA.
        out.extend(frame(FRAME_HEADERS, TRAILERS, 1, b"grpc-status: 0"));
        out.extend(frame(FRAME_DATA, FLAG_END_STREAM, 3, &message(0)));
        // A padded frame without data, and one with padding only.
        out.extend(frame(FRAME_DATA, FLAG_PADDED, 5, &padded(0, &[])));
        out.extend(frame(FRAME_DATA, FLAG_PADDED, 5, &padded(4, &[])));
        // Stream 5 is reset in the middle of a message, which never counts.
        out.extend(frame(FRAME_DATA, 0, 5, &message(100)[..40]));
        out.extend(frame(FRAME_RST_STREAM, 0, 5, &[0, 0, 0, 8]));
        // Stream 7 is cut off by trailers in the middle of a message.
        out.extend(frame(FRAME_DATA, 0, 7, &message(100)[..2]));
        out.extend(frame(FRAME_HEADERS, TRAILERS, 7, b"grpc-status: 13"));
        (out, 10 + 300 + 20 + 50)
    }

    #[test]
    fn the_meter_counts_message_bytes_however_reads_split_them() {
        let (bytes, expected) = server_bytes();
        for size in 1..=bytes.len() {
            let mut meter = GrpcMessageMeter::default();
            let counted: u64 = bytes.chunks(size).map(|chunk| meter.feed(chunk)).sum();
            assert_eq!(counted, expected, "reads of {size} bytes");
            assert!(meter.messages.is_empty(), "{size}-byte reads");
            assert!(matches!(meter.frame, FrameState::Header { have: 0, .. }));
        }
    }

    #[test]
    fn a_message_counts_once_it_is_complete() {
        let mut bytes = frame(FRAME_DATA, 0, 1, &message(1000));
        bytes.extend(frame(FRAME_DATA, 0, 1, &message(24)));
        let mut meter = GrpcMessageMeter::default();
        // The frame header, the prefix and all but the last byte of the first message.
        assert_eq!(meter.feed(&bytes[..9 + 5 + 999]), 0);
        assert_eq!(meter.feed(&bytes[9 + 5 + 999..9 + 5 + 1000]), 1000);
        assert_eq!(meter.feed(&bytes[9 + 5 + 1000..]), 24);
    }

    #[tokio::test]
    async fn counting_stream_counts_messages_read_not_written() {
        let (ours, mut theirs) = tokio::io::duplex(1024);
        let counter = ByteCounter::default();
        let mut stream = CountingStream::new(ours, counter.clone());
        let request = frame(FRAME_DATA, 0, 1, &message(8));
        stream.write_all(&request).await.unwrap();
        let mut sent = frame(SETTINGS, 0, 0, &[]);
        sent.extend(frame(FRAME_HEADERS, END_HEADERS, 1, b"headers"));
        sent.extend(frame(FRAME_DATA, 0, 1, &[message(5), message(12)].concat()));
        theirs.write_all(&sent).await.unwrap();
        let mut read = vec![0; sent.len()];
        stream.read_exact(&mut read).await.unwrap();
        assert_eq!(read, sent, "bytes pass through unchanged");
        assert_eq!(counter.total(), 5 + 12);

        let mut written = vec![0; 9 + 5 + 8];
        theirs.read_exact(&mut written).await.unwrap();
        assert_eq!(counter.total(), 5 + 12, "writes don't count");
    }
}
