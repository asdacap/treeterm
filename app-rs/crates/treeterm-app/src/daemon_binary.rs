//! Where the bundled daemon executable lives.

use std::path::PathBuf;

const DAEMON_DIR_ENV: &str = "TREETERM_DAEMON_DIR";
const DAEMON_BINARY: &str = "treeterm-daemon";

/// Candidate locations, most specific first: `$TREETERM_DAEMON_DIR`, next to the
/// executable (Linux bundles), `Contents/Resources/daemon-rs/` (macOS .app), then — in
/// debug builds only — the repo's `out/daemon-rs/` from `npm run build:daemon-rs`.
pub fn candidates() -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Ok(dir) = std::env::var(DAEMON_DIR_ENV) {
        found.push(PathBuf::from(dir).join(DAEMON_BINARY));
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
    {
        found.push(exe_dir.join(DAEMON_BINARY));
        found.push(exe_dir.join("../Resources/daemon-rs").join(DAEMON_BINARY));
    }
    if cfg!(debug_assertions) {
        found.push(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../out/daemon-rs")
                .join(DAEMON_BINARY),
        );
    }
    found
}

/// The first candidate that exists, else the first candidate (so a missing daemon is
/// reported against the location that was expected to hold it).
pub fn locate() -> PathBuf {
    let all = candidates();
    all.iter()
        .find(|path| path.exists())
        .unwrap_or_else(|| all.first().expect("at least the executable-relative candidate"))
        .clone()
}
