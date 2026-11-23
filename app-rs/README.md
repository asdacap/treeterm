# TreeTerm app (`app-rs/`)

The TreeTerm desktop app: a single Rust/GPUI process over the daemon in `daemon-rs/`. It
replaced the Electron app, which was removed after the parity audit in `parity-audit/` (its last
version is commit `f05be955`); it reads the sessions, workspace files and `settings.json` that
app wrote. Crate layout and rules are in `AGENTS.md` ("App"); the full plan (milestones M0–M10)
lives with the project notes.

Mentions of Electron below compare with that app, as its code was at `f05be955`.

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
`settings.json`: `debug`, `info`, `warn`, `error` or `silent`, in any case, read at start-up;
else `info`. A panic
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

- **Daemons.** `build-daemons` builds `daemon-rs/` (`cargo build --release`) with the daemon's
  own (default) toolchain, into `<repo>/out/daemon-rs/`: the local daemon for
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
  channels), the socket path the Electron app used, local daemon autostart.
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
  kill-on-close), app registry from settings, tab layout in flex-layout's model, as the
  Electron app saved it (`metadata.layoutModel`: tabsets side by side or stacked as saved, sized by their weights,
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
  (About, Services, Hide, Close Window, Edit's Undo, Redo, Cut, Copy,
  Paste, Paste and Match Style, Delete and Select All on whatever has focus, a Browser or Web
  App page included, Actual Size,
  Zoom In, Zoom Out (the whole window, web pages included; Cmd/Ctrl+= zooms in too; the level
  is kept in `zoom.json` and restored at the next start), Toggle Full Screen, Reset Input, Minimize, Zoom,
  Bring All to Front, Help; its commands also work while a dialog has focus;
  off macOS the window draws the menu bar, and a focused terminal keeps the keys it encodes,
  as with xterm.js), Exit and Kill Daemon; all 13 Settings sections (reopened on the one shown
  last; Escape leaves the dialog open), whose controls Tab reaches as a browser's native ones
  (Space or Enter presses a button, Space toggles a checkbox, Space or an arrow opens a
  select, Tab leaves a textarea, a linked label focuses its control, the panel scrolls a
  control focused from the keyboard or its label into view, and a keybinding button keeps the
  focus while and after it records); live reload when
  `settings.json` changes on disk; close confirmation for unmerged workspaces; Window → Active
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
  highlighted viewer, Markdown preview, images, PDF and 3D models (webview; pdf.js, with its
  CMaps and standard fonts, and three.js bundled by `assets/pdf-viewer/build.sh` and
  `assets/model-viewer/build.sh`),
  download, search panel (in file / in repo), find, go to
  definition (F12, Cmd/Ctrl+click, the context menu; a word with a definition is underlined
  while Cmd/Ctrl is held over it) and Peek Definition (Alt+F12, the Peek submenu), review
  comments (click a line number; commented lines marked in the glyph margin); editor tab with
  go to definition, Change All Occurrences (Cmd/Ctrl+F2), Format Document and Format
  Selection for JSON (Shift+Alt+F, Cmd/Ctrl+K Cmd/Ctrl+F), and suggestions while typing
  (Ctrl+Space asks; none in comments and strings; Enter or Tab takes one): CSS, SCSS and
  Less property names; HTML elements, end tags, attributes, attribute values (picking an
  attribute that has some asks for them) and character entities (Monaco's HTML worker,
  ported); JSON keys and values the file uses elsewhere; else the words of the file and of
  the workspace's other open files of its language. Both code editors mark problems (their
  message on hover): JSON's (Monaco's JSON worker, ported), TypeScript and JavaScript syntax
  errors, and CSS lint warnings (empty rules, unknown and duplicate properties, unknown
  at-rules, incomplete `@font-face`); they show the HTML worker's documentation on hover
  (elements, attributes, attribute values, character entities) and the CSS worker's (a
  declaration's property), and highlight an element's start and end tag names while the
  cursor is on one; they paint CSS, SCSS and Less
  colours, and underline links ("Follow link (cmd + click)" on hover): Cmd/Ctrl+click opens
  an `http(s)://` link in the browser and a `file://` link in an Editor tab, at the line its
  `#L<n>` names. The Files viewer and the Editor tab save their scroll offset in the tab
  state when the tab is hidden and open there again after a restart; a line they were opened
  at is moved to once.
