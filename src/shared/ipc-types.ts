/**
 * IPC type definitions for Electron IPC communication.
 * Single source of truth for all IPC messages between main and renderer processes.
 */

/** Discriminated union for IPC results — use instead of { success: boolean; data?: T; error?: string } */
export type IpcOk<T = Record<never, never>> = { success: true } & T
export type IpcErr = { success: false; error: string }
export type IpcResult<T = Record<never, never>> = IpcOk<T> | IpcErr

import type {
  SandboxConfig,
  Settings,
  Session,
  TTYSessionInfo,
  WorkspaceInput,
  RunAction,
  SSHConnectionConfig,
  ConnectionInfo,
  PortForwardConfig,
  PortForwardInfo,
  ReasoningEffort
} from './types'

/** Discriminated union for PTY output events (mirrors gRPC PtyOutput) */
export type PtyEvent =
  | { type: 'data'; data: Uint8Array }
  | { type: 'exit'; exitCode: number; signal?: number }
  | { type: 'resize'; cols: number; rows: number }
  | { type: 'error'; message: string }
  | { type: 'end' }

/** Discriminated union for exec output events (mirrors PtyEvent pattern) */
export type ExecEvent =
  | { type: 'stdout'; data: string }
  | { type: 'stderr'; data: string }
  | { type: 'exit'; exitCode: number }
  | { type: 'error'; message: string }

import type {
  GitInfo,
  WorktreeResult,
  WorktreeInfo,
  DiffResult,
  DiffFile,
  ConflictCheckResult,
  UncommittedChanges,
  FileDiffContents,
  GitLogResult,
  DirectoryContents,
  FileContents,
  FileEntry,
  GitHubPrInfoResult
} from '../renderer/types'

// === Request Types (renderer calls, server handles) ===

export interface IpcRequests {
  // PTY operations
  ptyCreate: {
    params: [connectionId: string, handle: string, cwd: string, sandbox?: SandboxConfig, startupCommand?: string]
    result: IpcResult<{ sessionId: string }>
  }
  ptyAttach: {
    params: [connectionId: string, handle: string, sessionId: string]
    result: IpcResult
  }
  ptyList: {
    params: [connectionId: string]
    result: TTYSessionInfo[]
  }

  // Git operations
  gitGetInfo: {
    params: [connectionId: string, dirPath: string]
    result: GitInfo
  }
  gitCreateWorktree: {
    params: [connectionId: string, repoPath: string, name: string, baseBranch?: string, operationId?: string]
    result: WorktreeResult
  }
  gitRemoveWorktree: {
    params: [connectionId: string, repoPath: string, worktreePath: string, deleteBranch?: boolean, operationId?: string]
    result: IpcResult
  }
  gitListWorktrees: {
    params: [connectionId: string, repoPath: string]
    result: WorktreeInfo[]
  }
  gitListLocalBranches: {
    params: [connectionId: string, repoPath: string]
    result: string[]
  }
  gitListRemoteBranches: {
    params: [connectionId: string, repoPath: string]
    result: string[]
  }
  gitGetBranchesInWorktrees: {
    params: [connectionId: string, repoPath: string]
    result: string[]
  }
  gitCreateWorktreeFromBranch: {
    params: [connectionId: string, repoPath: string, branch: string, worktreeName: string, operationId?: string]
    result: WorktreeResult
  }
  gitCreateWorktreeFromRemote: {
    params: [connectionId: string, repoPath: string, remoteBranch: string, worktreeName: string, operationId?: string]
    result: WorktreeResult
  }
  gitGetDiff: {
    params: [connectionId: string, worktreePath: string, parentBranch: string]
    result: IpcResult<{ diff: DiffResult }>
  }
  gitGetFileDiff: {
    params: [connectionId: string, worktreePath: string, parentBranch: string, filePath: string]
    result: IpcResult<{ diff: string }>
  }
  gitMerge: {
    params: [connectionId: string, targetWorktreePath: string, worktreeBranch: string, squash: boolean, operationId?: string]
    result: IpcResult
  }
  gitCheckMergeConflicts: {
    params: [connectionId: string, repoPath: string, sourceBranch: string, targetBranch: string]
    result: ConflictCheckResult
  }
  gitHasUncommittedChanges: {
    params: [connectionId: string, repoPath: string]
    result: boolean
  }
  gitCommitAll: {
    params: [connectionId: string, repoPath: string, message: string]
    result: IpcResult
  }
  gitDeleteBranch: {
    params: [connectionId: string, repoPath: string, branchName: string, operationId?: string]
    result: IpcResult
  }
  gitRenameBranch: {
    params: [connectionId: string, repoPath: string, oldName: string, newName: string]
    result: IpcResult
  }
  gitGetUncommittedChanges: {
    params: [connectionId: string, repoPath: string]
    result: IpcResult<{ changes: UncommittedChanges }>
  }
  gitGetUncommittedFileDiff: {
    params: [connectionId: string, repoPath: string, filePath: string, staged: boolean]
    result: IpcResult<{ diff: string }>
  }
  gitStageFile: {
    params: [connectionId: string, repoPath: string, filePath: string]
    result: IpcResult
  }
  gitUnstageFile: {
    params: [connectionId: string, repoPath: string, filePath: string]
    result: IpcResult
  }
  gitStageAll: {
    params: [connectionId: string, repoPath: string]
    result: IpcResult
  }
  gitUnstageAll: {
    params: [connectionId: string, repoPath: string]
    result: IpcResult
  }
  gitCommitStaged: {
    params: [connectionId: string, repoPath: string, message: string]
    result: IpcResult
  }
  gitGetFileContentsForDiff: {
    params: [connectionId: string, worktreePath: string, parentBranch: string, filePath: string]
    result: IpcResult<{ contents: FileDiffContents }>
  }
  gitGetUncommittedFileContentsForDiff: {
    params: [connectionId: string, repoPath: string, filePath: string, staged: boolean]
    result: IpcResult<{ contents: FileDiffContents }>
  }
  gitGetHeadCommitHash: {
    params: [connectionId: string, repoPath: string]
    result: IpcResult<{ hash: string }>
  }
  gitGetLog: {
    params: [connectionId: string, repoPath: string, parentBranch: string | null, skip: number, limit: number]
    result: IpcResult<{ result: GitLogResult }>
  }
  gitGetCommitDiff: {
    params: [connectionId: string, repoPath: string, commitHash: string]
    result: IpcResult<{ files: DiffFile[] }>
  }
  gitGetCommitFileDiff: {
    params: [connectionId: string, repoPath: string, commitHash: string, filePath: string]
    result: IpcResult<{ contents: FileDiffContents }>
  }

