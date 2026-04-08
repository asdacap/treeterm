import { contextBridge } from 'electron'
import type { SandboxConfig, Session, TTYSessionInfo, WorkspaceInput, Settings, SSHConnectionConfig, ConnectionInfo, PortForwardConfig, PortForwardInfo, ReasoningEffort } from '../shared/types'
import type { PtyEvent, ExecEvent } from '../shared/ipc-types'
import { IpcClient } from './ipc-client'
import type { PreloadApi, Platform } from '../renderer/types'

type PtyEventCallback = (event: PtyEvent) => void
const ptyEventListeners = new Map<string, PtyEventCallback[]>()

type ExecEventCallback = (event: ExecEvent) => void
const execEventListeners = new Map<string, ExecEventCallback[]>()

// Initialize IPC client
const client = new IpcClient()

// Listen for pty events from main process (unified stream)
client.onPtyEvent((handle, event) => {
  const listeners = ptyEventListeners.get(handle)
  if (listeners) {
    listeners.forEach((cb) => { cb(event); })
  }
  if (event.type === 'end') {
    ptyEventListeners.delete(handle)
  }
})

// Listen for exec events from main process
client.onExecEvent((execId, event) => {
  const listeners = execEventListeners.get(execId)
  if (listeners) {
    listeners.forEach((cb) => { cb(event); })
  }
  if (event.type === 'exit' || event.type === 'error') {
    execEventListeners.delete(execId)
  }
})

type SettingsOpenCallback = () => void
const settingsOpenListeners: SettingsOpenCallback[] = []

client.onSettingsOpen(() => {
  settingsOpenListeners.forEach((cb) => { cb(); })
})

type CloseConfirmCallback = () => void
const closeConfirmListeners: CloseConfirmCallback[] = []

client.onAppConfirmClose(() => {
  closeConfirmListeners.forEach((cb) => { cb(); })
})

type CapsLockCallback = (event: { type: string; key: string; code: string }) => void
const capsLockListeners: CapsLockCallback[] = []

client.onCapsLockEvent((event) => {
  capsLockListeners.forEach((cb) => { cb(event); })
})

type ReadyCallback = (session: Session | null) => void
const readyListeners: ReadyCallback[] = []
let isReady = false
let initialSession: Session | null = null

client.onAppReady((session) => {
  isReady = true
  initialSession = session
  readyListeners.forEach((cb) => { cb(session); })
})

type SessionsCallback = (sessions: TTYSessionInfo[]) => void
const daemonSessionsListeners: SessionsCallback[] = []

client.onDaemonSessions((sessions) => {
  daemonSessionsListeners.forEach((cb) => { cb(sessions); })
})


type SessionSyncCallback = (connectionId: string, session: Session) => void
const sessionSyncListeners: SessionSyncCallback[] = []

client.onSessionSync((connectionId, session) => {
  sessionSyncListeners.forEach((cb) => { cb(connectionId, session); })
})

type SshAutoConnectedCallback = (session: Session, connection: ConnectionInfo) => void
const sshAutoConnectedListeners: SshAutoConnectedCallback[] = []

client.onSshAutoConnected((session, connection) => {
  sshAutoConnectedListeners.forEach((cb) => { cb(session, connection); })
})

type ConnectionReconnectedCallback = (session: Session, connection: ConnectionInfo) => void
const connectionReconnectedListeners: ConnectionReconnectedCallback[] = []

client.onConnectionReconnected((session, connection) => {
  connectionReconnectedListeners.forEach((cb) => { cb(session, connection); })
})

type DaemonDisconnectedCallback = () => void
const daemonDisconnectedListeners: DaemonDisconnectedCallback[] = []

client.onDaemonDisconnected(() => {
  daemonDisconnectedListeners.forEach((cb) => { cb(); })
})

type ActiveProcessesOpenCallback = () => void
const activeProcessesOpenListeners: ActiveProcessesOpenCallback[] = []

client.onActiveProcessesOpen(() => {
  activeProcessesOpenListeners.forEach((cb) => { cb(); })
})

type SshConnectionStatusCallback = (info: ConnectionInfo) => void
const sshConnectionStatusListeners: SshConnectionStatusCallback[] = []

client.onSshConnectionStatus((info) => {
  sshConnectionStatusListeners.forEach((cb) => { cb(info); })
})

