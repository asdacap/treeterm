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
  ConnectionInfo
} from './types'

import type {
  GitInfo,
  WorktreeResult,
  WorktreeInfo,
  ChildWorktreeInfo,
  DiffResult,
  ConflictCheckResult,
  UncommittedChanges,
  FileDiffContents,
  DirectoryContents,
  FileContents,
  FileEntry
} from '../renderer/types'

// === Request Types (renderer calls, server handles) ===

export interface IpcRequests {
  // PTY operations
  ptyCreate: {
    params: [cwd: string, sandbox?: SandboxConfig, startupCommand?: string]
    result: string | null
  }
  ptyAttach: {
    params: [sessionId: string]
    result: { success: boolean; scrollback?: string[]; exitCode?: number; error?: string }
  }
  ptyDetach: {
    params: [sessionId: string]
    result: void
  }
  ptyList: {
    params: []
    result: SessionInfo[]
  }
  ptyIsAlive: {
    params: [id: string]
    result: boolean
  }

  // Git operations
  gitGetInfo: {
    params: [dirPath: string]
    result: GitInfo
  }
  gitCreateWorktree: {
    params: [repoPath: string, name: string, baseBranch?: string]
    result: WorktreeResult
  }
  gitRemoveWorktree: {
    params: [repoPath: string, worktreePath: string, deleteBranch?: boolean]
    result: { success: boolean; error?: string }
  }
  gitListWorktrees: {
    params: [repoPath: string]
    result: WorktreeInfo[]
  }
  gitGetChildWorktrees: {
    params: [repoPath: string, parentBranch: string | null]
    result: ChildWorktreeInfo[]
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
    params: [repoPath: string, branch: string, worktreeName: string]
    result: WorktreeResult
  }
  gitCreateWorktreeFromRemote: {
    params: [repoPath: string, remoteBranch: string, worktreeName: string]
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
    params: [mainRepoPath: string, worktreeBranch: string, targetBranch: string, squash: boolean]
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
    params: [repoPath: string, branchName: string]
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
    params: [config: SSHConnectionConfig]
    result: ConnectionInfo
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
}

// === Fire-and-Forget Types (renderer sends, no response) ===

export interface IpcSends {
  ptyWrite: {
    params: [id: string, data: string]
  }
  ptyResize: {
    params: [id: string, cols: number, rows: number]
  }
  ptyKill: {
    params: [id: string]
  }
  appCloseConfirmed: {
    params: []
  }
  appCloseCancelled: {
    params: []
  }
}

// === Event Types (server emits, client listens) ===

export interface IpcEvents {
  ptyData: {
    params: [id: string, data: string]
  }
  ptyExit: {
    params: [id: string, exitCode: number]
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
  terminalNew: {
    params: []
  }
  terminalShowSessions: {
    params: []
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
}

// === Type Helpers ===

export type RequestName = keyof IpcRequests
export type SendName = keyof IpcSends
export type EventName = keyof IpcEvents
