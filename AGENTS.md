# AGENTS.md

A guide for AI agents working on the TreeTerm codebase.

## Architecture

TreeTerm is a native desktop app plus a persistent daemon per host, connected by gRPC:

- **App** (`app-rs/`, Rust/GPUI, single process): UI, high-level business logic (Git workflows, output parsing, session sync), SSH connections, the gRPC client.
- **Daemon** (`daemon-rs/`, Rust, persistent process): low-level primitives: PTY, exec, file I/O, session persistence.

Communication: App ↔ Daemon via gRPC (tonic) over a Unix socket; a remote daemon is reached through an SSH tunnel.

The daemon survives app restarts; terminal sessions and state persist when the app closes.

The Electron app (`src/main`, `src/preload`, `src/renderer`) has been removed; `app-rs/` replaced it. Its last version is commit `f05be955`. Comments in app-rs that say "Port of `src/…`" or compare a behaviour with Electron's refer to that tree: read it with `git show f05be955:<path>` (e.g. `git show f05be955:src/renderer/store/createSessionStore.ts`).

## Daemon (`daemon-rs/`)

The daemon is a standalone Rust binary. Build it with `cargo build --release` in `daemon-rs/`. `cargo run -p xtask -- build-daemons` in `app-rs/` builds it, plus the static musl daemons for remote Linux hosts, into `out/daemon-rs/`.

| Module | Path | Responsibility |
|--------|------|---------------|
| PTY Manager | `pty_manager.rs` | PTY lifecycle via `forkpty()`, async I/O, scrollback compaction |
| Exec Manager | `exec_manager.rs` | One-shot command execution with streaming I/O |
| Filesystem | `filesystem.rs` | Secure file I/O scoped to workspace boundaries |
| Session Store | `session_store.rs` | In-memory session persistence + watch broadcasting |
| gRPC Server | `grpc_server.rs` | Exposes the operations of `treeterm.proto` via gRPC (tonic) |

The protocol is `crates/treeterm-proto/proto/treeterm.proto`. The `treeterm-proto` crate compiles it for both the daemon and the app.

**Git ops are NOT in the daemon.** They live in the app and run through `ExecStream`.

## Layer Responsibilities

**The daemon exposes stable primitives:** `ExecStream`, `PtyStream`, `ReadFile`/`WriteFile`, session persistence and multiplexing.

**The app handles high-level logic:** Git orchestration, output parsing, error interpretation, workflow composition (stage → commit → push). Business logic lives in the stores of `treeterm-model` (pure logic in `treeterm-core`), not in the views of `treeterm-app`.

**Never add business logic to the daemon.** Never write workspace files directly from the app; use the daemon's `WriteFile` gRPC.

## Directory Structure

```
app-rs/                 # The app (separate Cargo workspace, pinned toolchain)
├── crates/
│   ├── treeterm-core/  # Daemon client, connections, paths, wire formats, pure logic (no UI)
│   ├── treeterm-term/  # Terminal emulation on alacritty_terminal (no UI)
│   ├── treeterm-model/ # Stores and business logic (GPUI entities)
│   ├── treeterm-app/   # GPUI views and the treeterm binary
│   └── treeterm-testkit/# In-process daemon for tests
├── vendor/             # Patched GPUI, GPUI Kit, gpui-webview, avif-decode (see app-rs/README.md)
├── packaging/          # Info.plist, .desktop entry, icon
├── parity-audit/       # The Electron → app-rs parity audit (historical)
└── xtask/              # Daemon builds and packaging
daemon-rs/              # Rust daemon (standalone binary)
├── crates/
│   ├── treeterm-proto/ # treeterm.proto (proto/) and the gRPC stubs generated from it
│   └── treeterm-daemon/# Daemon logic + binary
└── tests/container/    # Boot test of the static daemon on stock distros
```

## App (`app-rs/`)

A single-process Rust/GPUI app. It is a separate Cargo workspace, so the daemon's lockfile and its static musl/Nix builds never pull in GUI crates. It pins its own toolchain (`app-rs/rust-toolchain.toml`).

| Crate | Responsibility |
|-------|---------------|
| `treeterm-core` | No UI. Daemon client (tonic over the Unix socket), local daemon start-up, paths (`settings.json` and the files beside it, where the Electron app kept them), wire formats of the workspace and settings files, pure logic (tree ordering, …). `rt` bridges tokio to any executor: `Fut`/`Sub` cancel their work when dropped. |
| `treeterm-term` | No UI. Terminal emulation on `alacritty_terminal`: `TermCore` fed from the daemon's PTY stream (the daemon owns the size; query answers gated on focus and replay), key/mouse encoding matching xterm.js, viewport queries, fit, idle detector. |
| `treeterm-model` | Stores as GPUI entities (`AppStore`, `SessionStore`, `TerminalModel`, …). **All business logic lives here**, testable headlessly with `TestAppContext`. |
| `treeterm-app` | GPUI Kit views only: render model entities, dispatch to them. No logic. |
| `treeterm-testkit` | A real daemon in-process (`treeterm_daemon::serve`) on a temp socket, for contract and model tests. |

