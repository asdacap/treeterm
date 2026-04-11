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
  SSHConnectionConfig,
  ConnectionInfo,
  PortForwardConfig,
  PortForwardInfo
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
  DirectoryContents,
  FileContents,
  FileEntry,
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

  // Session operations (keyed by connectionId — the renderer passes connection.id to identify which daemon session)
  sessionUpdate: {
    params: [connectionId: string, workspaces: WorkspaceInput[], senderUuid?: string, expectedVersion?: number]
    result: IpcResult<{ session: Session }>
  }
  sessionLock: {
    params: [connectionId: string, ttlMs?: number]
    result: IpcResult<{ acquired: boolean; session: Session }>
  }
  sessionUnlock: {
    params: [connectionId: string]
    result: IpcResult<{ session: Session }>
  }
  sessionForceUnlock: {
    params: [connectionId: string]
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

  // PTY create session (daemon primitive — no stream)
  ptyCreateSession: {
    params: [connectionId: string, cwd: string, startupCommand?: string]
    result: IpcResult<{ sessionId: string }>
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

  // Local daemon connection (renderer-driven, mirrors sshConnect)
  localConnect: {
    params: [windowUuid: string]
    result: { info: ConnectionInfo; session: Session }
  }

  // SSH operations
  sshConnect: {
    params: [config: SSHConnectionConfig, options?: { refreshDaemon?: boolean; allowOutdatedDaemon?: boolean }]
    result: { info: ConnectionInfo; session: Session | null }
  }
  sshDisconnect: {
    params: [connectionId: string]
    result: undefined
  }
  sshReconnect: {
    params: [connectionId: string]
    result: undefined
  }
  sshReconnectNow: {
    params: [connectionId: string]
    result: undefined
  }
  sshForceReconnect: {
    params: [connectionId: string]
    result: undefined
  }
  sshCancelReconnect: {
    params: [connectionId: string]
    result: undefined
  }
  sshListConnections: {
    params: []
    result: ConnectionInfo[]
  }
  sshSaveConnection: {
    params: [config: SSHConnectionConfig]
    result: undefined
  }
  sshGetSavedConnections: {
    params: []
    result: SSHConnectionConfig[]
  }
  sshRemoveSavedConnection: {
    params: [id: string]
    result: undefined
  }
  sshWatchBootstrapOutput: {
    params: [connectionId: string]
    result: { scrollback: string[] }
  }
  sshUnwatchBootstrapOutput: {
    params: [connectionId: string]
    result: undefined
  }
  sshWatchTunnelOutput: {
    params: [connectionId: string]
    result: { scrollback: string[] }
  }
  sshUnwatchTunnelOutput: {
    params: [connectionId: string]
    result: undefined
  }
  sshWatchDaemonOutput: {
    params: [connectionId: string]
    result: { scrollback: string[] }
  }
  sshUnwatchDaemonOutput: {
    params: [connectionId: string]
    result: undefined
  }
  sshWatchConnectionStatus: {
    params: [connectionId: string]
    result: { initial: ConnectionInfo }
  }
  sshUnwatchConnectionStatus: {
    params: [connectionId: string]
    result: undefined
  }
  sshAddPortForward: {
    params: [config: PortForwardConfig]
    result: PortForwardInfo
  }
  sshRemovePortForward: {
    params: [portForwardId: string]
    result: undefined
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
    result: undefined
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
    params: []
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
  connectionReconnected: {
    params: [session: Session, connection: ConnectionInfo]
  }
  activeProcessesOpen: {
    params: []
  }
  sshConnectionStatus: {
    params: [info: ConnectionInfo]
  }
  sshBootstrapOutput: {
    params: [connectionId: string, line: string]
  }
  sshTunnelOutput: {
    params: [connectionId: string, line: string]
  }
  sshDaemonOutput: {
    params: [connectionId: string, line: string]
  }
  sshPortForwardStatus: {
    params: [info: PortForwardInfo]
  }
  sshPortForwardOutput: {
    params: [portForwardId: string, line: string]
  }
  execEvent: {
    params: [execId: string, event: ExecEvent]
  }
}

// === Type Helpers ===

export type RequestName = keyof IpcRequests
export type SendName = keyof IpcSends
export type EventName = keyof IpcEvents
