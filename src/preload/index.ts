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

type SettingsOpenCallback = () => void
const settingsOpenListeners: SettingsOpenCallback[] = []

ipcRenderer.on('settings:open', () => {
  settingsOpenListeners.forEach((cb) => cb())
})

interface SandboxConfig {
  enabled: boolean
  allowNetwork: boolean
  allowedPaths: string[]
}

contextBridge.exposeInMainWorld('electron', {
  terminal: {
    create: (cwd: string, sandbox?: SandboxConfig, startupCommand?: string): Promise<string> => {
      return ipcRenderer.invoke('pty:create', cwd, sandbox, startupCommand)
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
    },
    getDiff: (worktreePath: string, parentBranch: string) => {
      return ipcRenderer.invoke('git:getDiff', worktreePath, parentBranch)
    },
    getFileDiff: (worktreePath: string, parentBranch: string, filePath: string) => {
      return ipcRenderer.invoke('git:getFileDiff', worktreePath, parentBranch, filePath)
    },
    merge: (mainRepoPath: string, worktreeBranch: string, targetBranch: string, squash: boolean = false) => {
      return ipcRenderer.invoke('git:merge', mainRepoPath, worktreeBranch, targetBranch, squash)
    },
    hasUncommittedChanges: (repoPath: string) => {
      return ipcRenderer.invoke('git:hasUncommittedChanges', repoPath)
    },
    commitAll: (repoPath: string, message: string) => {
      return ipcRenderer.invoke('git:commitAll', repoPath, message)
    },
    deleteBranch: (repoPath: string, branchName: string) => {
      return ipcRenderer.invoke('git:deleteBranch', repoPath, branchName)
    }
  },
  settings: {
    load: () => {
      return ipcRenderer.invoke('settings:load')
    },
    save: (settings: unknown) => {
      return ipcRenderer.invoke('settings:save', settings)
    },
    onOpen: (callback: SettingsOpenCallback): (() => void) => {
      settingsOpenListeners.push(callback)
      return () => {
        const index = settingsOpenListeners.indexOf(callback)
        if (index > -1) {
          settingsOpenListeners.splice(index, 1)
        }
      }
    }
  }
})
