export interface Workspace {
  id: string
  name: string
  path: string
  parentId: string | null
  children: string[]
  status: 'active' | 'merged' | 'abandoned'
}

export interface TerminalApi {
  create: (cwd: string) => Promise<string>
  write: (id: string, data: string) => void
  resize: (id: string, cols: number, rows: number) => void
  kill: (id: string) => void
  onData: (id: string, callback: (data: string) => void) => () => void
}

export interface ElectronApi {
  terminal: TerminalApi
  selectFolder: () => Promise<string | null>
}

declare global {
  interface Window {
    electron: ElectronApi
  }
}
