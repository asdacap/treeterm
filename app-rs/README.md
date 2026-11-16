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

The app logs to stdout. Its level is `RUST_LOG` when that is set (tracing's directives, such
as `RUST_LOG=treeterm_core=debug`); else `"logLevel"` in `ui-state.json` beside
`settings.json`: `debug`, `info`, `warn`, `error` or `silent`, in any case, read at start-up
(Electron's renderer read it from its localStorage `treeterm:logLevel`); else `info`. A panic
is logged as an error (target `panic`, with a backtrace when `RUST_BACKTRACE` is set), or
printed to stderr when the level leaves that out.

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
  preedit, width-limit strip, scrollbar), xterm.js-compatible keys/mouse/paste, xterm.js's
  selection gestures (its word separators for a double-click, Shift+click extends, Alt+drag
  selects columns, Alt+click moves the cursor, a drag past the top or bottom scrolls, a
  right-click on macOS selects the word first, Alt+wheel scrolls 5x), links (an OSC 8
  `http(s)` hyperlink opens in the browser on a click, a URL in the text on Cmd/Ctrl+click;
  underlined, with a pointer cursor, under the pointer), scroll badge/pin, ALT SCREEN, size
  mismatch, DEBUG badge (Settings > Debug), overlays, refresh, status bar (activity, idle
  detector, col limit), Show Raw Characters (each chunk of output of the shown workspace's
  terminals logs the last 50 characters as an info `[RAW]` line). 200k lines render in
  ~0.4 s.
- **Session/workspace sync (M2):** full write side of `createSessionStore` (ref CAS with
  retry, body CAS with parentHash chain, echo suppression, conflict rewrite, watch reopen,
  lock ops), tabs (add/close/activate/state, PTY creation keyed by `ptyHandle`, close-on-exit,
  kill-on-close), app registry from settings, tab layout of Electron's flex-layout model
  (`metadata.layoutModel`: tabsets side by side or stacked as saved, sized by their weights,
  each with its strip and selected tab; 4px splitters dragged to resize, a tabset maximized
  and restored from its strip, a tabset gone with its last tab; a tab dragged onto a pane
  docks into it or beside it, splitting it, or along an edge of the whole layout on its edge
  docks, outlined where it lands; saved back as tabs are added, closed, selected, dragged into
  order or into another tabset, and docked; strips scroll when full, Hidden tabs menu, "+"
  kept at its end), new-tab menu.
- **Git and worktrees (M2):** exec/fs/git clients, open a directory as a workspace, child
  workspaces (new branch, existing branch, remote branch, existing worktree, quick fork; live
  git output), merge/squash-merge (keep or remove), remove with or without git cleanup.
