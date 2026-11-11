//! The daemons a bundle ships: the local one for this host (started by the app) and static
//! musl ones for remote Linux hosts (uploaded over ssh by treeterm-core's `DaemonBinaries`,
//! which look for `treeterm-daemon-<arch>-linux`). Built from `daemon-rs/` exactly like
//! `npm run build:daemon-rs` and `build:daemon-rs:linux-*`, into the same `out/daemon-rs/`.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::{Context, Repo, Result, cargo, install_file, step, warn};

/// The local daemon's file name (`treeterm-app/src/daemon_binary.rs`).
pub const LOCAL_DAEMON: &str = "treeterm-daemon";

/// A remote Linux platform the bundle carries a static daemon for.
pub struct RemoteTarget {
    /// `uname -m`, normalized as treeterm-core's `normalize_arch` does.
    pub arch: &'static str,
    triple: &'static str,
    /// The linker `daemon-rs/.cargo/config.toml` names for `triple`.
    default_linker: &'static str,
}

pub const REMOTE_TARGETS: [RemoteTarget; 2] = [
    RemoteTarget {
        arch: "x86_64",
        triple: "x86_64-unknown-linux-musl",
        default_linker: "x86_64-linux-musl-gcc",
    },
    RemoteTarget {
        arch: "aarch64",
        triple: "aarch64-unknown-linux-musl",
        default_linker: "aarch64-linux-musl-gcc",
    },
];

impl RemoteTarget {
    /// What treeterm-core's `find_daemon_binary` looks for: `treeterm-daemon-<arch>-<os>`.
    pub fn binary_name(&self) -> String {
        format!("{LOCAL_DAEMON}-{}-linux", self.arch)
    }

    /// Cargo's per-target linker override, which wins over `.cargo/config.toml`.
    fn linker_var(&self) -> String {
        format!(
            "CARGO_TARGET_{}_LINKER",
            self.triple.to_uppercase().replace('-', "_")
        )
    }

    fn linker(&self) -> OsString {
        std::env::var_os(self.linker_var()).unwrap_or_else(|| self.default_linker.into())
    }
}

/// `program` run in `daemon-rs/` with the daemon's own toolchain, as npm runs it: `cargo run`
/// from `app-rs/` exports app-rs's pinned toolchain as `RUSTUP_TOOLCHAIN`, which would win.
fn in_daemon_rs(repo: &Repo, program: &str) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(repo.daemon_rs())
        .env_remove("RUSTUP_TOOLCHAIN");
    command
}

fn daemon_build(repo: &Repo) -> Command {
    let mut command = in_daemon_rs(repo, "cargo");
    command.args(["build", "--release", "--bin", LOCAL_DAEMON]);
    command
}

pub fn build(repo: &Repo) -> Result<()> {
    let out = repo.daemon_out();
    fs::create_dir_all(&out).context(|| format!("create {}", out.display()))?;

    step("Building the local daemon");
    let built = cargo::build_executable(daemon_build(repo), LOCAL_DAEMON)?;
    install_file(&built, &out.join(LOCAL_DAEMON))?;

    let sysroot = sysroot(repo)?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut skipped = Vec::new();
    for target in &REMOTE_TARGETS {
        if let Some(reason) = missing_toolchain(target, &sysroot, &target.linker(), &path)? {
            warn(format!(
                "skipping the {} Linux daemon (remote hosts): {reason}",
                target.arch
            ));
            skipped.push(target.binary_name());
            continue;
        }
        step(format!(
            "Building the {} Linux daemon (static, musl)",
            target.arch
        ));
        let mut command = daemon_build(repo);
        command.args(["--target", target.triple]);
        let built = cargo::build_executable(command, LOCAL_DAEMON)?;
        install_file(&built, &out.join(target.binary_name()))?;
    }

    step(format!("Daemons are in {}", out.display()));
    if !skipped.is_empty() {
        warn(format!(
            "not built: {}. A bundle carries whatever {} already has of them (an older \
             build, or nothing: then it cannot set up those remote hosts)",
            skipped.join(", "),
            out.display()
        ));
    }
    Ok(())
}

/// The daemon toolchain's sysroot (installed targets, `rust-lld`).
fn sysroot(repo: &Repo) -> Result<std::path::PathBuf> {
    let mut command = in_daemon_rs(repo, "rustc");
    command.args(["--print", "sysroot"]);
    let output = command.output().context(|| format!("run {command:?}"))?;
    if !output.status.success() {
        return Err(format!("{command:?} failed ({})", output.status));
    }
    let sysroot = String::from_utf8(output.stdout).context(|| format!("read {command:?}"))?;
    Ok(sysroot.trim().into())
}

