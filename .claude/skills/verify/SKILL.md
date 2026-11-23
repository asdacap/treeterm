---
name: verify
description: Launch the TreeTerm app (app-rs) in an isolated sandbox (its own HOME, daemon and socket) and drive it through its automation socket to verify app, model and daemon changes end-to-end.
---

# Verifying TreeTerm end-to-end

## Build

From the repo root:

```sh
(cd daemon-rs && cargo build -p treeterm-daemon)
(cd app-rs && cargo build -p treeterm-app --features automation)
```

- App builds do not rebuild the daemon. Rebuild it after daemon changes, or the sandbox runs a stale one.
- Build app-rs with the same profile settings every time (e.g. always the same `CARGO_PROFILE_DEV_DEBUG`). Mixing them duplicates `app-rs/target`, which has grown past 60 GB that way. Run `cargo clean` there when disk space is low.

## Isolated launch (doesn't touch your real settings, daemon or sessions)

```sh
TT=/tmp/ttv   # keep it short: the daemon socket path must stay under ~104 bytes on macOS
rm -rf "$TT" && mkdir -p "$TT/home" "$TT/repo"
git -C "$TT/repo" init -q -b master
git -C "$TT/repo" -c user.name=v -c user.email=v@example.invalid commit -q --allow-empty -m init

# settings.json: macOS reads it from "$TT/home/Library/Application Support/treeterm",
# Linux from "$TT/home/.config/treeterm". Point the LLM somewhere local: the terminal
# analyzer sends screen text to the default OpenRouter endpoint even without an API key.
CONF="$TT/home/Library/Application Support/treeterm"
mkdir -p "$CONF" && echo '{ "llm": { "baseUrl": "http://127.0.0.1:9" } }' > "$CONF/settings.json"

HOME="$TT/home" XDG_CONFIG_HOME="$TT/home/.config" \
TREETERM_SOCKET_PATH="$TT/d.sock" \
TREETERM_DAEMON_DIR="$PWD/daemon-rs/target/debug" \
TREETERM_AUTOMATION_SOCKET="$TT/a.sock" \
app-rs/target/debug/treeterm "$TT/repo" > "$TT/app.log" 2>&1
```

Run the last command in the background. The app logs to stdout (`RUST_LOG=treeterm_core=debug` etc. for more).

- To get an AI harness tab without a real AI CLI, add an `aiHarness.instances` entry with `"command": "bash"` to the seeded `settings.json`.
- The app starts its own daemon from `TREETERM_DAEMON_DIR` on `TREETERM_SOCKET_PATH`, which persists to `$HOME/.treeterm`, so session state stays in the sandbox too.

## Driving the app

The automation socket takes one command per line and answers each with one JSON line. A small client that takes each command as one argument:

```sh
tt() { python3 -c '
import socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1]); f = s.makefile("rw")
for cmd in sys.argv[2:]:
    f.write(cmd + "\n"); f.flush(); print(f.readline(), end="")
' "$TT/a.sock" "$@"; }

tt state
tt 'type echo hello' 'key enter' "screenshot $TT/shot.png"
```

The socket appears once the window is up: retry `state` until it answers. Commands (full list in `app-rs/crates/treeterm-app/src/automation.rs`):

- `state`: what the window shows: `body` (`starting` or `connected`), `sessions`, `workspaces` (each with its `tabs`: `id`, `app`, `title`, `state`), `active` and `shown`, `terminals` (`screen` text, `focused`, `overlay`, `cols`/`rows`), and `dialogs`. Wait for a condition by polling it.
- `select <workspace-id>`, `tab <workspace-id> <tab-id>`, `newtab <workspace-id> [app-id]`, `closetab <workspace-id> <tab-id>`, `open <directory>`, `fork <workspace-id>`, `fileopen <path>`, `workspacedialog`, `action <action>` (e.g. `action treeterm::ActiveProcesses`).
- `type <text>` (keystrokes), `key <keystroke>` (`enter`, `ctrl-c`, `cmd-shift-p`).
- `click`/`rclick`/`dblclick <x> <y> [modifiers]`, `move <x> <y> [modifiers]`, `drag <x1> <y1> <x2> <y2> [modifiers] [hold]`, `release <x> <y>`, `scroll <x> <y> <dy> [modifiers]`: positions in window points, held keys such as `alt` or `cmd-shift`.
- `screenshot <path.png>`: renders the last frame offscreen, so no screen-recording permission is needed. Read the PNG to see it.

Gotchas:

- The sandbox window is occluded, so it gets no display-link frames: each command draws one frame first. A click's result appears in the next command's frame, and work it schedules for the next frame shows up one command later still. Send `state` before a `screenshot` when in doubt.
- Native webviews (Browser and Web App tabs, the PDF and 3D model viewers) draw above the window and do not appear in screenshots.
- With the LLM pointed at `127.0.0.1:9`, the analyzer's requests fail ("Connection error: …") and an AI harness tab's activity badge shows the error. That is expected, and proves the analyzer pipeline is live.

## Cleanup

The daemon survives the app by design, and a leftover daemon restores old tabs into the next run. Stop both, then remove the sandbox:

```sh
pkill -f "$TT/repo"   # the app (or kill the background job)
kill "$(cat "$TT/home/.treeterm/daemon.pid")"
rm -rf "$TT"
```

## Automated end-to-end tests

`app-rs/crates/treeterm-app/tests/e2e.rs` does the same in code (daemon lifecycle, terminal output across restarts). They open windows, so they run only when asked:

```sh
(cd daemon-rs && cargo build -p treeterm-daemon)
cd app-rs && cargo test -p treeterm-app --features automation --test e2e -- --ignored --test-threads=1
```
