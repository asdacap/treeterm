/**
 * Unix Domain Socket server for daemon-client communication
 */

import * as net from 'net'
import * as fs from 'fs'
import * as os from 'os'
import * as path from 'path'
import type { DaemonPtyManager } from './ptyManager'
import type { SessionStore } from './sessionStore'
import type {
  DaemonMessage,
  DaemonResponse,
  CreateMessage,
  AttachMessage,
  DetachMessage,
  WriteMessage,
  ResizeMessage,
  KillMessage,
  GetScrollbackMessage,
  ShutdownMessage,
  CreateSessionMessage,
  UpdateSessionMessage,
  ListSessionsMessage,
  GetSessionMessage,
  DeleteSessionMessage
} from './protocol'
import { parseMessage, serializeResponse } from './protocol'

interface Client {
  id: string
  socket: net.Socket
  buffer: string
}

export class SocketServer {
  private server: net.Server | null = null
  private clients: Map<string, Client> = new Map()
  private clientCounter = 0
  private sessionStore: SessionStore

  constructor(
    private socketPath: string,
    private ptyManager: DaemonPtyManager,
    sessionStore?: SessionStore
  ) {
    // Import SessionStore dynamically to avoid circular dependency
    const { SessionStore: SS } = require('./sessionStore')
    this.sessionStore = sessionStore || new SS()
  }

  start(): Promise<void> {
    return new Promise((resolve, reject) => {
      // Remove stale socket file if exists
      if (fs.existsSync(this.socketPath)) {
        console.log(`[socketServer] removing stale socket at ${this.socketPath}`)
        fs.unlinkSync(this.socketPath)
      }

      // Ensure socket directory exists
      const socketDir = path.dirname(this.socketPath)
      if (!fs.existsSync(socketDir)) {
        fs.mkdirSync(socketDir, { recursive: true, mode: 0o700 })
      }

      this.server = net.createServer((socket) => this.handleConnection(socket))

      this.server.on('error', (error) => {
        console.error('[socketServer] server error:', error)
        reject(error)
      })

      this.server.listen(this.socketPath, () => {
        console.log(`[socketServer] listening on ${this.socketPath}`)
        // Set socket permissions (user-only)
        fs.chmodSync(this.socketPath, 0o600)
        resolve()
      })

      // Set up PTY event forwarding
      this.ptyManager.onData((sessionId, data) => {
        this.broadcast(sessionId, {
          type: 'data',
          sessionId,
          payload: data
        })
      })

      this.ptyManager.onExit((sessionId, exitCode, signal) => {
        this.broadcast(sessionId, {
          type: 'exit',
          sessionId,
          payload: { exitCode, signal }
        })
      })
    })
  }

  stop(): void {
    console.log('[socketServer] stopping server')

    // Close all client connections
    for (const [clientId, client] of this.clients) {
      client.socket.destroy()
      this.clients.delete(clientId)
    }

    // Close server
    if (this.server) {
      this.server.close()
      this.server = null
    }

    // Remove socket file
    if (fs.existsSync(this.socketPath)) {
      fs.unlinkSync(this.socketPath)
    }
  }

  private handleConnection(socket: net.Socket): void {
    const clientId = `client-${++this.clientCounter}`
    const client: Client = {
      id: clientId,
      socket,
      buffer: ''
    }

    this.clients.set(clientId, client)
    console.log(`[socketServer] client ${clientId} connected`)

    socket.on('data', (data) => {
      client.buffer += data.toString()
      this.processBuffer(client)
    })

    socket.on('error', (error) => {
      console.error(`[socketServer] client ${clientId} error:`, error)
    })

    socket.on('close', () => {
      console.log(`[socketServer] client ${clientId} disconnected`)
      // Detach client from all PTY sessions
      const sessions = this.ptyManager.listSessions()
      for (const session of sessions) {
        this.ptyManager.detach(session.id, clientId)
      }
      // Detach client from all sessions
      this.sessionStore.detachClient(clientId)
      this.clients.delete(clientId)
    })
  }

  private processBuffer(client: Client): void {
    while (true) {
      const newlineIndex = client.buffer.indexOf('\n')
      if (newlineIndex === -1) break

      const line = client.buffer.slice(0, newlineIndex)
      client.buffer = client.buffer.slice(newlineIndex + 1)

      if (line.trim()) {
        this.handleMessage(client, line)
      }
    }
  }

  private handleMessage(client: Client, data: string): void {
    try {
      const message = parseMessage(data)
      const response = this.processMessage(client.id, message)
      this.sendResponse(client, response)
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Unknown error'
      console.error(`[socketServer] error processing message:`, errorMessage)
      console.error(`[socketServer] invalid message data (first 200 chars):`, data.slice(0, 200))
      console.error(`[socketServer] invalid message length:`, data.length)
      this.sendResponse(client, {
        type: 'error',
        error: errorMessage
      })
    }
  }

