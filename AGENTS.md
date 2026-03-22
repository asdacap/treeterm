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

**Daemon exposes stable primitives:** `ExecStream`, `PtyStream`, `ReadFile`/`WriteFile`, session persistence and multiplexin.

**Main handles high-level logic:** Git orchestration via `ExecStream`, output parsing, error interpretation, workflow composition (stage → commit → push). Crucually, it should not do multiplexing, each of the window or application is self contained, and for pty, each terminal should have a new connection to the daemon.

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

### Onclosed ID on large object
- Prefer to have store with id within it and expose operation to that store instead of having a service based pattern.
- eg: Instead of WorkspaceApi.addTab(workspaceId), have SessionApi.getWorkspace(workspaceId).addTab()
- Special care need to be taken with react where the key={itemId} need to be specified for the update to work correctly.

### Reactivity
- A lot the time we will an actively changing resource such as the worktree.
- Usually, there is two rpc, getResource and watchResoure. When this happen, remove getResource and just make watchResource return initial value as first event.

### Interactivity
- ANY async calls should show a loading screen to the user. Even if its very quick, its not necessarily the case in all condition.
- Similarly, this async call should have a clear error UI where the user will be notified if there are any error.
- An exception to this is if the async call itself is called by an upper level async call, then the upper level one must handle
the error UI.

### No duplicated state
- Use the same data as Daemon as much as possible.
- Use the same id for example.
- Do not copy the daemon state, rather wrap them if needed. 

### Remove simple code if redundant.
- Let say you have two function, A and B. 
- A is on initialize, B is on update, so B have some more code.
- But if you call B with the same parameter as A, it would have the exact same effect.
- Then remove A, even if A is simpler as the net impact is reduction in code and complexity.

### Do not do stupid things
- Some stupid agent decided to not save a state because its updated too much. How do you know what is too much? Too much that it still works? No, dont do that. 
- Do no optimize until I say so. Just make the flow as is.

### Anti Pattern
- If you have a daemon type and a main type variant, something is wrong. Eg: DaemonSession and Session. They should use the same type. Use a wrapper like UISession if needed, but its a wrapper, not a copy, it should contain a Session.
- Composition over Inheritence. No inheritence! I got burned on that many times already!
- What should not be nullable, do not mark as nullable. Prefer no nullable parameter where possible.
- Do not add code for cases that cannot or should not happen. For example, workspace should always be possible in a tab, or session should be always available in a workspace. So dont care for case where workspace it not available in a tab. 
- No overengineering unless explicitly told to. No `useMemo`!

### Worktree and File Changes
All file/worktree mutations **must go through the daemon** (via gRPC `WriteFile`). No `fs.writeFile` in Main or Renderer.

### Missing Functionality — Ask, Don't Assume
If you find an incomplete implementation (e.g., a gRPC method defined in proto but not in the client), **ask the user** before workarounds. Determine if it's intentionally removed, a TODO, or an oversight — then implement correctly or get clarification.

### MVVC
- The zustand store should have all business logic. Coordination between store happens within the store itself, not within the react view.
- Prefer not to use useEffect. This causes the logic to be specific to the UI which can be error prone when unmounting.

### Dependency injection
Prefer to inject dependencies rather than using window or electron singleton. An exception is at the very top level where these dependencies are injected. This also means do not use global mutation state where possible.

### Lazy validation
Prefer to validate data at the last minute. Eg: do not check for valid parent id while loading the session. Instead when rendering the tree, if the parent id is unknow, just put the orphant worktree in a separate section. Similarly with pty id, if tty is missing, show error instead of trying to get the actually tty connectino before even opening the terminal.

### State
- It is expected that the same session can be opened by multiple window.
- Therefore high level window state need to be synced. 
- This is generally handled through the workspace metadata.

## Testing

- Run `npm run test:coverage` and `npm run build` when writing or modifying code
- AlWAYS run `npm run build` and check coverage and test before committing.
  - Just install dependencies if needed to run it.
- Minimum 10% code and branch coverage for new code
- Daemon: unit tests in isolation; Main/Renderer: integration tests; gRPC: contract tests

## Decision Guide

> "Is this a primitive operation or a workflow?"
- **Primitive** (exec, I/O, PTY) → Daemon
- **Workflow** (git, complex parsing) → Main
- **UI State** → Renderer