  // Git fetch/pull operations
  gitFetch: {
    params: [connectionId: string, repoPath: string]
    result: IpcResult
  }
  gitPull: {
    params: [connectionId: string, repoPath: string]
    result: IpcResult
  }
  gitGetBehindCount: {
    params: [connectionId: string, repoPath: string]
    result: number
  }

  // GitHub operations
  gitGetRemoteUrl: {
    params: [connectionId: string, repoPath: string]
    result: IpcResult<{ url: string }>
  }
  githubGetPrInfo: {
    params: [connectionId: string, repoPath: string, head: string, base: string]
    result: GitHubPrInfoResult
  }

  // Settings operations
  settingsLoad: {
    params: []
    result: Settings
  }
  settingsSave: {
    params: [settings: Settings]
    result: { success: boolean }
  }

  // Filesystem operations
  fsReadDirectory: {
    params: [connectionId: string, workspacePath: string, dirPath: string]
    result: IpcResult<{ contents: DirectoryContents }>
  }
  fsReadFile: {
    params: [connectionId: string, workspacePath: string, filePath: string]
    result: IpcResult<{ file: FileContents }>
  }
  fsWriteFile: {
    params: [connectionId: string, workspacePath: string, filePath: string, content: string]
    result: IpcResult
  }
  fsSearchFiles: {
    params: [connectionId: string, workspacePath: string, query: string]
    result: IpcResult<{ entries: FileEntry[] }>
  }

  // Session operations
  sessionUpdate: {
    params: [sessionId: string, workspaces: WorkspaceInput[], senderUuid?: string, expectedVersion?: number]
    result: IpcResult<{ session: Session }>
  }

  // Daemon operations
  daemonShutdown: {
    params: [connectionId: string]
    result: IpcResult
  }

  // Dialog operations
  dialogSelectFolder: {
    params: []
    result: string | null
  }
  dialogGetRecentDirectories: {
    params: []
    result: string[]
  }

  // Sandbox operations
  sandboxIsAvailable: {
    params: []
    result: boolean
  }

  // Run Actions operations
  runActionsDetect: {
    params: [connectionId: string, workspacePath: string]
    result: RunAction[]
  }
  runActionsRun: {
    params: [connectionId: string, workspacePath: string, actionId: string]
    result: IpcResult<{ ptyId: string }>
  }

  // App operations
  appGetInitialWorkspace: {
    params: []
    result: string | null
  }

  // Window UUID (used for session sync deduplication)
  appGetWindowUuid: {
    params: []
    result: string
  }

