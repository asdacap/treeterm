/**
 * Protocol for communication between Electron app and daemon via Unix domain socket
 */

// Import shared types
import type {
  SandboxConfig,
  Tab,
  Workspace,
  Session,
  DaemonSessionInfo,
  WorkspaceInput
} from '../shared/types'

// Re-export for backward compatibility
export type {
  SandboxConfig,
  Tab,
  Workspace,
  Session,
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

// Response payload type map for better type safety
export interface ResponsePayloadMap {
  success: { sessionId?: string } | Session | Session[] | SessionInfo[] | null
  error: never
  data: string
  scrollback: string[]
  exit: { exitCode: number; signal?: number }
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
  payload?: ResponsePayloadMap['success']
}

export interface ErrorResponse extends DaemonResponse {
  type: 'error'
  error: string
}

export interface DataResponse extends DaemonResponse {
  type: 'data'
  sessionId: string
  payload: ResponsePayloadMap['data']
}

export interface ScrollbackResponse extends DaemonResponse {
  type: 'scrollback'
  sessionId: string
  payload: ResponsePayloadMap['scrollback']
}

export interface ExitResponse extends DaemonResponse {
  type: 'exit'
  sessionId: string
  payload: ResponsePayloadMap['exit']
}

// Type-safe discriminated union for responses
export type TypedDaemonResponse = SuccessResponse | ErrorResponse | DataResponse | ScrollbackResponse | ExitResponse

// Type guard for typed responses
export function isTypedResponse(response: DaemonResponse): response is TypedDaemonResponse {
  return (
    response.type === 'success' ||
    response.type === 'error' ||
    response.type === 'data' ||
    response.type === 'scrollback' ||
    response.type === 'exit'
  )
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

// NDJSON serialization functions removed - now using gRPC
// Protocol types kept for backward compatibility with internal APIs