type LlmDeltaCallback = (requestId: string, text: string) => void
const llmDeltaListeners: LlmDeltaCallback[] = []

client.onLlmChatDelta((requestId, text) => {
  llmDeltaListeners.forEach((cb) => { cb(requestId, text); })
})

type LlmDoneCallback = (requestId: string) => void
const llmDoneListeners: LlmDoneCallback[] = []

client.onLlmChatDone((requestId) => {
  llmDoneListeners.forEach((cb) => { cb(requestId); })
})

type LlmErrorCallback = (requestId: string, error: string) => void
const llmErrorListeners: LlmErrorCallback[] = []

client.onLlmChatError((requestId, error) => {
  llmErrorListeners.forEach((cb) => { cb(requestId, error); })
})

type SshOutputCallback = (connectionId: string, line: string) => void
const sshBootstrapOutputListeners: SshOutputCallback[] = []
const sshTunnelOutputListeners: SshOutputCallback[] = []
const sshDaemonOutputListeners: SshOutputCallback[] = []

client.onSshBootstrapOutput((connectionId, line) => {
  sshBootstrapOutputListeners.forEach((cb) => { cb(connectionId, line); })
})
client.onSshTunnelOutput((connectionId, line) => {
  sshTunnelOutputListeners.forEach((cb) => { cb(connectionId, line); })
})
client.onSshDaemonOutput((connectionId, line) => {
  sshDaemonOutputListeners.forEach((cb) => { cb(connectionId, line); })
})

type GitOutputCallback = (operationId: string, data: string) => void
const gitOutputListeners: GitOutputCallback[] = []

client.onGitOutput((operationId, data) => {
  gitOutputListeners.forEach((cb) => { cb(operationId, data); })
})

type SshOutputWatchCallback = (line: string) => void
const sshBootstrapWatchCallbacks = new Map<string, SshOutputWatchCallback>()
const sshTunnelWatchCallbacks = new Map<string, SshOutputWatchCallback>()
const sshDaemonWatchCallbacks = new Map<string, SshOutputWatchCallback>()

client.onSshBootstrapOutput((connectionId, line) => {
  sshBootstrapWatchCallbacks.get(connectionId)?.(line)
})
client.onSshTunnelOutput((connectionId, line) => {
  sshTunnelWatchCallbacks.get(connectionId)?.(line)
})
client.onSshDaemonOutput((connectionId, line) => {
  sshDaemonWatchCallbacks.get(connectionId)?.(line)
})

type SshStatusWatchCallback = (info: ConnectionInfo) => void
const sshStatusWatchCallbacks = new Map<string, SshStatusWatchCallback>()

client.onSshConnectionStatus((info) => {
  const cb = sshStatusWatchCallbacks.get(info.id)
  if (cb) cb(info)
})

type PortForwardStatusCallback = (info: PortForwardInfo) => void
const portForwardStatusListeners: PortForwardStatusCallback[] = []

client.onSshPortForwardStatus((info) => {
  portForwardStatusListeners.forEach((cb) => { cb(info); })
})

type PortForwardOutputWatchCallback = (line: string) => void
const portForwardOutputWatchCallbacks = new Map<string, PortForwardOutputWatchCallback>()

client.onSshPortForwardOutput((portForwardId, line) => {
  const cb = portForwardOutputWatchCallbacks.get(portForwardId)
  if (cb) cb(line)
})

