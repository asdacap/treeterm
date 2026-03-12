# AGENTS.md

A guide for AI agents working on the TreeTerm codebase.

## Architecture Overview

TreeTerm follows a three-layer architecture designed for stability and evolution:

```
┌─────────────────────────────────────────┐
│           Renderer (React)              │
│  - UI components and state management   │
│  - Thin orchestration layer             │
└──────────────┬──────────────────────────┘
               │ IPC (Electron)
┌──────────────▼──────────────────────────┐
│         Main (Electron Process)         │
│  - Window management                    │
│  - HIGH-LEVEL BUSINESS LOGIC (Git)      │
│  - Orchestrates daemon operations       │
│  - Complex workflows and parsing        │
└──────────────┬──────────────────────────┘
               │ gRPC over Unix Socket
┌──────────────▼──────────────────────────┐
│            Daemon Process               │
│  - PTY session management               │
│  - LOW-LEVEL PRIMITIVES (exec, I/O)     │
│  - Filesystem operations                │
│  - Session persistence                  │
│  - Reviews storage                      │
└─────────────────────────────────────────┘
```

**Key Principles**:
- The daemon is a persistent process that survives app restarts
- Terminal sessions and state persist even when the Electron app is closed and reopened
- **All changes to worktrees and files MUST go through the daemon**
- **Git operations are now handled in Main via ExecStream**

## Daemon: The Low-Level Primitives Layer

The daemon (`src/daemon/`) exposes **minimal, stable primitives** for execution, I/O, and persistence. It does not contain business logic - that lives in the Main process.

### Core Modules

| Module | Path | Responsibility |
|--------|------|---------------|
| **PTY Manager** | `src/daemon/ptyManager.ts` | Low-level PTY lifecycle (create, attach, resize, kill, I/O) |
| **Exec Manager** | `src/daemon/execManager.ts` | One-shot command execution with streaming I/O |
| **Filesystem** | `src/daemon/filesystem.ts` | Secure file I/O (scoped to workspace boundaries) |
| **Session Store** | `src/daemon/sessionStore.ts` | In-memory session persistence |
| **Reviews** | `src/daemon/reviews.ts` | Code review comment management |
| **gRPC Server** | `src/daemon/grpcServer.ts` | Exposes all operations via gRPC |

**Note**: Git operations are no longer in the daemon. They are handled in the Main process using the ExecStream primitive.

### Why the Daemon?

1. **Persistence**: Sessions survive Electron app restarts
2. **Stability**: Low-level operations rarely change
3. **Security**: Mutations happen through a controlled boundary
4. **Testing**: Can be tested independently of Electron

## Key Principle: Low-Level in Daemon, High-Level in Main

**Daemon exposes minimal, stable primitives:**
- `ExecStream` - Execute shell commands with streaming I/O
- `PtyStream` - Interactive terminal sessions  
- `ReadFile` / `WriteFile` - File operations with workspace scoping
- Session persistence

**Main handles high-level business logic:**
- Git operations orchestrated via `ExecStream`
- Complex parsing of command output
- Error interpretation and user-facing messages
- Workflow composition (stage → commit → push)

**Why this separation:**
1. **Daemon stability** - Only primitive operations, rarely changes
2. **Evolution** - Git logic can iterate without daemon changes
3. **Consistency** - Uses system git directly, no library dependencies
4. **Debugging** - Commands visible in logs, easy to reproduce

## Directory Structure

```
src/
├── daemon/              # Low-level primitives - minimal, stable operations
│   ├── index.ts         # Entry point
│   ├── grpcServer.ts    # gRPC API implementation
│   ├── ptyManager.ts    # PTY session management
│   ├── execManager.ts   # One-shot command execution with streaming I/O
│   ├── filesystem.ts    # File I/O
│   ├── sessionStore.ts  # Session persistence
│   └── reviews.ts       # Review comments
│
├── main/                # Electron main process - high-level business logic
│   ├── grpcClient.ts    # Daemon client
│   ├── git.ts           # Git operations (via ExecStream)
│   ├── index.ts         # App lifecycle
│   └── ipc.ts           # IPC handlers
│
├── renderer/            # React UI - state management and orchestration
│   ├── components/      # React components
│   ├── store/           # Zustand state stores
│   └── hooks/           # React hooks
│
├── proto/               # Protocol definitions
│   └── treeterm.proto   # gRPC service definition
│
├── applications/        # Application types (Terminal, AI Harness, etc.)
└── shared/              # Shared types and utilities
```

## Common Patterns

### Adding a New Git Operation

Git operations are now handled in the Main process using the daemon's `ExecStream` primitive.

