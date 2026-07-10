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
  type SessionWatchRequest,
  type WatchFileRequest,
  type FileWatchEvent as ProtoFileWatchEvent,
  type FileSignalEvent as ProtoFileSignalEvent,
  type LockSessionRequest,
  type LockSessionResponse,
  type DirectoryContents,
  type FileEntry
} from '../generated/treeterm'
import { getDefaultSocketPath } from './socketPath'
import {
  PtyEventType,
  FileWatchEventType,
  type PtyEvent,
  type IpcResult,
  type FsWriteFileResult,
  type FileWatchEvent
} from '../shared/ipc-types'
import type { FileContents } from '../renderer/types'
import type {
  SandboxConfig,
  TTYSessionInfo,
  WorkspaceRef,
  Session
} from '../shared/types'

// Alias for backward compat
type CreateSessionConfig = {
  cwd: string
  env?: Record<string, string>
  cols?: number
  rows?: number
  sandbox?: SandboxConfig
  startupCommand?: string
  /** Client-minted idempotency key: the daemon returns the existing live PTY for a
   *  known handle instead of spawning a duplicate. */
  handle?: string
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
  private pendingWrites: Array<(err: Error | null) => void> = []

  constructor(client: TreeTermDaemonClient, handle: string, sessionId: string, onEvent: (event: PtyEvent) => void) {
    this.handle = handle
    this.sessionId = sessionId
    const metadata = new grpc.Metadata()
    this.stream = client.ptyStream(metadata)

    // Set up event forwarding BEFORE sending start so no events are dropped
    this.stream.on('data', (output: PtyOutput) => {
      if (output.data) {
        onEvent({ type: PtyEventType.Data, data: output.data.data })
      } else if (output.exit) {
        onEvent({ type: PtyEventType.Exit, exitCode: output.exit.exitCode, signal: output.exit.signal })
      } else if (output.resize) {
        onEvent({ type: PtyEventType.Resize, cols: output.resize.cols, rows: output.resize.rows })
      }
    })

    this.stream.on('error', (error) => {
      console.error(`[PtyStream ${this.handle}] stream error for ${sessionId}:`, error)
      this.closed = true
      this.drainPendingWrites(error)
      onEvent({ type: PtyEventType.Error, message: error.message })
    })

    this.stream.on('end', () => {
      this.closed = true
      this.drainPendingWrites(new Error('pty stream ended'))
      onEvent({ type: PtyEventType.End })
    })

    this.stream.write({ start: { sessionId } })
  }

  private drainPendingWrites(error: Error): void {
    const pending = this.pendingWrites
    this.pendingWrites = []
    for (const cb of pending) cb(error)
  }

  write(data: string): Promise<void> {
    if (this.closed) return Promise.reject(new Error('pty stream closed'))
    return new Promise((resolve, reject) => {
      // Node gRPC's ClientDuplexStream inherits Writable semantics: the
      // callback fires once the message has been flushed to the HTTP/2
      // transport. Under server-side backpressure (daemon awaiting a prior
      // PTY write via AsyncFd) the peer's stream receive window stays closed
      // and this callback is deferred until the daemon drains — which is
      // exactly the end-to-end backpressure we rely on.
      const onDone = (err: Error | null): void => {
        const idx = this.pendingWrites.indexOf(onDone)
        if (idx !== -1) this.pendingWrites.splice(idx, 1)
        if (err) reject(err)
        else resolve()
      }
      this.pendingWrites.push(onDone)
      try {
        this.stream.write({ write: { data: Buffer.from(data, 'utf-8') } }, onDone)
      } catch (error) {
        const idx = this.pendingWrites.indexOf(onDone)
        if (idx !== -1) this.pendingWrites.splice(idx, 1)
        this.closed = true
        reject(error instanceof Error ? error : new Error(String(error)))
      }
    })
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
        this.watchConnectivity()
        resolve()
      })
    })
  }

  /**
   * Surface transport-level drops to `onDisconnect` listeners.
   *
   * A unary RPC failing with UNAVAILABLE is not a reliable signal — the channel
   * may still recover, and a suspended peer can leave calls failing while the
   * channel never reports an error on its own. The channel's connectivity state
   * is the signal: once it leaves READY the transport is gone.
   */
  private watchConnectivity(): void {
    const client = this.client
    if (!client) return
    const channel = client.getChannel()

    const step = (): void => {
      // A reconnect swaps in a new client; the old channel's watcher must go quiet.
      if (this.client !== client || !this.connected) return
      const current = channel.getConnectivityState(false)
      channel.watchConnectivityState(current, Infinity, () => {
        if (this.client !== client || !this.connected) return
        const next = channel.getConnectivityState(false)
        if (current === grpc.connectivityState.READY && next !== grpc.connectivityState.READY) {
          this.notifyDisconnect()
          return
        }
        step()
      })
    }
    step()
  }

  private notifyDisconnect(): void {
    if (!this.connected) return
    this.connected = false
    console.error('[grpcDaemonClient] transport lost for', this.socketPath)
    for (const listener of Array.from(this.disconnectListeners)) {
      listener()
    }
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
        startupCommand: config.startupCommand,
        handle: config.handle
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
    workspaceRefs: WorkspaceRef[],
    senderId?: string,
    expectedVersion?: number
  ): Promise<Session> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }
    const client = this.client

    return new Promise((resolve, reject) => {
      const request: UpdateSessionRequest = {
        workspaceRefs,
        senderId,
        expectedVersion
      }

      client.updateSession(request, (error, response) => {
        if (error) {
          reject(new Error(error.message))
        } else {
          resolve(response)
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
            session: response.session
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
          resolve(response)
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
          resolve(response)
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
        const session = event.session
        if (isFirst) {
          isFirst = false
          console.log(`[grpcClient] watchSession initial data received: session=${session.id}, workspaceRefs=${String(session.workspaceRefs.length)}`)
          resolveInitial(session)
        } else {
          console.log(`[grpcClient] watchSession update received: session=${session.id}, workspaceRefs=${String(session.workspaceRefs.length)}`)
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

  /** Watch a file's content. The first event is the current state. Maps the proto
   *  oneof to the shared `FileWatchEvent` discriminated union. */
  watchFile(
    watcherId: string,
    workspacePath: string,
    filePath: string,
    onEvent: (event: FileWatchEvent) => void,
    onError?: (error: Error) => void
  ): { unsubscribe: () => void } {
    if (!this.client) {
      console.error('[grpcDaemonClient] cannot watch file: not connected')
      if (onError) onError(new Error('Not connected to daemon'))
      return { unsubscribe: () => {} }
    }

    const request: WatchFileRequest = { watcherId, workspacePath, filePath }
    const stream = this.client.watchFile(request)

    stream.on('data', (event: ProtoFileWatchEvent) => {
      if (event.present) {
        onEvent({
          type: FileWatchEventType.Present,
          content: event.present.content.toString('utf-8'),
          sha256: event.present.sha256
        })
      } else if (event.absent) {
        onEvent({ type: FileWatchEventType.Absent })
      }
    })

    stream.on('error', (error: Error) => {
      // gRPC reports stream cancellation as an error — suppress it (intentional unsubscribe).
      if ((error as grpc.ServiceError).code === grpc.status.CANCELLED) return
      console.error('[grpcClient] watchFile stream error:', error)
      onEvent({ type: FileWatchEventType.Error, message: error.message })
      if (onError) onError(error)
    })

    return { unsubscribe: () => { stream.cancel() } }
  }

  /** Signal-only file watch (sha256, no content). Exposed for future use cases;
   *  the daemon serves it via a distinct RPC so no daemon change is needed later. */
  watchFileSignal(
    watcherId: string,
    workspacePath: string,
    filePath: string,
    onEvent: (event: { present: boolean; sha256?: string }) => void,
    onError?: (error: Error) => void
  ): { unsubscribe: () => void } {
    if (!this.client) {
      if (onError) onError(new Error('Not connected to daemon'))
      return { unsubscribe: () => {} }
    }

    const request: WatchFileRequest = { watcherId, workspacePath, filePath }
    const stream = this.client.watchFileSignal(request)

    stream.on('data', (event: ProtoFileSignalEvent) => {
      if (event.present) onEvent({ present: true, sha256: event.present.sha256 })
      else if (event.absent) onEvent({ present: false })
    })

    stream.on('error', (error: Error) => {
      if ((error as grpc.ServiceError).code === grpc.status.CANCELLED) return
      console.error('[grpcClient] watchFileSignal stream error:', error)
      if (onError) onError(error)
    })

    return { unsubscribe: () => { stream.cancel() } }
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

  async writeFile(workspacePath: string, filePath: string, content: string, expectedSha256?: string): Promise<FsWriteFileResult> {
    if (!this.client) throw new Error('Not connected to daemon')
    const client = this.client

    return new Promise((resolve, reject) => {
      const stream = client.writeFile((error, response) => {
        if (error) { reject(new Error(error.message)); return }
        if (response.success) resolve({ success: true })
        else if (response.conflict) resolve({ success: false, error: response.error ?? 'write conflict', conflict: true })
        else resolve({ success: false, error: response.error ?? 'write failed' })
      })

      // Send header first
      stream.write({ header: { workspacePath, filePath, expectedSha256 } })

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

  async deleteFile(workspacePath: string, filePath: string): Promise<IpcResult> {
    if (!this.client) throw new Error('Not connected to daemon')
    const client = this.client
    return new Promise((resolve, reject) => {
      client.deleteFile({ workspacePath, filePath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response.success) resolve({ success: true })
        else resolve({ success: false, error: response.error ?? 'delete failed' })
      })
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
    // Cleared first so the connectivity watcher treats the ensuing state change
    // as an intentional teardown and stays silent.
    const client = this.client
    this.connected = false
    this.client = null
    client?.close()
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