  private processMessage(clientId: string, message: DaemonMessage): DaemonResponse {
    switch (message.type) {
      case 'create': {
        const msg = message as CreateMessage
        const sessionId = this.ptyManager.create(msg.payload)
        // Auto-attach the creating client
        this.ptyManager.attach(sessionId, clientId)
        return {
          type: 'success',
          payload: { sessionId },
          requestId: message.requestId
        }
      }

      case 'attach': {
        const msg = message as AttachMessage
        const result = this.ptyManager.attach(msg.sessionId, clientId)
        return {
          type: 'scrollback',
          sessionId: msg.sessionId,
          payload: result.scrollback,
          requestId: message.requestId
        }
      }

      case 'detach': {
        const msg = message as DetachMessage
        this.ptyManager.detach(msg.sessionId, clientId)
        return {
          type: 'success',
          requestId: message.requestId
        }
      }

      case 'write': {
        const msg = message as WriteMessage
        this.ptyManager.write(msg.sessionId, msg.payload)
        return {
          type: 'success',
          requestId: message.requestId
        }
      }

      case 'resize': {
        const msg = message as ResizeMessage
        this.ptyManager.resize(msg.sessionId, msg.payload.cols, msg.payload.rows)
        return {
          type: 'success',
          requestId: message.requestId
        }
      }

      case 'kill': {
        const msg = message as KillMessage
        this.ptyManager.kill(msg.sessionId)
        return {
          type: 'success',
          requestId: message.requestId
        }
      }

      case 'list': {
        const sessions = this.ptyManager.listSessions()
        console.log(`[socketServer] list request - found ${sessions.length} sessions:`)
        sessions.forEach((session) => {
          console.log(`  - ${session.id}: worktree=${session.cwd}, clients=${session.attachedClients}`)
        })
        return {
          type: 'success',
          payload: sessions,
          requestId: message.requestId
        }
      }

      case 'getScrollback': {
        const msg = message as GetScrollbackMessage
        const scrollback = this.ptyManager.getScrollback(msg.sessionId)
        return {
          type: 'scrollback',
          sessionId: msg.sessionId,
          payload: scrollback,
          requestId: message.requestId
        }
      }

      case 'shutdown': {
        console.log('[socketServer] shutdown requested')
        // Schedule shutdown after sending response
        setTimeout(() => {
          console.log('[socketServer] initiating shutdown')
          this.stop()
          this.ptyManager.shutdown()
          process.exit(0)
        }, 100)
        return {
          type: 'success',
          requestId: message.requestId
        }
      }

      case 'createSession': {
        const msg = message as CreateSessionMessage
        const session = this.sessionStore.createSession(clientId, msg.payload.workspaces)
        return {
          type: 'success',
          payload: session,
          requestId: message.requestId
        }
      }

      case 'updateSession': {
        const msg = message as UpdateSessionMessage
        const session = this.sessionStore.updateSession(clientId, msg.payload.sessionId, msg.payload.workspaces)
        if (!session) {
          return {
            type: 'error',
            error: `Session not found: ${msg.payload.sessionId}`,
            requestId: message.requestId
          }
        }
        return {
          type: 'success',
          payload: session,
          requestId: message.requestId
        }
      }

      case 'listSessions': {
        const sessions = this.sessionStore.listSessions()
        console.log(`[socketServer] listSessions - found ${sessions.length} session(s)`)
        return {
          type: 'success',
          payload: sessions,
          requestId: message.requestId
        }
      }

      case 'getSession': {
        const msg = message as GetSessionMessage
        const session = this.sessionStore.getSession(msg.payload.sessionId)
        return {
          type: 'success',
          payload: session,
          requestId: message.requestId
        }
      }

      case 'deleteSession': {
        const msg = message as DeleteSessionMessage
        this.sessionStore.deleteSession(msg.payload.sessionId)
        return {
          type: 'success',
          requestId: message.requestId
        }
      }

      default:
        throw new Error(`Unknown message type: ${message.type}`)
    }
  }

  private sendResponse(client: Client, response: DaemonResponse): void {
    try {
      const data = serializeResponse(response)
      client.socket.write(data)
    } catch (error) {
      console.error('[socketServer] failed to send response:', error)
    }
  }

  private broadcast(sessionId: string, response: DaemonResponse): void {
    // Find all clients attached to this session
    const sessions = this.ptyManager.listSessions()
    const session = sessions.find((s) => s.id === sessionId)

    if (!session) return

    for (const [clientId, client] of this.clients) {
      // Send to all clients (they'll filter on their end if needed)
      // In a more optimized version, we'd track which clients are attached to which sessions
      this.sendResponse(client, response)
    }
  }
}

export function getDefaultSocketPath(): string {
  const uid = process.getuid ? process.getuid() : os.userInfo().uid
  return path.join(os.tmpdir(), `treeterm-${uid}`, 'daemon.sock')
}
