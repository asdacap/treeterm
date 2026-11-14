[![codecov](https://codecov.io/gh/asdacap/treeterm/graph/badge.svg)](https://codecov.io/gh/asdacap/treeterm)

# TreeTerm

A personal terminal multiplexer built for my own use. It makes it easy to run multiple CLI agents at the same time across different machines via SSH, which is nice. It also has an auto-approve toggle that uses a separate model to evaluate whether a pending approval is safe before accepting it, which is also nice.

Session state is stored in a daemon process per host, so I can update the client UI without losing anything, or disconnect my laptop and reconnect later without losing any agent sessions.

Under the hood, it's a native Rust app (built on [GPUI](https://www.gpui.rs/)) that manages multiple workspaces using Git worktrees, enabling branching development with AI agents.

> [!WARNING]
> **The Electron app is deprecated** in favour of the native Rust rework in [`app-rs/`](app-rs/README.md). It still works (both apps talk to the same daemon and share sessions, workspaces and `settings.json`, so they can run side by side), but new work goes into the native app, and `src/main`, `src/preload` and `src/renderer` will be removed once the native app reaches parity.

<img width="3024" height="1896" alt="image" src="https://github.com/user-attachments/assets/b0e0ae76-311e-48c2-a249-e7455440feb9" />

## Architecture

TreeTerm is a client app plus a persistent daemon per host, connected by gRPC over a Unix socket:

- **Native app** (`app-rs/`, Rust/GPUI, single process) — UI, business logic, Git operations, SSH connections, gRPC client. See [app-rs/README.md](app-rs/README.md).
- **Daemon** (`daemon-rs/`, Rust, persistent process) — Low-level primitives: PTY, exec, file I/O, session persistence

The daemon survives app restarts — terminal sessions and state persist when the app closes.

The deprecated Electron app (`src/`) is a three-layer client over the same daemon: a React **renderer**, an Electron **main** process (IPC bridge, gRPC client) and the daemon, with Renderer ↔ Main over Electron IPC.

## Features

- **Hierarchical Workspaces** - Create parent and child workspaces using Git worktrees, forming a tree structure for branching development
- **Multi-Tab Interface** - Each workspace supports multiple tabs for different applications
- **Git Integration** - Full Git support including worktree management, merging with optional squashing, conflict detection, and diff viewing
- **Built-in Applications**:
  - **Terminal** - Full PTY support, run in the persistent daemon
  - **Custom Runner** - Custom terminal instances with configurable startup commands
  - **Filesystem** - File browser and viewer
  - **Editor** - Code editor with go-to-definition via grep-based search
  - **AI Harness** - Integration with configurable AI CLI tools (Claude is the default)
  - **Review** - Review and merge changes from parent workspaces with collapsible diff tree view
  - **Chat** - Built-in LLM chat with configurable model, reasoning, and effort level settings
  - **Comments** - Comment display and management
  - **Terminal Analyzer Debugger** - Debug and inspect AI system prompts with model/reasoning/timing controls
  - **Analyzer History** - Browse past terminal analyzer results
  - **Workspace Settings** - Per-workspace configuration
- **SSH Connections** - Connect to remote machines with session panel, system monitor, remote directory browser, port forwarding with persistence, and connected session indicator
- **Terminal Analyzer** - AI-powered terminal state estimation that syncs activity state from terminal buffer content
- **Auto Workspace Naming** - Automatically generate workspace titles and descriptions
- **Squash Merge** - Optional squash merge via dropdown on the merge button
- **Daemon Persistence** - Terminal sessions survive app restarts via a background Rust daemon process
- **Process Sandboxing** - Optional sandboxing with macOS sandbox-exec and Linux Bubblewrap
- **Prefix Mode Keybindings** - tmux-style prefix key system for workspace and tab navigation
- **Activity State Tracking** - Real-time indicators showing if applications are idle, working, or waiting for input, with AI-powered analysis
- **Drag-and-Drop Workspace Reordering** - Reorder workspaces in the sidebar with drag and drop, persisted across sessions
- **Collapsible Sidebar** - Collapse the left panel to a compact icon rail
- **Collapsible Session Panels** - Collapse/expand workspace list per session in the sidebar
- **Status Bar** - Bottom status bar with auto-approve safe permissions toggle
- **Persistent State** - Workspaces and tabs persist across sessions

## Installation and usage

### Native app

Build the daemon, then the app (`app-rs/` pins its own Rust toolchain in `app-rs/rust-toolchain.toml`):

```bash
(cd daemon-rs && cargo build)
cd app-rs
cargo build -p treeterm-app
./target/debug/treeterm   # finds or starts the local daemon
```

Packaging (from `app-rs/`):

```bash
cargo run -p xtask -- dist   # macOS: target/dist/TreeTerm.app + zip; Linux: tarball
```

With [Nix flakes](https://nixos.wiki/wiki/Flakes) on Linux:

```bash
nix build .#treeterm-native   # or: nix run .#treeterm-native
```

See [app-rs/README.md](app-rs/README.md) for packaging details, end-to-end tests and the parity status, and [NIX_SETUP.md](NIX_SETUP.md) for Nix setup.

### CLI

```bash
treeterm [directory]           # Open with optional workspace directory
treeterm ssh user@host[:port]  # Open a remote session
treeterm shutdown-daemon       # Shutdown the daemon process
```

### Testing

```bash
npm run test:app-rs   # Native app tests
cd app-rs && cargo clippy --workspace --all-targets
```

### Electron app (deprecated)

```bash
npm install
npm run dev            # Development mode with hot reload
npm run build          # Build into out/
npm run preview        # Run the built app
npm run test:run       # Unit tests
npm run test:e2e       # Playwright end-to-end tests
nix run                # Run it via Nix
```

## Project Structure

```
app-rs/                 # Native Rust/GPUI app
├── crates/
│   ├── treeterm-core/  # Daemon client, connections, paths, wire formats (no UI)
│   ├── treeterm-term/  # Terminal emulation on alacritty_terminal (no UI)
│   ├── treeterm-model/ # Stores and business logic (GPUI entities)
│   ├── treeterm-app/   # GPUI views and the treeterm binary
│   └── treeterm-testkit/# In-process daemon for tests
└── xtask/              # Packaging
daemon-rs/              # Rust daemon (persistent process)
├── crates/
│   ├── treeterm-proto/ # Generated gRPC stubs from treeterm.proto
│   └── treeterm-daemon/# Daemon logic + binary (PTY, exec, filesystem, sessions)
src/                    # Deprecated Electron app
├── main/             # Electron main process (git, IPC bridge, gRPC client, settings)
├── preload/          # Electron context bridge
├── renderer/         # React UI (components, Zustand stores, hooks)
├── applications/     # Application type definitions (terminal, aiHarness, editor, filesystem, review, chat, comments, terminalAnalyzerDebugger, analyzerHistory, customRunner, workspaceSettings)
├── proto/            # Protobuf definitions (treeterm.proto)
├── generated/        # Auto-generated protobuf TypeScript
└── shared/           # Shared types (IPC types, common types)
```

## Configuration

Settings are stored in `settings.json`, shared by the native and Electron apps:

- **macOS**: `~/Library/Application Support/treeterm/settings.json`
- **Linux**: `~/.config/treeterm/settings.json`
- **Windows**: `%APPDATA%\treeterm\settings.json`

### Available Settings

| Category | Setting | Default |
|----------|---------|---------|
| Terminal | fontSize | 14 |
| Terminal | fontFamily | Menlo, Monaco, Consolas, monospace |
| Terminal | cursorStyle | block |
| Terminal | cursorBlink | true |
| Terminal | showRawChars | false |
| Terminal | instances | [] (custom terminal variants) |
| Sandbox | enabledByDefault | false |
| Sandbox | allowNetworkByDefault | true |
| AI Harness | instances | [{id: claude, command: npx @earendil-works/pi-coding-agent, ...}] |
| Custom Runner | instances | [] |
| Appearance | theme | dark |
| Prefix Mode | enabled | true |
| Prefix Mode | prefixKey | Control+B |
| Prefix Mode | timeout | 1500 |
| Keybindings | newTab | c |
| Keybindings | closeTab | x |
| Keybindings | nextTab | n |
| Keybindings | prevTab | p |
| Keybindings | openSettings | , |
| Keybindings | workspaceFocus | w |
| Daemon | scrollbackLines | 10000 |
| Daemon | mergeThreshold | 51200 |
| Daemon | compactedLimit | 1048576 |
| SSH | savedConnections | [] |
| LLM | baseUrl | https://openrouter.ai/api/v1 |
| LLM | apiKey | (empty) |
| LLM | model | gpt-4o |
| Terminal Analyzer | model | openai/gpt-oss-safeguard-20b |
| Terminal Analyzer | reasoningEffort | low |
| Terminal Analyzer | bufferLines | 30 |
| Terminal Analyzer | safePaths | [/tmp] |
| GitHub | pat | (empty) |
| GitHub | autodetectViaGh | true |
| Global | globalDefaultApplicationId | terminal |
| Global | recentDirectories | [] |
| Debug | showBadge | false |

### Keybindings

#### Prefix Mode (Default: `Ctrl+B`)

Prefix mode provides tmux-style keybindings. Press the prefix key followed by an action key:

| Action | Default Key (after prefix) |
|--------|----------|
| New Tab | `c` |
| Close Tab | `x` |
| Next Tab | `n` |
| Previous Tab | `p` |
| Settings | `,` |
| Workspace Focus | `w` (then use arrows + Enter) |

All keybindings are customizable in settings.

## Tech Stack

- **Rust** - Native app and daemon
- **GPUI** + **GPUI Kit** (`gpui-component`) - UI framework
- **alacritty_terminal** - Terminal emulation
- **tonic** - gRPC between the app and the daemon
- **tokio** - Async runtime
- **forkpty** - Pseudo-terminal creation (in the daemon via libc)

The deprecated Electron app uses Electron 33, React, TypeScript, Zustand, xterm.js, Monaco Editor, `@grpc/grpc-js` + ts-proto, `@pierre/diffs`, electron-vite, Vitest and Playwright.

## License

ISC