- **Keys and settings (M3):** prefix mode + overlay, workspace focus, Cmd/Ctrl+1–9, app menu
  (Electron's template: About, Services, Hide, Close Window, Edit's Undo, Redo, Cut, Copy,
  Paste, Paste and Match Style, Delete and Select All on whatever has focus, a Browser or Web
  App page included, Actual Size,
  Zoom In, Zoom Out (the whole window, web pages included; Cmd/Ctrl+= zooms in too; the level
  is kept in `zoom.json` and restored at the next start, as Electron's), Toggle Full Screen, Reset Input, Minimize, Zoom,
  Bring All to Front, Help; its commands also work while a dialog has focus;
  off macOS the window draws the menu bar, and a focused terminal keeps the keys it encodes,
  as with xterm.js), Exit and Kill Daemon; all 13 Settings sections (reopened on the one shown
  last; Escape leaves the dialog open), whose controls Tab reaches as Electron's native ones
  (Space or Enter presses a button, Space toggles a checkbox, Space or an arrow opens a
  select, Tab leaves a textarea, a linked label focuses its control, the panel scrolls a
  control focused from the keyboard or its label into view, and a keybinding button keeps the
  focus while and after it records); live reload when
  Electron edits `settings.json`; close confirmation for unmerged workspaces; Window → Active
  Processes.
- **Connections (M4):** local + SSH sessions on the core connection manager (bootstrap,
  tunnel, reconnect with backoff, outdated-daemon refresh/accept), SSH connection picker with
  saved connections, remote directory browser, session pane with logs and port forwards.
  While a connection is down its session stays in the sidebar, dimmed under the connection's
  banner, and its workspace stays open; workspace changes whose writes were deferred or failed
  meanwhile are written on the new connection before the session is rebuilt from it; until
  the rebuilt session has loaded, a connection lost again brings back the session as it was. A
  connect whose session does not load stops at its error ("Remote daemon is outdated. Retry
  with 'Refresh remote daemon' checked. …" or "Connected but failed to load session: …"),
  without a session; saved port forwards start once the session has loaded, and again when a
  Retry, Refresh daemon or Connect anyway replaces a failed connection (a reconnect restarts
  the running ones).
  (SSH tested in core with a fake ssh host and contract tests; not yet against a real host.)
- **AI harness (M5):** per-tab LLM analyzer (classification, auto-approve, titles, branch
  names), activity states with transitions, unread attention markers with the ding.
- **Files / editor (M6):** file tree with favourites (a child worktree inherits its ancestors'),
  highlighted viewer, Markdown preview, images, PDF and 3D models (webview; three.js bundled
  by `assets/model-viewer/build.sh`), download, search panel (in file / in repo), find, go to
  definition (F12, Cmd/Ctrl+click, the context menu; a word with a definition is underlined
  while Cmd/Ctrl is held over it), review comments (click a line number; commented lines
  marked in the glyph margin); editor tab with go to definition, Change All Occurrences
  (Cmd/Ctrl+F2) and suggestions while typing (Ctrl+Space asks): CSS, SCSS and Less property
  names, HTML element, end tag and attribute names, else the file's words. Both code editors
  mark problems (their message on hover): JSON's (Monaco's JSON worker, ported), TypeScript
  and JavaScript syntax errors, and CSS lint warnings (empty rules, unknown and duplicate
  properties, unknown at-rules, incomplete `@font-face`); they paint CSS, SCSS and Less
  colours, and underline links ("Follow link (cmd + click)" on hover): Cmd/Ctrl+click opens
  an `http(s)://` link in the browser and a `file://` link in an Editor tab, at the line its
  `#L<n>` names. The Files viewer and the Editor tab save their scroll offset in the tab
  state when the tab is hidden and open there again after a restart; a line they were opened
  at is moved to once.
- **Review (M7):** committed / uncommitted / commits views, stage/unstage/commit, inline
  comments (Electron's metadata format), viewed files, image diffs (before / after, SVG
  and AVIF included), each file's diff read once its section comes within 1500px of the viewport
  ("Scroll to load diff" until then; find reads every file, four at a time), loading,
  failing (also when its part of the patch does not parse) and refreshed (its header's
  refresh button) on its own, a diff over 20,000 lines or 2 MiB drawn only after "Load anyway",
  favourites outside the diff shown in the Files viewer (with its comments) in the diff's place,
  find in the diff (Cmd/Ctrl+F: matches in the file at the top of the diff, then the rest of
  the diff, then the repo; the diff's menu searches or copies the selected text or the word
  under the pointer), prompting the AI harness; live merge
  output; git status (uncommitted / conflicts / behind, Pull) and PR indicators.
- **Web (M8):** Browser tabs in a native webview. As in Electron, the start page (and a full
  address typed) must be on localhost, but links, redirects and frames then load in the tab
  from any host; links to other apps (`mailto:`, `vscode:`) and popups open in the system,
  `file:` links are refused. The toolbar follows the webview's own history (Back / Forward),
  loading (Stop and the progress line from the request on) and navigations within the page
  (`pushState`, fragments), which the tab also remembers; Reload bypasses the cache. A load
  that ends without a page (server down) shows "This site can’t be reached" and its URL in
  the page's place until the next load (WKWebView draws no error page; the reason, Chromium's
  error code, is not known); Web App tabs (free port
  probed on the daemon's host, dev server with `$PORT`, page once it answers); both reopen
  on the page they last showed (its path on the start page, whatever port the page is on
  this time), and show it only while it is ready; on remote sessions both reach their page
  through an ephemeral SSH port forward owned by the tab, with a Port Forward sub-tab
  (status, Restart, ssh output) — remote path not yet run against a host. Markdown pages
  (`text/markdown`, or `.md` sent as `text/plain`) are rendered in the page's place; WebKit
  cannot display `text/markdown`, so such a page is reloaded through the tab's own URL
  scheme, which fetches it and serves it as text (only pages the tab itself sent there).
- **Workspace pane:** header with git status, PR link, Pull, ▶ Run (npm/make/just/Taskfile/
  VS Code tasks, custom runners), Review & Merge / Abandon; AI-harness Prompt Commit /
  Prompt Rebase; Prompt Description / Skip beside a description not yet sent to the
  harness (shown while an AI-harness tab is active); activity glyphs on tabs (as on tree,
  favourite and rail rows, hovering one names its state: "Working..."). A workspace
  being created shows git's output once there is some; one that failed shows why, with
  Dismiss. A failed operation (merge, removal) shows "Operation failed" with Force Unlock
  and Cancel in place of the tabs.
- **Tree and sidebar:** sessions (dragged into order in the sidebar or the collapsed rail,
  kept in `ui-state.json`; "No sessions. Click + to browse." once all are disconnected),
  favourites, drag and drop (reorder / reparent), unread markers (with a "Saving workspace
  attention" spinner until an edit of one is saved), the attention flash on tree, favourite
  and rail rows, PR badges, collapsible rail,
  title/branch refresh, Auto Open All Recent Worktrees (worktree registry shared with
  Electron), upstream warning before forking,
  force-unlock of a held session lock; session renaming (double-click its name; kept in
  `ui-state.json` by connection target); the Open Workspace dialog's Recent Directories (kept
  in `ui-state.json` by connection target; Electron's, from its localStorage, are added once
  after the ones already listed); session pane with info, logs, port forwards, a system
  monitor, the session as JSON and its TTYs (listed when the tab is shown, on Refresh and
  after a Kill, through the connection's daemon of the moment; the last list and an open
  viewer stay while the connection is down and once it is back, until Refresh). Each
  session has an active workspace (its first
  one once loaded, a new or forked one at once): the window opens on it at start-up and
  after a remote connect, and disconnecting the shown session shows the next one's (or its
  session pane when it has none). Until a workspace is shown (a local session without any
  at start-up) or once every session is disconnected, the window reads "Select a workspace
  to get started". A connect from the SSH picker shows the session pane while
  it connects; a remote session without workspaces gets one at the remote `$HOME`
  (`/home/<user>` if that cannot be read).
- **Tool tabs:** GitHub PR details (reviews, checks, unresolved threads), review comments
  (prompt the harness, push to GitHub), analyzer history and system-prompt debugger (from
  the activity badge's right-click menu), Chat, TTY list with orphan kill, workspace
  settings (also from the Settings item of a tree row or favourite), SSH upload (ssh
  sessions only; the tab keeps its file, directory and an upload's result across reconnects
  until it is closed; not yet run against a real host).
- **Traffic:** download rate per connection (session header: the gRPC messages received,
  without HTTP/2 framing or message prefixes, as Electron counts them) and per workspace (PTY
  output, workspace header). Totals and rates are kept across
  reconnects and a failed connection connected again, until the connection is disconnected
  or the workspace removed.
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
- `src/diff/state.rs`, `src/diff/document.rs`: `DiffState::update_files` replaces the files
  keeping the reader's place: the row at the top of the viewport stays (the same line or file
  header, as far scrolled past), files still listed stay collapsed, and files whose content
  did not change keep their opened folds and highlighting (after `expand_unchanged`, new and
  changed files show every line too), their selected lines and a line-number drag still
  selecting in them, as Electron's `StackedDiffList` keys its sections by path. Text
  selected (or still being dragged) in files whose content and place in the list did not
  change stays selected, as a page's selection stays while another section re-renders (the
  selection names its files by place); `set_files` resets all of that and scrolls to the
  top. Files it is told to open
  (whose rows just arrived) open in the same step: `set_file_collapsed` afterwards would put
  the first line of the file at the top in place of its header.