- **Review (M7):** committed / uncommitted / commits views, stage/unstage/commit, inline
  comments (`reviewComments` workspace metadata, the Electron app's format), viewed files, image diffs (before / after, SVG
  and AVIF included), each file's diff read once its section comes within 1500px of the viewport
  ("Scroll to load diff" until then; find reads every file, four at a time), loading,
  failing (also when its part of the patch does not parse) and refreshed (its header's
  refresh button) on its own, a diff over 20,000 lines or 2 MiB drawn only after "Load anyway",
  favourites outside the diff shown in the Files viewer (with its comments) in the diff's place,
  find in the diff (Cmd/Ctrl+F: matches in the file at the top of the diff, then the rest of
  the diff, then the repo; the diff's menu searches or copies the selected text or the word
  under the pointer), prompting the AI harness; live merge
  output; git status (uncommitted / conflicts / behind, Pull) and PR indicators.
- **Web (M8):** Browser tabs in a native webview. The start page (and a full
  address typed) must be on localhost, but links, redirects and frames then load in the tab
  from any host; links to other apps (`mailto:`, `vscode:`) and popups open in the system,
  `file:` links are refused. The toolbar follows the webview's own history (Back / Forward),
  loading (Stop and the progress line from the request on) and navigations within the page
  (`pushState`, fragments), which the tab also remembers; Reload bypasses the cache. A load
  that fails before its page comes (server down; WKWebView's navigation delegate reports it,
  also for a load the page starts while it still loads; a load stopped or turned into a
  download is no failure) shows "This site can’t be reached" and its URL in the page's place
  until a page comes, the address bar staying on the page shown, and Reload loads it again
  (WKWebView draws no error page of its own, WebKitGTK does); Web App tabs (free port
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
  title/branch refresh, Auto Open All Recent Worktrees (worktree registry
  `~/.treeterm/worktrees.json`), upstream warning before forking,
  force-unlock of a held session lock; session renaming (double-click its name; kept in
  `ui-state.json` by connection target); the Open Workspace dialog's Recent Directories (kept
  in `ui-state.json` by connection target; the Electron app's, from its localStorage, are
  added once after the ones already listed); session pane with info, logs, port forwards, a system
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
  without HTTP/2 framing or message prefixes) and per workspace (PTY
  output, workspace header). Totals and rates are kept across
  reconnects and a failed connection connected again, until the connection is disconnected
  or the workspace removed.
- **CLI:** `treeterm [dir]`, `treeterm ssh user@host[:port]`, `treeterm shutdown-daemon`.
- **Packaging (M9):** `cargo run -p xtask -- dist` (macOS `.app` + zip verified; Linux
  tarball and the Nix package untested).
- **Cut-over (M10):** the parity audit and its fix campaign (`parity-audit/STATUS.md`), then
  the Electron app was removed.

Not yet:

- Running on Linux (the workspace compiles and is clippy-clean for Linux:
  `cargo run -p xtask -- check-linux`; CI jobs added, not yet run) and a real SSH host test.

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
- `src/input/popovers/completion_menu.rs`: a snippet completion (`InsertTextFormat::SNIPPET`)
  inserts the text it stands for with its first tab stop selected, as Monaco's snippet
  controller leaves it (a JSON `{}` with the cursor between the braces, an HTML attribute's
  between its quotes). The Kit inserted the snippet's source, `$1` and all. Only that first
  stop is reached: there is no snippet session, so Tab indents rather than going on to the
  later stops (`$2`, then `$0` or the end).
- `src/input/popovers/completion_menu.rs`: a completion whose command is
  `editor.action.triggerSuggest` asks for suggestions again once it is in and the menu is
  dismissed, as Monaco runs its command (a picked HTML attribute's values show between its
  quotes). The Kit ignored the command.
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

## Coming from the Electron app

[`parity-audit/DIFFERENCES.md`](parity-audit/DIFFERENCES.md) lists where the app looks or
behaves differently from the Electron app: what GPUI and GPUI Kit cannot do (no language
servers behind the code editors, no animated or tiled AVIFs in Review, …) and the behaviour
changed on purpose (Remove keeps child worktrees, the attention ding is the system bell, …).
