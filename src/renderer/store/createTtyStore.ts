import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import type { IDisposable } from '../../shared/lifecycle'

/** Subset of TerminalApi needed by TtyStore (without connectionId params — bound by session store) */
export interface TtyTerminalDeps {
  write: (handle: string, data: string) => Promise<void>
  resize: (handle: string, cols: number, rows: number) => void
  kill: (sessionId: string) => void
  /** Detach this attachment's main-side stream (by routing handle) without killing the PTY. */
  detach: (handle: string) => void
}

export interface TtyState {
  ptyId: string
  write(data: string): Promise<void>
  resize(cols: number, rows: number): void
  /** Destroys the daemon-side PTY process. This is `close()`, not `dispose()`. */
  kill(): void
}

/**
 * A live attachment to a PTY.
 *
 * `dispose()` detaches the main-side gRPC stream and releases *this window's* event
 * subscription — the PTY keeps running. `kill()` destroys the PTY itself. They are
 * deliberately different verbs; see "Unmount is not close" in AGENTS.md.
 *
 * The Tty owns its subscription, so its lifetime is one thing rather than a handle plus
 * a cleanup lambda that a caller could drop. Hand it to a `DisposableStore` and the
 * subscription cannot outlive it.
 */
export interface Tty extends StoreApi<TtyState>, IDisposable {}

export function createTtyStore(
  ptyId: string,
  handle: string,
  terminal: TtyTerminalDeps,
  subscription: IDisposable,
): Tty {
  const store = createStore<TtyState>()(() => ({
    ptyId,
    write: (data: string) => terminal.write(handle, data),
    resize: (cols: number, rows: number) => { terminal.resize(handle, cols, rows); },
    kill: () => { terminal.kill(ptyId); },
  }))
  // Detach exactly once: double-dispose (e.g. a DisposableStore also owning it) must
  // not fire a second detach for a handle the main side has already torn down.
  let disposed = false
  return Object.assign(store, {
    dispose: () => {
      if (disposed) return
      disposed = true
      terminal.detach(handle)
      subscription.dispose()
    },
  })
}

/** Write-only view of a Tty, for callers that just need to send input or kill a PTY */
export interface TtyWriter {
  write(data: string): Promise<void>
  kill(): void
}
