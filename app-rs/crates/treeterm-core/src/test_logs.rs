//! What a test logs, for tests that check a log line Electron printed to its console.
//!
//! [`capture`] collects what the calling thread logs, so tests running in parallel never see
//! each other's lines. Only events on that thread are captured: those of a `#[tokio::test]`
//! (a current-thread runtime) and of GPUI's test executor qualify, those of another runtime's
//! worker threads do not.
//!
//! One subscriber, set for the whole test binary by the first capture, formats the events of
//! the threads that capture and drops the others'. A subscriber per test (`set_default`) would
//! miss events: tracing caches whether a call site is enabled, and while only one such
//! subscriber exists, a call site another thread reaches first is cached as disabled.

use std::cell::RefCell;
use std::io;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::{Arc, Mutex, Once, PoisonError};

use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt as _;

thread_local! {
    /// Where this thread's lines go while it captures.
    static CAPTURE: RefCell<Option<Buffer>> = const { RefCell::new(None) };
}

/// The lines logged on this thread at any level while it lives, as the app prints them
/// (without colours).
#[must_use = "dropping it stops the capture"]
pub struct Captured {
    buffer: Buffer,
    /// Captures this thread's lines: it stays on it.
    _thread: PhantomData<Rc<()>>,
}

impl Captured {
    /// Everything logged so far.
    pub fn text(&self) -> String {
        let bytes = self.buffer.0.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// The first line logged so far that contains `needle`, with its level and target.
    pub fn line(&self, needle: &str) -> Option<String> {
        self.text()
            .lines()
            .find(|line| line.contains(needle))
            .map(str::to_owned)
    }
}

impl Drop for Captured {
    fn drop(&mut self) {
        CAPTURE.with(|capture| *capture.borrow_mut() = None);
    }
}

/// Where the current thread's lines go: none unless it captures. A thread logging while its
/// thread-locals are being destroyed does not capture any more.
fn thread_capture() -> Option<Buffer> {
    CAPTURE
        .try_with(|capture| capture.borrow().clone())
        .unwrap_or_default()
}

/// Capture what this thread logs until the result is dropped.
///
/// Panics if this thread already captures, or if something else set the test binary's
/// subscriber.
pub fn capture() -> Captured {
    static SUBSCRIBER: Once = Once::new();
    SUBSCRIBER.call_once(|| {
        let format = tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(ThreadWriter);
        // A filter for the whole subscriber, not the layer's own, so that `tracing::enabled!`
        // also says no on a thread that does not capture.
        let capturing =
            tracing_subscriber::filter::dynamic_filter_fn(|_, _| thread_capture().is_some());
        let subscriber = tracing_subscriber::registry().with(format).with(capturing);
        tracing::subscriber::set_global_default(subscriber)
            .expect("only test_logs sets the test binary's subscriber");
    });
    let buffer = Buffer::default();
    let previous = CAPTURE.with(|capture| capture.borrow_mut().replace(buffer.clone()));
    assert!(previous.is_none(), "this thread already captures its logs");
    Captured {
        buffer,
        _thread: PhantomData,
    }
}

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl io::Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The buffer of the thread an event is logged on; an event only gets here while it captures.
struct ThreadWriter;

impl MakeWriter<'_> for ThreadWriter {
    type Writer = Box<dyn io::Write>;

    fn make_writer(&self) -> Self::Writer {
        match thread_capture() {
            Some(buffer) => Box::new(buffer),
            None => Box::new(io::sink()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_this_threads_lines_at_every_level_while_it_lives() {
        let logs = capture();
        tracing::trace!(answer = 42, "a trace line");
        tracing::info!("an info line");
        std::thread::spawn(|| tracing::info!("another thread's line"))
            .join()
            .unwrap();
        let text = logs.text();
        let trace = logs.line("a trace line").expect(&text);
        assert!(
            trace.contains("TRACE") && trace.ends_with("a trace line answer=42"),
            "{trace}"
        );
        assert!(
            logs.line("an info line").expect(&text).contains(" INFO "),
            "{text}"
        );
        assert_eq!(logs.line("another thread's line"), None, "{text}");
        assert!(!text.contains('\u{1b}'), "no colours: {text}");
        assert!(tracing::enabled!(tracing::Level::TRACE));
        let elsewhere = std::thread::spawn(|| tracing::enabled!(tracing::Level::ERROR));
        assert!(
            !elsewhere.join().unwrap(),
            "nothing logs another thread's lines"
        );

        drop(logs);
        tracing::info!("a line after");
        let again = capture();
        tracing::info!("a line again");
        assert_eq!(again.line("a line after"), None);
        assert!(again.line("a line again").is_some(), "{}", again.text());
    }

    /// Another thread reaching a call site first, while this one captures, does not hide it.
    #[test]
    fn a_call_site_another_thread_reaches_first_is_captured() {
        fn log() {
            tracing::info!("a shared call site");
        }
        let logs = capture();
        std::thread::spawn(log).join().unwrap();
        log();
        assert!(logs.line("a shared call site").is_some(), "{}", logs.text());
    }
}
