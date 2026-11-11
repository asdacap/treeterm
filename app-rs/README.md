# TreeTerm native app (`app-rs/`)

A single-process Rust/GPUI replacement for the Electron app (`src/main`, `src/preload`,
`src/renderer`). It talks to the same daemon, session, workspace files and `settings.json`, so
both apps run side by side until the native one reaches parity. Crate layout and rules are in
`AGENTS.md` ("Native app"); the full plan (milestones M0–M10) lives with the project notes.

## Running it

```sh
cd app-rs
cargo build -p treeterm-app            # add --features automation for the control socket
../app-rs/target/debug/treeterm        # finds or starts the local daemon
```

For an isolated sandbox (separate HOME, socket, daemon) and the automation socket, see
`AGENTS.md`.

## Packaging

`app-rs/xtask` packages the app with plain `cargo`/`std::process` (no packaging crates, nothing
downloaded at build time). From `app-rs/`:

```sh
cargo run -p xtask -- build-daemons   # daemons into <repo>/out/daemon-rs/
cargo run -p xtask -- bundle          # release build of the app + bundle, into target/dist/
cargo run -p xtask -- dist            # both
cargo run -p xtask -- bundle --debug  # bundle a debug build (quick check of the layout)
```

- **Daemons.** `build-daemons` builds `daemon-rs/` like `npm run build:daemon-rs` and
  `build:daemon-rs:remote` do, with the daemon's own (default) toolchain: the local daemon for
  this host (`treeterm-daemon`) and static musl daemons for remote Linux hosts
  (`treeterm-daemon-x86_64-linux`, `treeterm-daemon-aarch64-linux`), which treeterm-core uploads
  over ssh. A remote target is skipped, with the reason, when its Rust target is not installed
  (`rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl`, in `daemon-rs/`)
  or its linker is missing. The linkers are the `*-linux-musl-gcc` ones from
  `daemon-rs/.cargo/config.toml` (macOS: `brew install filosottile/musl-cross/musl-cross`);
  `CARGO_TARGET_<TRIPLE>_LINKER=rust-lld` (as CI does) needs nothing extra.
- **macOS** (`bundle` on a Mac): `target/dist/TreeTerm.app` and
  `TreeTerm-<version>-<arch>-macos.zip`. The bundle holds `Contents/MacOS/treeterm`, the daemons
  in `Contents/Resources/daemon-rs/`, `Info.plist` (from `packaging/macos/Info.plist`) and an
  icon rendered from `packaging/treeterm.svg` with `sips`/`iconutil`. It is signed ad-hoc only;
  sharing it beyond your machines needs a Developer ID signature and notarization.
- **Linux** (`bundle` on Linux): `target/dist/treeterm-<version>-<arch>-linux/` and its
  `.tar.gz`, with the daemons next to `treeterm`, `share/applications/treeterm.desktop` and
  `share/icons/hicolor/scalable/apps/treeterm.svg`. Unpack it anywhere, put `treeterm` on PATH
  (a symlink is fine) and copy `share/` into `~/.local/share/` for the launcher entry. It needs
  the same system libraries as the build (GTK 3, WebKitGTK 4.1, Vulkan, xkbcommon, Wayland/X11).
  No AppImage: `appimagetool` would have to be downloaded.
- **Nix** (Linux): `nix build .#treeterm-native` builds the app with the toolchain pinned in
  `rust-toolchain.toml` and the local daemon next to it (no remote daemons).

The app finds the local daemon at `$TREETERM_DAEMON_DIR/treeterm-daemon` (development), next to
its executable (tarball, Nix), in `../Resources/daemon-rs/` (macOS bundle) or, in debug builds,
in the repo's `out/daemon-rs/` (`treeterm-app/src/daemon_binary.rs`). The `native-app-bundle`
CI job runs `dist` on macOS and Linux and uploads both archives.

## Status

Done (tested against a real in-process daemon unless noted):

- **Plumbing (M0):** daemon as a library (`serve`), tonic client over UDS (main + session
  channels), Electron-compatible socket path, local daemon autostart.
- **Terminal (M1):** `alacritty_terminal` core fed from `PtyStream` (replay-aware query gating,
  OSC 52 rules), GPUI renderer (grid-forced shaping, wide chars, cursor, selection, IME
  preedit, width-limit strip, scrollbar), xterm.js-compatible keys/mouse/paste, links, scroll
  badge/pin, ALT SCREEN, size mismatch, overlays, refresh, status bar (activity, idle detector,
  col limit). 200k lines render in ~0.4 s.
- **Session/workspace sync (M2):** full write side of `createSessionStore` (ref CAS with
  retry, body CAS with parentHash chain, echo suppression, conflict rewrite, watch reopen,
  lock ops), tabs (add/close/activate/state, PTY creation keyed by `ptyHandle`, close-on-exit,
  kill-on-close), app registry from settings, layout-ordered tab strip, new-tab menu.
- **Git and worktrees (M2):** exec/fs/git clients, open a directory as a workspace, child
  workspaces (new branch, existing branch, remote branch, existing worktree, quick fork; live
  git output), merge/squash-merge (keep or remove), remove with or without git cleanup.
- **Keys and settings (M3):** prefix mode + overlay, workspace focus, Cmd/Ctrl+1–9, app menu,
  Exit and Kill Daemon; all 13 Settings sections; live reload when Electron edits
  `settings.json`; close confirmation for unmerged workspaces; Window → Active Processes.
- **Connections (M4):** local + SSH sessions on the core connection manager (bootstrap,
  tunnel, reconnect with backoff, outdated-daemon refresh/accept), SSH connection picker with
  saved connections, remote directory browser, session pane with logs and port forwards.
  (SSH tested in core with a fake ssh host and contract tests; not yet against a real host.)
- **AI harness (M5):** per-tab LLM analyzer (classification, auto-approve, titles, branch
  names), activity states with transitions, unread attention markers with the ding.
- **Files / editor (M6):** file tree, highlighted viewer, images, PDF (webview), editor tab.
- **Review (M7):** committed / uncommitted / commits views, stage/unstage/commit.
- **Web (M8):** Browser tabs in a native webview (localhost guard).

Not yet:

- Git status polling and badges, PR indicators, GitHub app, review comments, viewed files,
  search panel, go-to-definition (core logic ported; UI pending).
- Analyzer history / prompt debugger, chat, prompt buttons (commit, rebase, review comments).
- Tree drag/drop, favourites, abandon, worktree registry, auto-open worktrees.
- Web apps (dev server with `$PORT`), 3D model viewer; CLI (`treeterm [dir]`, `treeterm ssh`).
- Packaging (M9), parity sweep and cut-over (M10). Linux build verification (CI job added).

## Known differences from Electron (deliberate)

- Placeholders for refs another window removed are dropped with the ref.
- A replayed OSC 52 copy or bell is ignored (it already happened when it first ran).
- Corrupt `settings.json` is an error (logged; defaults used) instead of being overwritten.
- Merge-and-remove stages untracked files before its WIP commit (Electron's `commit -am`
  skips them, failing the merge or losing new files with the deleted worktree).
- Daemon fix: each gRPC connection now gets its own id, so the session lock actually excludes
  other connections (it was shared by all connections before).
- The attention ding is the system bell (Electron synthesised an 880 Hz tone).
- The analyzer reads the tab's own terminal; Electron attached a second, headless one.