/// Why `target` cannot be built here, if it cannot: its standard library is not installed,
/// or `linker` is neither on `path` nor shipped with the toolchain (`rust-lld`).
fn missing_toolchain(
    target: &RemoteTarget,
    sysroot: &Path,
    linker: &OsStr,
    path: &OsStr,
) -> Result<Option<String>> {
    let rustlib = sysroot.join("lib/rustlib");
    if !rustlib.join(target.triple).join("lib").is_dir() {
        return Ok(Some(format!(
            "the Rust target {triple} is not installed for the daemon's toolchain \
             (run `rustup target add {triple}` in daemon-rs/)",
            triple = target.triple
        )));
    }
    let linker_path = Path::new(linker);
    let found = if linker_path.components().count() > 1 {
        linker_path.is_file()
    } else {
        let mut dirs: Vec<_> = std::env::split_paths(path).collect();
        for entry in fs::read_dir(&rustlib).context(|| format!("list {}", rustlib.display()))? {
            let entry = entry.context(|| format!("list {}", rustlib.display()))?;
            dirs.push(entry.path().join("bin"));
        }
        dirs.iter().any(|dir| dir.join(linker_path).is_file())
    };
    if found {
        return Ok(None);
    }
    Ok(Some(format!(
        "its linker `{}` was not found (install a musl cross toolchain, e.g. \
         `brew install filosottile/musl-cross/musl-cross` on macOS, or set {}=rust-lld)",
        linker_path.display(),
        target.linker_var()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn remote_binaries_are_named_as_find_daemon_binary_expects() {
        let names: Vec<_> = REMOTE_TARGETS
            .iter()
            .map(RemoteTarget::binary_name)
            .collect();
        assert_eq!(
            names,
            [
                "treeterm-daemon-x86_64-linux",
                "treeterm-daemon-aarch64-linux"
            ]
        );
        assert_eq!(
            REMOTE_TARGETS[1].linker_var(),
            "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER"
        );
    }

    fn executable(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A sysroot with the x86_64 musl target installed and `rust-lld` shipped.
    fn sysroot_with_musl() -> tempfile::TempDir {
        let sysroot = tempfile::tempdir().unwrap();
        let rustlib = sysroot.path().join("lib/rustlib");
        fs::create_dir_all(rustlib.join("x86_64-unknown-linux-musl/lib")).unwrap();
        executable(&rustlib.join("aarch64-apple-darwin/bin/rust-lld"));
        sysroot
    }

    #[test]
    fn skips_a_target_whose_standard_library_is_missing() {
        let sysroot = sysroot_with_musl();
        let reason = missing_toolchain(
            &REMOTE_TARGETS[1],
            sysroot.path(),
            "rust-lld".as_ref(),
            "".as_ref(),
        )
        .unwrap()
        .unwrap();
        assert!(
            reason.contains("rustup target add aarch64-unknown-linux-musl"),
            "{reason}"
        );
    }

    #[test]
    fn skips_a_target_whose_linker_is_missing() {
        let sysroot = sysroot_with_musl();
        let reason = missing_toolchain(
            &REMOTE_TARGETS[0],
            sysroot.path(),
            "x86_64-linux-musl-gcc".as_ref(),
            "/nonexistent".as_ref(),
        )
        .unwrap()
        .unwrap();
        assert!(
            reason.contains("`x86_64-linux-musl-gcc` was not found"),
            "{reason}"
        );
        assert!(
            reason.contains("CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld"),
            "{reason}"
        );
    }

    #[test]
    fn finds_the_linker_on_path_or_in_the_toolchain() {
        let sysroot = sysroot_with_musl();
        let bin = tempfile::tempdir().unwrap();
        executable(&bin.path().join("x86_64-linux-musl-gcc"));
        let path = std::env::join_paths([Path::new("/nonexistent"), bin.path()]).unwrap();
        let target = &REMOTE_TARGETS[0];

        let on_path = missing_toolchain(
            target,
            sysroot.path(),
            "x86_64-linux-musl-gcc".as_ref(),
            &path,
        );
        assert_eq!(on_path, Ok(None));
        let shipped = missing_toolchain(target, sysroot.path(), "rust-lld".as_ref(), "".as_ref());
        assert_eq!(shipped, Ok(None));
        let absolute = bin.path().join("x86_64-linux-musl-gcc");
        let by_path = missing_toolchain(target, sysroot.path(), absolute.as_os_str(), "".as_ref());
        assert_eq!(by_path, Ok(None));
    }
}
