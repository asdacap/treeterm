import { contextBridge } from 'electron'
import type { SandboxConfig, Session, SessionInfo, WorkspaceInput, Settings, SSHConnectionConfig, ConnectionInfo } from '../shared/types'
import { IpcClient } from './ipc-client'

type DataCallback = (data: string) => void
const dataListeners = new Map<string, DataCallback[]>()

type ExitCallback = (exitCode: number) => void
const exitListeners = new Map<string, ExitCallback[]>()

// Initialize IPC client
const client = new IpcClient()

// Listen for pty data from main process
client.onPtyData((id, data) => {
  const listeners = dataListeners.get(id)
  if (listeners) {
    listeners.forEach((cb) => cb(data))
  }
})

client.onPtyExit((id, exitCode) => {
  const listeners = exitListeners.get(id)
  if (listeners) {
    listeners.forEach((cb) => cb(exitCode))
  }
  dataListeners.delete(id)
  exitListeners.delete(id)
})

type SettingsOpenCallback = () => void
const settingsOpenListeners: SettingsOpenCallback[] = []

client.onSettingsOpen(() => {
  settingsOpenListeners.forEach((cb) => cb())
})

type CloseConfirmCallback = () => void
const closeConfirmListeners: CloseConfirmCallback[] = []

client.onAppConfirmClose(() => {
  closeConfirmListeners.forEach((cb) => cb())
})

type CapsLockCallback = (event: { type: string; key: string; code: string }) => void
const capsLockListeners: CapsLockCallback[] = []

client.onCapsLockEvent((event) => {
  capsLockListeners.forEach((cb) => cb(event))
})

type ReadyCallback = (session: Session | null) => void
const readyListeners: ReadyCallback[] = []
let isReady = false
let initialSession: Session | null = null

client.onAppReady((session) => {
  isReady = true
  initialSession = session
  readyListeners.forEach((cb) => cb(session))
})

type SessionsCallback = (sessions: SessionInfo[]) => void
const daemonSessionsListeners: SessionsCallback[] = []

client.onDaemonSessions((sessions) => {
  daemonSessionsListeners.forEach((cb) => cb(sessions))
})


type TerminalMenuCallback = () => void
const terminalNewListeners: TerminalMenuCallback[] = []
const terminalShowSessionsListeners: TerminalMenuCallback[] = []

client.onTerminalNew(() => {
  terminalNewListeners.forEach((cb) => cb())
})

client.onTerminalShowSessions(() => {
  terminalShowSessionsListeners.forEach((cb) => cb())
})

type SessionMenuCallback = () => void
const sessionShowSessionsListeners: SessionMenuCallback[] = []

client.onSessionShowSessions(() => {
  sessionShowSessionsListeners.forEach((cb) => cb())
})

type SessionSyncCallback = (session: Session) => void
const sessionSyncListeners: SessionSyncCallback[] = []

client.onSessionSync((session) => {
  sessionSyncListeners.forEach((cb) => cb(session))
})

type DaemonDisconnectedCallback = () => void
const daemonDisconnectedListeners: DaemonDisconnectedCallback[] = []

client.onDaemonDisconnected(() => {
  daemonDisconnectedListeners.forEach((cb) => cb())
})

type ActiveProcessesOpenCallback = () => void
const activeProcessesOpenListeners: ActiveProcessesOpenCallback[] = []

client.onActiveProcessesOpen(() => {
  activeProcessesOpenListeners.forEach((cb) => cb())
})

type SshConnectionStatusCallback = (info: ConnectionInfo) => void
const sshConnectionStatusListeners: SshConnectionStatusCallback[] = []

client.onSshConnectionStatus((info) => {
  sshConnectionStatusListeners.forEach((cb) => cb(info))
})

type SshOutputCallback = (connectionId: string, line: string) => void
const sshOutputListeners: SshOutputCallback[] = []

client.onSshOutput((connectionId, line) => {
  sshOutputListeners.forEach((cb) => cb(connectionId, line))
})

