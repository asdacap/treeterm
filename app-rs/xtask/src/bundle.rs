//! The app with its daemons, laid out where `treeterm-app/src/daemon_binary.rs` (local
//! daemon) and treeterm-core's `DaemonBinaries` (remote daemons) look for them:
//!
//! - macOS: `TreeTerm.app/Contents/{Info.plist, MacOS/treeterm, Resources/TreeTerm.icns,
//!   Resources/daemon-rs/treeterm-daemon[-<arch>-linux]}`, ad-hoc signed, plus a zip of it.
//! - Linux: `treeterm-<version>-<arch>-linux/{treeterm, treeterm-daemon[-<arch>-linux],
//!   share/applications/treeterm.desktop, share/icons/hicolor/scalable/apps/treeterm.svg}`,
//!   plus a `.tar.gz` of it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::daemons::{LOCAL_DAEMON, REMOTE_TARGETS};
use crate::{
    Context, Profile, Repo, Result, cargo, exec, install_file, remove_dir_if_exists, step, warn,
};

/// treeterm-app's version: the xtask shares the workspace version with it.
const VERSION: &str = env!("CARGO_PKG_VERSION");
const APP_BINARY: &str = "treeterm";

pub fn bundle(repo: &Repo, profile: Profile) -> Result<()> {
    // Before the (long) app build, so a missing daemon fails fast.
    let daemons = Daemons::collect(&repo.daemon_out())?;
    let app = build_app(repo, profile)?;
    let dist = repo.dist();
    fs::create_dir_all(&dist).context(|| format!("create {}", dist.display()))?;
    let artifacts = match std::env::consts::OS {
        "macos" => macos(&repo.packaging(), &app, &daemons, &dist)?,
        "linux" => linux(&repo.packaging(), &app, &daemons, &dist)?,
        other => {
            return Err(format!(
                "bundling is supported on macOS and Linux, not {other}"
            ));
        }
    };
    step("Bundled");
    for artifact in artifacts {
        eprintln!("    {}", artifact.display());
    }
    Ok(())
}

fn build_app(repo: &Repo, profile: Profile) -> Result<PathBuf> {
    // `cargo run` names the cargo it was started with (app-rs's pinned toolchain).
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .current_dir(repo.app_rs())
        .args(["build", "-p", "treeterm-app", "--bin", APP_BINARY]);
    match profile {
        Profile::Release => {
            step("Building the app (release)");
            command.arg("--release");
        }
        Profile::Debug => step("Building the app (debug)"),
    }
    cargo::build_executable(command, APP_BINARY)
}

/// The daemon binaries to ship, by file name. The local daemon is required (the app cannot
/// run without it); a missing remote daemon only costs setting up that kind of remote host.
struct Daemons {
    files: Vec<(String, PathBuf)>,
}

impl Daemons {
    fn collect(dir: &Path) -> Result<Self> {
        let local = dir.join(LOCAL_DAEMON);
        if !local.is_file() {
            return Err(format!(
                "no local daemon at {}: run `cargo run -p xtask -- build-daemons` first \
                 (or `dist`, which builds the daemons and then bundles)",
                local.display()
            ));
        }
        let mut files = vec![(LOCAL_DAEMON.to_owned(), local)];
        for target in &REMOTE_TARGETS {
            let name = target.binary_name();
            let path = dir.join(&name);
            if path.is_file() {
                files.push((name, path));
            } else {
                warn(format!(
                    "no {}: this bundle cannot set up remote {} Linux hosts",
                    path.display(),
                    target.arch
                ));
            }
        }
        Ok(Self { files })
    }

    fn install_into(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir).context(|| format!("create {}", dir.display()))?;
        for (name, from) in &self.files {
            install_file(from, &dir.join(name))?;
        }
        Ok(())
    }
}

// === macOS ===================================================================================