const preloadApi: PreloadApi = {
  platform: process.platform as Platform,
  terminal: {
    create: (connectionId: string, handle: string, cwd: string, sandbox?: SandboxConfig, startupCommand?: string) => {
      return client.ptyCreate(connectionId, handle, cwd, sandbox, startupCommand)
    },
    attach: (connectionId: string, handle: string, sessionId: string) => {
      return client.ptyAttach(connectionId, handle, sessionId)
    },
    list: (connectionId: string): Promise<TTYSessionInfo[]> => {
      return client.ptyList(connectionId)
    },
    write: (id: string, data: string): void => {
      client.ptyWrite(id, data)
    },
    resize: (id: string, cols: number, rows: number): void => {
      client.ptyResize(id, cols, rows)
    },
    kill: (connectionId: string, id: string): void => {
      client.ptyKill(connectionId, id)
    },
    onEvent: (id: string, callback: PtyEventCallback): (() => void) => {
      if (!ptyEventListeners.has(id)) {
        ptyEventListeners.set(id, [])
      }
      ptyEventListeners.get(id)?.push(callback)

      return () => {
        const listeners = ptyEventListeners.get(id)
        if (listeners) {
          const index = listeners.indexOf(callback)
          if (index > -1) listeners.splice(index, 1)
        }
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
    getInfo: (connectionId: string, dirPath: string) => {
      return client.gitGetInfo(connectionId, dirPath)
    },
    createWorktree: (connectionId: string, repoPath: string, name: string, baseBranch?: string, operationId?: string) => {
      return client.gitCreateWorktree(connectionId, repoPath, name, baseBranch, operationId)
    },
    removeWorktree: (connectionId: string, repoPath: string, worktreePath: string, deleteBranch: boolean = true, operationId?: string) => {
      return client.gitRemoveWorktree(connectionId, repoPath, worktreePath, deleteBranch, operationId)
    },
    listWorktrees: (connectionId: string, repoPath: string) => {
      return client.gitListWorktrees(connectionId, repoPath)
    },
    listLocalBranches: (connectionId: string, repoPath: string) => {
      return client.gitListLocalBranches(connectionId, repoPath)
    },
    listRemoteBranches: (connectionId: string, repoPath: string) => {
      return client.gitListRemoteBranches(connectionId, repoPath)
    },
    getBranchesInWorktrees: (connectionId: string, repoPath: string) => {
      return client.gitGetBranchesInWorktrees(connectionId, repoPath)
    },
    createWorktreeFromBranch: (connectionId: string, repoPath: string, branch: string, worktreeName: string, operationId?: string) => {
      return client.gitCreateWorktreeFromBranch(connectionId, repoPath, branch, worktreeName, operationId)
    },
    createWorktreeFromRemote: (connectionId: string, repoPath: string, remoteBranch: string, worktreeName: string, operationId?: string) => {
      return client.gitCreateWorktreeFromRemote(connectionId, repoPath, remoteBranch, worktreeName, operationId)
    },
    getDiff: (connectionId: string, worktreePath: string, parentBranch: string) => {
      return client.gitGetDiff(connectionId, worktreePath, parentBranch)
    },
    getFileDiff: (connectionId: string, worktreePath: string, parentBranch: string, filePath: string) => {
      return client.gitGetFileDiff(connectionId, worktreePath, parentBranch, filePath)
    },
    merge: (connectionId: string, targetWorktreePath: string, worktreeBranch: string, squash: boolean = false, operationId?: string) => {
      return client.gitMerge(connectionId, targetWorktreePath, worktreeBranch, squash, operationId)
    },
    checkMergeConflicts: (connectionId: string, repoPath: string, sourceBranch: string, targetBranch: string) => {
      return client.gitCheckMergeConflicts(connectionId, repoPath, sourceBranch, targetBranch)
    },
    hasUncommittedChanges: (connectionId: string, repoPath: string) => {
      return client.gitHasUncommittedChanges(connectionId, repoPath)
    },
    commitAll: (connectionId: string, repoPath: string, message: string) => {
      return client.gitCommitAll(connectionId, repoPath, message)
    },
    deleteBranch: (connectionId: string, repoPath: string, branchName: string, operationId?: string) => {
      return client.gitDeleteBranch(connectionId, repoPath, branchName, operationId)
    },
    renameBranch: (connectionId: string, repoPath: string, oldName: string, newName: string) => {
      return client.gitRenameBranch(connectionId, repoPath, oldName, newName)
    },
    getUncommittedChanges: (connectionId: string, repoPath: string) => {
      return client.gitGetUncommittedChanges(connectionId, repoPath)
    },
    getUncommittedFileDiff: (connectionId: string, repoPath: string, filePath: string, staged: boolean) => {
      return client.gitGetUncommittedFileDiff(connectionId, repoPath, filePath, staged)
    },
    stageFile: (connectionId: string, repoPath: string, filePath: string) => {
      return client.gitStageFile(connectionId, repoPath, filePath)
    },
    unstageFile: (connectionId: string, repoPath: string, filePath: string) => {
      return client.gitUnstageFile(connectionId, repoPath, filePath)
    },
    stageAll: (connectionId: string, repoPath: string) => {
      return client.gitStageAll(connectionId, repoPath)
    },
    unstageAll: (connectionId: string, repoPath: string) => {
      return client.gitUnstageAll(connectionId, repoPath)
    },
    commitStaged: (connectionId: string, repoPath: string, message: string) => {
      return client.gitCommitStaged(connectionId, repoPath, message)
    },
    getFileContentsForDiff: (connectionId: string, worktreePath: string, parentBranch: string, filePath: string) => {
      return client.gitGetFileContentsForDiff(connectionId, worktreePath, parentBranch, filePath)
    },
    getUncommittedFileContentsForDiff: (connectionId: string, repoPath: string, filePath: string, staged: boolean) => {
      return client.gitGetUncommittedFileContentsForDiff(connectionId, repoPath, filePath, staged)
    },
    getHeadCommitHash: (connectionId: string, repoPath: string) => {
      return client.gitGetHeadCommitHash(connectionId, repoPath)
    },
    getLog: (connectionId: string, repoPath: string, parentBranch: string | null, skip: number, limit: number) => {
      return client.gitGetLog(connectionId, repoPath, parentBranch, skip, limit)
    },
    getCommitDiff: (connectionId: string, repoPath: string, commitHash: string) => {
      return client.gitGetCommitDiff(connectionId, repoPath, commitHash)
    },
    getCommitFileDiff: (connectionId: string, repoPath: string, commitHash: string, filePath: string) => {
      return client.gitGetCommitFileDiff(connectionId, repoPath, commitHash, filePath)
    },
    fetch: (connectionId: string, repoPath: string) => {
      return client.gitFetch(connectionId, repoPath)
    },
    pull: (connectionId: string, repoPath: string) => {
      return client.gitPull(connectionId, repoPath)
    },
    getBehindCount: (connectionId: string, repoPath: string) => {
      return client.gitGetBehindCount(connectionId, repoPath)
    },
    getRemoteUrl: (connectionId: string, repoPath: string) => {
      return client.gitGetRemoteUrl(connectionId, repoPath)
    },
    onOutput: (callback: GitOutputCallback): (() => void) => {
      gitOutputListeners.push(callback)
      return () => {
        const index = gitOutputListeners.indexOf(callback)
        if (index > -1) gitOutputListeners.splice(index, 1)
      }
    }
  },
  github: {
    getPrInfo: (connectionId: string, repoPath: string, head: string, base: string) => {
      return client.githubGetPrInfo(connectionId, repoPath, head, base)
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
    readDirectory: (connectionId: string, workspacePath: string, dirPath: string) => {
      return client.fsReadDirectory(connectionId, workspacePath, dirPath)
    },
    readFile: (connectionId: string, workspacePath: string, filePath: string) => {
      return client.fsReadFile(connectionId, workspacePath, filePath)
    },
    writeFile: (connectionId: string, workspacePath: string, filePath: string, content: string) => {
      return client.fsWriteFile(connectionId, workspacePath, filePath, content)
    },
    searchFiles: (connectionId: string, workspacePath: string, query: string) => {
      return client.fsSearchFiles(connectionId, workspacePath, query)
    }
  },
  exec: {
    start: (connectionId: string, cwd: string, command: string, args: string[]) => {
      return client.execStart(connectionId, cwd, command, args)
    },
    kill: (execId: string): void => {
      client.execKill(execId)
    },
    onEvent: (execId: string, callback: ExecEventCallback): (() => void) => {
      if (!execEventListeners.has(execId)) {
        execEventListeners.set(execId, [])
      }
      execEventListeners.get(execId)?.push(callback)

      return () => {
        const listeners = execEventListeners.get(execId)
        if (listeners) {
          const index = listeners.indexOf(callback)
          if (index > -1) listeners.splice(index, 1)
        }
      }
    }
  },
  runActions: {
    detect: (connectionId: string, workspacePath: string) => {
      return client.runActionsDetect(connectionId, workspacePath)
    },
    run: (connectionId: string, workspacePath: string, actionId: string) => {
      return client.runActionsRun(connectionId, workspacePath, actionId)
    }
  },
  sandbox: {
    isAvailable: (): Promise<boolean> => {
      return client.sandboxIsAvailable()
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
    },
    onSshAutoConnected: (callback: SshAutoConnectedCallback): (() => void) => {
      sshAutoConnectedListeners.push(callback)
      return () => {
        const index = sshAutoConnectedListeners.indexOf(callback)
        if (index > -1) {
          sshAutoConnectedListeners.splice(index, 1)
        }
      }
    },
    onConnectionReconnected: (callback: ConnectionReconnectedCallback): (() => void) => {
      connectionReconnectedListeners.push(callback)
      return () => {
        const index = connectionReconnectedListeners.indexOf(callback)
        if (index > -1) {
          connectionReconnectedListeners.splice(index, 1)
        }
      }
    }
  },
  daemon: {
    shutdown: (connectionId: string) => {
      return client.daemonShutdown(connectionId)
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
    update: (sessionId: string, workspaces: WorkspaceInput[], senderUuid?: string, expectedVersion?: number) => {
      return client.sessionUpdate(sessionId, workspaces, senderUuid, expectedVersion)
    },
    onSync: (callback: SessionSyncCallback): (() => void) => {
      sessionSyncListeners.push(callback)
      return () => {
        const index = sessionSyncListeners.indexOf(callback)
        if (index > -1) sessionSyncListeners.splice(index, 1)
      }
    }
  },
  clipboard: {
    writeText: (text: string): void => { client.clipboardWriteText(text) },
    readText: (): Promise<string> => client.clipboardReadText(),
  },
  getWindowUuid: (): Promise<string> => {
    return client.appGetWindowUuid()
  },
  llm: {
    send: (requestId: string, messages: { role: 'user' | 'assistant' | 'system'; content: string }[], settings: { baseUrl: string; apiKey: string; model: string; reasoning: ReasoningEffort }): Promise<void> => {
      return client.llmChatSend(requestId, messages, settings)
    },
    cancel: (requestId: string): void => {
      client.llmChatCancel(requestId)
    },
    analyzeTerminal: (buffer, cwd, settings) => {
      return client.llmAnalyzeTerminal(buffer, cwd, settings)
    },
    clearAnalyzerCache: (): Promise<void> => {
      return client.llmClearAnalyzerCache()
    },
    generateTitle: (buffer: string, settings: { baseUrl: string; apiKey: string; model: string; titleSystemPrompt: string; reasoningEffort: ReasoningEffort }): Promise<{ title: string; description: string; branchName: string } | { error: string }> => {
      return client.llmGenerateTitle(buffer, settings)
    },
    onDelta: (callback: LlmDeltaCallback): (() => void) => {
      llmDeltaListeners.push(callback)
      return () => {
        const index = llmDeltaListeners.indexOf(callback)
        if (index > -1) llmDeltaListeners.splice(index, 1)
      }
    },
    onDone: (callback: LlmDoneCallback): (() => void) => {
      llmDoneListeners.push(callback)
      return () => {
        const index = llmDoneListeners.indexOf(callback)
        if (index > -1) llmDoneListeners.splice(index, 1)
      }
    },
    onError: (callback: LlmErrorCallback): (() => void) => {
      llmErrorListeners.push(callback)
      return () => {
        const index = llmErrorListeners.indexOf(callback)
        if (index > -1) llmErrorListeners.splice(index, 1)
      }
    }
  },
  ssh: {
    connect: (config: SSHConnectionConfig, options?: { refreshDaemon?: boolean; allowOutdatedDaemon?: boolean }) => {
      return client.sshConnect(config, options)
    },
    disconnect: (connectionId: string): Promise<void> => {
      return client.sshDisconnect(connectionId)
    },
    reconnect: (connectionId: string): Promise<void> => {
      return client.sshReconnect(connectionId)
    },
    reconnectNow: (connectionId: string): Promise<void> => {
      return client.sshReconnectNow(connectionId)
    },
    forceReconnect: (connectionId: string): Promise<void> => {
      return client.sshForceReconnect(connectionId)
    },
    cancelReconnect: (connectionId: string): Promise<void> => {
      return client.sshCancelReconnect(connectionId)
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
    onConnectionStatus: (callback: SshConnectionStatusCallback): (() => void) => {
      sshConnectionStatusListeners.push(callback)
      return () => {
        const index = sshConnectionStatusListeners.indexOf(callback)
        if (index > -1) sshConnectionStatusListeners.splice(index, 1)
      }
    },
    onBootstrapOutput: (callback: SshOutputCallback): (() => void) => {
      sshBootstrapOutputListeners.push(callback)
      return () => {
        const index = sshBootstrapOutputListeners.indexOf(callback)
        if (index > -1) sshBootstrapOutputListeners.splice(index, 1)
      }
    },
    onTunnelOutput: (callback: SshOutputCallback): (() => void) => {
      sshTunnelOutputListeners.push(callback)
      return () => {
        const index = sshTunnelOutputListeners.indexOf(callback)
        if (index > -1) sshTunnelOutputListeners.splice(index, 1)
      }
    },
    onDaemonOutput: (callback: SshOutputCallback): (() => void) => {
      sshDaemonOutputListeners.push(callback)
      return () => {
        const index = sshDaemonOutputListeners.indexOf(callback)
        if (index > -1) sshDaemonOutputListeners.splice(index, 1)
      }
    },
    watchBootstrapOutput: async (connectionId: string, cb: (line: string) => void): Promise<{ scrollback: string[], unsubscribe: () => void }> => {
      sshBootstrapWatchCallbacks.set(connectionId, cb)
      const result = await client.sshWatchBootstrapOutput(connectionId)
      return {
        scrollback: result.scrollback,
        unsubscribe: () => {
          sshBootstrapWatchCallbacks.delete(connectionId)
          client.sshUnwatchBootstrapOutput(connectionId).catch(() => {})
        }
      }
    },
    watchTunnelOutput: async (connectionId: string, cb: (line: string) => void): Promise<{ scrollback: string[], unsubscribe: () => void }> => {
      sshTunnelWatchCallbacks.set(connectionId, cb)
      const result = await client.sshWatchTunnelOutput(connectionId)
      return {
        scrollback: result.scrollback,
        unsubscribe: () => {
          sshTunnelWatchCallbacks.delete(connectionId)
          client.sshUnwatchTunnelOutput(connectionId).catch(() => {})
        }
      }
    },
    watchDaemonOutput: async (connectionId: string, cb: (line: string) => void): Promise<{ scrollback: string[], unsubscribe: () => void }> => {
      sshDaemonWatchCallbacks.set(connectionId, cb)
      const result = await client.sshWatchDaemonOutput(connectionId)
      return {
        scrollback: result.scrollback,
        unsubscribe: () => {
          sshDaemonWatchCallbacks.delete(connectionId)
          client.sshUnwatchDaemonOutput(connectionId).catch(() => {})
        }
      }
    },
    watchConnectionStatus: async (connectionId: string, cb: (info: ConnectionInfo) => void): Promise<{ initial: ConnectionInfo, unsubscribe: () => void }> => {
      sshStatusWatchCallbacks.set(connectionId, cb)
      const result = await client.sshWatchConnectionStatus(connectionId)
      return {
        initial: result.initial,
        unsubscribe: () => {
          sshStatusWatchCallbacks.delete(connectionId)
          client.sshUnwatchConnectionStatus(connectionId).catch(() => {})
        }
      }
    },
    addPortForward: (config: PortForwardConfig): Promise<PortForwardInfo> => {
      return client.sshAddPortForward(config)
    },
    removePortForward: (portForwardId: string): Promise<void> => {
      return client.sshRemovePortForward(portForwardId)
    },
    listPortForwards: (connectionId: string): Promise<PortForwardInfo[]> => {
      return client.sshListPortForwards(connectionId)
    },
    onPortForwardStatus: (callback: PortForwardStatusCallback): (() => void) => {
      portForwardStatusListeners.push(callback)
      return () => {
        const index = portForwardStatusListeners.indexOf(callback)
        if (index > -1) portForwardStatusListeners.splice(index, 1)
      }
    },
    watchPortForwardOutput: async (portForwardId: string, cb: (line: string) => void): Promise<{ scrollback: string[], unsubscribe: () => void }> => {
      portForwardOutputWatchCallbacks.set(portForwardId, cb)
      const result = await client.sshWatchPortForwardOutput(portForwardId)
      return {
        scrollback: result.scrollback,
        unsubscribe: () => {
          portForwardOutputWatchCallbacks.delete(portForwardId)
          client.sshUnwatchPortForwardOutput(portForwardId).catch(() => {})
        }
      }
    }
  }
}

contextBridge.exposeInMainWorld('electron', preloadApi)
