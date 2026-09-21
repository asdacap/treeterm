---
name: verify
description: Launch the built TreeTerm Electron app in an isolated sandbox and drive it with Playwright to verify renderer/main/daemon changes end-to-end.
---

# Verifying TreeTerm end-to-end

Build first: `npm run build` (produces `out/main/index.js`, `out/renderer/`, `out/daemon-rs/treeterm-daemon`).

## Isolated launch (doesn't touch your real settings, daemon, or sessions)

Drive with Playwright's `_electron` (already in devDependencies). Key gotchas learned the hard way:

- **`ELECTRON_RENDERER_URL` may be set in your shell** (from a past `npm run dev`). If it leaks into the launch env the app loads `http://localhost:5173` and shows a blank white window (`chrome-error://chromewebdata/`). Set it to `undefined` in the `env` you pass to `electron.launch`.
- **Daemon socket path must be short.** The daemon rejects sockets longer than macOS `SUN_LEN` (~104 bytes) with `path must be shorter than SUN_LEN`. Use something like `/tmp/tt-verify/daemon.sock` via `TREETERM_SOCKET_PATH`, never a deep scratchpad path.
- **`userData` cannot be redirected via `HOME`** on macOS. Use a bootstrap main script as the Electron entry:
  ```js
  const { app } = require('electron')
  app.setPath('userData', process.env.TEST_USER_DATA)
  require('<repo>/out/main/index.js')
  ```
  Seed `settings.json` inside `TEST_USER_DATA` (e.g. an `aiHarness.instances` entry with `command: "bash"` to get an AI harness tab without a real AI CLI).
- **Override `HOME`** in the launch env anyway — the Rust daemon persists to `$HOME/.treeterm`, so this isolates session state. Kill the daemon after via `$HOME/.treeterm/daemon.pid` (it survives app close by design, and a leftover daemon restores old tabs into your next run).
- Pass `--workspace=<dir>` (a git repo; `git init -b master` a temp dir) and `NODE_ENV=test` (skips the loading window).

## Driving the UI

- Default terminal ready: wait for visible `.xterm`.
- New tab: click `button.flexlayout-new-tab-btn`, then `.app-menu-item` by app name.
- Terminal and AI harness tabs share the same layout: `.terminal-app-wrapper > .terminal-app-body` (the terminal) + `.terminal-status-bar` (badge `.activity-state-badge`, sliders `.terminal-toggle`). The plain terminal bar has the "Idle detector" slider; the harness bar adds "Auto-approve safe". Hidden tabs keep their DOM, so scope selectors to the visible bar (e.g. `.terminal-status-bar:has-text("Auto-approve")`) rather than `.first()`. The checkbox inputs are visually hidden — click the `.terminal-toggle` label, not the input.
- The workspace is not auto-selected on a fresh isolated profile: click its name in the tree (`text=<dirname>`) before waiting for `.xterm`.
- Read terminal text through the engine buffer published on the container (`.terminal-container` element's `.terminal` property, `buffer.active.getLine(y).translateToString(true)`) — DOM scraping doesn't work for either engine. See `e2e/helpers.ts#getTerminalText`.
- Without LLM auth the analyzer logs `401 Missing Authentication header` and the badge shows `error` — expected in an isolated env, and proof the analyzer pipeline is live.
