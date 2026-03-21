import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import type { TerminalApi } from '../types'

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

export function createTtyStore(ptyId: string, handle: string, terminal: TerminalApi): Tty {
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
