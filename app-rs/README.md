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

End-to-end tests launch the real binary in such a sandbox (daemon lifecycle, terminal output
across restarts). They open a window, so they only run when asked:

```sh
(cd daemon-rs && cargo build -p treeterm-daemon)
cd app-rs && cargo test -p treeterm-app --features automation --test e2e -- --ignored --test-threads=1
```

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
- **Files / editor (M6):** file tree with favourites, highlighted viewer, Markdown preview,
  images, PDF and 3D models (webview; three.js bundled by `assets/model-viewer/build.sh`),
  download, search panel (in file / in repo), find, go to definition, review comments
  (click a line number; commented lines marked in the glyph margin); editor tab.
- **Review (M7):** committed / uncommitted / commits views, stage/unstage/commit, inline
  comments (Electron's metadata format), viewed files, image diffs (before / after),
  favourites outside the diff shown in the Files viewer (with its comments) in the diff's place,
  find in the diff (Cmd/Ctrl+F: matches in the file being read, then the rest of the diff,
  then the repo; the diff's menu searches or copies the selected text), prompting the AI
  harness; live merge
  output; git status (uncommitted / conflicts / behind, Pull) and PR indicators.
- **Web (M8):** Browser tabs in a native webview (localhost guard); Web App tabs (free port
  probed on the daemon's host, dev server with `$PORT`, page once it answers); on remote
  sessions both reach their page through an ephemeral SSH port forward owned by the tab, with
  a Port Forward sub-tab (status, Restart, ssh output) — remote path not yet run against a
  host. Markdown pages (`text/markdown`, or `.md` sent as `text/plain`) are rendered in the
  page's place; WebKit cannot display `text/markdown`, so such a page is reloaded through
  the tab's own URL scheme, which fetches it and serves it as text.
- **Workspace pane:** header with git status, PR link, Pull, ▶ Run (npm/make/just/Taskfile/
  VS Code tasks, custom runners), Review & Merge / Abandon; AI-harness Prompt Commit /
  Prompt Rebase; activity glyphs on tabs.
- **Tree and sidebar:** sessions (dragged into order, kept in `ui-state.json`), favourites,
  drag and drop (reorder / reparent), unread markers, PR badges, collapsible rail,
  title/branch refresh, Auto Open All Recent Worktrees (worktree registry shared with
  Electron), upstream warning before forking,
  force-unlock of a held session lock; session renaming (double-click its name; kept in
  `ui-state.json` by connection target); session pane with info, logs, port forwards, a system
  monitor, the session as JSON and its TTYs.
- **Tool tabs:** GitHub PR details (reviews, checks, unresolved threads), review comments
  (prompt the harness, push to GitHub), analyzer history and system-prompt debugger (from
  the activity badge's right-click menu), Chat, TTY list with orphan kill, workspace
  settings (also from the tree's Settings item), SSH upload (ssh sessions only; not yet run
  against a real host).
- **Traffic:** download rate per connection (session header) and per workspace (PTY output,
  workspace header), counted at the daemon socket.
- **CLI:** `treeterm [dir]`, `treeterm ssh user@host[:port]`, `treeterm shutdown-daemon`.
- **Packaging (M9):** `cargo run -p xtask -- dist` (macOS `.app` + zip verified; Linux
  tarball and the Nix package untested).
- **Coexistence:** Electron and the native app on one daemon and session were checked
  together (tabs added in either show in both, input in a shared terminal shows in both, no
  CAS conflicts).

Not yet:

- Running on Linux (the workspace compiles and is clippy-clean for Linux:
  `cargo run -p xtask -- check-linux`; CI jobs added, not yet run) and a real SSH host test.
- Parity sweep sign-off and cut-over (M10): remove `src/main`, `src/preload`,
  `src/renderer` once the checklist in the plan passes.

## Vendored crates

`vendor/gpui-component` is GPUI Kit's `gpui-component` 0.9.1 as published on crates.io,
swapped in with `[patch.crates-io]` (and excluded from the workspace, so its tests and lints
are not ours). Its changes are marked `TreeTerm patch` (`grep -rn "TreeTerm patch" vendor/`;
`diff -r` against the registry copy shows nothing else):

- `src/diff/mod.rs`: a Unified row has one line-number gutter, numbering the line it shows
  (the new number for unchanged and added lines, the old one for removed lines), as Pierre's
  unified diff in Electron's Review tab does; the Kit draws an old and a new gutter.
- `src/diff/sticky.rs` (new), `src/diff/mod.rs`, `src/diff/state.rs`:
  `Diff::render_sticky_header` pins the header of the file at the top of the viewport while
  its rows scroll under it, and the next file's header pushes it out, as Electron's
  `.file-diff-header` (`position: sticky; top: 0`) does; the Kit's headers scroll away.
- `src/highlighter/languages.rs`: TSX is highlighted with the Kit's TypeScript query, as
  TypeScript is. The Kit used `tree_sitter_typescript::HIGHLIGHTS_QUERY`, which only adds
  TypeScript's captures to the JavaScript query, so `.tsx` files (in the Files viewer, the
  Editor tab and Review) showed type names coloured but strings, comments and most
  keywords plain.

When `gpui-kit` is upgraded, copy the new `gpui-component` over this directory and re-apply
the marked changes (or drop the patch if the Kit gained the option).

## Appearance

The chrome was matched to Electron's computed styles, measured side by side on one session
(sizes, padding, fonts, colours, icons; Lucide icons through GPUI Kit). What still differs:

- Text: GPUI renders the system font 1–3px wider per label and has no `letter-spacing`;
  Chromium colour-manages saturated colours (same tokens, different pixels).
- GPUI Kit limits: tooltips are not the native macOS ones; dialog shadows are lighter; the
  diff view keeps 19.2px rows, its own fold rows and two columns for added/deleted files;
  the editor has no minimap and cannot open space between lines, so the Files viewer lists
  its file's review comments (and the comment box) in a panel under the editor instead of
  Monaco's view zones; selects open a Kit menu, not the native popup; textareas have no
  resize grip.
- With the sidebar collapsed the header keeps clear of the macOS window buttons (Electron's
  title runs under them).
- Review's find: GPUI Kit's diff does not report the word under the pointer or the file at
  the top of the scroll, so the right-click menu needs a selection (double-click a word) and
  "In this file" is the file last under the pointer (else the one last selected).

## Known differences from Electron (deliberate)

- Review comments made in the Files tab are saved with the workspace-relative path, as the
  Review tab saves them (Electron's Files tab saves the absolute path), so they also show in
  Review and can be pushed to GitHub. Comments with Electron's absolute paths still show in
  the Files viewer and the Comments tab; Electron's Files tab does not show the relative ones.

- Placeholders for refs another window removed are dropped with the ref.
- A replayed OSC 52 copy or bell is ignored (it already happened when it first ran).
- Corrupt `settings.json` is an error (logged; defaults used) instead of being overwritten.
- Merge-and-remove stages untracked files before its WIP commit (Electron's `commit -am`
  skips them, failing the merge or losing new files with the deleted worktree).
- Daemon fix: each gRPC connection now gets its own id, so the session lock actually excludes
  other connections (it was shared by all connections before).
- Daemon fix: a PTY's child process is fork-safe (environment, cwd and argv built before
  `forkpty`; the child only calls `chdir` + `execve`). Before, `std::env::set_var` in the
  child of the multi-threaded daemon could deadlock it, and the stuck child kept other
  commands' pipes open (a git command started at the same time never finished).
- The attention ding is the system bell (Electron synthesised an 880 Hz tone).
- The analyzer reads the tab's own terminal; Electron attached a second, headless one.