1. Add to `src/main/git.ts`:
```typescript
export async function getBranchUpstream(
  workspacePath: string,
  branchName: string
): Promise<string | null> {
  const result = await this.exec(workspacePath, ['rev-parse', '--abbrev-ref', `${branchName}@{upstream}`])
  if (result.exitCode === 0) {
    return result.stdout.trim()
  }
  return null
}
```

2. Add IPC handler in `src/main/index.ts`:
```typescript
server.onGitGetBranchUpstream(async (repoPath, branchName) => {
  if (!daemonClient) throw new Error('Daemon not initialized')
  initializeGitClient()
  if (!gitClient) throw new Error('Git client not initialized')
  const upstream = await gitClient.getBranchUpstream(repoPath, branchName)
  return { success: true, upstream }
})
```

3. Call from renderer via main IPC

### Adding a New PTY Feature

PTY operations go through `DaemonPtyManager` in `src/daemon/ptyManager.ts`. Keep operations primitive:

- `create()` - spawn new PTY
- `write()` - send data
- `resize()` - change dimensions
- `kill()` - terminate session

**Don't** add complex logic like "auto-restart on crash" to PTY manager. Compose that in the renderer.

### Adding Exec Streaming Commands

For any command-line tool that needs streaming I/O, use the daemon's `ExecStream`:

```typescript
// In main process
private async execCommand(
  cwd: string,
  command: string,
  args: string[]
): Promise<{ exitCode: number; stdout: string; stderr: string }> {
  return new Promise((resolve, reject) => {
    const stdout: Buffer[] = []
    const stderr: Buffer[] = []
    
    const stream = this.daemonClient.execStream()
    
    stream.write({
      start: { cwd, command, args, timeoutMs: 30000 }
    })
    stream.end()
    
    stream.on('data', (output) => {
      if (output.stdout) stdout.push(output.stdout.data)
      else if (output.stderr) stderr.push(output.stderr.data)
      else if (output.result) {
        resolve({
          exitCode: output.result.exitCode,
          stdout: Buffer.concat(stdout).toString(),
          stderr: Buffer.concat(stderr).toString()
        })
      }
    })
  })
}
```

## Testing Philosophy

- **Daemon**: Unit test business logic in isolation
- **Main/Renderer**: Integration tests that verify orchestration
- **gRPC**: Contract tests to ensure protocol compliance

## Development Practices

### Type Safety First

Always add types and use typed operations where possible. Structure the application to enforce type safety at compile time.

**Always add explicit types:**
```typescript
// Good
function getWorktreePath(workspaceId: string): string | null {
  // ...
}

// Bad
function getWorktreePath(workspaceId) {
  // ...
}
```

**Use typed operations over loose alternatives:**
```typescript
// Good - typed IPC
const result = await ipcRenderer.invoke<GetDiffResponse>('git:getDiff', request)

// Bad - untyped
const result = await ipcRenderer.invoke('git:getDiff', request)
```

**Prefer discriminated unions for state:**
```typescript
// Good - exhaustive type checking
type ConnectionState =
  | { status: 'disconnected' }
  | { status: 'connecting' }
  | { status: 'connected'; client: GrpcClient }
  | { status: 'error'; error: Error }

// Bad - loose typing
type ConnectionState = {
  status: string
  client?: GrpcClient
  error?: Error
}
```

**Use Zod for runtime validation at boundaries:**
```typescript
// Good - validated at system boundary
const configSchema = z.object({
  socketPath: z.string(),
  timeout: z.number().default(5000),
})

const config = configSchema.parse(rawConfig)
```

**Structure code to make invalid states unrepresentable:**
```typescript
// Good - can't have sessions without a connection
interface ConnectedDaemon {
  client: GrpcClient
  sessions: Map<string, Session>
}

// Bad - allows invalid state
interface Daemon {
  client?: GrpcClient
  sessions: Map<string, Session>  // Sessions without client?
}
```

### Worktree and File Changes

**All modifications to worktrees and files must go through the daemon.** This ensures:
- Consistent state management
- Proper locking and conflict handling
- Audit trail of changes
- Security boundary enforcement

```typescript
// Good - file write through daemon
await daemonClient.writeFile({
  workspacePath,
  filePath,
  content,
})

// Bad - direct file write in renderer/main
await fs.writeFile(filePath, content)
```

## Key Takeaways

1. **Daemon is stable** - Low-level operations that rarely change
2. **Business logic lives in main** - High-level orchestration, not in UI or daemon
3. **Primitives, not workflows** - Daemon exposes composable building blocks
4. **Protocol-first** - Define in `.proto`, implement in daemon, consume in UI
5. **Thin layers** - Main orchestrates and handles high-level logic, renderer manages UI state

When in doubt, ask: "Is this a primitive operation or a workflow?"
- **Primitives** (exec, I/O, PTY) → Daemon
- **Workflows** (git operations, complex parsing) → Main
- **UI State** → Renderer
