//! The app's panic hook (`logging::install_panic_hook`), in a test binary of its own: the
//! hook is the whole process's.

#[path = "../src/test_logs.rs"]
mod test_logs;

use std::sync::atomic::{AtomicUsize, Ordering};

use treeterm_core::logging::install_panic_hook;

/// Calls of the hook that was set before the app's.
static EARLIER_HOOK: AtomicUsize = AtomicUsize::new(0);

#[test]
fn a_panic_is_logged_and_one_the_log_would_not_show_goes_to_the_earlier_hook() {
    std::panic::set_hook(Box::new(|info| {
        EARLIER_HOOK.fetch_add(1, Ordering::SeqCst);
        eprintln!("{info}");
    }));
    install_panic_hook();
    let logs = test_logs::capture();

    let caught = std::panic::catch_unwind(|| panic!("boom"));
    let not_a_string = std::panic::catch_unwind(|| std::panic::panic_any(5));
    // Another thread's log is not captured, so nothing would show its panic.
    let elsewhere = std::thread::spawn(|| panic!("elsewhere")).join();
    let text = logs.text();
    // A failed assertion's panic is printed (by the earlier hook) from here on.
    drop(logs);

    assert!(caught.is_err() && not_a_string.is_err() && elsewhere.is_err());
    let line = text
        .lines()
        .find(|line| line.contains(" panicked at "))
        .expect(&text);
    assert!(line.contains(" ERROR panic: thread '"), "{line}");
    assert!(
        line.contains(&format!("' panicked at {}:", file!())),
        "{line}"
    );
    assert!(text.contains(":\nboom\n"), "{text}");
    assert!(text.contains(":\nBox<dyn Any>\n"), "{text}");
    assert!(!text.contains("elsewhere"), "{text}");
    assert_eq!(EARLIER_HOOK.load(Ordering::SeqCst), 1, "only elsewhere's");
}
