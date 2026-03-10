/**
 * Protocol for communication between Electron app and daemon via Unix domain socket
 */

// Import shared types
import type {
  SandboxConfig,
  DaemonTab,
  DaemonWorkspace,
  DaemonSession,
  DaemonSessionInfo,
  WorkspaceInput
} from '../shared/types'

// Re-export for backward compatibility
export type {
  SandboxConfig,
  DaemonTab,
  DaemonWorkspace,
  DaemonSession,
  WorkspaceInput
}

export type MessageType =
  | 'create'
  | 'attach'
  | 'detach'
  | 'write'
  | 'resize'
  | 'kill'
  | 'list'
  | 'getScrollback'
  | 'shutdown'
  // Session message types (workspace sessions, not PTY sessions)
  | 'createSession'
  | 'updateSession'
  | 'listSessions'
  | 'getSession'
  | 'deleteSession'

export type ResponseType = 'success' | 'error' | 'data' | 'scrollback' | 'exit'

export interface CreateSessionConfig {
  cwd: string
  env?: Record<string, string>
  cols?: number
  rows?: number
  sandbox?: SandboxConfig
  startupCommand?: string
}

export interface SessionInfo {
  id: string
  cwd: string
  cols: number
  rows: number
  createdAt: number
  lastActivity: number
  attachedClients: number
}

export interface DaemonMessage {
  type: MessageType
  sessionId?: string
  payload?: unknown
  requestId?: string
}

export interface CreateMessage extends DaemonMessage {
  type: 'create'
  payload: CreateSessionConfig
}

export interface AttachMessage extends DaemonMessage {
  type: 'attach'
  sessionId: string
}

export interface DetachMessage extends DaemonMessage {
  type: 'detach'
  sessionId: string
}

export interface WriteMessage extends DaemonMessage {
  type: 'write'
  sessionId: string
  payload: string
}

export interface ResizeMessage extends DaemonMessage {
  type: 'resize'
  sessionId: string
  payload: { cols: number; rows: number }
}

export interface KillMessage extends DaemonMessage {
  type: 'kill'
  sessionId: string
}

export interface ListMessage extends DaemonMessage {
  type: 'list'
}

export interface GetScrollbackMessage extends DaemonMessage {
  type: 'getScrollback'
  sessionId: string
}

export interface ShutdownMessage extends DaemonMessage {
  type: 'shutdown'
}

export interface DaemonResponse {
  type: ResponseType
  sessionId?: string
  payload?: unknown
  error?: string
  requestId?: string
}

export interface SuccessResponse extends DaemonResponse {
  type: 'success'
  payload?: unknown
}

export interface ErrorResponse extends DaemonResponse {
  type: 'error'
  error: string
}

export interface DataResponse extends DaemonResponse {
  type: 'data'
  sessionId: string
  payload: string
}

export interface ScrollbackResponse extends DaemonResponse {
  type: 'scrollback'
  sessionId: string
  payload: string[]
}

export interface ExitResponse extends DaemonResponse {
  type: 'exit'
  sessionId: string
  payload: { exitCode: number; signal?: number }
}

// Session message interfaces (workspace sessions)
export interface CreateSessionMessage extends DaemonMessage {
  type: 'createSession'
  payload: { workspaces: WorkspaceInput[] }
}

export interface UpdateSessionMessage extends DaemonMessage {
  type: 'updateSession'
  payload: {
    sessionId: string
    workspaces: WorkspaceInput[]
  }
}

export interface ListSessionsMessage extends DaemonMessage {
  type: 'listSessions'
}

export interface GetSessionMessage extends DaemonMessage {
  type: 'getSession'
  payload: { sessionId: string }
}

export interface DeleteSessionMessage extends DaemonMessage {
  type: 'deleteSession'
  payload: { sessionId: string }
}

export function serializeMessage(msg: DaemonMessage): string {
  return JSON.stringify(msg) + '\n'
}

export function parseMessage(data: string): DaemonMessage {
  const msg = JSON.parse(data) as DaemonMessage
  if (!msg.type) {
    throw new Error('Invalid message: missing type field')
  }
  return msg
}

export function serializeResponse(res: DaemonResponse): string {
  return JSON.stringify(res) + '\n'
}

export function parseResponse(data: string): DaemonResponse {
  const res = JSON.parse(data) as DaemonResponse
  if (!res.type) {
    throw new Error('Invalid response: missing type field')
  }
  return res
}
