import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'

/** Subset of TerminalApi needed by TtyStore (without connectionId params — bound by session store) */
export interface TtyTerminalDeps {
  write: (handle: string, data: string) => Promise<void>
  resize: (handle: string, cols: number, rows: number) => void
  kill: (sessionId: string) => void
}

export interface TtyState {
  ptyId: string
  write(data: string): Promise<void>
  resize(cols: number, rows: number): void
  kill(): void
}

export type Tty = StoreApi<TtyState>

export function createTtyStore(ptyId: string, handle: string, terminal: TtyTerminalDeps): Tty {
  return createStore<TtyState>()(() => ({
    ptyId,
    write: (data: string) => terminal.write(handle, data),
    resize: (cols: number, rows: number) => { terminal.resize(handle, cols, rows); },
    kill: () => { terminal.kill(ptyId); },
  }))
}

/** Write-only wrapper for callers that just need to send input or kill a PTY */
export interface TtyWriter {
  write(data: string): Promise<void>
  kill(): void
}

export function createTtyWriter(ptyId: string, handle: string, terminal: TtyTerminalDeps): TtyWriter {
  return {
    write: (data: string) => terminal.write(handle, data),
    kill: () => { terminal.kill(ptyId); },
  }
}
