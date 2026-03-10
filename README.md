# TreeTerm

A hierarchical terminal manager and IDE built for AI agent workflows. TreeTerm is an Electron-based desktop application that manages multiple workspaces using Git worktrees, enabling branching development with AI agents.

## Features

- **Hierarchical Workspaces** - Create parent and child workspaces using Git worktrees, forming a tree structure for branching development
- **Multi-Tab Interface** - Each workspace supports multiple tabs for different applications
- **Git Integration** - Full Git support including worktree management, merging with optional squashing, conflict detection, and diff viewing
- **Built-in Applications**:
  - **Terminal** - Full PTY support with xterm, customizable terminal instances
  - **Files** - File browser and viewer
  - **Editor** - Monaco-based code editor with vim mode support
  - **Claude** - Integration with Claude AI for agent workflows
  - **Review** - Review and merge changes from parent workspaces
- **Speech-to-Text** - Push-to-talk functionality with multiple STT providers:
  - Web Speech API (browser-based)
  - OpenAI Whisper API
  - Local Whisper (planned)
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
npm test          # Run tests in watch mode
npm run test:run  # Run tests once
```

### Global CLI

```bash
npm install -g
treeterm [directory]  # Open with optional workspace directory
```

## Project Structure

```
src/
├── main/                    # Electron main process
│   ├── index.ts             # Window creation, IPC handlers
│   ├── git.ts               # Git operations
│   ├── pty.ts               # PTY management
│   ├── filesystem.ts        # Filesystem handlers
│   ├── settings.ts          # Settings persistence
│   ├── stt.ts               # Speech-to-text handlers
│   └── menu.ts              # Application menu
├── preload/
│   └── index.ts             # Context bridge for secure IPC
├── applications/            # Application definitions
│   ├── terminal/            # Terminal application
│   ├── filesystem/          # File browser
│   ├── editor/              # Monaco editor
│   ├── claude/              # Claude AI integration
│   └── review/              # Review/merge interface
└── renderer/                # React frontend
    ├── App.tsx              # Main app component
    ├── components/          # React components
    ├── store/               # Zustand state management
    ├── hooks/               # React hooks (PTT, keybindings)
    ├── stt/                 # STT providers
    └── types/               # TypeScript definitions
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
| Sandbox | enabledByDefault | false |
| Sandbox | allowNetworkByDefault | true |
| Claude | command | claude |
| Claude | startByDefault | false |
| Claude | enableSandbox | false |
| Appearance | theme | dark |
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

- **Electron** - Desktop application runtime
- **React** - UI framework
- **TypeScript** - Type-safe language
- **Zustand** - State management
- **xterm.js** - Terminal emulation
- **node-pty** - Pseudo-terminal creation
- **simple-git** - Git operations
- **Monaco Editor** - Code editor with vim mode support (monaco-vim)
- **OpenAI SDK** - Speech-to-text via Whisper API
- **tinykeys** - Keybinding management
- **react-markdown** - Markdown rendering
- **electron-vite** - Build tooling

## License

ISC
