/**
 * Daemon Client for Electron
 * Connects to the daemon and provides an API for managing PTY sessions
 */

import * as net from 'net'
import { spawn } from 'child_process'
import * as path from 'path'
import * as fs from 'fs'
import { app } from 'electron'
import type {
  DaemonMessage,
  DaemonResponse,
  CreateSessionConfig,
  SessionInfo
} from '../daemon/protocol'
import { serializeMessage, parseResponse } from '../daemon/protocol'
import { getDefaultSocketPath } from '../daemon/socketServer'

type ResponseHandler = (response: DaemonResponse) => void
type DataListener = (data: string) => void
type ExitListener = (exitCode: number, signal?: number) => void

export class DaemonClient {
  private socket: net.Socket | null = null
  private connected: boolean = false
  private buffer: string = ''
  private requestCounter: number = 0
  private responseHandlers: Map<string, ResponseHandler> = new Map()
  private dataListeners: Map<string, Set<DataListener>> = new Map()
  private exitListeners: Map<string, Set<ExitListener>> = new Map()

  constructor(private socketPath: string = getDefaultSocketPath()) {}

  async connect(): Promise<void> {
    if (this.connected) {
      return
    }

    return new Promise((resolve, reject) => {
      this.socket = net.createConnection(this.socketPath)

      const timeout = setTimeout(() => {
        if (!this.connected) {
          this.socket?.destroy()
          reject(new Error('Connection timeout'))
        }
      }, 5000)

      this.socket.on('connect', () => {
        clearTimeout(timeout)
        this.connected = true
        console.log('[daemonClient] connected to daemon')
        resolve()
      })

      this.socket.on('data', (data) => {
        this.buffer += data.toString()
        this.processBuffer()
      })

      this.socket.on('error', (error) => {
        console.error('[daemonClient] socket error:', error)
        this.connected = false
        if (!this.connected) {
          clearTimeout(timeout)
          reject(error)
        }
      })

      this.socket.on('close', () => {
        console.log('[daemonClient] connection closed')
        this.connected = false
        this.socket = null
      })
    })
  }

  async ensureDaemonRunning(): Promise<void> {
    try {
      await this.connect()
    } catch (error) {
      console.log('[daemonClient] daemon not running, starting it...')
      await this.spawnDaemon()

      // Wait for daemon to be ready and try connecting again
      await this.waitForSocket()
      await this.connect()
    }
  }

  async createSession(config: CreateSessionConfig): Promise<string> {
    const response = await this.sendMessage({
      type: 'create',
      payload: config
    })

    if (response.type === 'error') {
      throw new Error(response.error)
    }

    const sessionId = (response.payload as any)?.sessionId
    if (!sessionId) {
      throw new Error('Failed to create session: no sessionId returned')
    }

    return sessionId
  }

  async attachSession(sessionId: string): Promise<{ scrollback: string[] }> {
    const response = await this.sendMessage({
      type: 'attach',
      sessionId
    })

    if (response.type === 'error') {
      throw new Error(response.error)
    }

    if (response.type === 'scrollback') {
      return { scrollback: response.payload as string[] }
    }

    throw new Error('Unexpected response type for attach')
  }

  async detachSession(sessionId: string): Promise<void> {
    await this.sendMessage({
      type: 'detach',
      sessionId
    })
  }

  writeToSession(sessionId: string, data: string): void {
    this.sendMessage({
      type: 'write',
      sessionId,
      payload: data
    }).catch((error) => {
      console.error('[daemonClient] failed to write to session:', error)
    })
  }

  resizeSession(sessionId: string, cols: number, rows: number): void {
    this.sendMessage({
      type: 'resize',
      sessionId,
      payload: { cols, rows }
    }).catch((error) => {
      console.error('[daemonClient] failed to resize session:', error)
    })
  }

  async killSession(sessionId: string): Promise<void> {
    await this.sendMessage({
      type: 'kill',
      sessionId
    })

    // Clean up listeners
    this.dataListeners.delete(sessionId)
    this.exitListeners.delete(sessionId)
  }

  async listSessions(): Promise<SessionInfo[]> {
    const response = await this.sendMessage({
      type: 'list'
    })

    if (response.type === 'error') {
      throw new Error(response.error)
    }

    return (response.payload as SessionInfo[]) || []
  }