contextBridge.exposeInMainWorld('electron', {
  platform: process.platform,
  terminal: {
    create: (cwd: string, sandbox?: SandboxConfig, startupCommand?: string): Promise<string | null> => {
      return client.ptyCreate(cwd, sandbox, startupCommand)
    },
    attach: (sessionId: string): Promise<{ success: boolean; scrollback?: string[]; error?: string }> => {
      return client.ptyAttach(sessionId)
    },
    list: (): Promise<SessionInfo[]> => {
      return client.ptyList()
    },
    write: (id: string, data: string): void => {
      client.ptyWrite(id, data)
    },
    resize: (id: string, cols: number, rows: number): void => {
      client.ptyResize(id, cols, rows)
    },
    kill: (id: string): void => {
      client.ptyKill(id)
    },
    isAlive: (id: string): Promise<boolean> => {
      return client.ptyIsAlive(id)
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
    onExit: (id: string, callback: ExitCallback): (() => void) => {
      if (!exitListeners.has(id)) {
        exitListeners.set(id, [])
      }
      exitListeners.get(id)!.push(callback)

      // Return unsubscribe function
      return () => {
        const listeners = exitListeners.get(id)
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
    },
    onActiveProcessesOpen: (callback: ActiveProcessesOpenCallback): (() => void) => {
      activeProcessesOpenListeners.push(callback)
      return () => {
        const index = activeProcessesOpenListeners.indexOf(callback)
        if (index > -1) activeProcessesOpenListeners.splice(index, 1)
      }
    }
  },
  selectFolder: (): Promise<string | null> => {
    return client.dialogSelectFolder()
  },
  getRecentDirectories: (): Promise<string[]> => {
    return client.dialogGetRecentDirectories()
  },
  git: {
    getInfo: (dirPath: string) => {
      return client.gitGetInfo(dirPath)
    },
    createWorktree: (repoPath: string, name: string, baseBranch?: string) => {
      return client.gitCreateWorktree(repoPath, name, baseBranch)
    },
    removeWorktree: (repoPath: string, worktreePath: string, deleteBranch: boolean = true) => {
      return client.gitRemoveWorktree(repoPath, worktreePath, deleteBranch)
    },
    listWorktrees: (repoPath: string) => {
      return client.gitListWorktrees(repoPath)
    },
    getChildWorktrees: (repoPath: string, parentBranch: string | null) => {
      return client.gitGetChildWorktrees(repoPath, parentBranch)
    },
    listLocalBranches: (repoPath: string) => {
      return client.gitListLocalBranches(repoPath)
    },
    listRemoteBranches: (repoPath: string) => {
      return client.gitListRemoteBranches(repoPath)
    },
    getBranchesInWorktrees: (repoPath: string) => {
      return client.gitGetBranchesInWorktrees(repoPath)
    },
    createWorktreeFromBranch: (repoPath: string, branch: string, worktreeName: string) => {
      return client.gitCreateWorktreeFromBranch(repoPath, branch, worktreeName)
    },
    createWorktreeFromRemote: (repoPath: string, remoteBranch: string, worktreeName: string) => {
      return client.gitCreateWorktreeFromRemote(repoPath, remoteBranch, worktreeName)
    },
    getDiff: (worktreePath: string, parentBranch: string) => {
      return client.gitGetDiff(worktreePath, parentBranch)
    },
    getFileDiff: (worktreePath: string, parentBranch: string, filePath: string) => {
      return client.gitGetFileDiff(worktreePath, parentBranch, filePath)
    },
    merge: (mainRepoPath: string, worktreeBranch: string, targetBranch: string, squash: boolean = false) => {
      return client.gitMerge(mainRepoPath, worktreeBranch, targetBranch, squash)
    },
    checkMergeConflicts: (repoPath: string, sourceBranch: string, targetBranch: string) => {
      return client.gitCheckMergeConflicts(repoPath, sourceBranch, targetBranch)
    },
    hasUncommittedChanges: (repoPath: string) => {
      return client.gitHasUncommittedChanges(repoPath)
    },
    commitAll: (repoPath: string, message: string) => {
      return client.gitCommitAll(repoPath, message)
    },
    deleteBranch: (repoPath: string, branchName: string) => {
      return client.gitDeleteBranch(repoPath, branchName)
    },
    getUncommittedChanges: (repoPath: string) => {
      return client.gitGetUncommittedChanges(repoPath)
    },
    getUncommittedFileDiff: (repoPath: string, filePath: string, staged: boolean) => {
      return client.gitGetUncommittedFileDiff(repoPath, filePath, staged)
    },
    stageFile: (repoPath: string, filePath: string) => {
      return client.gitStageFile(repoPath, filePath)
    },
    unstageFile: (repoPath: string, filePath: string) => {
      return client.gitUnstageFile(repoPath, filePath)
    },
    stageAll: (repoPath: string) => {
      return client.gitStageAll(repoPath)
    },
    unstageAll: (repoPath: string) => {
      return client.gitUnstageAll(repoPath)
    },
    commitStaged: (repoPath: string, message: string) => {
      return client.gitCommitStaged(repoPath, message)
    },
    getFileContentsForDiff: (worktreePath: string, parentBranch: string, filePath: string) => {
      return client.gitGetFileContentsForDiff(worktreePath, parentBranch, filePath)
    },
    getUncommittedFileContentsForDiff: (repoPath: string, filePath: string, staged: boolean) => {
      return client.gitGetUncommittedFileContentsForDiff(repoPath, filePath, staged)
    },
    getHeadCommitHash: (repoPath: string) => {
      return client.gitGetHeadCommitHash(repoPath)
    }
  },
  settings: {
    load: () => {
      return client.settingsLoad()
    },
    save: (settings: Settings) => {
      return client.settingsSave(settings)
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
      return client.fsReadDirectory(workspacePath, dirPath)
    },
    readFile: (workspacePath: string, filePath: string) => {
      return client.fsReadFile(workspacePath, filePath)
    },
    writeFile: (workspacePath: string, filePath: string, content: string) => {
      return client.fsWriteFile(workspacePath, filePath, content)
    },
    searchFiles: (workspacePath: string, query: string) => {
      return client.fsSearchFiles(workspacePath, query)
    }
  },
  runActions: {
    detect: (workspacePath: string) => {
      return client.runActionsDetect(workspacePath)
    },
    run: (workspacePath: string, actionId: string) => {
      return client.runActionsRun(workspacePath, actionId)
    }
  },
  sandbox: {
    isAvailable: (): Promise<boolean> => {
      return client.sandboxIsAvailable()
    }
  },
  stt: {
    transcribeOpenAI: (audioBuffer: ArrayBuffer, apiKey: string, language?: string): Promise<{ text: string }> => {
      return client.sttTranscribeOpenai(audioBuffer, apiKey, language)
    },
    transcribeLocal: (
      audioBuffer: ArrayBuffer,
      modelPath: string,
      language?: string
    ): Promise<{ text: string }> => {
      return client.sttTranscribeLocal(audioBuffer, modelPath, language)
    },
    checkMicPermission: (): Promise<boolean> => {
      return client.sttCheckMicPermission()
    }
  },
  getInitialWorkspace: (): Promise<string | null> => {
    return client.appGetInitialWorkspace()
  },
  app: {
    onReady: (callback: ReadyCallback): (() => void) => {
      if (isReady) {
        // Already ready, call immediately with the initial session
        callback(initialSession)
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
      client.appCloseConfirmed()
    },
    cancelClose: (): void => {
      client.appCloseCancelled()
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
      return client.daemonShutdown()
    },
    onSessions: (callback: SessionsCallback): (() => void) => {
      daemonSessionsListeners.push(callback)
      return () => {
        const index = daemonSessionsListeners.indexOf(callback)
        if (index > -1) {
          daemonSessionsListeners.splice(index, 1)
        }
      }
    },
    onDisconnected: (callback: DaemonDisconnectedCallback): (() => void) => {
      daemonDisconnectedListeners.push(callback)
      return () => {
        const index = daemonDisconnectedListeners.indexOf(callback)
        if (index > -1) {
          daemonDisconnectedListeners.splice(index, 1)
        }
      }
    }
  },
  session: {
    create: (workspaces: WorkspaceInput[]): Promise<{ success: boolean; session?: Session; error?: string }> => {
      return client.sessionCreate(workspaces)
    },
    update: (sessionId: string, workspaces: WorkspaceInput[], senderUuid?: string): Promise<{ success: boolean; session?: Session; error?: string }> => {
      return client.sessionUpdate(sessionId, workspaces, senderUuid)
    },
    list: (): Promise<{ success: boolean; sessions?: Session[]; error?: string }> => {
      return client.sessionList()
    },
    delete: (sessionId: string): Promise<{ success: boolean; error?: string }> => {
      return client.sessionDelete(sessionId)
    },
    openInNewWindow: (sessionId: string): Promise<{ success: boolean; error?: string }> => {
      return client.sessionOpenInNewWindow(sessionId)
    },
    onShowSessions: (callback: SessionMenuCallback): (() => void) => {
      sessionShowSessionsListeners.push(callback)
      return () => {
        const index = sessionShowSessionsListeners.indexOf(callback)
        if (index > -1) sessionShowSessionsListeners.splice(index, 1)
      }
    },
    onSync: (callback: SessionSyncCallback): (() => void) => {
      sessionSyncListeners.push(callback)
      return () => {
        const index = sessionSyncListeners.indexOf(callback)
        if (index > -1) sessionSyncListeners.splice(index, 1)
      }
    }
  },
  getWindowUuid: (): Promise<string> => {
    return client.appGetWindowUuid()
  },
  ssh: {
    connect: (config: SSHConnectionConfig): Promise<ConnectionInfo> => {
      return client.sshConnect(config)
    },
    disconnect: (connectionId: string): Promise<void> => {
      return client.sshDisconnect(connectionId)
    },
    listConnections: (): Promise<ConnectionInfo[]> => {
      return client.sshListConnections()
    },
    saveConnection: (config: SSHConnectionConfig): Promise<void> => {
      return client.sshSaveConnection(config)
    },
    getSavedConnections: (): Promise<SSHConnectionConfig[]> => {
      return client.sshGetSavedConnections()
    },
    removeSavedConnection: (id: string): Promise<void> => {
      return client.sshRemoveSavedConnection(id)
    },
    getOutput: (connectionId: string): Promise<string[]> => {
      return client.sshGetOutput(connectionId)
    },
    onConnectionStatus: (callback: SshConnectionStatusCallback): (() => void) => {
      sshConnectionStatusListeners.push(callback)
      return () => {
        const index = sshConnectionStatusListeners.indexOf(callback)
        if (index > -1) sshConnectionStatusListeners.splice(index, 1)
      }
    },
    onOutput: (callback: SshOutputCallback): (() => void) => {
      sshOutputListeners.push(callback)
      return () => {
        const index = sshOutputListeners.indexOf(callback)
        if (index > -1) sshOutputListeners.splice(index, 1)
      }
    }
  }
})
