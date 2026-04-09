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
  type KillPtyRequest,
  type PtyInput,
  type PtyOutput,
  type ExecInput,
  type ExecOutput,
  type UpdateSessionRequest,
  type Session as ProtoSession,
  type Workspace as ProtoWorkspace,
  type SessionWatchRequest,
  type LockSessionRequest,
  type LockSessionResponse,
  type DirectoryContents,
  type FileEntry
} from '../generated/treeterm'
import { getDefaultSocketPath } from './socketPath'
import type { PtyEvent, IpcResult } from '../shared/ipc-types'
import type { FileContents } from '../renderer/types'
import type {
  SandboxConfig,
  TTYSessionInfo,
  Workspace,
  Session,
  AppState
} from '../shared/types'

// Alias for backward compat
type CreateSessionConfig = {
  cwd: string
  env?: Record<string, string>
  cols?: number
  rows?: number
  sandbox?: SandboxConfig
  startupCommand?: string
}

type DisconnectListener = () => void

/**
 * Self-contained class owning one gRPC duplex stream for one terminal.
 * No shared state — each terminal gets its own independent PtyStream.
 */
export class PtyStream {
  readonly handle: string
  readonly sessionId: string
  private stream: grpc.ClientDuplexStream<PtyInput, PtyOutput>
  private closed: boolean = false

  constructor(client: TreeTermDaemonClient, handle: string, sessionId: string, onEvent: (event: PtyEvent) => void) {
    this.handle = handle
    this.sessionId = sessionId
    const metadata = new grpc.Metadata()
    this.stream = client.ptyStream(metadata)

    // Set up event forwarding BEFORE sending start so no events are dropped
    this.stream.on('data', (output: PtyOutput) => {
      if (output.data) {
        onEvent({ type: 'data', data: output.data.data })
      } else if (output.exit) {
        onEvent({ type: 'exit', exitCode: output.exit.exitCode, signal: output.exit.signal })
      } else if (output.resize) {
        onEvent({ type: 'resize', cols: output.resize.cols, rows: output.resize.rows })
      }
    })

    this.stream.on('error', (error) => {
      console.error(`[PtyStream ${this.handle}] stream error for ${sessionId}:`, error)
      this.closed = true
      onEvent({ type: 'error', message: error.message })
    })

    this.stream.on('end', () => {
      this.closed = true
      onEvent({ type: 'end' })
    })

    this.stream.write({ start: { sessionId } })
  }

  write(data: string): void {
    if (this.closed) return
    try {
      this.stream.write({ write: { data: Buffer.from(data, 'utf-8') } })
    } catch (error) {
      console.error(`[PtyStream ${this.handle}] failed to write:`, error)
      this.closed = true
    }
  }

  resize(cols: number, rows: number): void {
    if (this.closed) return
    try {
      this.stream.write({ resize: { cols, rows } })
    } catch (error) {
      console.error(`[PtyStream ${this.handle}] failed to resize:`, error)
      this.closed = true
    }
  }

  close(): void {
    if (this.closed) return
    this.closed = true
    this.stream.end()
  }
}

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'ico', 'avif', 'svg'])

function isImageFile(filePath: string): boolean {
  const ext = filePath.split('.').pop()?.toLowerCase() || ''
  return IMAGE_EXTENSIONS.has(ext)
}

export class GrpcDaemonClient {
  private client: TreeTermDaemonClient | null = null
  private connected: boolean = false
  private disconnectListeners: Set<DisconnectListener> = new Set()
  private clientId: string = `client-${String(Date.now())}`

  constructor(private _socketPath: string = getDefaultSocketPath()) {
    console.log('[grpcDaemonClient] initialized with socket path:', _socketPath)
  }

  get socketPath(): string {
    return this._socketPath
  }