- `src/diff/state.rs`, `src/diff/mod.rs`: `DiffEvent::TopBandChanged` reports, after each
  layout, the files with a row in the top tenth of the viewport (`DiffState::top_band`), the
  band Electron's `IntersectionObserver` watches to make a file the active one (the file
  tree's highlight, where Previous / Next unviewed start).
- `src/diff/state.rs`, `src/diff/mod.rs`: `DiffEvent::NearFilesChanged` reports, after each
  layout whose rows are measured, the files with a row within `NEAR_MARGIN` (1500px) above or
  below the viewport (`DiffState::near_files`; rows not laid out count as tall as a code row,
  or a header as the last header laid out), the margin within which Electron's
  `FileDiffSection` loads a file's diff.
- `src/diff/state.rs`, `src/diff/selection.rs`: `DiffState::text_at` (the line and character
  painted under a point) and `DiffState::file_at` (the file whose row is under it), so the
  diff's right-click menu searches or copies the word under the pointer and searches the
  file right-clicked, as Electron's `StackedDiffList` does.
- `src/diff/state.rs`: `DiffState::scroll_to_line_at` parks a line part of the way down the
  viewport, so a find match sits a third of the way down, clear of the pinned file header,
  as in Electron; `scroll_to_line` puts it at the very top, under that header.
- `src/diff/state.rs`, `src/diff/mod.rs`: `DiffState::clicked_line`, the line number a
  line-number press was released on when it is the one it pressed (each number listens for
  the release). Review opens the comment box only for such a click, Shift or not, as
  Electron's `onLineNumberClick` fires only for a DOM `click` on a line number: not for a
  press released in the code, on a header, outside the diff or on another line number.
