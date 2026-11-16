# TreeTerm native app (`app-rs/`)

A single-process Rust/GPUI replacement for the Electron app (`src/main`, `src/preload`,
`src/renderer`), which is deprecated in its favour. It talks to the same daemon, session, workspace files and `settings.json`, so
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
  kill-on-close), app registry from settings, tab strip in the order of Electron's
  flex-layout model (`metadata.layoutModel`, saved back as tabs are added, closed, selected
  and dragged into order; scrolls when full, Hidden tabs menu, "+" kept at its end),
  new-tab menu.
- **Git and worktrees (M2):** exec/fs/git clients, open a directory as a workspace, child
  workspaces (new branch, existing branch, remote branch, existing worktree, quick fork; live
  git output), merge/squash-merge (keep or remove), remove with or without git cleanup.
- **Keys and settings (M3):** prefix mode + overlay, workspace focus, Cmd/Ctrl+1–9, app menu
  (Electron's template: About, Services, Hide, Close Window, Toggle Full Screen, Reset Input,
  Minimize, Zoom, Bring All to Front, Help; its commands also work while a dialog has focus;
  off macOS the window draws the menu bar, and a focused terminal keeps the keys it encodes,
  as with xterm.js), Exit and Kill Daemon; all 13 Settings sections; live reload when Electron
  edits `settings.json`; close confirmation for unmerged workspaces; Window → Active Processes.
- **Connections (M4):** local + SSH sessions on the core connection manager (bootstrap,
  tunnel, reconnect with backoff, outdated-daemon refresh/accept), SSH connection picker with
  saved connections, remote directory browser, session pane with logs and port forwards.
  While a connection is down its session stays in the sidebar, dimmed under the connection's
  banner, and its workspace stays open; workspace changes whose writes were deferred or failed
  meanwhile are written on the new connection before the session is rebuilt from it.
  (SSH tested in core with a fake ssh host and contract tests; not yet against a real host.)
- **AI harness (M5):** per-tab LLM analyzer (classification, auto-approve, titles, branch
  names), activity states with transitions, unread attention markers with the ding.
- **Files / editor (M6):** file tree with favourites (a child worktree inherits its ancestors'),
  highlighted viewer, Markdown preview, images, PDF and 3D models (webview; three.js bundled
  by `assets/model-viewer/build.sh`), download, search panel (in file / in repo), find, go to
  definition, review comments (click a line number; commented lines marked in the glyph
  margin); editor tab.
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
- `src/diff/state.rs`, `src/diff/sticky.rs`: `DiffState::file_header_top` (where a file's
  header, in the list or pinned, was drawn) and `DiffState::scroll_file_header_to`, so
  checking "Viewed" on an open file brings the next file's header to where the checked one
  was, its "Viewed" under the pointer, as Electron's `FileDiffSection` does.
- `src/highlighter/languages.rs`: TSX is highlighted with the Kit's TypeScript query, as
  TypeScript is. The Kit used `tree_sitter_typescript::HIGHLIGHTS_QUERY`, which only adds
  TypeScript's captures to the JavaScript query, so `.tsx` files (in the Files viewer, the
  Editor tab and Review) showed type names coloured but strings, comments and most
  keywords plain.
- `src/highlighter/languages.rs`, `Cargo.toml`, `src/highlighter/languages/{proto,cmake,graphql}/`
  (new): the languages the Kit left uncoloured get highlight queries. C++ layers its rules
  over C's query, as upstream's `tree-sitter.json` does (the Kit used the C++ additions
  alone, so the `tree-sitter-cpp` feature now also pulls in `tree-sitter-c`). C# and Swift
  use their grammar crates' queries. Protobuf's comes from its crate, which does not export
  it, plus the `"proto3"` version string. CMake's comes from its crate with Neovim's
  `#lua-match?` rewritten as `#match?`: tree-sitter skips predicates it does not know, so as
  shipped it would colour every argument as a constant and every file's first comment as a
  `#!` line. GraphQL's crate ships none, so that one is written for it.
- `src/menu/app_menu_bar.rs`: `AppMenuBar`'s menus paint at `POPUP_PRIORITY`, above
  dialogs, as the Kit's other popups do; they painted under them. Off macOS the app draws
  its menu bar above dialogs (Electron's GTK bar stays usable while a page modal is open),
  so its menus must show there too.
- `src/window_ext.rs`: `WindowExt::active_dialog_count`, how many dialogs are open, which
  the automation socket's `state` reports for the end-to-end tests (the Kit only says
  whether any is).
- `src/highlighter/highlighter.rs`: captures that no theme can colour (`@spell`,
  `@local.reference`, `@_name`, …) are dropped before captures of one range are merged.
  The merge keeps the first name, so Scala's `(comment) @spell @comment` left comments
  plain.

When `gpui-kit` is upgraded, copy the new `gpui-component` over this directory and re-apply
the marked changes (or drop the patch if the Kit gained the option).

## Appearance

The chrome was matched to Electron's computed styles, measured side by side on one session
(sizes, padding, fonts, colours, icons; Lucide icons through GPUI Kit). What still differs:

- Text: GPUI renders the system font 1–3px wider per label and has no `letter-spacing`;
  Chromium colour-manages saturated colours (same tokens, different pixels).