  async connect(): Promise<void> {
    if (this.connected) {
      console.log('[grpcDaemonClient] already connected, skipping')
      return
    }

    console.log('[grpcDaemonClient] attempting connection to', this.socketPath)

    // Fast fail: check if socket file exists before attempting gRPC connection
    if (!fs.existsSync(this.socketPath)) {
      console.log('[grpcDaemonClient] socket file does not exist at', this.socketPath)
      throw new Error(`Daemon socket not found at ${this.socketPath}`)
    }

    console.log('[grpcDaemonClient] socket file exists, connecting via gRPC...')

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

      const deadline = Date.now() + 5000
      console.log('[grpcDaemonClient] waitForReady with 5s deadline')
      this.client.waitForReady(deadline, (error) => {
        if (error) {
          console.error('[grpcDaemonClient] gRPC waitForReady failed:', error.message)
          reject(error)
          return
        }

        console.log('[grpcDaemonClient] connected to daemon')
        this.connected = true
        resolve()
      })
    })
  }

  /**
   * Open a new independent PtyStream for a given session.
   * Each caller gets its own gRPC duplex stream — no shared state.
   */
  openPtyStream(handle: string, sessionId: string, onEvent: (event: PtyEvent) => void): PtyStream {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }
    return new PtyStream(this.client, handle, sessionId, onEvent)
  }

  onDisconnect(listener: DisconnectListener): () => void {
    this.disconnectListeners.add(listener)
    return () => this.disconnectListeners.delete(listener)
  }

  isConnected(): boolean {
    return this.connected
  }

  async ensureDaemonRunning(): Promise<void> {
    console.log('[grpcDaemonClient] ensureDaemonRunning: checking existing daemon...')
    try {
      await this.connect()
      console.log('[grpcDaemonClient] ensureDaemonRunning: connected to existing daemon')
    } catch (err) {
      console.log('[grpcDaemonClient] ensureDaemonRunning: connect failed:', err instanceof Error ? err.message : String(err))

      // Clean up stale state so the new daemon's check_already_running()
      // doesn't see an old alive PID and exit immediately
      const pidFile = path.join(app.getPath('home'), '.treeterm', 'daemon.pid')
      const pidExists = fs.existsSync(pidFile)
      const socketExists = fs.existsSync(this.socketPath)
      console.log('[grpcDaemonClient] stale state: pidFile=%s (exists=%s), socket=%s (exists=%s)',
        pidFile, String(pidExists), this.socketPath, String(socketExists))

      if (pidExists) {
        const pidContent = fs.readFileSync(pidFile, 'utf-8').trim()
        console.log('[grpcDaemonClient] stale PID file contains:', pidContent)
        fs.unlinkSync(pidFile)
        console.log('[grpcDaemonClient] removed stale PID file')
      }
      if (socketExists) {
        fs.unlinkSync(this.socketPath)
        console.log('[grpcDaemonClient] removed stale socket')
      }

      this.spawnDaemon()

      // Wait for socket file to appear, then give gRPC server a moment to fully bind
      console.log('[grpcDaemonClient] waiting for socket to appear...')
      await this.waitForSocket()
      console.log('[grpcDaemonClient] socket appeared, waiting 300ms for gRPC to bind...')
      await new Promise(resolve => setTimeout(resolve, 300))
      console.log('[grpcDaemonClient] attempting connection to new daemon...')
      await this.connect()
      console.log('[grpcDaemonClient] ensureDaemonRunning: connected to new daemon')
    }
  }

  async createPtySession(config: CreateSessionConfig): Promise<string> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }
    const client = this.client

    return new Promise((resolve, reject) => {
      const request: CreatePtyRequest = {
        cwd: config.cwd,
        env: config.env || {},
        cols: config.cols,
        rows: config.rows,
        sandbox: config.sandbox,
        startupCommand: config.startupCommand
      }

      client.createPty(request, (error, response) => {
        if (error) {
          reject(new Error(error.message))
        } else {
          resolve(response.sessionId)
        }
      })
    })
  }

  async killPtySession(sessionId: string): Promise<void> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }
    const client = this.client

    return new Promise((resolve, reject) => {
      const request: KillPtyRequest = { sessionId }

      client.killPty(request, (error) => {
        if (error) {
          reject(new Error(error.message))
        } else {
          resolve()
        }
      })
    })
  }

  async listPtySessions(): Promise<TTYSessionInfo[]> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }
    const client = this.client

    return new Promise((resolve, reject) => {
      client.listPtySessions({}, (error, response) => {
        if (error) {
          reject(new Error(error.message))
        } else {
          // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition
          resolve(response?.sessions as TTYSessionInfo[] ?? [])
        }
      })
    })
  }

  async shutdownDaemon(): Promise<void> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }
    const client = this.client

    return new Promise((resolve, reject) => {
      client.shutdown({}, (error) => {
        if (error) {
          reject(new Error(error.message))
        } else {
          this.disconnect()
          resolve()
        }
      })
    })
  }

  async updateSession(
    workspaces: Omit<Workspace, 'createdAt' | 'lastActivity'>[],
    senderId?: string,
    expectedVersion?: number
  ): Promise<Session> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }
    const client = this.client

    return new Promise((resolve, reject) => {
      const request: UpdateSessionRequest = {
        workspaces: this.convertToProtoWorkspaceInputs(workspaces),
        senderId,
        expectedVersion
      }

      client.updateSession(request, (error, response) => {
        if (error) {
          reject(new Error(error.message))
        } else {
          resolve(this.convertFromProtoSession(response))
        }
      })
    })
  }

  async lockSession(
    ttlMs?: number
  ): Promise<{ acquired: boolean; session: Session }> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }
    const client = this.client

    return new Promise((resolve, reject) => {
      const request: LockSessionRequest = {
        ttlMs: ttlMs ?? 60_000
      }

      client.lockSession(request, (error: grpc.ServiceError | null, response: LockSessionResponse) => {
        if (error) {
          reject(new Error(error.message))
        } else if (!response.session) {
          reject(new Error('LockSession response missing session'))
        } else {
          resolve({
            acquired: response.acquired,
            session: this.convertFromProtoSession(response.session)
          })
        }
      })
    })
  }

  async unlockSession(): Promise<Session> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }
    const client = this.client

    return new Promise((resolve, reject) => {
      client.unlockSession({}, (error: grpc.ServiceError | null, response: ProtoSession) => {
        if (error) {
          reject(new Error(error.message))
        } else {
          resolve(this.convertFromProtoSession(response))
        }
      })
    })
  }

  async forceUnlockSession(): Promise<Session> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }
    const client = this.client

    return new Promise((resolve, reject) => {
      client.forceUnlockSession({}, (error: grpc.ServiceError | null, response: ProtoSession) => {
        if (error) {
          reject(new Error(error.message))
        } else {
          resolve(this.convertFromProtoSession(response))
        }
      })
    })
  }

  watchSession(
    listenerId: string,
    onUpdate: (session: Session) => void,
    onError?: (error: Error) => void
  ): { initial: Promise<Session>; unsubscribe: () => void } {
    if (!this.client) {
      console.error('[grpcDaemonClient] cannot watch session: not connected')
      return {
        initial: Promise.reject(new Error('Not connected to daemon')),
        unsubscribe: () => {}
      }
    }

    const request: SessionWatchRequest = { listenerId }
    const stream = this.client.sessionWatch(request)

    let isFirst = true
    let resolveInitial: (session: Session) => void
    let rejectInitial: (error: Error) => void
    const initial = new Promise<Session>((resolve, reject) => {
      resolveInitial = resolve
      rejectInitial = reject
    })

    console.log(`[grpcClient] watchSession started for listener=${listenerId}`)

    stream.on('data', (event: { session?: ProtoSession }) => {
      if (event.session) {
        const session = this.convertFromProtoSession(event.session)
        if (isFirst) {
          isFirst = false
          console.log(`[grpcClient] watchSession initial data received: session=${session.id}, workspaces=${String(session.workspaces.length)}`)
          resolveInitial(session)
        } else {
          console.log(`[grpcClient] watchSession update received: session=${session.id}, workspaces=${String(session.workspaces.length)}`)
          onUpdate(session)
        }
      }
    })

    stream.on('error', (error: Error) => {
      console.error(`[grpcClient] watchSession stream error:`, error)
      if (isFirst) {
        isFirst = false
        rejectInitial(error)
      }
      if (onError) {
        onError(error)
      }
    })

    return {
      initial,
      unsubscribe: () => {
        stream.cancel()
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

  async readDirectory(workspacePath: string, dirPath: string): Promise<IpcResult<{ contents: DirectoryContents }>> {
    if (!this.client) throw new Error('Not connected to daemon')
    const client = this.client
    return new Promise((resolve, reject) => {
      client.readDirectory({ workspacePath, dirPath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else resolve(response as IpcResult<{ contents: DirectoryContents }>)
      })
    })
  }

  async readFile(workspacePath: string, filePath: string): Promise<IpcResult<{ file: FileContents }>> {
    if (!this.client) throw new Error('Not connected to daemon')
    const client = this.client

    return new Promise((resolve, reject) => {
      const stream = client.readFile({ workspacePath, filePath })
      const chunks: Buffer[] = []
      let fileMetadata: { path: string; size: number; language: string } | null = null

      stream.on('data', (chunk: { header?: { path: string; size: number | string; language: string }; data?: { data: Buffer }; end?: { success: boolean; error?: string } }) => {
        if (chunk.header) {
          fileMetadata = { path: chunk.header.path, size: Number(chunk.header.size), language: chunk.header.language }
        } else if (chunk.data) {
          chunks.push(chunk.data.data)
        } else if (chunk.end) {
          if (!chunk.end.success) {
            resolve({ success: false as const, error: chunk.end.error ?? 'Unknown error' })
          } else if (fileMetadata) {
            const isImage = isImageFile(fileMetadata.path)
            resolve({
              success: true,
              file: {
                path: fileMetadata.path,
                content: isImage ? Buffer.concat(chunks).toString('base64') : Buffer.concat(chunks).toString('utf-8'),
                size: fileMetadata.size,
                language: isImage ? 'image' : fileMetadata.language
              }
            })
          }
        }
      })

      stream.on('error', (err) => { reject(err); })
    })
  }

  async writeFile(workspacePath: string, filePath: string, content: string): Promise<IpcResult> {
    if (!this.client) throw new Error('Not connected to daemon')
    const client = this.client

    return new Promise((resolve, reject) => {
      const stream = client.writeFile((error, response) => {
        if (error) reject(new Error(error.message))
        else resolve(response as IpcResult)
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

  async searchFiles(workspacePath: string, query: string): Promise<IpcResult<{ entries: FileEntry[] }>> {
    if (!this.client) throw new Error('Not connected to daemon')
    const client = this.client
    return new Promise((resolve, reject) => {
      client.searchFiles({ workspacePath, query }, (error, response) => {
        if (error) reject(new Error(error.message))
        else resolve(response as IpcResult<{ entries: FileEntry[] }>)
      })
    })
  }

  disconnect(): void {
    if (this.client) {
      this.client.close()
      this.client = null
    }
    this.connected = false
  }

  // Helper methods for proto conversion

  private convertToProtoWorkspaceInputs(
    workspaces: Omit<Workspace, 'createdAt' | 'lastActivity'>[]
  ): ProtoWorkspace[] {
    return workspaces.map(w => {
      const protoAppStates: { [key: string]: { applicationId: string; title: string; state: Buffer } } = {}
      for (const [key, s] of Object.entries(w.appStates)) {
        protoAppStates[key] = {
          applicationId: s.applicationId,
          title: s.title,
          state: Buffer.from(JSON.stringify(s.state), 'utf-8')
        }
      }
      return {
        id: w.id,
        path: w.path,
        name: w.name,
        parentId: w.parentId || undefined,
        status: w.status,
        isGitRepo: w.isGitRepo,
        gitBranch: w.gitBranch || undefined,
        gitRootPath: w.gitRootPath || undefined,
        isWorktree: w.isWorktree,
        isDetached: w.isDetached,
        appStates: protoAppStates,
        activeTabId: w.activeTabId || undefined,
        createdAt: 0,
        lastActivity: 0,
        metadata: Buffer.from(JSON.stringify(w.metadata), 'utf-8')
      }
    })
  }

  private convertFromProtoSession(protoSession: ProtoSession): Session {
    return {
      id: protoSession.id,
      workspaces: protoSession.workspaces.map(w => this.convertFromProtoWorkspace(w)),
      createdAt: protoSession.createdAt,
      lastActivity: protoSession.lastActivity,
      version: protoSession.version,
      lock: protoSession.lock
        ? { acquiredAt: protoSession.lock.acquiredAt, expiresAt: protoSession.lock.expiresAt }
        : null
    }
  }

  private convertFromProtoWorkspace(protoWorkspace: ProtoWorkspace): Workspace {
    const appStates: Record<string, AppState> = {}
    for (const [key, value] of Object.entries(protoWorkspace.appStates)) {
      appStates[key] = {
        applicationId: value.applicationId,
        title: value.title,
        state: JSON.parse(value.state.toString('utf-8')) as unknown
      }
    }
    return {
      id: protoWorkspace.id,
      path: protoWorkspace.path,
      name: protoWorkspace.name,
      parentId: protoWorkspace.parentId || null,
      status: protoWorkspace.status as 'active' | 'merged' | 'abandoned',
      isGitRepo: protoWorkspace.isGitRepo,
      gitBranch: protoWorkspace.gitBranch || null,
      gitRootPath: protoWorkspace.gitRootPath || null,
      isWorktree: protoWorkspace.isWorktree,
      isDetached: protoWorkspace.isDetached ?? false,
      appStates,
      activeTabId: protoWorkspace.activeTabId || null,
      settings: { defaultApplicationId: '' },
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition
      metadata: protoWorkspace.metadata?.length ? JSON.parse(protoWorkspace.metadata.toString('utf-8')) as Record<string, string> : {},
      createdAt: protoWorkspace.createdAt,
      lastActivity: protoWorkspace.lastActivity
    }
  }

  private spawnDaemon(): void {
    const daemonPath = app.isPackaged
      ? path.join(process.resourcesPath, 'daemon-rs', 'treeterm-daemon')
      : path.join(__dirname, '../daemon-rs/treeterm-daemon')

    console.log('[grpcDaemonClient] daemon binary path:', daemonPath, 'exists:', String(fs.existsSync(daemonPath)))

    if (!fs.existsSync(daemonPath)) {
      throw new Error(`Daemon executable not found at ${daemonPath}`)
    }

    const logPath = path.join(app.getPath('userData'), 'daemon.log')
    console.log('[grpcDaemonClient] daemon log path:', logPath)
    console.log('[grpcDaemonClient] TREETERM_SOCKET_PATH will be:', this.socketPath)

    const child = spawn(daemonPath, [], {
      detached: true,
      stdio: ['ignore', fs.openSync(logPath, 'a'), fs.openSync(logPath, 'a')],
      env: {
        ...process.env,
        TREETERM_SOCKET_PATH: this.socketPath
      }
    })

    child.on('error', (err) => {
      console.error('[grpcDaemonClient] daemon spawn error:', err.message)
    })
    child.on('exit', (code, signal) => {
      console.log('[grpcDaemonClient] daemon process exited: code=%s signal=%s', String(code), String(signal))
    })

    child.unref()
    console.log('[grpcDaemonClient] daemon spawned with PID', String(child.pid))
  }

  private async waitForSocket(): Promise<void> {
    const maxAttempts = 20
    const delay = 250

    for (let i = 0; i < maxAttempts; i++) {
      if (fs.existsSync(this.socketPath)) {
        console.log('[grpcDaemonClient] socket ready after %d attempts', i + 1)
        return
      }
      if (i > 0 && i % 4 === 0) {
        console.log('[grpcDaemonClient] still waiting for socket... attempt %d/%d', i + 1, maxAttempts)
      }
      await new Promise((resolve) => setTimeout(resolve, delay))
    }

    console.error('[grpcDaemonClient] socket not found after %dms at %s', maxAttempts * delay, this.socketPath)
    throw new Error('Daemon failed to create socket in time')
  }
}