fn macos(packaging: &Path, app: &Path, daemons: &Daemons, dist: &Path) -> Result<Vec<PathBuf>> {
    let bundle = dist.join("TreeTerm.app");
    step(format!("Assembling {}", bundle.display()));
    stage_macos(packaging, app, daemons, &bundle)?;
    exec(
        Command::new("plutil")
            .arg("-lint")
            .arg(bundle.join("Contents/Info.plist"))
            .stdout(Stdio::null()),
    )?;
    make_icns(
        &packaging.join("treeterm.svg"),
        &bundle.join("Contents/Resources/TreeTerm.icns"),
        dist,
    )?;
    sign(&bundle)?;

    let zip = dist.join(format!(
        "TreeTerm-{VERSION}-{}-macos.zip",
        std::env::consts::ARCH
    ));
    step(format!("Zipping {}", zip.display()));
    remove_file_if_exists(&zip)?;
    // ditto keeps permissions, symlinks and the signature intact.
    exec(
        Command::new("ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(&bundle)
            .arg(&zip),
    )?;
    Ok(vec![bundle, zip])
}

/// Everything but the icon and the signature (which need macOS tools).
fn stage_macos(packaging: &Path, app: &Path, daemons: &Daemons, bundle: &Path) -> Result<()> {
    remove_dir_if_exists(bundle)?;
    let contents = bundle.join("Contents");
    let macos = contents.join("MacOS");
    fs::create_dir_all(&macos).context(|| format!("create {}", macos.display()))?;
    let template = packaging.join("macos/Info.plist");
    let plist = fs::read_to_string(&template)
        .context(|| format!("read {}", template.display()))?
        .replace("@VERSION@", VERSION);
    let info = contents.join("Info.plist");
    fs::write(&info, plist).context(|| format!("write {}", info.display()))?;
    install_file(app, &macos.join(APP_BINARY))?;
    daemons.install_into(&contents.join("Resources/daemon-rs"))
}

/// `TreeTerm.icns` rendered from the SVG icon (`sips` rasterizes SVG on macOS).
fn make_icns(svg: &Path, icns: &Path, work_dir: &Path) -> Result<()> {
    let iconset = work_dir.join("TreeTerm.iconset");
    remove_dir_if_exists(&iconset)?;
    fs::create_dir_all(&iconset).context(|| format!("create {}", iconset.display()))?;
    for size in [16, 32, 128, 256, 512] {
        for scale in [1, 2] {
            let suffix = if scale == 2 { "@2x" } else { "" };
            let pixels = (size * scale).to_string();
            exec(
                Command::new("sips")
                    .args(["-s", "format", "png", "-z", &pixels, &pixels])
                    .arg(svg)
                    .arg("--out")
                    .arg(iconset.join(format!("icon_{size}x{size}{suffix}.png")))
                    .stdout(Stdio::null()),
            )?;
        }
    }
    exec(
        Command::new("iconutil")
            .args(["-c", "icns", "-o"])
            .arg(icns)
            .arg(&iconset),
    )?;
    fs::remove_dir_all(&iconset).context(|| format!("remove {}", iconset.display()))
}

/// Ad-hoc signature: it seals Info.plist and the resources so the bundle validates (Apple
/// silicon will not run unsigned code). Distribution outside this machine still needs a
/// Developer ID signature and notarization on top.
fn sign(bundle: &Path) -> Result<()> {
    step("Signing (ad-hoc)");
    let daemon = bundle
        .join("Contents/Resources/daemon-rs")
        .join(LOCAL_DAEMON);
    for code in [daemon.as_path(), bundle] {
        exec(
            Command::new("codesign")
                .args(["--force", "--sign", "-"])
                .arg(code),
        )?;
    }
    exec(
        Command::new("codesign")
            .args(["--verify", "--strict"])
            .arg(bundle),
    )
}

// === Linux ===================================================================================

fn linux(packaging: &Path, app: &Path, daemons: &Daemons, dist: &Path) -> Result<Vec<PathBuf>> {
    let name = format!("treeterm-{VERSION}-{}-linux", std::env::consts::ARCH);
    let dir = dist.join(&name);
    step(format!("Assembling {}", dir.display()));
    stage_linux(packaging, app, daemons, &dir)?;
    let tarball = dist.join(format!("{name}.tar.gz"));
    step(format!("Packing {}", tarball.display()));
    exec(
        Command::new("tar")
            .arg("-czf")
            .arg(&tarball)
            .arg("-C")
            .arg(dist)
            .arg(&name),
    )?;
    Ok(vec![dir, tarball])
}

fn stage_linux(packaging: &Path, app: &Path, daemons: &Daemons, dir: &Path) -> Result<()> {
    remove_dir_if_exists(dir)?;
    fs::create_dir_all(dir).context(|| format!("create {}", dir.display()))?;
    install_file(app, &dir.join(APP_BINARY))?;
    // Next to the executable, where daemon_binary.rs looks on Linux.
    daemons.install_into(dir)?;
    copy_into(
        &packaging.join("linux/treeterm.desktop"),
        &dir.join("share/applications"),
    )?;
    copy_into(
        &packaging.join("treeterm.svg"),
        &dir.join("share/icons/hicolor/scalable/apps"),
    )
}

fn copy_into(file: &Path, dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).context(|| format!("create {}", dir.display()))?;
    let name = file.file_name().expect("a file path");
    fs::copy(file, dir.join(name))
        .context(|| format!("copy {} to {}", file.display(), dir.display()))?;
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path).context(|| format!("remove {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct Fixture {
        dir: tempfile::TempDir,
    }

    impl Fixture {
        /// A built app and an `out/daemon-rs/` holding `daemons`.
        fn new(daemons: &[&str]) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let fixture = Self { dir };
            fixture.executable("target/release/treeterm");
            for daemon in daemons {
                fixture.executable(&format!("out/daemon-rs/{daemon}"));
            }
            fixture
        }

        fn executable(&self, relative: &str) {
            let path = self.path(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, relative).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        fn path(&self, relative: &str) -> PathBuf {
            self.dir.path().join(relative)
        }

        fn daemons(&self) -> Result<Daemons> {
            Daemons::collect(&self.path("out/daemon-rs"))
        }

        fn app(&self) -> PathBuf {
            self.path("target/release/treeterm")
        }
    }

    fn packaging() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../packaging")
    }

    const ALL_DAEMONS: [&str; 3] = [
        "treeterm-daemon",
        "treeterm-daemon-x86_64-linux",
        "treeterm-daemon-aarch64-linux",
    ];

    fn assert_executable(path: &Path, contents: &str) {
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            contents,
            "{}",
            path.display()
        );
        let mode = fs::metadata(path).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "{} is not executable", path.display());
    }

    #[test]
    fn the_local_daemon_is_required() {
        let fixture = Fixture::new(&["treeterm-daemon-x86_64-linux"]);
        let error = fixture.daemons().err().unwrap();
        assert!(error.contains("xtask -- build-daemons"), "{error}");
    }

    #[test]
    fn missing_remote_daemons_are_left_out() {
        let fixture = Fixture::new(&["treeterm-daemon", "treeterm-daemon-aarch64-linux"]);
        let names: Vec<_> = fixture
            .daemons()
            .unwrap()
            .files
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, ["treeterm-daemon", "treeterm-daemon-aarch64-linux"]);
    }

    #[test]
    fn the_app_bundle_holds_the_binary_and_its_daemons_in_resources() {
        let fixture = Fixture::new(&ALL_DAEMONS);
        let bundle = fixture.path("dist/TreeTerm.app");
        stage_macos(
            &packaging(),
            &fixture.app(),
            &fixture.daemons().unwrap(),
            &bundle,
        )
        .unwrap();

        let contents = bundle.join("Contents");
        assert_executable(&contents.join("MacOS/treeterm"), "target/release/treeterm");
        for daemon in ALL_DAEMONS {
            // daemon_binary.rs: <exe dir>/../Resources/daemon-rs/treeterm-daemon.
            assert_executable(
                &contents.join("MacOS/../Resources/daemon-rs").join(daemon),
                &format!("out/daemon-rs/{daemon}"),
            );
        }
        let plist = fs::read_to_string(contents.join("Info.plist")).unwrap();
        assert!(!plist.contains("@VERSION@"));
        assert!(plist.contains(&format!("<string>{VERSION}</string>")));
        assert!(plist.contains("<key>CFBundleExecutable</key>\n\t<string>treeterm</string>"));
        assert!(plist.contains("<key>CFBundleIconFile</key>\n\t<string>TreeTerm</string>"));
    }

    #[test]
    fn the_tarball_dir_holds_the_daemons_next_to_the_binary() {
        let fixture = Fixture::new(&ALL_DAEMONS);
        let dir = fixture.path("dist/treeterm-0.1.0-x86_64-linux");
        stage_linux(
            &packaging(),
            &fixture.app(),
            &fixture.daemons().unwrap(),
            &dir,
        )
        .unwrap();

        assert_executable(&dir.join("treeterm"), "target/release/treeterm");
        for daemon in ALL_DAEMONS {
            assert_executable(&dir.join(daemon), &format!("out/daemon-rs/{daemon}"));
        }
        let desktop = fs::read_to_string(dir.join("share/applications/treeterm.desktop")).unwrap();
        assert!(desktop.contains("\nExec=treeterm\n"), "{desktop}");
        assert!(desktop.contains("\nIcon=treeterm\n"), "{desktop}");
        assert!(
            dir.join("share/icons/hicolor/scalable/apps/treeterm.svg")
                .is_file()
        );
    }

    #[test]
    fn restaging_drops_what_the_previous_bundle_had() {
        let fixture = Fixture::new(&ALL_DAEMONS);
        let dir = fixture.path("dist/treeterm-0.1.0-x86_64-linux");
        stage_linux(
            &packaging(),
            &fixture.app(),
            &fixture.daemons().unwrap(),
            &dir,
        )
        .unwrap();
        fs::remove_file(fixture.path("out/daemon-rs/treeterm-daemon-aarch64-linux")).unwrap();

        stage_linux(
            &packaging(),
            &fixture.app(),
            &fixture.daemons().unwrap(),
            &dir,
        )
        .unwrap();

        assert!(dir.join("treeterm-daemon-x86_64-linux").is_file());
        assert!(!dir.join("treeterm-daemon-aarch64-linux").exists());
    }
}