Rules:
- Discriminated unions → enums with data; match exhaustively. `Option` only at wire boundaries (serde), converted on read.
- File formats: workspace files, `settings.json` and the files beside it keep the formats the Electron app wrote, so existing sessions and settings still load. Omit an optional field rather than writing `null`, keep unknown keys, and put data only the app uses in workspace `metadata`.
- Dropping a `Task`/`Fut`/`Sub` cancels it. Something must own every store entity; an observation does not keep it alive. `Drop` never kills a daemon resource (unmount is not close).
- Fail loudly: no `let _ =` / `.ok()` on results; `unused_must_use` and `let_underscore_must_use` are denied.
- Run `cargo test --workspace` and `cargo clippy --workspace --all-targets` in `app-rs/` when changing it.
- To see the app: build it with `cargo build -p treeterm-app --features automation` and run it with an isolated `HOME`, a short `TREETERM_SOCKET_PATH`, `TREETERM_DAEMON_DIR=<repo>/daemon-rs/target/debug` and `TREETERM_AUTOMATION_SOCKET=<path>`. The automation socket takes line commands: `state`, `select <ws>`, `tab <ws> <tab>`, `newtab <ws>`, `type <text>`, `key <keystroke>`, `click`/`rclick`/`dblclick <x> <y> [modifiers]`, `drag <x1> <y1> <x2> <y2> [modifiers] [hold]`, `scroll <x> <y> <dy> [modifiers]` (positions in window points, held keys such as `alt` or `cmd-shift`) and `screenshot <png>`. It takes one command per line, so quote each command as one argument. See `treeterm-app/src/automation.rs` for the full list. Screenshots render offscreen, so no screen-recording permission is needed. Afterwards, kill the daemon it started via the pid in `<HOME>/.treeterm/daemon.pid`. The `/verify` skill (`.claude/skills/verify/`) walks through this.

## Adding New Operations

**New Git op:** Add it to a store in `treeterm-model` (pure parsing in `treeterm-core`). Run git through the daemon's `ExecStream` and parse the output in the app.

**New PTY feature:** Keep it primitive in the daemon's `pty_manager.rs` (create/write/resize/kill). Compose complex logic in the app.

**New exec command:** The app starts it through the daemon's `ExecStream`: it writes the start event, collects the stdout/stderr chunks and resolves on the result event.

**New daemon RPC:** Add it to `treeterm.proto`, implement it in `grpc_server.rs`, wrap it in `treeterm-core`'s daemon client, and add a contract test (`treeterm-testkit/tests/contract.rs`).

## Development Rules

### Fail Loudly
Never silently swallow errors or return empty/default values on failure. Return errors to callers and let the upper level decide whether to log them or show a dialog. Never catch and ignore.

### Type Safety First
- Use enums with data (discriminated unions) for state
- Use serde for runtime validation at system boundaries (the wire, files on disk)
- Make invalid states unrepresentable via interface design

### Prefer non-optional
- Treat `Option` as a last resort.
- Prefer an enum with a variant per state over optional fields. Optional fields add conditional checks, which make branch coverage worse.
- A view that renders by state matches the state enum exhaustively.
- If a value can be missing, return early rather than making further state optional. Same with a type check: if the type assumption fails, return an error rather than making the type optional.

### For each change make test
- Make test. Focus on branch coverage. Untestable branch is likely removable branch or out of scope or impossible case.

### Do not store information that can be re-calculated when needed.
- For example, do not store parent branch name. This is because parent branch can be calculated using the parent id. 
- If parent branch is stored, then if parent id changed, then parent branch also need to change, which adds a failure point and more code.

### Enclosed ID on large object
- Prefer a store with its id inside it that exposes operations on itself, over a service-based pattern.
- eg: Instead of `workspace_api.add_tab(workspace_id)`, have `session.workspace(workspace_id).add_tab()`.

### Reactivity
- A lot of the time we will have an actively changing resource such as the worktree.
- Usually, there are two RPCs, getResource and watchResource. When this happens, remove getResource and just make watchResource return the initial value as its first event.
- Be aware that an RPC may emit a change event before the write RPC returns. This caused multiple issues in the past.

### Interactivity
- ANY async call should show a loading screen to the user. Even if it's very quick, it's not necessarily the case in all conditions.
- Similarly, the async call should have a clear error UI where the user is notified of any error.
- An exception is an async call made by an upper-level async call; then the upper-level one must handle the error UI.
- ANY async call that uses a tty stream should have a loading screen or a view that streams the tty output.

