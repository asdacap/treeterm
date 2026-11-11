//! Where the bundled daemon executable lives. The layouts are produced by
//! `cargo run -p xtask -- bundle` (`app-rs/xtask`): the macOS `.app` keeps the daemons in
//! `Contents/Resources/daemon-rs/`, the Linux tarball (and the Nix package) next to `treeterm`.

use std::path::{Path, PathBuf};

const DAEMON_DIR_ENV: &str = "TREETERM_DAEMON_DIR";
const DAEMON_BINARY: &str = "treeterm-daemon";

/// What the candidates are derived from: the process environment, or a test's.
struct Sources {
    /// `$TREETERM_DAEMON_DIR` (development).
    daemon_dir: Option<PathBuf>,
    /// The running executable, symlinks resolved.
    exe: Option<PathBuf>,
    /// The repo's `out/daemon-rs/` (debug builds only).
    dev_dir: Option<PathBuf>,
}

impl Sources {
    fn of_process() -> Self {
        let exe = match std::env::current_exe() {
            Ok(exe) => Some(resolve_links(exe)),
            Err(error) => {
                tracing::warn!(%error, "cannot tell where the executable is; not looking for a bundled daemon");
                None
            }
        };
        Self {
            daemon_dir: std::env::var_os(DAEMON_DIR_ENV).map(PathBuf::from),
            exe,
            dev_dir: cfg!(debug_assertions)
                .then(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../out/daemon-rs")),
        }
    }
}

/// macOS reports the executable by the path it was started with, so a `treeterm` symlinked
/// onto PATH would look for daemons beside the link instead of inside the bundle.
fn resolve_links(exe: PathBuf) -> PathBuf {
    // Canonicalizing fails only if the file is gone; the reported path is the best guess.
    std::fs::canonicalize(&exe).unwrap_or(exe)
}

/// Candidate locations, most specific first: `$TREETERM_DAEMON_DIR`, next to the
/// executable (Linux tarball, Nix), `Contents/Resources/daemon-rs/` (macOS .app), then the
/// repo's `out/daemon-rs/` (`cargo run -p xtask -- build-daemons`, `npm run build:daemon-rs`).
fn candidates(sources: Sources) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    dirs.extend(sources.daemon_dir);
    if let Some(exe_dir) = sources.exe.as_deref().and_then(Path::parent) {
        dirs.push(exe_dir.to_owned());
        if let Some(contents) = exe_dir.parent() {
            dirs.push(contents.join("Resources/daemon-rs"));
        }
    }
    dirs.extend(sources.dev_dir);
    dirs.into_iter()
        .map(|dir| dir.join(DAEMON_BINARY))
        .collect()
}

/// The first candidate that exists, else the first candidate (so a missing daemon is
/// reported against the location that was expected to hold it).
fn first_existing(all: Vec<PathBuf>) -> PathBuf {
    let expected = all.first().cloned();
    all.into_iter()
        .find(|path| path.exists())
        .or(expected)
        .unwrap_or_else(|| PathBuf::from(DAEMON_BINARY))
}

pub fn locate() -> PathBuf {
    first_existing(candidates(Sources::of_process()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fresh directory under the system temp dir, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "treeterm-daemon-binary-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&dir).unwrap();
            // Canonical, so paths compare equal to resolved ones (macOS: /var → /private/var).
            Self(fs::canonicalize(dir).unwrap())
        }

        fn touch(&self, relative: &str) -> PathBuf {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"bin").unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            // Not a panic: this may run while a failed assertion unwinds.
            if let Err(error) = fs::remove_dir_all(&self.0) {
                eprintln!("leaving {}: {error}", self.0.display());
            }
        }
    }

    fn installed_at(exe: PathBuf) -> Sources {
        Sources {
            daemon_dir: None,
            exe: Some(exe),
            dev_dir: None,
        }
    }

    #[test]
    fn finds_the_daemon_in_the_macos_app_resources() {
        let root = Scratch::new();
        let exe = root.touch("TreeTerm.app/Contents/MacOS/treeterm");
        let daemon = root.touch("TreeTerm.app/Contents/Resources/daemon-rs/treeterm-daemon");
        // The remote (musl) daemons sit beside it; they are not the local daemon.
        root.touch("TreeTerm.app/Contents/Resources/daemon-rs/treeterm-daemon-x86_64-linux");

        assert_eq!(first_existing(candidates(installed_at(exe))), daemon);
    }

    #[test]
    fn finds_the_daemon_next_to_the_executable_in_the_linux_tarball() {
        let root = Scratch::new();
        let exe = root.touch("treeterm-0.1.0-x86_64-linux/treeterm");
        let daemon = root.touch("treeterm-0.1.0-x86_64-linux/treeterm-daemon");

        assert_eq!(first_existing(candidates(installed_at(exe))), daemon);
    }

    #[test]
    fn the_daemon_dir_override_wins_over_the_bundle() {
        let root = Scratch::new();
        let exe = root.touch("bundle/treeterm");
        root.touch("bundle/treeterm-daemon");
        let dev = root.touch("dev/treeterm-daemon");
        let sources = Sources {
            daemon_dir: Some(root.0.join("dev")),
            ..installed_at(exe)
        };

        assert_eq!(first_existing(candidates(sources)), dev);
    }

    #[test]
    fn falls_back_to_the_repo_build_output_in_development() {
        let root = Scratch::new();
        let exe = root.touch("app-rs/target/debug/treeterm");
        let daemon = root.touch("out/daemon-rs/treeterm-daemon");
        let sources = Sources {
            dev_dir: Some(root.0.join("out/daemon-rs")),
            ..installed_at(exe)
        };

        assert_eq!(first_existing(candidates(sources)), daemon);
    }

    #[test]
    fn reports_a_missing_daemon_at_the_most_specific_location() {
        let root = Scratch::new();
        let exe = root.0.join("TreeTerm.app/Contents/MacOS/treeterm");
        assert_eq!(
            first_existing(candidates(installed_at(exe.clone()))),
            exe.with_file_name("treeterm-daemon")
        );

        let sources = Sources {
            daemon_dir: Some(root.0.join("dev")),
            ..installed_at(exe)
        };
        assert_eq!(
            first_existing(candidates(sources)),
            root.0.join("dev/treeterm-daemon")
        );
    }

    #[test]
    fn with_nothing_to_go_on_names_the_bare_binary() {
        let sources = Sources {
            daemon_dir: None,
            exe: None,
            dev_dir: None,
        };
        assert_eq!(
            first_existing(candidates(sources)),
            PathBuf::from("treeterm-daemon")
        );
    }

    #[test]
    fn a_symlinked_executable_finds_the_daemon_in_its_bundle() {
        let root = Scratch::new();
        let exe = root.touch("TreeTerm.app/Contents/MacOS/treeterm");
        let daemon = root.touch("TreeTerm.app/Contents/Resources/daemon-rs/treeterm-daemon");
        let link = root.0.join("bin/treeterm");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&exe, &link).unwrap();

        assert_eq!(resolve_links(link.clone()), exe);
        assert_eq!(
            first_existing(candidates(installed_at(resolve_links(link)))),
            daemon
        );
    }

    #[test]
    fn a_vanished_executable_keeps_its_reported_path() {
        let root = Scratch::new();
        let gone = root.0.join("treeterm");
        assert_eq!(resolve_links(gone.clone()), gone);
    }
}
