/**
 * IPC type definitions for Electron IPC communication.
 * Single source of truth for all IPC messages between main and renderer processes.
 */

import type {
  SandboxConfig,
  Settings,
  Session,
  SessionInfo,
  WorkspaceInput,
  RunAction,
  SSHConnectionConfig,
  ConnectionInfo,
  ReasoningEffort
} from './types'

/** Discriminated union for PTY output events (mirrors gRPC PtyOutput) */
export type PtyEvent =
  | { type: 'data'; data: Uint8Array }
  | { type: 'exit'; exitCode: number; signal?: number }
  | { type: 'resize'; cols: number; rows: number }

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
    params: [connectionId: string, cwd: string, sandbox?: SandboxConfig, startupCommand?: string]
    result: { sessionId: string; handle: string } | null
  }
  ptyAttach: {
    params: [connectionId: string, sessionId: string]
    result: { success: boolean; handle?: string; error?: string }
  }
  ptyList: {
    params: [connectionId: string]
    result: SessionInfo[]
  }
  ptyIsAlive: {
    params: [connectionId: string, id: string]
    result: boolean
  }

  // Git operations
  gitGetInfo: {
    params: [dirPath: string]
    result: GitInfo
  }
  gitCreateWorktree: {
    params: [repoPath: string, name: string, baseBranch?: string, operationId?: string]
    result: WorktreeResult
  }
  gitRemoveWorktree: {
    params: [repoPath: string, worktreePath: string, deleteBranch?: boolean, operationId?: string]
    result: { success: boolean; error?: string }
  }
  gitListWorktrees: {
    params: [repoPath: string]
    result: WorktreeInfo[]
  }
  gitListLocalBranches: {
    params: [repoPath: string]
    result: string[]
  }
  gitListRemoteBranches: {
    params: [repoPath: string]
    result: string[]
  }
  gitGetBranchesInWorktrees: {
    params: [repoPath: string]
    result: string[]
  }
  gitCreateWorktreeFromBranch: {
    params: [repoPath: string, branch: string, worktreeName: string, operationId?: string]
    result: WorktreeResult
  }
  gitCreateWorktreeFromRemote: {
    params: [repoPath: string, remoteBranch: string, worktreeName: string, operationId?: string]
    result: WorktreeResult
  }
  gitGetDiff: {
    params: [worktreePath: string, parentBranch: string]
    result: { success: boolean; diff?: DiffResult; error?: string }
  }
  gitGetFileDiff: {
    params: [worktreePath: string, parentBranch: string, filePath: string]
    result: { success: boolean; diff?: string; error?: string }
  }
  gitMerge: {
    params: [targetWorktreePath: string, worktreeBranch: string, squash: boolean, operationId?: string]
    result: { success: boolean; error?: string }
  }
  gitCheckMergeConflicts: {
    params: [repoPath: string, sourceBranch: string, targetBranch: string]
    result: ConflictCheckResult
  }
  gitHasUncommittedChanges: {
    params: [repoPath: string]
    result: boolean
  }
  gitCommitAll: {
    params: [repoPath: string, message: string]
    result: { success: boolean; error?: string }
  }
  gitDeleteBranch: {
    params: [repoPath: string, branchName: string, operationId?: string]
    result: { success: boolean; error?: string }
  }
  gitRenameBranch: {
    params: [repoPath: string, oldName: string, newName: string]
    result: { success: boolean; error?: string }
  }
  gitGetUncommittedChanges: {
    params: [repoPath: string]
    result: { success: boolean; changes?: UncommittedChanges; error?: string }
  }
  gitGetUncommittedFileDiff: {
    params: [repoPath: string, filePath: string, staged: boolean]
    result: { success: boolean; diff?: string; error?: string }
  }
  gitStageFile: {
    params: [repoPath: string, filePath: string]
    result: { success: boolean; error?: string }
  }
  gitUnstageFile: {
    params: [repoPath: string, filePath: string]
    result: { success: boolean; error?: string }
  }
  gitStageAll: {
    params: [repoPath: string]
    result: { success: boolean; error?: string }
  }
  gitUnstageAll: {
    params: [repoPath: string]
    result: { success: boolean; error?: string }
  }
  gitCommitStaged: {
    params: [repoPath: string, message: string]
    result: { success: boolean; error?: string }
  }
  gitGetFileContentsForDiff: {
    params: [worktreePath: string, parentBranch: string, filePath: string]
    result: { success: boolean; contents?: FileDiffContents; error?: string }
  }
  gitGetUncommittedFileContentsForDiff: {
    params: [repoPath: string, filePath: string, staged: boolean]
    result: { success: boolean; contents?: FileDiffContents; error?: string }
  }
  gitGetHeadCommitHash: {
    params: [repoPath: string]
    result: { success: boolean; hash?: string; error?: string }
  }
  gitGetLog: {
    params: [repoPath: string, parentBranch: string | null, skip: number, limit: number]
    result: { success: boolean; result?: GitLogResult; error?: string }
  }
  gitGetCommitDiff: {
    params: [repoPath: string, commitHash: string]
    result: { success: boolean; files?: DiffFile[]; error?: string }
  }
  gitGetCommitFileDiff: {
    params: [repoPath: string, commitHash: string, filePath: string]
    result: { success: boolean; contents?: FileDiffContents; error?: string }
  }

  // Git fetch/pull operations
  gitFetch: {
    params: [repoPath: string]
    result: { success: boolean; error?: string }
  }
  gitPull: {
    params: [repoPath: string]
    result: { success: boolean; error?: string }
  }
  gitGetBehindCount: {
    params: [repoPath: string]
    result: number
  }

  // GitHub operations
  gitGetRemoteUrl: {
    params: [repoPath: string]
    result: { url?: string; error?: string }
  }
  githubGetPrInfo: {
    params: [repoPath: string, head: string, base: string]
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
    params: [workspacePath: string, dirPath: string]
    result: { success: boolean; contents?: DirectoryContents; error?: string }
  }
  fsReadFile: {
    params: [workspacePath: string, filePath: string]
    result: { success: boolean; file?: FileContents; error?: string }
  }
  fsWriteFile: {
    params: [workspacePath: string, filePath: string, content: string]
    result: { success: boolean; error?: string }
  }
  fsSearchFiles: {
    params: [workspacePath: string, query: string]
    result: { success: boolean; entries?: FileEntry[]; error?: string }
  }

  // STT operations
  sttTranscribeOpenai: {
    params: [audioBuffer: ArrayBuffer, apiKey: string, language?: string]
    result: { text: string }
  }
  sttTranscribeLocal: {
    params: [audioBuffer: ArrayBuffer, modelPath: string, language?: string]
    result: { text: string }
  }
  sttCheckMicPermission: {
    params: []
    result: boolean
  }

  // Session operations
  sessionCreate: {
    params: [workspaces: WorkspaceInput[]]
    result: { success: boolean; session?: Session; error?: string }
  }
  sessionUpdate: {
    params: [sessionId: string, workspaces: WorkspaceInput[], senderUuid?: string]
    result: { success: boolean; session?: Session; error?: string }
  }
  sessionList: {
    params: []
    result: { success: boolean; sessions?: Session[]; error?: string }
  }
  sessionDelete: {
    params: [sessionId: string]
    result: { success: boolean; error?: string }
  }
  sessionOpenInNewWindow: {
    params: [sessionId: string]
    result: { success: boolean; error?: string }
  }

  // Daemon operations
  daemonShutdown: {
    params: []
    result: { success: boolean; error?: string }
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
    params: [workspacePath: string]
    result: RunAction[]
  }
  runActionsRun: {
    params: [workspacePath: string, actionId: string]
    result: string | null  // returns ptyId
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
    result: { info: ConnectionInfo, session?: Session }
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
    result: { initial: ConnectionInfo | undefined }
  }
  sshUnwatchConnectionStatus: {
    params: [connectionId: string]
    result: void
  }

  // LLM operations
  llmChatSend: {
    params: [requestId: string, messages: { role: 'user' | 'assistant' | 'system'; content: string }[], settings: { baseUrl: string; apiKey: string; model: string }]
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
    params: [sessions: SessionInfo[]]
  }
  sessionShowSessions: {
    params: []
  }
  sessionSync: {
    params: [session: Session]
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
}

// === Type Helpers ===

export type RequestName = keyof IpcRequests
export type SendName = keyof IpcSends
export type EventName = keyof IpcEvents