- GPUI Kit limits: tooltips are not the native macOS ones; dialog shadows are lighter; the
  diff view keeps 19.2px rows, its own fold rows and two columns for added/deleted files,
  and shows a file once, so a partially staged file has one section with both Unstage and
  Stage (Electron lists it twice, each section with the staged hunks);
  the editor has no minimap and cannot open space between lines, so the Files viewer lists
  its file's review comments (and the comment box) in a panel under the editor instead of
  Monaco's view zones; selects open a Kit menu, not the native popup; textareas have no
  resize grip. Off macOS the menu bar is GPUI Kit's, drawn in the window (not GTK's), above
  dialogs so that it stays usable while one is open, as GTK's is.
- The code editors (Files viewer, Editor tab), through GPUI Kit: Tab inserts a whole indent
  step, not spaces up to the next tab stop; 8px of padding stays under the last line; the
  current line's outline is drawn over the text, so it stops at the scrollbar; a scrollbar
  fades 2 s after scrolling stops (Monaco: 0.5 s). The Kit has no R, XML, Less or Dockerfile
  grammar: `.r` files are plain, and `.xml`, `.less` and `.dockerfile` files are coloured
  with the HTML, CSS and shell grammars.
- With the sidebar collapsed the header keeps clear of the macOS window buttons (Electron's
  title runs under them).
- GPUI cannot decode AVIF, so the Files viewer shows `.avif` images in a webview (WebKit
  draws them, centred and scaled down as Electron's `<img>`).
- PDFs show in WebKit's viewer, which has no toolbar: a strip above it has Chromium's Zoom
  out, zoom level, Zoom in and Print (100% is the width WebKit fits the pages to, not the
  pages' own size). There is no page box, thumbnails, fit or rotate; those need a bundled
  PDF viewer (pdf.js).
- Review's find: GPUI Kit's diff does not report the word under the pointer or the file at
  the top of the scroll, so the right-click menu needs a selection (double-click a word) and
  "In this file" is the file last under the pointer (else the one last selected).

## Known differences from Electron (deliberate)

- Review comments made in the Files tab are saved with the workspace-relative path, as the
  Review tab saves them (Electron's Files tab saves the absolute path), so they also show in
  Review and can be pushed to GitHub. Comments with Electron's absolute paths still show in
  the Files viewer and the Comments tab; Electron's Files tab does not show the relative ones.

- After a reconnect the window shows the workspace it showed before (the first one if it is
  gone) and leaves other sessions' views alone; Electron always jumps to the reconnected
  session's first workspace.
- A workspace write still in flight when the connection drops is written after the reconnect
  too (Electron's flush waited for it to fail and then dropped it).
- The session's force-unlock button hides once the lock has expired (the daemon ignores an
  expired lock); Electron shows it as long as the daemon still reports the lock.
- A stale local daemon socket is replaced at once (Electron waited up to 5 s for it to
  answer). Connections to use still wait up to 5 s for the daemon, as in Electron.
- Placeholders for refs another window removed are dropped with the ref.
- A replayed OSC 52 copy or bell is ignored (it already happened when it first ran).
- Corrupt `settings.json` is an error (logged; defaults used) instead of being overwritten.
- Merge-and-remove stages untracked files before its WIP commit (Electron's `commit -am`
  skips them, failing the merge or losing new files with the deleted worktree).
- Review's Refresh also reloads the Commits list once it was shown, keeping as many commits
  as are listed and the picked commit (Electron never reloaded the list after its first load).
- Review refreshes whenever its tab is shown, a switch between tabs of the same workspace
  included. Electron keeps every tab of the workspace shown mounted and visible
  (`tabEnableRenderOnDemand: false`), so it reloaded only when the workspace was shown again.
- Review's fresh conflict check before a merge shows "Checking for conflicts..." and disables
  the header's actions while it runs (Electron showed nothing and left them clickable).
- A failed merge's output ends with `Merge failed: <error>` once (Electron doubled the prefix
  when `git merge` itself failed), or `Merge succeeded but cleanup failed: <error>` when only
  the worktree removal after it failed.
- Daemon fix: each gRPC connection now gets its own id, so the session lock actually excludes
  other connections (it was shared by all connections before).
- Daemon fix: a PTY's child process is fork-safe (environment, cwd and argv built before
  `forkpty`; the child only calls `chdir` + `execve`). Before, `std::env::set_var` in the
  child of the multi-threaded daemon could deadlock it, and the stuck child kept other
  commands' pipes open (a git command started at the same time never finished).
- A terminal takes keyboard focus when it comes into view as the shown tab, but not from an
  open dialog, and not again on every workspace change while it stays shown (Electron
  focused it on each, taking focus from the sidebar's inputs and from dialogs).
- A tab dropped in the strip becomes the active tab (`activeTabId`). Electron's strip showed
  the dropped tab, but its `activeTabId`, which its tab shortcuts act on, stayed on the tab
  that was active before.
- In a strip too full to show every tab, tabs scrolled out of view are no drop target: a tab
  dropped past the last tab shown lands after it. flex-layout also counted the hidden tabs'
  places past the strip's edge, so a drop over "+" could land among them, out of sight.
- The attention ding is the system bell (Electron synthesised an 880 Hz tone).
- The analyzer reads the tab's own terminal; Electron attached a second, headless one.
- The code editors colour files Electron shows as plain text: Makefile, Dockerfile,
  CMakeLists.txt, `.toml`, `.markdown`, `.mjs`/`.cjs`/`.mts`/`.cts`, `.jsonc`,
  `.cc`/`.cxx`/`.hh`, `.zig`, `.ex`/`.exs`, `.proto` and `.diff`/`.patch`.
