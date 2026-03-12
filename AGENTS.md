# AGENTS.md

A guide for AI agents working on the TreeTerm codebase.

## Architecture Overview

TreeTerm follows a three-layer architecture designed for stability and separation of concerns:

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
│  - Thin orchestration layer             │
└──────────────┬──────────────────────────┘
               │ gRPC over Unix Socket
┌──────────────▼──────────────────────────┐
│            Daemon Process               │
│  - PTY session management               │
│  - Git operations                       │
│  - Filesystem operations                │
│  - Session persistence                  │
│  - LOW-LEVEL BUSINESS LOGIC LIVES HERE  │
└─────────────────────────────────────────┘
```

**Key Principles**:
- The daemon is a persistent process that survives app restarts
- Terminal sessions and state persist even when the Electron app is closed and reopened
- **All changes to worktrees and files MUST go through the daemon**

## Daemon: The Low-Level Business Logic Layer

The daemon (`src/daemon/`) is the **home for low-level business logic**. It exposes primitive operations via gRPC that deal with the filesystem, Git, and PTY sessions.

### Core Modules

| Module | Path | Responsibility |
|--------|------|---------------|
| **PTY Manager** | `src/daemon/ptyManager.ts` | Low-level PTY lifecycle (create, attach, resize, kill, I/O) |
| **Git Operations** | `src/daemon/git.ts` | Primitive Git operations (worktrees, diff, commit, merge, etc.) |
| **Filesystem** | `src/daemon/filesystem.ts` | Secure file I/O (scoped to workspace boundaries) |
| **Session Store** | `src/daemon/sessionStore.ts` | In-memory session persistence |
| **Reviews** | `src/daemon/reviews.ts` | Code review comment management |
| **gRPC Server** | `src/daemon/grpcServer.ts` | Exposes all operations via gRPC |

### Why the Daemon?

1. **Persistence**: Sessions survive Electron app restarts
2. **Stability**: Low-level operations rarely change
3. **Separation**: Business logic is isolated from UI concerns
4. **Testing**: Can be tested independently of Electron

## Guiding Principles for Changes

When adding features or making changes, follow these principles:

### 1. Business Logic Goes in the Daemon

**Good**: Implement new Git operation in `src/daemon/git.ts`
```typescript
// src/daemon/git.ts
export async function cherryPickCommit(
  worktreePath: string,
  commitHash: string
): Promise<void> {
  const git = simpleGit(worktreePath)
  await git.raw(['cherry-pick', commitHash])
}
```

**Bad**: Implementing Git logic in renderer or main process

### 2. Daemon Operations Should Be Primitive

Keep daemon APIs **low-level and composable**. Complex workflows should compose primitives from the renderer/main layers.

**Good**: Daemon exposes primitives
```typescript
// Daemon exposes:
- stageFile(path)
- commitStaged(message)
- createWorktree(branch)

// Renderer composes them:
async function commitAndBranch(files, message, branchName) {
  for (const file of files) await stageFile(file)
  await commitStaged(message)
  await createWorktree(branchName)
}
```

**Bad**: Daemon exposes complex workflow
```typescript
// Avoid this in daemon:
async function commitAndBranch(files, message, branchName) {
  // Complex workflow in daemon
}
```

### 3. Main and Renderer Are Thin Orchestration Layers

- **Main process** (`src/main/`): Manages Electron lifecycle, forwards calls to daemon
- **Renderer** (`src/renderer/`): UI state management, orchestrates daemon calls

These layers should have **minimal logic** - they coordinate, they don't implement.

### 4. Protocol Definitions Are the Contract

All daemon APIs are defined in `src/proto/treeterm.proto`. This is the single source of truth.

**When adding a new feature:**
1. Define the RPC in `src/proto/treeterm.proto`
2. Implement in `src/daemon/grpcServer.ts`
3. Call from main via `src/main/grpcClient.ts`
4. Orchestrate in renderer

## Directory Structure

```
src/
├── daemon/              # Business logic - low-level operations
│   ├── index.ts         # Entry point
│   ├── grpcServer.ts    # gRPC API implementation
│   ├── ptyManager.ts    # PTY session management
│   ├── git.ts           # Git operations
│   ├── filesystem.ts    # File I/O
│   ├── sessionStore.ts  # Session persistence
│   └── reviews.ts       # Review comments
│
├── main/                # Electron main process - thin orchestration
│   ├── grpcClient.ts    # Daemon client
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

1. Add to `src/proto/treeterm.proto`:
```protobuf
rpc GetBranchUpstream(GetBranchUpstreamRequest) returns (GetBranchUpstreamResponse);

message GetBranchUpstreamRequest {
  string workspace_path = 1;
  string branch_name = 2;
}

message GetBranchUpstreamResponse {
  string upstream = 1;
}
```

2. Implement in `src/daemon/git.ts`:
```typescript
export async function getBranchUpstream(
  workspacePath: string,
  branchName: string
): Promise<string | null> {
  const git = simpleGit(workspacePath)
  // Implementation
}
```

3. Wire up in `src/daemon/grpcServer.ts`:
```typescript
server.addService(TreeTermService, {
  // ... other handlers
  getBranchUpstream: handleGetBranchUpstream,
})
```

4. Call from renderer via main IPC or directly via gRPC client

### Adding a New PTY Feature

PTY operations go through `DaemonPtyManager` in `src/daemon/ptyManager.ts`. Keep operations primitive:

- `create()` - spawn new PTY
- `write()` - send data
- `resize()` - change dimensions
- `kill()` - terminate session

**Don't** add complex logic like "auto-restart on crash" to PTY manager. Compose that in the renderer.

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
2. **Business logic lives in daemon** - Not in UI or main process
3. **Primitives, not workflows** - Daemon exposes composable building blocks
4. **Protocol-first** - Define in `.proto`, implement in daemon, consume in UI
5. **Thin layers** - Main and renderer orchestrate, they don't implement

When in doubt, ask: "Is this a primitive operation or a workflow?" If it's a workflow, it belongs in the renderer. If it's a primitive, it belongs in the daemon.