- `src/diff/mod.rs`: `Diff::highlight_theme` paints a diff's code in its own highlight
  theme's syntax colours and editor foreground, so Review uses VS Code's Dark+ (Electron's
  Pierre diff registers shiki's `dark-plus` as `treeterm-dark`) while the Files viewer and
  the Editor tab keep Monaco's `vs-dark`. The Kit painted every diff in the app theme's.
- `src/highlighter/languages/{rust,javascript,typescript}/highlights.scm`,
  `src/highlighter/{registry,wasm_stub}.rs`: Rust numbers are `@number` (they were
  `@constant.builtin`, `true`'s colour), escapes `@string.escape` (the Kit's `@escape` no
  theme colours), `self` `@variable.special`, a path's modules (`std` and `sync` in
  `std::sync::Arc`) `@namespace.path`, a new highlight name (not `@namespace`, which Go's
  and Scala's queries capture and is not a highlight name, so they stay as they were), a
  macro named by a path (`tracing::error!`) or defined by `macro_rules!` `@function.macro`,
  and plain text `@primary`: what a `use` or `extern crate` brings in unless capitalised
  (`fs` in `use std::fs;`), a module's name, and attributes (they were `@attribute`, an HTML
  attribute's colour). The identifiers no other rule captures are `@variable` (they were
  left plain; the macro rule is split in two, so a macro's name still wins). JavaScript and
  TypeScript's `this` and `super` are `@variable.special` (they were `@variable.builtin`, a
  variable's colour). The Files viewer colours these as Monaco does, and Review as Dark+
  does, but for the names listed under Appearance.
- `src/highlighter/languages/{rust,go,javascript,typescript,python}/highlights.scm`
  (`python` new, from its grammar crate), `src/highlighter/languages.rs`,
  `src/highlighter/{registry,wasm_stub}.rs`: control keywords (`if`, `for`, `return`,
  `match`/`switch`/`case`, `try`/`catch`, `await`, `yield`, `import`/`export`/`from`, …)
  are `@keyword.control`, a new highlight name a theme may colour apart (VS Code's Dark+
  paints `keyword.control` `#C586C0`); unset, it takes `@keyword`'s colour, so the Files
  viewer and the Editor tab keep Monaco's. Each language's list is the keywords VS Code's
  grammar scopes `keyword.control`: among them Rust's `try` (left plain before),
  TypeScript's `satisfies`, `as` and the `type` of an import or export, and Python's `pass`,
  `as`, the `in` of a loop and the `async` of `async for` and `async with`.
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
- `src/input/popovers/completion_menu.rs`: a key the open completion menu takes (Enter,
  Escape, Up, Down) stops there, as in Monaco's suggest widget. The Kit let it propagate,
  so the window went on to type it: Enter took the Editor tab's suggestion and also typed a
  newline.
- `src/menu/popup_menu.rs`: a custom element item (`PopupMenuItem::element`) can be
  narrower than its content, so in a menu given a max width its text can ellipsise, as the
  Run menu's long script names do in Electron. The Kit's row grew with its content and the
  menu cut it off at its edge.

`vendor/gpui-pre` (GPUI, `gpui-pre` 0.3.12, without its `examples/` directory, which only
its own example targets use) and `vendor/gpui-webview` (GPUI Kit's `gpui-webview` 0.9.1) are
vendored the same way, for the window zoom that View > Actual Size,
Zoom In and Zoom Out apply, as Electron's page zoom scales the whole page:

- `gpui-pre` `src/window.rs`, `src/interactive.rs`, `src/platform.rs`: `Window::zoom` and
  `Window::set_zoom`. A window zoomed `z` times lays itself out in pixels `z` times as large:
  its scale factor is the display's times `z`, its viewport `z` times smaller, and the
  platform's pointer and file-drop positions and pixel scroll distances are divided by `z` as
  they come in (`PlatformInput::zoomed_out`); the IME's rectangles and character lookups, and
  `Window::resize`, are converted back. GPUI had no page zoom (its scale factor is the
  display's).
- `gpui-webview` `src/lib.rs`: on macOS a page's view is placed in points (`z` times the
  window's pixels) and its page zoom is multiplied by the window's, as Chromium's
  `<webview>` follows its embedder's zoom. WebKitGTK's view already follows GPUI's scale
  factor.

When `gpui-kit` is upgraded, copy the new crates over these directories and re-apply the
marked changes (or drop a patch if the Kit gained the option).

`vendor/avif-decode` is avif-decode 3.0.0 as published on crates.io (Review's AVIF image
diffs), swapped in and excluded the same way. Its one change, marked `TreeTerm patch` in
`Cargo.toml`, builds rav1d without its assembly on every target, as upstream already does
off x86: rav1d's x86 assembly needs NASM, which the app's builds (`xtask check-linux`
included) would otherwise need. Drop the patch when avif-decode lets its users choose.

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
  Monaco's view zones; selects open a Kit menu, not the native popup (a focused one has no
  type-ahead); the comment box and the Workspace Settings description have no resize grip,
  and Settings' and Add Child's textareas resize down to one line, where Chromium's go down
  to none; GPUI has no backdrop filter, so the prefix-mode
  overlay does not blur what is behind it; GPUI has no animated scrolling, so the Chat tab
  jumps to its last message where Electron's smooth-scrolls there. Off
  macOS the menu bar is GPUI Kit's, drawn in the window (not GTK's), above dialogs so that it
  stays usable while one is open, as GTK's is. On macOS GPUI greys out a
  Cut, Copy, Paste or Select All that nothing focused can do (Cut in a terminal, Paste with no
  input focused); Electron's stay enabled and do nothing there. Off macOS the Edit menu acts on a
  web page (Browser and Web App tabs) once the page has been clicked, not when it has the
  keyboard otherwise (an `autofocus` field).
- The Comments tab's code and generated prompt, and the Settings tab's Raw Workspace JSON,
  are each one run of GPUI Kit text: a drag past the end of a line copies the line break
  too, where Chromium's selection stops at the end of the line.
- A press on one of the app's classic scrollbars (search results, the Files tree and its
  file's comments, the Markdown preview, the Comments tab's code, the Raw Workspace JSON)
  clears the current text selection, as GPUI Kit's window selection clears it on every left
  press, before the bar sees it; Chromium keeps the selection.
- In the Review diff, a review comment's text and the diff's code are selected apart: a
  selection running from a comment into the code (or back) copies the comment text only,
  not the code it spans, where Electron copies both, in reading order.
