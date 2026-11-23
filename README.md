[![codecov](https://codecov.io/gh/asdacap/treeterm/graph/badge.svg)](https://codecov.io/gh/asdacap/treeterm)

# TreeTerm

A personal terminal multiplexer built for my own use. It makes it easy to run multiple CLI agents at the same time across different machines via SSH, which is nice. It also has an auto-approve toggle that uses a separate model to evaluate whether a pending approval is safe before accepting it, which is also nice.

Session state is stored in a daemon process per host, so I can update the client UI without losing anything, or disconnect my laptop and reconnect later without losing any agent sessions.

Under the hood, it's a native Rust app (built on [GPUI](https://www.gpui.rs/)) that manages multiple workspaces using Git worktrees, enabling branching development with AI agents.

TreeTerm started as an Electron app. The native app replaced it and the Electron code was removed; it reads the same sessions, workspaces and `settings.json`. The last Electron version is commit `f05be955`.

<img width="3024" height="1896" alt="image" src="https://github.com/user-attachments/assets/b0e0ae76-311e-48c2-a249-e7455440feb9" />

## Architecture

TreeTerm is a client app plus a persistent daemon per host, connected by gRPC over a Unix socket:

- **App** (`app-rs/`, Rust/GPUI, single process) — UI, business logic, Git operations, SSH connections, gRPC client. See [app-rs/README.md](app-rs/README.md).
- **Daemon** (`daemon-rs/`, Rust, persistent process) — Low-level primitives: PTY, exec, file I/O, session persistence

The daemon survives app restarts — terminal sessions and state persist when the app closes. For a remote host the app uploads a static Linux daemon over SSH and reaches it through an SSH tunnel.

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

Requires Rust (via [rustup](https://rustup.rs/)) and `protoc`. On macOS, also Xcode (GPUI compiles its Metal shaders with Xcode's `metal`). Build the daemon, then the app (`app-rs/` pins its own Rust toolchain in `app-rs/rust-toolchain.toml`):

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

The bundles carry the local daemon and the static musl daemons for remote Linux hosts. Building those needs `rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl` (in `daemon-rs/`) and a musl linker (see [app-rs/README.md](app-rs/README.md#packaging)); a missing one is skipped with the reason.

With [Nix flakes](https://nixos.wiki/wiki/Flakes) on Linux:

```bash
nix build    # or: nix run
```

See [app-rs/README.md](app-rs/README.md) for packaging details, end-to-end tests and what the app does, and [NIX_SETUP.md](NIX_SETUP.md) for Nix setup.

### CLI

```bash
treeterm [directory]           # Open with optional workspace directory
treeterm ssh user@host[:port]  # Open a remote session
treeterm shutdown-daemon       # Shutdown the daemon process
```

### Testing

```bash
(cd daemon-rs && cargo test)                                      # Daemon tests
(cd app-rs && cargo test --workspace)                             # App tests
(cd app-rs && cargo clippy --workspace --all-targets)
daemon-rs/tests/container/run-container-test.sh                   # Daemon boot test on Ubuntu and Fedora (Docker)
```

End-to-end tests of the app open windows, so they run only when asked; see [app-rs/README.md](app-rs/README.md#running-it).

## Project Structure

```
app-rs/                 # The app (Rust/GPUI)
├── crates/
│   ├── treeterm-core/  # Daemon client, connections, paths, wire formats (no UI)
│   ├── treeterm-term/  # Terminal emulation on alacritty_terminal (no UI)
│   ├── treeterm-model/ # Stores and business logic (GPUI entities)
│   ├── treeterm-app/   # GPUI views and the treeterm binary
│   └── treeterm-testkit/# In-process daemon for tests
├── vendor/             # Patched GPUI, GPUI Kit, gpui-webview, avif-decode
├── packaging/          # Info.plist, .desktop entry, icon
└── xtask/              # Daemon builds and packaging
daemon-rs/              # Rust daemon (persistent process)
├── crates/
│   ├── treeterm-proto/ # treeterm.proto (proto/) and the gRPC stubs generated from it
│   └── treeterm-daemon/# Daemon logic + binary (PTY, exec, filesystem, sessions)
└── tests/container/    # Boot test of the static daemon on stock distros
```

## Configuration

Settings are stored in `settings.json` (the place the Electron app kept it, so existing settings carry over):

- **macOS**: `~/Library/Application Support/treeterm/settings.json`
- **Linux**: `$XDG_CONFIG_HOME/treeterm/settings.json` (`~/.config/treeterm/settings.json` by default)

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

## License

ISC
