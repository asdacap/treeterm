# AGENTS.md

A guide for AI agents working on the TreeTerm codebase.

## Architecture

Three-layer architecture connected by IPC and gRPC:

- **Renderer** (React): UI components, Zustand state, thin orchestration layer
- **Main** (Electron): High-level business logic, Git operations, IPC handlers, orchestrates daemon
- **Daemon** (persistent process): Low-level primitives — PTY, exec, file I/O, session persistence, reviews

Communication: Renderer ↔ Main via Electron IPC; Main ↔ Daemon via gRPC over Unix socket.

The daemon survives app restarts; terminal sessions and state persist when Electron closes.

## Daemon Modules (`src/daemon/`)

| Module | Path | Responsibility |
|--------|------|---------------|
| PTY Manager | `ptyManager.ts` | PTY lifecycle (create, attach, resize, kill, I/O) |
| Exec Manager | `execManager.ts` | One-shot command execution with streaming I/O |
| Filesystem | `filesystem.ts` | Secure file I/O scoped to workspace boundaries |
| Session Store | `sessionStore.ts` | In-memory session persistence |
| Reviews | `reviews.ts` | Code review comment management |
| gRPC Server | `grpcServer.ts` | Exposes all operations via gRPC |

**Git ops are NOT in the daemon** — they live in `src/main/git.ts` using `ExecStream`.

## Layer Responsibilities

**Daemon exposes stable primitives:** `ExecStream`, `PtyStream`, `ReadFile`/`WriteFile`, session persistence.

**Main handles high-level logic:** Git orchestration via `ExecStream`, output parsing, error interpretation, workflow composition (stage → commit → push).

**Never add business logic to the daemon.** Never do direct filesystem ops in Main — use daemon's `WriteFile` gRPC.

## Directory Structure

```
src/
├── daemon/         # Low-level primitives
├── main/           # Electron main — git.ts, ipc.ts, grpcClient.ts
├── renderer/       # React UI — components/, store/, hooks/
├── proto/          # treeterm.proto (gRPC definitions)
├── applications/   # Application types (Terminal, AI Harness, etc.)
└── shared/         # Shared types and utilities
```

## Adding New Operations

**New Git op:** Add to `src/main/git.ts`, add IPC handler in `src/main/index.ts`, call from renderer via IPC.

**New PTY feature:** Keep it primitive in `ptyManager.ts` (create/write/resize/kill). Compose complex logic in the renderer.

**New exec command:** Use daemon's `ExecStream` from the Main process — write start event, collect stdout/stderr chunks, resolve on result event.

## Development Rules

### Fail Loudly
Never silently swallow errors or return empty/default values on failure. Throw errors; return them to callers; let the upper level decide to log or show a dialog. Never catch and ignore.

### Type Safety First
- Always add explicit types to function signatures
- Use typed IPC invocations (`invoke<T>`)
- Use discriminated unions for state
- Use Zod for runtime validation at system boundaries
- Make invalid states unrepresentable via interface design

### Anti Pattern
- If you have a daemon type and a main type variant, something is wrong. Eg: DaemonSession and Session. They should use the same type. Use a wrapper like UISession if needed, but its a wrapper, not a copy, it should contain a Session.

### Worktree and File Changes
All file/worktree mutations **must go through the daemon** (via gRPC `WriteFile`). No `fs.writeFile` in Main or Renderer.

### Missing Functionality — Ask, Don't Assume
If you find an incomplete implementation (e.g., a gRPC method defined in proto but not in the client), **ask the user** before workarounds. Determine if it's intentionally removed, a TODO, or an oversight — then implement correctly or get clarification.

## Testing

- Run `npm run test:coverage` when writing or modifying code
- Minimum 10% code and branch coverage for new code
- Daemon: unit tests in isolation; Main/Renderer: integration tests; gRPC: contract tests

## Decision Guide

> "Is this a primitive operation or a workflow?"
- **Primitive** (exec, I/O, PTY) → Daemon
- **Workflow** (git, complex parsing) → Main
- **UI State** → Renderer
