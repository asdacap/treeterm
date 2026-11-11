//! `check-linux`: type-check and lint the whole workspace for Linux from any host.
//!
//! The Linux GUI stack's `-sys` crates ask pkg-config for system libraries (fontconfig, gtk,
//! webkitgtk…) in their build scripts. Nothing is linked by `cargo check`/`clippy`, so a stub
//! pkg-config that reports every library present, with no flags, is enough to get the Rust
//! code checked under Linux `cfg`s. It catches compile errors, not runtime behaviour.

use std::fs;
use std::process::Command;

use crate::{Context, Repo, Result, exec, step};

/// musl, because its Rust target is the one the remote daemons already need.
pub const TARGET: &str = "x86_64-unknown-linux-musl";

const STUB: &str = r#"#!/bin/sh
# Written by `cargo xtask check-linux`: every library exists, at a high version, with no
# flags. Only valid for checking; nothing is linked.
for arg in "$@"; do
  case "$arg" in
    --modversion) echo "99.0"; exit 0 ;;
    --variable=*|--print-variable*) echo ""; exit 0 ;;
  esac
done
exit 0
"#;

pub fn check(repo: &Repo) -> Result<()> {
    let dir = repo.app_rs().join("target/xtask");
    fs::create_dir_all(&dir).context(|| format!("create {}", dir.display()))?;
    let stub = dir.join("pkg-config-stub");
    fs::write(&stub, STUB).context(|| format!("write {}", stub.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755))
            .context(|| format!("make {} executable", stub.display()))?;
    }
    step(format!("clippy for {TARGET} (needs `rustup target add {TARGET}`)"));
    exec(
        Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()))
            .current_dir(repo.app_rs())
            .args(["clippy", "--workspace", "--all-targets", "--features", "treeterm-app/automation"])
            .args(["--target", TARGET])
            .env("PKG_CONFIG", &stub)
            .env("PKG_CONFIG_ALLOW_CROSS", "1")
            .env("RUST_FONTCONFIG_DLOPEN", "1"),
    )
}
