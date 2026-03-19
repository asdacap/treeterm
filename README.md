# TreeTerm

A hierarchical terminal manager and IDE built for AI agent workflows. TreeTerm is an Electron-based desktop application that manages multiple workspaces using Git worktrees, enabling branching development with AI agents.

## Architecture

TreeTerm uses a three-layer architecture connected by IPC and gRPC:

- **Renderer** (React) — UI components, Zustand state, thin orchestration layer
- **Main** (Electron) — High-level business logic, Git operations, IPC bridge, gRPC client
- **Daemon** (persistent process) — Low-level primitives: PTY, exec, file I/O, session persistence

Communication: Renderer ↔ Main via Electron IPC; Main ↔ Daemon via gRPC over Unix socket.

The daemon survives app restarts — terminal sessions and state persist when Electron closes.

## Features

- **Hierarchical Workspaces** - Create parent and child workspaces using Git worktrees, forming a tree structure for branching development
- **Multi-Tab Interface** - Each workspace supports multiple tabs for different applications
- **Git Integration** - Full Git support including worktree management, merging with optional squashing, conflict detection, and diff viewing
- **Built-in Applications**:
  - **Terminal** - Full PTY support with xterm.js, run in the persistent daemon
  - **Terminal Variants** - Custom terminal instances with configurable startup commands
  - **Filesystem** - File browser and viewer
  - **Editor** - Monaco-based code editor with vim mode support
  - **AI Harness** - Integration with configurable AI CLI tools (Claude is the default)
  - **Review** - Review and merge changes from parent workspaces
- **Daemon Persistence** - Terminal sessions survive app restarts via a background daemon process
- **Speech-to-Text** - Push-to-talk functionality with multiple STT providers:
  - Web Speech API (browser-based)
  - OpenAI Whisper API
  - Local Whisper (stub, not yet functional)
- **Process Sandboxing** - Optional sandboxing with macOS sandbox-exec and Linux Bubblewrap
- **Prefix Mode Keybindings** - tmux-style prefix key system for workspace and tab navigation
- **Activity State Tracking** - Real-time indicators showing if applications are idle, working, or waiting for input
- **Persistent State** - Workspaces and tabs persist across sessions

## Installation

### Using npm

```bash
npm install
```

### Using Nix (Recommended for reproducible environments)

With [Nix flakes](https://nixos.wiki/wiki/Flakes) enabled:

```bash
# Enter development shell with all dependencies
nix develop

# Or build the application
nix build

# Or run directly
nix run
```

See [NIX_SETUP.md](NIX_SETUP.md) for detailed Nix setup instructions.

## Usage

### Development

```bash
npm run dev
```

Starts the application in development mode with hot reload.

### Build

```bash
npm run build
```

Builds the application for distribution. Output is placed in the `out/` directory.

### Preview

```bash
npm run preview
```

Runs the built application.

### Testing

```bash
npm test               # Run tests in watch mode
npm run test:run       # Run tests once
npm run test:coverage  # Run tests with coverage report
npm run test:e2e       # Run Playwright end-to-end tests
```

### Global CLI

```bash
npm install -g
treeterm [directory]   # Open with optional workspace directory
treeterm list-sessions # List all active daemon sessions
treeterm shutdown-daemon  # Shutdown the daemon process
treeterm status        # Show daemon status
treeterm --help        # Show help
```

## Project Structure

```
src/
├── daemon/           # Persistent daemon process (gRPC server, PTY, filesystem, exec, sessions)
├── main/             # Electron main process (git, IPC bridge, gRPC client, settings)
├── preload/          # Electron context bridge
├── renderer/         # React UI (components, Zustand stores, hooks)
├── applications/     # Application type definitions (terminal, aiHarness, editor, filesystem, review)
├── proto/            # Protobuf definitions (treeterm.proto)
├── generated/        # Auto-generated protobuf TypeScript
└── shared/           # Shared types (IPC types, common types)
```

## Configuration

Settings are stored in the Electron userData directory:

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
| Terminal | startByDefault | true |
| Terminal | instances | [] (custom terminal variants) |
| Sandbox | enabledByDefault | false |
| Sandbox | allowNetworkByDefault | true |
| AI Harness | instances | [{id: claude, command: claude, ...}] |
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
| STT | enabled | true |
| STT | provider | openaiWhisper |
| STT | openaiApiKey | (empty) |
| STT | localWhisperModelPath | (empty) |
| STT | pushToTalkKey | Shift+Space |
| STT | language | en |
| Daemon | enabled | true |
| Daemon | scrollbackLimit | 50000 |
| Global | globalDefaultApplicationId | terminal |
| Global | recentDirectories | [] |

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

#### Other Shortcuts

| Action | Shortcut |
|--------|----------|
| Push-to-Talk (STT) | `Shift+Space` |

## Tech Stack

- **Electron 33** - Desktop application runtime
- **React** - UI framework
- **TypeScript** - Type-safe language
- **Zustand** - State management
- **xterm.js** - Terminal emulation
- **node-pty** - Pseudo-terminal creation (runs in the daemon)
- **@grpc/grpc-js** + **ts-proto** - gRPC communication between Main and Daemon
- **Monaco Editor** - Code editor with vim mode support (monaco-vim)
- **pino** - Structured logging
- **lucide-react** - Icons
- **OpenAI SDK** - Speech-to-text via Whisper API
- **tinykeys** - Keybinding management
- **react-markdown** - Markdown rendering
- **electron-vite** - Build tooling
- **Vitest** - Unit and integration tests
- **Playwright** - End-to-end tests

## License

ISC
