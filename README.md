# TreeTerm

A hierarchical terminal manager and IDE built for AI agent workflows. TreeTerm is an Electron-based desktop application that manages multiple workspaces using Git worktrees, enabling branching development with AI agents.

## Features

- **Hierarchical Workspaces** - Create parent and child workspaces using Git worktrees, forming a tree structure for branching development
- **Multi-Tab Interface** - Each workspace supports multiple tabs for different applications
- **Git Integration** - Full Git support including worktree management, merging with optional squashing, conflict detection, and diff viewing
- **Built-in Applications**:
  - **Terminal** - Full PTY support with xterm
  - **Files** - File browser and viewer
  - **Claude** - Integration with Claude AI for agent workflows
- **Process Sandboxing** - Optional sandboxing with macOS sandbox-exec and Linux Bubblewrap
- **Activity State Tracking** - Real-time indicators showing if applications are idle, working, or waiting for input
- **Persistent State** - Workspaces and tabs persist across sessions

## Installation

```bash
npm install
```

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
treeterm
```

## Project Structure

```
src/
├── main/                    # Electron main process
│   ├── index.ts             # Window creation, IPC handlers
│   ├── git.ts               # Git operations
│   ├── pty.ts               # PTY management
│   ├── filesystem.ts        # Filesystem handlers
│   └── settings.ts          # Settings persistence
├── preload/
│   └── index.ts             # Context bridge for secure IPC
└── renderer/                # React frontend
    ├── App.tsx              # Main app component
    ├── applications/        # Application definitions
    ├── components/          # React components
    ├── store/               # Zustand state management
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
| Sandbox | enabledByDefault | false |
| Sandbox | allowNetworkByDefault | true |
| Appearance | theme | dark |

### Keybindings

| Action | Shortcut |
|--------|----------|
| New Tab | `Cmd/Ctrl+T` |
| Close Tab | `Cmd/Ctrl+W` |
| Next Tab | `Cmd/Ctrl+Shift+]` |
| Previous Tab | `Cmd/Ctrl+Shift+[` |
| Settings | `Cmd/Ctrl+,` |

## Tech Stack

- **Electron** - Desktop application runtime
- **React** - UI framework
- **TypeScript** - Type-safe language
- **Zustand** - State management
- **xterm.js** - Terminal emulation
- **node-pty** - Pseudo-terminal creation
- **simple-git** - Git operations
- **electron-vite** - Build tooling

## License

ISC