  // SSH operations
  sshConnect: {
    params: [config: SSHConnectionConfig, options?: { refreshDaemon?: boolean }]
    result: { info: ConnectionInfo; session: Session | null }
  }
  sshDisconnect: {
    params: [connectionId: string]
    result: void
  }
  sshListConnections: {
    params: []
    result: ConnectionInfo[]
  }
  sshSaveConnection: {
    params: [config: SSHConnectionConfig]
    result: void
  }
  sshGetSavedConnections: {
    params: []
    result: SSHConnectionConfig[]
  }
  sshRemoveSavedConnection: {
    params: [id: string]
    result: void
  }
  sshGetOutput: {
    params: [connectionId: string]
    result: string[]
  }
  sshWatchOutput: {
    params: [connectionId: string]
    result: { scrollback: string[] }
  }
  sshUnwatchOutput: {
    params: [connectionId: string]
    result: void
  }
  sshWatchConnectionStatus: {
    params: [connectionId: string]
    result: { initial: ConnectionInfo }
  }
  sshUnwatchConnectionStatus: {
    params: [connectionId: string]
    result: void
  }
  sshAddPortForward: {
    params: [config: PortForwardConfig]
    result: PortForwardInfo
  }
  sshRemovePortForward: {
    params: [portForwardId: string]
    result: void
  }
  sshListPortForwards: {
    params: [connectionId: string]
    result: PortForwardInfo[]
  }
  sshWatchPortForwardOutput: {
    params: [portForwardId: string]
    result: { scrollback: string[] }
  }
  sshUnwatchPortForwardOutput: {
    params: [portForwardId: string]
    result: void
  }

  // LLM operations
  llmChatSend: {
    params: [requestId: string, messages: { role: 'user' | 'assistant' | 'system'; content: string }[], settings: { baseUrl: string; apiKey: string; model: string; reasoning: ReasoningEffort }]
    result: void
  }
  llmAnalyzeTerminal: {
    params: [buffer: string, cwd: string, settings: { baseUrl: string; apiKey: string; model: string; systemPrompt: string; reasoningEffort: ReasoningEffort; safePaths: string[] }]
    result: { state: string; reason: string } | { error: string }
  }
  llmClearAnalyzerCache: {
    params: []
    result: void
  }
  llmGenerateTitle: {
    params: [buffer: string, settings: { baseUrl: string; apiKey: string; model: string; titleSystemPrompt: string; reasoningEffort: ReasoningEffort }]
    result: { title: string; description: string; branchName: string } | { error: string }
  }

  // Clipboard operations
  clipboardReadText: {
    params: []
    result: string
  }

  // Exec operations (streaming command execution)
  execStart: {
    params: [connectionId: string, cwd: string, command: string, args: string[]]
    result: IpcResult<{ execId: string }>
  }
}

// === Fire-and-Forget Types (renderer sends, no response) ===

export interface IpcSends {
  ptyWrite: {
    params: [handle: string, data: string]
  }
  ptyResize: {
    params: [handle: string, cols: number, rows: number]
  }
  ptyKill: {
    params: [connectionId: string, sessionId: string]
  }
  appCloseConfirmed: {
    params: []
  }
  appCloseCancelled: {
    params: []
  }
  llmChatCancel: {
    params: [requestId: string]
  }
  clipboardWriteText: {
    params: [text: string]
  }
  execKill: {
    params: [execId: string]
  }
}

// === Event Types (server emits, client listens) ===

export interface IpcEvents {
  ptyEvent: {
    params: [handle: string, event: PtyEvent]
  }
  settingsOpen: {
    params: []
  }
  appConfirmClose: {
    params: []
  }
  appReady: {
    params: [session: Session | null]
  }
  capsLockEvent: {
    params: [event: { type: string; key: string; code: string }]
  }
  daemonSessions: {
    params: [sessions: TTYSessionInfo[]]
  }
  sessionSync: {
    params: [connectionId: string, session: Session]
  }
  sshAutoConnected: {
    params: [session: Session, connection: ConnectionInfo]
  }
  daemonDisconnected: {
    params: []
  }
  activeProcessesOpen: {
    params: []
  }
  sshConnectionStatus: {
    params: [info: ConnectionInfo]
  }
  sshOutput: {
    params: [connectionId: string, line: string]
  }
  sshPortForwardStatus: {
    params: [info: PortForwardInfo]
  }
  sshPortForwardOutput: {
    params: [portForwardId: string, line: string]
  }
  llmChatDelta: {
    params: [requestId: string, text: string]
  }
  llmChatDone: {
    params: [requestId: string]
  }
  llmChatError: {
    params: [requestId: string, error: string]
  }
  gitOutput: {
    params: [operationId: string, data: string]
  }
  execEvent: {
    params: [execId: string, event: ExecEvent]
  }
}

// === Type Helpers ===

export type RequestName = keyof IpcRequests
export type SendName = keyof IpcSends
export type EventName = keyof IpcEvents