  onSessionData(sessionId: string, callback: DataListener): () => void {
    if (!this.dataListeners.has(sessionId)) {
      this.dataListeners.set(sessionId, new Set())
    }
    this.dataListeners.get(sessionId)!.add(callback)

    return () => {
      const listeners = this.dataListeners.get(sessionId)
      if (listeners) {
        listeners.delete(callback)
        if (listeners.size === 0) {
          this.dataListeners.delete(sessionId)
        }
      }
    }
  }

  onSessionExit(sessionId: string, callback: ExitListener): () => void {
    if (!this.exitListeners.has(sessionId)) {
      this.exitListeners.set(sessionId, new Set())
    }
    this.exitListeners.get(sessionId)!.add(callback)

    return () => {
      const listeners = this.exitListeners.get(sessionId)
      if (listeners) {
        listeners.delete(callback)
        if (listeners.size === 0) {
          this.exitListeners.delete(sessionId)
        }
      }
    }
  }

  disconnect(): void {
    if (this.socket) {
      this.socket.destroy()
      this.socket = null
    }
    this.connected = false
  }

  private async sendMessage(message: DaemonMessage): Promise<DaemonResponse> {
    if (!this.connected || !this.socket) {
      throw new Error('Not connected to daemon')
    }

    const requestId = `req-${++this.requestCounter}`
    message.requestId = requestId

    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        this.responseHandlers.delete(requestId)
        reject(new Error('Request timeout'))
      }, 10000)

      this.responseHandlers.set(requestId, (response) => {
        clearTimeout(timeout)
        this.responseHandlers.delete(requestId)
        resolve(response)
      })

      const data = serializeMessage(message)
      this.socket!.write(data)
    })
  }

  private processBuffer(): void {
    while (true) {
      const newlineIndex = this.buffer.indexOf('\n')
      if (newlineIndex === -1) break

      const line = this.buffer.slice(0, newlineIndex)
      this.buffer = this.buffer.slice(newlineIndex + 1)

      if (line.trim()) {
        this.handleResponse(line)
      }
    }
  }

  private handleResponse(data: string): void {
    try {
      const response = parseResponse(data)

      // Handle request/response pairs
      if (response.requestId) {
        const handler = this.responseHandlers.get(response.requestId)
        if (handler) {
          handler(response)
          return
        }
      }

      // Handle broadcast messages (data, exit)
      if (response.type === 'data' && response.sessionId) {
        const listeners = this.dataListeners.get(response.sessionId)
        if (listeners) {
          for (const listener of listeners) {
            listener(response.payload as string)
          }
        }
      } else if (response.type === 'exit' && response.sessionId) {
        const payload = response.payload as { exitCode: number; signal?: number }
        const listeners = this.exitListeners.get(response.sessionId)
        if (listeners) {
          for (const listener of listeners) {
            listener(payload.exitCode, payload.signal)
          }
        }
      }
    } catch (error) {
      console.error('[daemonClient] failed to parse response:', error)
    }
  }

  private async spawnDaemon(): Promise<void> {
    const daemonPath = app.isPackaged
      ? path.join(process.resourcesPath, 'daemon', 'daemon', 'index.js')
      : path.join(__dirname, '../daemon/daemon/index.js')

    if (!fs.existsSync(daemonPath)) {
      throw new Error(`Daemon executable not found at ${daemonPath}`)
    }

    const logPath = path.join(app.getPath('userData'), 'daemon.log')

    console.log('[daemonClient] spawning daemon at', daemonPath)

    const child = spawn(process.execPath, [daemonPath], {
      detached: true,
      stdio: ['ignore', fs.openSync(logPath, 'a'), fs.openSync(logPath, 'a')],
      env: {
        ...process.env,
        TREETERM_DAEMON: '1',
        TREETERM_SOCKET_PATH: this.socketPath
      }
    })

    child.unref()
    console.log('[daemonClient] daemon spawned with PID', child.pid)
  }

  private async waitForSocket(): Promise<void> {
    const maxAttempts = 20
    const delay = 250

    for (let i = 0; i < maxAttempts; i++) {
      if (fs.existsSync(this.socketPath)) {
        console.log('[daemonClient] socket ready')
        return
      }
      await new Promise((resolve) => setTimeout(resolve, delay))
    }

    throw new Error('Daemon failed to create socket in time')
  }
}
