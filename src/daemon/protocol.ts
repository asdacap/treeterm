/**
 * Protocol for communication between Electron app and daemon via Unix domain socket
 */

import type { SandboxConfig } from '../main/pty'

export type MessageType =
  | 'create'
  | 'attach'
  | 'detach'
  | 'write'
  | 'resize'
  | 'kill'
  | 'list'
  | 'getScrollback'

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