### No duplicated state
- Use the same data as the daemon as much as possible.
- Use the same id, for example.
- Do not copy the daemon state; wrap it if needed.

### Update via daemon watch
- After an operation (e.g., git commit, session create), do NOT optimistically update UI state inline within the operation.
- Instead, let the daemon watch/subscription push the updated state to the app.
- This prevents inconsistencies when multiple windows or apps observe the same session: every one gets the update through the same watch channel.

### Remove simple code if redundant.
- Let say you have two function, A and B. 
- A is on initialize, B is on update, so B have some more code.
- But if you call B with the same parameter as A, it would have the exact same effect.
- Then remove A, even if A is simpler as the net impact is reduction in code and complexity.

### Do not do stupid things
- Some stupid agent decided to not save a state because its updated too much. How do you know what is too much? Too much that it still works? No, dont do that. 
- Do no optimize until I say so. Just make the flow as is.

### Anti Pattern
- If you have a daemon type and an app type variant, something is wrong. Eg: DaemonSession and Session. They should use the same type. Use a wrapper like UiSession if needed, but it's a wrapper, not a copy: it should contain a Session.
- Composition over Inheritance. No inheritance! I got burned on that many times already!
- What should not be optional, do not mark as optional. Prefer no optional parameter where possible.
- Do not add code for cases that cannot or should not happen. For example, a workspace should always be available for a tab, and a session should always be available for a workspace. So don't handle the case where the workspace is not available for a tab.
- No overengineering unless explicitly told to.

### Worktree and File Changes
All file/worktree mutations **must go through the daemon** (via gRPC `WriteFile`). No `std::fs` writes to a workspace from the app.

### Missing Functionality — Ask, Don't Assume
If you find an incomplete implementation (e.g., a gRPC method defined in proto but not in the client), **ask the user** before workarounds. Determine if it's intentionally removed, a TODO, or an oversight — then implement correctly or get clarification.

### Model and view
- The stores in `treeterm-model` hold all business logic. Coordination between stores happens within the stores themselves, not in the views of `treeterm-app`.
- Don't drive logic from a view's render, observers or subscriptions. That makes the logic depend on what is on screen, which is error prone when the view goes away.

### Dependency injection
Prefer to inject dependencies rather than reaching for globals or singletons. An exception is the very top level, where these dependencies are injected. This also means: do not use global mutable state where possible.

### Lazy validation
Prefer to validate data at the last minute. Eg: do not check for a valid parent id while loading the session. Instead, when rendering the tree, if the parent id is unknown, put the orphan worktree in a separate section. Similarly with the pty id: if the tty is missing, show an error instead of trying to get the actual tty connection before even opening the terminal.

### Unmount is not close
- A view going away (a workspace switch, a hidden tab) is not the tab closing. Dropping a view or its model never kills the daemon resource behind it (a PTY, an exec); closing the tab does that, explicitly.
- A tab is never guaranteed to exist by the time something writes back to it. Check, and skip the write if it is gone. Resurrecting a deleted tab from a missing entry is worse than dropping the write: it syncs a tab with no `applicationId` to the daemon.
- Write cleanup so that both a genuine close (state must die) and transient churn like a workspace switch (state must persist) are correct.
- Corollary: never persist state on close. It is discarded with the tab; the write only buys a wasted sync to the daemon.

### State
- It is expected that the same session can be opened by multiple windows.
- Therefore high-level window state needs to be synced.
- This is generally handled through the workspace metadata.

## Testing

- Daemon: `cargo test` in `daemon-rs/`. Coverage: `cargo llvm-cov --lcov --output-path ../coverage/daemon-lcov.info` there (requires `cargo-llvm-cov` and `llvm-tools-preview`).
- App: `cargo test --workspace` and `cargo clippy --workspace --all-targets` in `app-rs/`. CI also runs clippy with `--features treeterm-app/automation`.
- App end-to-end tests launch the real binary in a sandbox and open windows, so they only run when asked: build the daemon (`cargo build -p treeterm-daemon` in `daemon-rs/`), then `cargo test -p treeterm-app --features automation --test e2e -- --ignored --test-threads=1` in `app-rs/`.
- ALWAYS run the tests (and clippy) and check coverage before committing.
  - Just install dependencies if needed to run them.
- Minimum 10% code and branch coverage for new code
- Daemon: unit tests in isolation (`#[cfg(test)]` modules in each `.rs` file). App: model tests against a real in-process daemon (`treeterm-testkit`) with `TestAppContext`. gRPC: contract tests (`treeterm-testkit/tests/contract.rs`).

## Decision Guide

> "Is this a primitive operation or a workflow?"
- **Primitive** (exec, I/O, PTY) → Daemon
- **Workflow** (git, complex parsing) → App (`treeterm-model` stores, pure parsing in `treeterm-core`)
- **UI State** → App (`treeterm-model`; views in `treeterm-app` only render it)
