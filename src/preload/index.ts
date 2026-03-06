import { contextBridge, ipcRenderer } from 'electron'

type DataCallback = (data: string) => void
const dataListeners = new Map<string, DataCallback[]>()

// Listen for pty data from main process
ipcRenderer.on('pty:data', (_event, id: string, data: string) => {
  const listeners = dataListeners.get(id)
  if (listeners) {
    listeners.forEach((cb) => cb(data))
  }
})

ipcRenderer.on('pty:exit', (_event, id: string, _exitCode: number) => {
  dataListeners.delete(id)
})

contextBridge.exposeInMainWorld('electron', {
  terminal: {
    create: (cwd: string): Promise<string> => {
      return ipcRenderer.invoke('pty:create', cwd)
    },
    write: (id: string, data: string): void => {
      ipcRenderer.send('pty:write', id, data)
    },
    resize: (id: string, cols: number, rows: number): void => {
      ipcRenderer.send('pty:resize', id, cols, rows)
    },
    kill: (id: string): void => {
      ipcRenderer.send('pty:kill', id)
    },
    onData: (id: string, callback: DataCallback): (() => void) => {
      if (!dataListeners.has(id)) {
        dataListeners.set(id, [])
      }
      dataListeners.get(id)!.push(callback)

      // Return unsubscribe function
      return () => {
        const listeners = dataListeners.get(id)
        if (listeners) {
          const index = listeners.indexOf(callback)
          if (index > -1) {
            listeners.splice(index, 1)
          }
        }
      }
    }
  },
  selectFolder: (): Promise<string | null> => {
    return ipcRenderer.invoke('dialog:selectFolder')
  },
  git: {
    getInfo: (dirPath: string) => {
      return ipcRenderer.invoke('git:getInfo', dirPath)
    },
    createWorktree: (repoPath: string, name: string, baseBranch?: string) => {
      return ipcRenderer.invoke('git:createWorktree', repoPath, name, baseBranch)
    },
    removeWorktree: (repoPath: string, worktreePath: string, deleteBranch: boolean = true) => {
      return ipcRenderer.invoke('git:removeWorktree', repoPath, worktreePath, deleteBranch)
    },
    listWorktrees: (repoPath: string) => {
      return ipcRenderer.invoke('git:listWorktrees', repoPath)
    }
  }
})
