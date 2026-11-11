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
  channels), Electron-compatible socket path, local daemon autostart, reconnect with backoff.
- **Terminal (M1):** `alacritty_terminal` core fed from `PtyStream` (replay-aware query gating,
  OSC 52 rules), GPUI renderer (grid-forced shaping, wide chars, cursor, selection, IME
  preedit, width-limit strip, scrollbar), xterm.js-compatible keys/mouse/paste, links, scroll
  badge/pin, ALT SCREEN, size mismatch, overlays, refresh, status bar (activity, idle detector,
  col limit). 200k lines render in ~0.4 s.
- **Session/workspace sync (M2, core):** full write side of `createSessionStore` (ref CAS with
  retry, body CAS with parentHash chain, echo suppression, conflict rewrite, watch reopen,
  lock ops), workspace tabs (add/close/activate/state, PTY creation keyed by `ptyHandle`,
  close-on-exit, kill-on-close), app registry from settings (terminal variants, AI harness,
  custom runners), layout-ordered tab strip, new-tab menu, workspace removal (keep both).
- **Keys (M3, part):** prefix mode + overlay, workspace focus, Cmd/Ctrl+1–9, app menu,
  Exit and Kill Daemon.
- **Settings (M3, part):** full `settings.json` model with load/save (unknown keys kept).

Not yet:

- Git layer and worktree flows: open workspace, create child (new/existing/recent/branch/PR),
  merge/abandon/remove with git cleanup, git status polling.
- Settings dialog, close-confirm, context menus beyond removal, tree drag/drop, favourites UI.
- SSH/remote connections, port forwards, session info pane, CLI (M4).
- AI harness analyzer/LLM/chat, attention (M5); filesystem/editor (M6); review/diff/GitHub (M7);
  webview panes, PDF, 3D (M8); packaging (M9); parity sweep and cut-over (M10).
- Linux build verification (CI job added, not yet seen green).

## Known differences from Electron (deliberate)

- Placeholders for refs another window removed are dropped with the ref.
- A replayed OSC 52 copy or bell is ignored (it already happened when it first ran).
- Corrupt `settings.json` is an error (logged; defaults used) instead of being overwritten.
- Daemon fix: each gRPC connection now gets its own id, so the session lock actually excludes
  other connections (it was shared by all connections before).