- View → Actual Size, Zoom In and Zoom Out (in Chromium's steps and 25%–500% limits, for every
  window) restore their level at the next start from the native app's own `zoom.json`
  (Electron keeps its level in Chromium's `Preferences`, which Chromium rewrites whole), so
  each app starts at its own last level. The macOS window buttons stay where they are, as in
  Electron.
- Review's diff is coloured as Electron's (VS Code's Dark+), but through GPUI Kit's
  tree-sitter grammars, not shiki's TextMate ones: control keywords (`if`, `return`,
  `import`) are purple (`#C586C0`) in Rust, JavaScript, TypeScript, Python and Go only,
  and blue like every other keyword elsewhere, as the other grammars' queries capture all
  keywords alike; a few names are classed differently (Rust's all-caps
  constants and `macro_rules!` are blue, `Self` teal, TypeScript's class names in
  expressions and JSX tags light blue); and code inside Markdown (inline code, links,
  emphasis, a fenced block's language) is plain, and every name in a Rust macro's arguments
  light blue (a call there is not yellow, nor a module teal), as the diff skips injected
  grammars.
- The code editors (Files viewer, Editor tab), through GPUI Kit: Tab inserts a whole indent
  step, not spaces up to the next tab stop; 8px of padding stays under the last line; the
  current line's outline is drawn over the text, so it stops at the scrollbar; a scrollbar
  fades 2 s after scrolling stops (Monaco: 0.5 s). The Kit has no R, XML, Less or Dockerfile
  grammar: `.r` files are plain, and `.xml`, `.less` and `.dockerfile` files are coloured
  with the HTML, CSS and shell grammars. The word underlined under Cmd/Ctrl keeps the
  text's colour (Monaco's link is `#4E94CE`).
- There are no language servers behind the code editors. Monaco's TypeScript / JavaScript
  service (semantic errors such as unresolved imports, IntelliSense, hover types, signature
  help, references), JSON schemas (validation, completion, hover), JSON completion without
  a schema (the keys and values the file uses elsewhere, empty objects and arrays, the other
  boolean), hover documentation, CSS value and selector completion, HTML attribute value and
  entity completion, and formatting are missing. A problem's hover shows its message without
  the source and code Monaco adds after it (`json(513)`, `(1005)`). Picking an HTML end
  tag typed on a line of its own does not move it to its start tag's indentation, as
  Monaco's does. TypeScript and JavaScript syntax errors are read from GPUI Kit's tree-sitter
  grammars, not TypeScript's parser: they are marked where the parse fails, as "';'
  expected."-style messages or "Unexpected token.", and a `.tsx` file is read with JSX
  (Monaco's worker reads it as plain TypeScript and marks the JSX). CSS files get the lint
  warnings that read rules and declarations, not the CSS parser's errors; SCSS and Less files
  get none. JSON whose arrays and objects nest more than 512 deep, and style sheets whose
  blocks do, get no marks or colours (Monaco's workers give up only where JavaScript's stack
  runs out, a few thousand and about a thousand levels deep). Colours are painted behind
  their text, not as a swatch before it. A completed CSS property is inserted as `name: `
  and an HTML attribute as `name=""` with the cursor after it (Monaco's snippets put it
  between `:` and `;`, or the quotes). Other suggestions are the file's own words (Monaco
  also takes words from other open files of the same language, and suggests nothing while
  typing in comments and strings). The right-click menu has no Go to References, Peek,
  Format Document or Command Palette. Tab indents while suggestions are open (Enter takes
  one, as in Monaco).
- Neither the Files viewer nor the Editor tab has Monaco's Peek Definition (Alt+F12, and
  the Peek submenu of the right-click menu): GPUI Kit's editor has no view zones to show a
  peek in. Open Definition to the Side (Cmd/Ctrl+K F12) goes to the definition as F12
  does, as in Electron, whose editor opener opened it in a new Editor tab (there is no
  split editor in either app).
