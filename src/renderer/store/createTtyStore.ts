import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'

/** Subset of TerminalApi needed by TtyStore (without connectionId params — bound by session store) */
export interface TtyTerminalDeps {
  write: (handle: string, data: string) => void
  resize: (handle: string, cols: number, rows: number) => void
  kill: (sessionId: string) => void
  isAlive: (id: string) => Promise<boolean>
  onData: (handle: string, callback: (data: string) => void) => () => void
  onExit: (handle: string, callback: (exitCode: number) => void) => () => void
}

export interface TtyState {
  ptyId: string
  write(data: string): void
  resize(cols: number, rows: number): void
  kill(): void
  isAlive(): Promise<boolean>
  onData(cb: (data: string) => void): () => void
  onExit(cb: (exitCode: number) => void): () => void
}

export type Tty = StoreApi<TtyState>

export function createTtyStore(ptyId: string, handle: string, terminal: TtyTerminalDeps): Tty {
  return createStore<TtyState>()(() => ({
    ptyId,
    write: (data: string) => terminal.write(handle, data),
    resize: (cols: number, rows: number) => terminal.resize(handle, cols, rows),
    kill: () => terminal.kill(ptyId),
    isAlive: () => terminal.isAlive(ptyId),
    onData: (cb: (data: string) => void) => terminal.onData(handle, cb),
    onExit: (cb: (exitCode: number) => void) => terminal.onExit(handle, cb),
  }))
}
