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