- The Files viewer's right-click Search reads the word under the cursor in the language
  there, as Monaco does (JavaScript or CSS in the `<script>` and `<style>` blocks of an HTML,
  Vue, Svelte or PHP file, a Markdown code fence's language), except in Less's backquoted
  JavaScript, read as Less, and in the `<script>` and `<style>` blocks of a Handlebars,
  Razor, Twig or MDX block (a code fence, or a script block of that `type`), read as that
  language.
- With the sidebar collapsed the header keeps clear of the macOS window buttons (Electron's
  title runs under them).
- GPUI cannot decode AVIF, so the Files viewer shows `.avif` images in a webview (WebKit
  draws them, centred and scaled down as Electron's `<img>`). Review's image diffs, which
  scroll in a list, decode an `.avif` change's sides with rav1d (`vendor/avif-decode`) in a
  child process, the app run with `--decode-avif`: rav1d 1.1.0 aborts the process on some
  corrupt frames, so such a side shows as a broken image with its alt text
  (`Before: <path>`), as Chromium shows an image it cannot decode, and the app carries on.
  avif-decode's parser (avif-parse 2.1.0) reads still, single-image AVIFs only: an animated
  AVIF (`avis`) or one stored as a grid of tiles also shows in Review as a broken image with
  its alt text, where Electron shows the picture (animated, for an `avis`), and an AVIF's
  rotation, mirroring and crop (`irot`, `imir`, `clap`) are not applied.
- The Markdown preview shows raw HTML as its text, as react-markdown does, but each HTML
  block (`<p align="center">…</p>` and the like) on lines of its own with the paragraph gap
  between them: Electron's browser runs consecutive HTML blocks together into one paragraph
  of text. Images show as broken with their alt text, as in Electron (where none loads),
  with the grey image-off icon in place of Chromium's broken-picture icon.
- PDFs show in WebKit's viewer, which has no toolbar: a strip above it has Chromium's Zoom
  out, zoom level, Zoom in and Print (100% is the width WebKit fits the pages to, not the
  pages' own size). There is no page box, thumbnails, fit or rotate; those need a bundled
  PDF viewer (pdf.js).
- While a tab is dragged, the outline of where it would dock jumps to its place (flex-layout
  slides it there in 0.3 s), the edge docks' arrows are text triangles, and a pane showing a
  web page (Browser, Web App, a PDF) draws its page over the outline and edge docks: native
  webviews draw above the window's content.
- A session dragged in the sidebar or the rail, or a workspace dragged in the tree, dims as
  in Electron, but GPUI has no drag image of its section or row: a chip with its name follows
  the pointer instead.

## Known differences from Electron (deliberate)

- There is no separate splash window: the main window opens at once and shows Electron's
  splash ("TreeTerm", the spinner and "Connecting to daemon...") in its pane until the local
  session has loaded. Electron showed it in a small always-on-top window while it started the
  daemon, then opened the main window on "Select a workspace to get started" while it
  connected.

- Review comments made in the Files tab are saved with the workspace-relative path, as the
  Review tab saves them (Electron's Files tab saves the absolute path), so they also show in
  Review and can be pushed to GitHub. Comments with Electron's absolute paths still show in
  the Files viewer and the Comments tab; Electron's Files tab does not show the relative ones.

- After a reconnect a session keeps the workspace it had active (its first one if that is
  gone), and the window stays on the session it shows; Electron made the reconnected
  session's first workspace active and switched the window to it, even from another
  session's terminal in use.
- A session's fold and its workspaces' expand state survive a reconnect (Electron rebuilt
  the session panel on reconnect, unfolding it and expanding every workspace).
- A workspace counts as viewed (it gets no unread marker, and its marker is cleared) only
  while it is the one on screen. Electron counted the one its navigation last named (the
  one last clicked, or a session's first), also after a quick fork or a new workspace put
  another one on screen.
- A workspace's attention flash plays once per change of its activity, on every row that shows
  it. Electron also replayed it whenever a row was drawn anew (a folded session unfolded, the
  sidebar collapsed or expanded, a workspace newly marked favourite) while the workspace was
  in an attention state and its unread marker was not showing.
- A workspace write still in flight when the connection drops is written after the reconnect
  too (Electron's flush waited for it to fail and then dropped it).
- The session's force-unlock button hides once the lock has expired (the daemon ignores an
  expired lock); Electron shows it as long as the daemon still reports the lock.
- A stale local daemon socket is replaced at once (Electron waited up to 5 s for it to
  answer). Connections to use still wait up to 5 s for the daemon, as in Electron.
- Exit and Kill Daemon asks about unmerged workspaces before it stops the daemon, so Cancel
  leaves everything running; Electron stopped the daemon first and asked after, so Cancel left
  the window on a dead daemon. Pressed while Quit's confirmation is up, it makes that
  confirmation's Close Anyway stop the daemon too.
- A remote daemon that has no SessionWatch at all (`UNIMPLEMENTED`) is reported as outdated
  too; Electron only recognised a `NOT_FOUND` error about the session. A Reconnect from the
  error waits for the session again, so it stops at the error once more instead of
  reconnecting in a loop.
- SSH Upload's "Browse..." is disabled while the connection is down; the form keeps its file
  and directory under "Connection is … — uploads are disabled until connected.", as in
  Electron. Electron opened the browser, which could only show its failed listing.
- SSH Monitor shows the latest run's result: a run that finishes after a later one is
  dropped (Electron showed whichever finished last). A run whose result the daemon never
  sends reads "No result from the daemon after 40s" when the client gives up on it, unless a
  later run's result is already shown; Electron left it pending.
- Placeholders for refs another window removed are dropped with the ref.
- A collapsed workspace stays collapsed until it gains a child or is expanded (Electron
  re-expanded every parent whenever any workspace changed).
- The Settings item of a workspace row's or a favourite's menu opens or focuses that
  workspace's single Settings tab and shows the workspace (Electron added another Settings
  tab each time, without showing the workspace).
- A replayed OSC 52 copy or bell is ignored (it already happened when it first ran).
- Corrupt `settings.json` is an error (logged; defaults used) instead of being overwritten.
  Only Save in Settings and the sound toggle write over it; changes made in passing (a folder
  picked with Browse..., a saved or removed SSH connection, its port forwards, the prompt
  debugger's Update) are logged and not saved until it reads again.
- A corrupt `ui-state.json` (session names and order, Recent Directories) is logged and kept as
  `ui-state.json.corrupt-<ms since the epoch>`; the state starts empty and is saved again as
  usual. Electron dropped a corrupt stored value silently and overwrote it.
- A `logLevel` in `ui-state.json` that names no level is logged as a warning and ignored;
  Electron ignored an unknown `treeterm:logLevel` silently.
- Show Raw Characters logs its `[RAW]` lines to the app log (stdout), with the PTY's id, and
  its hint says so; Electron logged them to the DevTools console. They are info lines, so a
  `logLevel` above `info` hides them, where Electron's `console.log` always printed. As in
  Electron, only the terminals of the shown workspace log, starting over each time it is
  shown again; while a session pane is up none do, where Electron kept logging the session's
  active workspace's terminals behind it.
- An entry of the worktree registry (`~/.treeterm/worktrees.json`) that cannot be read, such as
  a hand-edited one with no `branch` or a fractional `lastUsedAt`, is left out of Auto Open
  and the recent workspaces (logged) and kept in the file as it is. Electron listed it with
  whatever fields it had.
- A git info refresh that git cannot answer (the daemon unreachable, git killed by the
  daemon's 30s timeout, the workspace's directory gone) leaves the workspace's git repo,
  branch and root as they were, and the git status button's tooltip says why. Electron
  recorded the workspace as no git repo then.
- Merge-and-remove stages untracked files before its WIP commit (Electron's `commit -am`
  skips them, failing the merge or losing new files with the deleted worktree).
- Review's Refresh also reloads the Commits list once it was shown, keeping as many commits
  as are listed and the picked commit (Electron never reloaded the list after its first load).
- Review's find in the diff searches only files whose rows are drawn: a diff too large to draw
  is searched once "Load anyway" draws it. Electron listed its matches but could not show
  them (choosing one opened the file on the size notice).
- Review reads the diffs of the files it has read again with each reload (Refresh, its
  workspace shown again, staging), keeping what they show until then; Electron kept a file's
  contents once loaded until its own refresh button was pressed.
- Review's find reads every file of the view's list, in list order, also the files the file
  filter or a folder filter hides; Electron read only the files shown, in the tree's order. Its
  count can read "…" a little longer while hidden files are read.
- Review shows a renamed file of the branch or of the index as a rename (its changes since
  its old path); Electron read both sides at the new path, so the whole file showed as added.
- Review measures a diff's size from git's whole-file patch (`git diff -U1000000`); Electron
  read both files. With "Ignore whitespace changes" on, unchanged lines count as the new side
  has them. A file whose patch has no hunks is never too large: a pure rename or a mode-only
  change. Electron measured both files, so a large file in either case showed "This diff is
  large (...)" and "Load anyway".
- Review's "Ignore whitespace changes" diffs git's patch again over trimmed lines, as jsdiff
  does (a change of indentation, trailing spaces or line ending is unchanged; one of spacing
  within a line still shows). A line that is unchanged only once trimmed shows its new text
  on both sides of the split view; Pierre showed each side's own.
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
- A terminal takes keyboard focus when it comes into view as the active tab, but not from an
  open dialog, and not again on every workspace change while it stays shown (Electron
  focused it on each, taking focus from the sidebar's inputs, from dialogs and from a
  terminal clicked in another pane).
- A tab dropped in a strip, or docked into or beside a pane or at the layout's edge, becomes
  the active tab (`activeTabId`). Electron showed the dropped tab, but its `activeTabId`,
  which its tab shortcuts act on, stayed on the tab that was active before.
- A tab opened with a pane's "+" opens in that pane (Electron opened it in the last active
  pane).
- In a strip too full to show every tab, tabs scrolled out of view are no drop target: a tab
  dropped past the last tab shown lands after it. flex-layout also counted the hidden tabs'
  places past the strip's edge, so a drop over "+" could land among them, out of sight.
- An AI harness set to Disable Scrollbar draws no scrollbar on its terminals. Electron's
  `disable-scrollbar` rule targets `.xterm-viewport`, which no longer holds the scrollbar
  since xterm.js 6, so the bar still shows there. In both apps the wheel scrolls the
  scrollback (or goes to a program that asked for mouse events).
- An AI harness's "Background color" is read as CSS Color 3 (`#rgb`/`#rrggbb` with or without
  alpha, `rgb()`, `hsl()`, `hwb()`, colour names) and drawn opaque; anything else gives
  xterm.js's black, as an unreadable colour does in Electron, also Chromium's newer `lab()`,
  `oklch()` or `color()`. A change reaches open harness tabs at once; Electron kept the colour
  a tab's view was mounted with until it was mounted again.
- An AI-harness tab whose state is not a harness state (no `sandbox` object, from a corrupt
  workspace file) shows "Error: Invalid AI harness state" in red, the message Electron's
  `AiHarness` view has for it; Electron's renderer checked first and left such a tab blank.
  In both apps its terminal and analyzer run on behind it.
- URLs in a terminal's text are links in every terminal tab, with the schemes ghostty-web
  links (`http(s)`, `mailto`, `ftp`, `ssh`, `git`, `tel`, `magnet`, `gemini`, `gopher`,
  `news`): Cmd/Ctrl+click opens one, and it is underlined only while Cmd/Ctrl is held, so a
  plain click still selects its text. Electron's terminals (xterm.js) linked only OSC 8
  hyperlinks; its Ghostty tabs opened URLs on a plain click. An OSC 8 hyperlink is a link
  only to an `http(s)` URI, as in xterm.js, also where Electron's Ghostty tabs opened any.
- A double-click, or a right-click on macOS, on an OSC 8 hyperlink selects the word under the
  pointer, as on any other text. xterm.js selected the hyperlink's whole text.
- "Terminal (Ghostty)" tabs (`ghostty-terminal`, 👻, offered in the new-tab menu as in
  Electron, which draws them with ghostty-web) run in the same native terminal as
  "Terminal". They copy a selection to the clipboard as a drag, double-click or triple-click
  makes it, as ghostty-web does (it has no triple-click), but otherwise select as xterm.js
  does (its word separators, Shift+click, Alt+drag and Alt+click, the macOS right-click). A
  plain click copies nothing; ghostty-web copied the character under it.
- The live view of a PTY (Active Processes, and Open in the session pane's TTY list) is the
  app's terminal: the terminal settings' font, cursor style and blink, the terminal palette
  (ANSI colours, cursor, selection), 50,000 lines of scrollback, and a tab's scroll badge,
  ALT SCREEN badge and refresh button. Electron's `PtyViewer` drew 13px `monospace` in
  xterm.js's default colours (it set only the background and foreground), with an
  always-blinking block cursor and 5000 lines. When its stream ends or fails without the
  program exiting, the view says so ("Terminal disconnected", or the error) in a banner;
  Electron's viewer showed nothing.
- The attention ding is the system bell (Electron synthesised an 880 Hz tone).
- The analyzer reads the tab's own terminal; Electron attached a second, headless one.
- A change of the analyzer's classification settings (provider, model, reasoning effort,
  endpoint, system prompt, criteria, safe paths) reaches an AI harness tab at once and counts
  as a change of its screen even if changed back within the idle debounce: the tab goes
  Working and its screen is classified again (from the remembered answer). Electron compared
  the settings themselves every 500 ms, so a change undone in time went unnoticed.
- A Prompt Description that sends nothing (no harness running yet, or the write failed)
  shows why in a notification; Electron failed silently. In both, the description is no
  longer offered.
- A workspace's title or branch re-label is under way for the whole workspace: one started
  from the tree's Refresh Title & Description also spins and disables the header's refresh
  button, and one from its Refresh Branch Name also shows "Renaming..." on the branch badge
  and leaves Auto rename branch out of its menu, until it finishes (Electron's button and
  badge knew only their own clicks). A re-label whose AI Harness tab closes (or whose
  terminal is replaced) before the LLM answers applies nothing and alerts "The AI Harness
  terminal went away before the labeller finished" (Electron applied the late answer).
- Go to definition moves within the Files viewer or the Editor tab when the definition is in
  the file it shows; Electron opened that file again in a new Editor tab.
- An Editor tab keeps what was typed in it when another window, saving something else at the
  same moment, wins the race to write the workspace with the tab's text from a moment before
  (up to 5 s): the typed text stays and is written again. Electron's editor took the older
  text, losing the keystrokes typed meanwhile. Text another window or Electron types is taken
  as it arrives, as in Electron.
- The code editors colour files Electron shows as plain text: Makefile, Dockerfile,
  CMakeLists.txt, `.toml`, `.markdown`, `.mjs`/`.cjs`/`.mts`/`.cts`, `.jsonc`,
  `.cc`/`.cxx`/`.hh`, `.zig`, `.ex`/`.exs`, `.proto` and `.diff`/`.patch`.
- The SSH picker's Add trims the forward's remote host, so a blank one is ignored like an
  empty one (Electron added a forward to " "). Enter on a focused button there presses only
  that button; Electron's dialog then also ran Connect with the form as it was.
- In Add Child Workspace, Enter on a focused tab or on Settings that starts opening the picked
  existing or recent worktree does only that; in Electron the button then also pressed itself
  (switching the tab or toggling Settings).
- In Open Workspace's remote directory browser, Enter in the path field or on a focused
  browser button ("..", a breadcrumb, an entry, Cancel or Select) acts on the browser only;
  Electron also opened the folder field's path, when it held one, and closed the dialog.
- `treeterm ssh` rejects a target whose host contains `@` or whose port is outside 1–65535
  (`me@a@box`, `me@box:0`, `me@box:70000`); Electron passed them on to ssh, which cannot
  use such a port and splits `me@a@box` on its last `@`, connecting to `box` as user `me@a`.
- A panic on the UI thread ends the app process: GPUI cannot recover inside the process, so
  there are no per-workspace or per-tab error fallbacks. A new process shows Electron's
  "Application Error" screen, with the panic's message and a Show Stack Trace toggle (Electron's
  app-level screen showed no error details; the panicked process and its console are gone),
  and its "Reload Application" starts the app again: terminals survive in the daemon, SSH
  sessions must be reconnected, and a directory given on the command line is not reopened (as
  after Electron's reload). The report stays in `crash.json` beside `settings.json`.
