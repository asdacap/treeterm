/**
 * gRPC Daemon Client for Electron
 * Connects to the daemon via gRPC and provides an API for managing PTY sessions
 */

import * as grpc from '@grpc/grpc-js'
import { spawn } from 'child_process'
import * as path from 'path'
import * as fs from 'fs'
import { app } from 'electron'
import {
  TreeTermDaemonClient,
  type CreatePtyRequest,
  type AttachPtyRequest,
  type DetachPtyRequest,
  type ResizePtyRequest,
  type KillPtyRequest,
  type GetScrollbackRequest,
  type PtyInput,
  type PtyOutput,
  type ExecInput,
  type ExecOutput,
  type CreateSessionRequest,
  type UpdateSessionRequest,
  type GetSessionRequest,
  type DeleteSessionRequest,
  type Session as ProtoSession,
  type Workspace as ProtoWorkspace,
  type WorkspaceInput,
  type SessionWatchRequest
} from '../generated/treeterm'
import { getDefaultSocketPath } from '../daemon/socketPath'
import type {
  CreateSessionConfig,
  SessionInfo,
  Workspace,
  Session,
  Tab
} from '../daemon/protocol'

type DataListener = (data: string) => void
type ExitListener = (exitCode: number, signal?: number) => void
type DisconnectListener = () => void

export class GrpcDaemonClient {
  private client: TreeTermDaemonClient | null = null
  private stream: grpc.ClientDuplexStream<PtyInput, PtyOutput> | null = null
  private connected: boolean = false
  private dataListeners: Map<string, Set<DataListener>> = new Map()
  private exitListeners: Map<string, Set<ExitListener>> = new Map()
  private disconnectListeners: Set<DisconnectListener> = new Set()
  private clientId: string = `client-${Date.now()}`

  constructor(private socketPath: string = getDefaultSocketPath()) {}

  async connect(): Promise<void> {
    if (this.connected) {
      return
    }

    // Fast fail: check if socket file exists before attempting gRPC connection
    if (!fs.existsSync(this.socketPath)) {
      throw new Error(`Daemon socket not found at ${this.socketPath}`)
    }

    return new Promise((resolve, reject) => {
      const socketUri = `unix://${this.socketPath}`
      const credentials = grpc.credentials.createInsecure()

      // Configure larger message size limits as a safety buffer
      // Default is 4MB, we set to 8MB for headroom
      // Note: Scrollback is limited to 1MB on the daemon side
      const channelOptions = {
        'grpc.max_receive_message_length': 8 * 1024 * 1024, // 8 MB
        'grpc.max_send_message_length': 8 * 1024 * 1024 // 8 MB
      }

      this.client = new TreeTermDaemonClient(socketUri, credentials, channelOptions)

      this.client.waitForReady(Date.now() + 5000, (error) => {
        if (error) {
          console.error('[grpcDaemonClient] connection failed:', error)
          reject(error)
          return
        }

        console.log('[grpcDaemonClient] connected to daemon')
        this.connected = true

        // Establish bidirectional stream for PTY I/O
        this.setupPtyStream()
        resolve()
      })
    })
  }

  private setupPtyStream(): void {
    if (!this.client) {
      console.error('[grpcDaemonClient] cannot setup stream: client not initialized')
      return
    }

    // Create metadata with client ID
    const metadata = new grpc.Metadata()
    metadata.set('client-id', this.clientId)

    this.stream = this.client.ptyStream(metadata)

    this.stream.on('data', (output: PtyOutput) => {
      if (output.data) {
        const { sessionId, data } = output.data
        const dataStr = data.toString('utf-8')
        const listeners = this.dataListeners.get(sessionId)
        if (listeners) {
          for (const listener of listeners) {
            listener(dataStr)
          }
        }
      } else if (output.exit) {
        const { sessionId, exitCode, signal } = output.exit
        const listeners = this.exitListeners.get(sessionId)
        if (listeners) {
          for (const listener of listeners) {
            listener(exitCode, signal)
          }
        }
        // Clean up listeners after exit
        this.dataListeners.delete(sessionId)
        this.exitListeners.delete(sessionId)
      }
    })

    this.stream.on('error', (error) => {
      console.error('[grpcDaemonClient] stream error:', error)
      this.connected = false
      for (const listener of this.disconnectListeners) {
        listener()
      }
    })

    this.stream.on('end', () => {
      console.log('[grpcDaemonClient] stream ended')
      this.connected = false
      this.stream = null
      for (const listener of this.disconnectListeners) {
        listener()
      }
    })
  }

