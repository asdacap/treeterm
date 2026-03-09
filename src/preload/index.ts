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

type CloseConfirmCallback = () => void
const closeConfirmListeners: CloseConfirmCallback[] = []

ipcRenderer.on('app:confirm-close', () => {
  closeConfirmListeners.forEach((cb) => cb())
})

type ReadyCallback = () => void
const readyListeners: ReadyCallback[] = []
let isReady = false

ipcRenderer.on('app:ready', () => {
  isReady = true
  readyListeners.forEach((cb) => cb())
})

interface SandboxConfig {
  enabled: boolean
  allowNetwork: boolean
  allowedPaths: string[]
}

contextBridge.exposeInMainWorld('electron', {
  platform: process.platform,
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
    isAlive: (id: string): Promise<boolean> => {
      return ipcRenderer.invoke('pty:isAlive', id)
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
    getChildWorktrees: (repoPath: string, parentBranch: string | null) => {
      return ipcRenderer.invoke('git:getChildWorktrees', repoPath, parentBranch)
    },
    getDiff: (worktreePath: string, parentBranch: string) => {
      return ipcRenderer.invoke('git:getDiff', worktreePath, parentBranch)
    },
    getFileDiff: (worktreePath: string, parentBranch: string, filePath: string) => {
      return ipcRenderer.invoke('git:getFileDiff', worktreePath, parentBranch, filePath)
    },
    getDiffAgainstHead: (worktreePath: string, parentBranch: string) => {
      return ipcRenderer.invoke('git:getDiffAgainstHead', worktreePath, parentBranch)
    },
    getFileDiffAgainstHead: (worktreePath: string, parentBranch: string, filePath: string) => {
      return ipcRenderer.invoke('git:getFileDiffAgainstHead', worktreePath, parentBranch, filePath)
    },
    merge: (mainRepoPath: string, worktreeBranch: string, targetBranch: string, squash: boolean = false) => {
      return ipcRenderer.invoke('git:merge', mainRepoPath, worktreeBranch, targetBranch, squash)
    },
    checkMergeConflicts: (repoPath: string, sourceBranch: string, targetBranch: string) => {
      return ipcRenderer.invoke('git:checkMergeConflicts', repoPath, sourceBranch, targetBranch)
    },
    hasUncommittedChanges: (repoPath: string) => {
      return ipcRenderer.invoke('git:hasUncommittedChanges', repoPath)
    },
    commitAll: (repoPath: string, message: string) => {
      return ipcRenderer.invoke('git:commitAll', repoPath, message)
    },
    deleteBranch: (repoPath: string, branchName: string) => {
      return ipcRenderer.invoke('git:deleteBranch', repoPath, branchName)
    },
    getUncommittedChanges: (repoPath: string) => {
      return ipcRenderer.invoke('git:getUncommittedChanges', repoPath)
    },
    getUncommittedFileDiff: (repoPath: string, filePath: string, staged: boolean) => {
      return ipcRenderer.invoke('git:getUncommittedFileDiff', repoPath, filePath, staged)
    },
    stageFile: (repoPath: string, filePath: string) => {
      return ipcRenderer.invoke('git:stageFile', repoPath, filePath)
    },
    unstageFile: (repoPath: string, filePath: string) => {
      return ipcRenderer.invoke('git:unstageFile', repoPath, filePath)
    },
    stageAll: (repoPath: string) => {
      return ipcRenderer.invoke('git:stageAll', repoPath)
    },
    unstageAll: (repoPath: string) => {
      return ipcRenderer.invoke('git:unstageAll', repoPath)
    },
    commitStaged: (repoPath: string, message: string) => {
      return ipcRenderer.invoke('git:commitStaged', repoPath, message)
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
  },
  filesystem: {
    readDirectory: (workspacePath: string, dirPath: string) => {
      return ipcRenderer.invoke('fs:readDirectory', workspacePath, dirPath)
    },
    readFile: (workspacePath: string, filePath: string) => {
      return ipcRenderer.invoke('fs:readFile', workspacePath, filePath)
    },
    writeFile: (workspacePath: string, filePath: string, content: string) => {
      return ipcRenderer.invoke('fs:writeFile', workspacePath, filePath, content)
    }
  },
  sandbox: {
    isAvailable: (): Promise<boolean> => {
      return ipcRenderer.invoke('sandbox:isAvailable')
    }
  },
  getInitialWorkspace: (): Promise<string | null> => {
    return ipcRenderer.invoke('app:getInitialWorkspace')
  },
  app: {
    onReady: (callback: ReadyCallback): (() => void) => {
      if (isReady) {
        // Already ready, call immediately
        callback()
      } else {
        readyListeners.push(callback)
      }
      return () => {
        const index = readyListeners.indexOf(callback)
        if (index > -1) {
          readyListeners.splice(index, 1)
        }
      }
    },
    onCloseConfirm: (callback: CloseConfirmCallback): (() => void) => {
      closeConfirmListeners.push(callback)
      return () => {
        const index = closeConfirmListeners.indexOf(callback)
        if (index > -1) {
          closeConfirmListeners.splice(index, 1)
        }
      }
    },
    confirmClose: (): void => {
      ipcRenderer.send('app:close-confirmed')
    },
    cancelClose: (): void => {
      ipcRenderer.send('app:close-cancelled')
    }
  }
})
