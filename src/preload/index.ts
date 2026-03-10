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

type CapsLockCallback = (event: { type: string; key: string; code: string }) => void
const capsLockListeners: CapsLockCallback[] = []

ipcRenderer.on('capslock-event', (_event, data) => {
  capsLockListeners.forEach((cb) => cb(data))
})

type ReadyCallback = () => void
const readyListeners: ReadyCallback[] = []
let isReady = false

ipcRenderer.on('app:ready', () => {
  isReady = true
  readyListeners.forEach((cb) => cb())
})

type DaemonSessionsCallback = (sessions: any[]) => void
const daemonSessionsListeners: DaemonSessionsCallback[] = []

ipcRenderer.on('daemon:sessions', (_event, sessions) => {
  daemonSessionsListeners.forEach((cb) => cb(sessions))
})


type TerminalMenuCallback = () => void
const terminalNewListeners: TerminalMenuCallback[] = []
const terminalShowSessionsListeners: TerminalMenuCallback[] = []

ipcRenderer.on('terminal:new', () => {
  terminalNewListeners.forEach((cb) => cb())
})

ipcRenderer.on('terminal:show-sessions', () => {
  terminalShowSessionsListeners.forEach((cb) => cb())
})

type SessionMenuCallback = () => void
const sessionShowSessionsListeners: SessionMenuCallback[] = []

ipcRenderer.on('session:show-sessions', () => {
  sessionShowSessionsListeners.forEach((cb) => cb())
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
    attach: (sessionId: string): Promise<{ success: boolean; scrollback?: string[]; error?: string }> => {
      return ipcRenderer.invoke('pty:attach', sessionId)
    },
    detach: (sessionId: string): Promise<void> => {
      return ipcRenderer.invoke('pty:detach', sessionId)
    },
    list: (): Promise<Array<{ id: string; cwd: string; createdAt: number; attachedClients: number }>> => {
      return ipcRenderer.invoke('pty:list')
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
    },
    onNewTerminal: (callback: TerminalMenuCallback): (() => void) => {
      terminalNewListeners.push(callback)
      return () => {
        const index = terminalNewListeners.indexOf(callback)
        if (index > -1) terminalNewListeners.splice(index, 1)
      }
    },
    onShowSessions: (callback: TerminalMenuCallback): (() => void) => {
      terminalShowSessionsListeners.push(callback)
      return () => {
        const index = terminalShowSessionsListeners.indexOf(callback)
        if (index > -1) terminalShowSessionsListeners.splice(index, 1)
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
    listLocalBranches: (repoPath: string) => {
      return ipcRenderer.invoke('git:listLocalBranches', repoPath)
    },
    listRemoteBranches: (repoPath: string) => {
      return ipcRenderer.invoke('git:listRemoteBranches', repoPath)
    },
    getBranchesInWorktrees: (repoPath: string) => {
      return ipcRenderer.invoke('git:getBranchesInWorktrees', repoPath)
    },
    createWorktreeFromBranch: (repoPath: string, branch: string, worktreeName: string) => {
      return ipcRenderer.invoke('git:createWorktreeFromBranch', repoPath, branch, worktreeName)
    },
    createWorktreeFromRemote: (repoPath: string, remoteBranch: string, worktreeName: string) => {
      return ipcRenderer.invoke('git:createWorktreeFromRemote', repoPath, remoteBranch, worktreeName)
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
    },
    getFileContentsForDiff: (worktreePath: string, parentBranch: string, filePath: string) => {
      return ipcRenderer.invoke('git:getFileContentsForDiff', worktreePath, parentBranch, filePath)
    },
    getFileContentsForDiffAgainstHead: (worktreePath: string, parentBranch: string, filePath: string) => {
      return ipcRenderer.invoke('git:getFileContentsForDiffAgainstHead', worktreePath, parentBranch, filePath)
    },
    getUncommittedFileContentsForDiff: (repoPath: string, filePath: string, staged: boolean) => {
      return ipcRenderer.invoke('git:getUncommittedFileContentsForDiff', repoPath, filePath, staged)
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
  stt: {
    transcribeOpenAI: (audioBuffer: ArrayBuffer, apiKey: string, language?: string): Promise<{ text: string }> => {
      return ipcRenderer.invoke('stt:transcribe-openai', audioBuffer, apiKey, language)
    },
    transcribeLocal: (
      audioBuffer: ArrayBuffer,
      modelPath: string,
      language?: string
    ): Promise<{ text: string }> => {
      return ipcRenderer.invoke('stt:transcribe-local', audioBuffer, modelPath, language)
    },
    checkMicPermission: (): Promise<boolean> => {
      return ipcRenderer.invoke('stt:check-mic-permission')
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
    },
    onCapsLockEvent: (callback: CapsLockCallback): (() => void) => {
      capsLockListeners.push(callback)
      return () => {
        const index = capsLockListeners.indexOf(callback)
        if (index > -1) {
          capsLockListeners.splice(index, 1)
        }
      }
    }
  },
  daemon: {
    shutdown: (): Promise<{ success: boolean; error?: string }> => {
      return ipcRenderer.invoke('daemon:shutdown')
    },
    onSessions: (callback: DaemonSessionsCallback): (() => void) => {
      daemonSessionsListeners.push(callback)
      return () => {
        const index = daemonSessionsListeners.indexOf(callback)
        if (index > -1) {
          daemonSessionsListeners.splice(index, 1)
        }
      }
    }
  },
  session: {
    create: (workspaces: any[]): Promise<{ success: boolean; session?: any; error?: string }> => {
      return ipcRenderer.invoke('session:create', workspaces)
    },
    update: (sessionId: string, workspaces: any[]): Promise<{ success: boolean; session?: any; error?: string }> => {
      return ipcRenderer.invoke('session:update', sessionId, workspaces)
    },
    list: (): Promise<{ success: boolean; sessions?: any[]; error?: string }> => {
      return ipcRenderer.invoke('session:list')
    },
    get: (sessionId: string): Promise<{ success: boolean; session?: any; error?: string }> => {
      return ipcRenderer.invoke('session:get', sessionId)
    },
    delete: (sessionId: string): Promise<{ success: boolean; error?: string }> => {
      return ipcRenderer.invoke('session:delete', sessionId)
    },
    onShowSessions: (callback: SessionMenuCallback): (() => void) => {
      sessionShowSessionsListeners.push(callback)
      return () => {
        const index = sessionShowSessionsListeners.indexOf(callback)
        if (index > -1) sessionShowSessionsListeners.splice(index, 1)
      }
    }
  }
})