  onDisconnect(listener: DisconnectListener): () => void {
    this.disconnectListeners.add(listener)
    return () => this.disconnectListeners.delete(listener)
  }

  isConnected(): boolean {
    return this.connected
  }

  async ensureDaemonRunning(): Promise<void> {
    try {
      await this.connect()
    } catch (error) {
      console.log('[grpcDaemonClient] daemon not running, starting it...')
      await this.spawnDaemon()

      // Wait for daemon to be ready and try connecting again
      await this.waitForSocket()
      await this.connect()
    }
  }

  async createPtySession(config: CreateSessionConfig): Promise<string> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }

    return new Promise((resolve, reject) => {
      const request: CreatePtyRequest = {
        cwd: config.cwd,
        env: config.env || {},
        cols: config.cols,
        rows: config.rows,
        sandbox: config.sandbox,
        startupCommand: config.startupCommand
      }

      this.client!.createPty(request, (error, response) => {
        if (error) {
          reject(new Error(error.message))
        } else if (response) {
          resolve(response.sessionId)
        } else {
          reject(new Error('No response from server'))
        }
      })
    })
  }

  async attachPtySession(sessionId: string): Promise<{ scrollback: string[] }> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }

    return new Promise((resolve, reject) => {
      const request: AttachPtyRequest = { sessionId }

      this.client!.attachPty(request, (error, response) => {
        if (error) {
          reject(new Error(error.message))
        } else if (response) {
          resolve({ scrollback: response.scrollback })
        } else {
          reject(new Error('No response from server'))
        }
      })
    })
  }

  async detachPtySession(sessionId: string): Promise<void> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }

    return new Promise((resolve, reject) => {
      const request: DetachPtyRequest = { sessionId }

      this.client!.detachPty(request, (error) => {
        if (error) {
          reject(new Error(error.message))
        } else {
          resolve()
        }
      })
    })
  }

  writeToPtySession(sessionId: string, data: string): void {
    if (!this.stream) {
      console.error('[grpcDaemonClient] cannot write: stream not established')
      return
    }

    try {
      const input: PtyInput = {
        write: {
          sessionId,
          data: Buffer.from(data, 'utf-8')
        }
      }
      this.stream.write(input)
    } catch (error) {
      // Terminal I/O is best-effort - log but don't block. User will see visual feedback
      // (missing characters or incorrect size) which is appropriate for this failure mode
      console.error('[grpcDaemonClient] failed to write to session:', error)
    }
  }

  resizePtySession(sessionId: string, cols: number, rows: number): void {
    if (!this.stream) {
      console.error('[grpcDaemonClient] cannot resize: stream not established')
      return
    }

    try {
      const input: PtyInput = {
        resize: {
          sessionId,
          cols,
          rows
        }
      }
      this.stream.write(input)
    } catch (error) {
      // Terminal I/O is best-effort - log but don't block. User will see visual feedback
      // (missing characters or incorrect size) which is appropriate for this failure mode
      console.error('[grpcDaemonClient] failed to resize session:', error)
    }
  }

  async killPtySession(sessionId: string): Promise<void> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }

    return new Promise((resolve, reject) => {
      const request: KillPtyRequest = { sessionId }

      this.client!.killPty(request, (error) => {
        if (error) {
          reject(new Error(error.message))
        } else {
          // Clean up listeners
          this.dataListeners.delete(sessionId)
          this.exitListeners.delete(sessionId)
          resolve()
        }
      })
    })
  }

  async listPtySessions(): Promise<SessionInfo[]> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }

    return new Promise((resolve, reject) => {
      this.client!.listPtySessions({}, (error, response) => {
        if (error) {
          reject(new Error(error.message))
        } else if (response) {
          resolve(response.sessions as SessionInfo[])
        } else {
          resolve([])
        }
      })
    })
  }

  async shutdownDaemon(): Promise<void> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }

    return new Promise((resolve, reject) => {
      this.client!.shutdown({}, (error) => {
        if (error) {
          reject(new Error(error.message))
        } else {
          this.disconnect()
          resolve()
        }
      })
    })
  }

  async createSession(workspaces: Omit<Workspace, 'createdAt' | 'lastActivity' | 'attachedClients'>[]): Promise<Session> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }

    return new Promise((resolve, reject) => {
      const request: CreateSessionRequest = {
        workspaces: this.convertToProtoWorkspaceInputs(workspaces)
      }

      this.client!.createSession(request, (error, response) => {
        if (error) {
          reject(new Error(error.message))
        } else if (response) {
          resolve(this.convertFromProtoSession(response))
        } else {
          reject(new Error('No response from server'))
        }
      })
    })
  }

  async updateSession(
    sessionId: string,
    workspaces: Omit<Workspace, 'createdAt' | 'lastActivity' | 'attachedClients'>[],
    senderId?: string
  ): Promise<Session> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }

    return new Promise((resolve, reject) => {
      const request: UpdateSessionRequest = {
        sessionId,
        workspaces: this.convertToProtoWorkspaceInputs(workspaces),
        senderId
      }

      this.client!.updateSession(request, (error, response) => {
        if (error) {
          reject(new Error(error.message))
        } else if (response) {
          resolve(this.convertFromProtoSession(response))
        } else {
          reject(new Error('No response from server'))
        }
      })
    })
  }

  watchSession(
    sessionId: string,
    listenerId: string,
    onUpdate: (session: Session) => void,
    onError?: (error: Error) => void
  ): () => void {
    if (!this.client) {
      console.error('[grpcDaemonClient] cannot watch session: not connected')
      return () => {}
    }

    const request: SessionWatchRequest = { sessionId, listenerId }
    const stream = this.client.sessionWatch(request)

    stream.on('data', (event) => {
      if (event.session) {
        onUpdate(this.convertFromProtoSession(event.session))
      }
    })

    stream.on('error', (error) => {
      console.error('[grpcDaemonClient] sessionWatch stream error:', error)
      if (onError) {
        onError(error)
      }
    })

    return () => {
      stream.cancel()
    }
  }

  async getSession(sessionId: string): Promise<Session | null> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }

    return new Promise((resolve, reject) => {
      const request: GetSessionRequest = { sessionId }

      this.client!.getSession(request, (error, response) => {
        if (error) {
          if (error.code === grpc.status.NOT_FOUND) {
            resolve(null)
          } else {
            reject(new Error(error.message))
          }
        } else if (response) {
          resolve(this.convertFromProtoSession(response))
        } else {
          resolve(null)
        }
      })
    })
  }

  async listSessions(): Promise<Session[]> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }

    return new Promise((resolve, reject) => {
      this.client!.listSessions({}, (error, response) => {
        if (error) {
          reject(new Error(error.message))
        } else if (response) {
          resolve(response.sessions.map(s => this.convertFromProtoSession(s)))
        } else {
          resolve([])
        }
      })
    })
  }

  async getDefaultSession(): Promise<Session> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }

    return new Promise((resolve, reject) => {
      this.client!.getDefaultSession({}, (error, response) => {
        if (error) {
          reject(new Error(error.message))
        } else if (response) {
          resolve(this.convertFromProtoSession(response))
        } else {
          reject(new Error('No response from daemon'))
        }
      })
    })
  }

  async deleteSession(sessionId: string): Promise<void> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }

    return new Promise((resolve, reject) => {
      const request: DeleteSessionRequest = { sessionId }

      this.client!.deleteSession(request, (error) => {
        if (error) {
          reject(new Error(error.message))
        } else {
          resolve()
        }
      })
    })
  }

  onPtySessionData(sessionId: string, callback: DataListener): () => void {
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

  onPtySessionExit(sessionId: string, callback: ExitListener): () => void {
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

  // Exec Stream - Execute shell commands with streaming I/O
  execStream(): grpc.ClientDuplexStream<ExecInput, ExecOutput> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }

    const metadata = new grpc.Metadata()
    metadata.set('client-id', this.clientId)

    return this.client.execStream(metadata)
  }

  // Filesystem Operations

  async readDirectory(workspacePath: string, dirPath: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.readDirectory({ workspacePath, dirPath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async readFile(workspacePath: string, filePath: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')

    return new Promise((resolve, reject) => {
      const stream = this.client!.readFile({ workspacePath, filePath })
      const chunks: Buffer[] = []
      let fileMetadata: { path: string; size: number; language: string } | null = null

      stream.on('data', (chunk) => {
        if (chunk.header) {
          fileMetadata = { path: chunk.header.path, size: Number(chunk.header.size), language: chunk.header.language }
        } else if (chunk.data) {
          chunks.push(chunk.data.data)
        } else if (chunk.end) {
          if (!chunk.end.success) {
            resolve({ success: false, error: chunk.end.error })
          } else if (fileMetadata) {
            resolve({
              success: true,
              file: {
                path: fileMetadata.path,
                content: Buffer.concat(chunks).toString('utf-8'),
                size: fileMetadata.size,
                language: fileMetadata.language
              }
            })
          }
        }
      })

      stream.on('error', (err) => reject(err))
    })
  }

  async writeFile(workspacePath: string, filePath: string, content: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')

    return new Promise((resolve, reject) => {
      const stream = this.client!.writeFile((error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })

      // Send header first
      stream.write({ header: { workspacePath, filePath } })

      // Stream content in 64KB chunks
      const chunkSize = 64 * 1024
      for (let i = 0; i < content.length; i += chunkSize) {
        const chunk = content.slice(i, i + chunkSize)
        stream.write({ data: { data: Buffer.from(chunk, 'utf-8') } })
      }

      // Signal end of stream
      stream.write({ end: {} })
      stream.end()
    })
  }

  async searchFiles(workspacePath: string, query: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.searchFiles({ workspacePath, query }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  disconnect(): void {
    if (this.stream) {
      this.stream.end()
      this.stream = null
    }
    if (this.client) {
      this.client.close()
      this.client = null
    }
    this.connected = false
  }

  // Helper methods for proto conversion

  private convertToProtoWorkspaceInputs(
    workspaces: Omit<Workspace, 'createdAt' | 'lastActivity' | 'attachedClients'>[]
  ): WorkspaceInput[] {
    return workspaces.map(w => ({
      id: w.id,
      path: w.path,
      name: w.name,
      parentId: w.parentId || undefined,
      children: w.children || [],
      status: w.status,
      isGitRepo: w.isGitRepo,
      gitBranch: w.gitBranch || undefined,
      gitRootPath: w.gitRootPath || undefined,
      isWorktree: w.isWorktree,
      isDetached: w.isDetached,
      tabs: w.tabs.map((t: Tab) => ({
        id: t.id,
        applicationId: t.applicationId,
        title: t.title,
        state: Buffer.from(JSON.stringify(t.state), 'utf-8')
      })),
      activeTabId: w.activeTabId || undefined
    }))
  }

  private convertFromProtoSession(protoSession: ProtoSession): Session {
    return {
      id: protoSession.id,
      workspaces: protoSession.workspaces.map(w => this.convertFromProtoWorkspace(w)),
      createdAt: protoSession.createdAt,
      lastActivity: protoSession.lastActivity,
      attachedClients: protoSession.attachedClients
    }
  }

  private convertFromProtoWorkspace(protoWorkspace: ProtoWorkspace): Workspace {
    return {
      id: protoWorkspace.id,
      path: protoWorkspace.path,
      name: protoWorkspace.name,
      parentId: protoWorkspace.parentId || null,
      children: protoWorkspace.children || [],
      status: protoWorkspace.status as 'active' | 'merged' | 'abandoned',
      isGitRepo: protoWorkspace.isGitRepo,
      gitBranch: protoWorkspace.gitBranch || null,
      gitRootPath: protoWorkspace.gitRootPath || null,
      isWorktree: protoWorkspace.isWorktree,
      isDetached: protoWorkspace.isDetached,
      tabs: protoWorkspace.tabs.map(t => ({
        id: t.id,
        applicationId: t.applicationId,
        title: t.title,
        state: JSON.parse(t.state.toString('utf-8'))
      })),
      activeTabId: protoWorkspace.activeTabId || null,
      createdAt: protoWorkspace.createdAt,
      lastActivity: protoWorkspace.lastActivity,
      attachedClients: protoWorkspace.attachedClients
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

    // Derive PID file path from socket path (replace .sock with .pid)
    const pidPath = this.socketPath.replace(/\.sock$/, '.pid')

    console.log('[grpcDaemonClient] spawning daemon at', daemonPath)

    const child = spawn(process.execPath, [daemonPath], {
      detached: true,
      stdio: ['ignore', fs.openSync(logPath, 'a'), fs.openSync(logPath, 'a')],
      env: {
        ...process.env,
        TREETERM_DAEMON: '1',
        TREETERM_SOCKET_PATH: this.socketPath,
        TREETERM_PID_FILE: pidPath
      }
    })

    child.unref()
    console.log('[grpcDaemonClient] daemon spawned with PID', child.pid)
  }

  private async waitForSocket(): Promise<void> {
    const maxAttempts = 20
    const delay = 250

    for (let i = 0; i < maxAttempts; i++) {
      if (fs.existsSync(this.socketPath)) {
        console.log('[grpcDaemonClient] socket ready')
        return
      }
      await new Promise((resolve) => setTimeout(resolve, delay))
    }

    throw new Error('Daemon failed to create socket in time')
  }
}
