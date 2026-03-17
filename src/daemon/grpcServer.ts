/**
 * gRPC server for daemon-client communication
 */

import * as grpc from '@grpc/grpc-js'
import * as fs from 'fs'
import * as path from 'path'
import type { DaemonPtyManager } from './ptyManager'
import type { SessionStore } from './sessionStore'
import type { Session, Workspace, Tab } from '../shared/types'
import { createModuleLogger } from './logger'
import { getDefaultSocketPath } from './socketPath'
import * as filesystem from './filesystem'
import { execManager } from './execManager'
import {
  TreeTermDaemonService,
  type CreatePtyRequest,
  type CreatePtyResponse,
  type AttachPtyRequest,
  type AttachPtyResponse,
  type DetachPtyRequest,
  type ResizePtyRequest,
  type KillPtyRequest,
  type GetScrollbackRequest,
  type GetScrollbackResponse,
  type ListPtySessionsResponse,
  type PtyInput,
  type PtyOutput,
  type CreateSessionRequest,
  type UpdateSessionRequest,
  type GetSessionRequest,
  type DeleteSessionRequest,
  type ListSessionsResponse,
  type Empty,
  type Session as ProtoSession,
  type WorkspaceInput,
  type PtySessionInfo,
  type ExecInput,
  type ExecOutput,
  type ReadDirectoryRequest,
  type ReadDirectoryResponse,
  type ReadFileRequest,
  type ReadFileResponse,
  type WriteFileRequest,
  type WriteFileResponse,
  type SearchFilesRequest,
  type SearchFilesResponse,
  type FileReadChunk,
  type FileWriteChunk,
  type DiffChunk,
  type SessionWatchRequest,
  type SessionWatchEvent
} from '../generated/treeterm'

const log = createModuleLogger('grpcServer')

export { getDefaultSocketPath }

// Track connected clients and their streams
interface ClientStream {
  clientId: string
  stream: grpc.ServerDuplexStream<PtyInput, PtyOutput>
  attachedSessions: Set<string>
}

interface SessionWatcher {
  listenerId: string
  sessionId: string
  stream: grpc.ServerWritableStream<SessionWatchRequest, SessionWatchEvent>
}

export class GrpcServer {
  private server: grpc.Server
  private ptyManager: DaemonPtyManager
  private sessionStore: SessionStore
  private clientStreams: Map<string, ClientStream> = new Map()
  private sessionWatchers: Map<string, SessionWatcher> = new Map() // listenerId -> watcher
  private clientCounter = 0

  constructor(
    private socketPath: string,
    ptyManager: DaemonPtyManager,
    sessionStore?: SessionStore
  ) {
    this.ptyManager = ptyManager
    // Import SessionStore dynamically to avoid circular dependency
    const { SessionStore: SS } = require('./sessionStore')
    this.sessionStore = sessionStore || new SS()

    // Configure larger message size limits as a safety buffer
    // Default is 4MB, we set to 8MB for headroom
    // Note: Scrollback is limited to 1MB on the PTY manager side
    this.server = new grpc.Server({
      'grpc.max_receive_message_length': 8 * 1024 * 1024, // 8 MB
      'grpc.max_send_message_length': 8 * 1024 * 1024 // 8 MB
    })
    this.server.addService(TreeTermDaemonService, this.createServiceImpl())
  }

  start(): Promise<void> {
    return new Promise((resolve, reject) => {
      // Remove stale socket file if exists
      if (fs.existsSync(this.socketPath)) {
        log.info({ socketPath: this.socketPath }, 'removing stale socket')
        fs.unlinkSync(this.socketPath)
      }

      // Ensure socket directory exists
      const socketDir = path.dirname(this.socketPath)
      if (!fs.existsSync(socketDir)) {
        fs.mkdirSync(socketDir, { recursive: true, mode: 0o700 })
      }

      // Bind to Unix socket
      const socketUri = `unix://${this.socketPath}`
      this.server.bindAsync(
        socketUri,
        grpc.ServerCredentials.createInsecure(),
        (error, port) => {
          if (error) {
            log.error({ err: error }, 'failed to bind server')
            reject(error)
            return
          }

          log.info({ socketPath: this.socketPath }, 'server listening')
          // Set socket permissions (user-only)
          fs.chmodSync(this.socketPath, 0o600)
          resolve()
        }
      )

      // Set up PTY event forwarding to all connected client streams
      this.ptyManager.onData((sessionId, data) => {
        this.broadcastPtyData(sessionId, data)
      })

      this.ptyManager.onExit((sessionId, exitCode, signal) => {
        this.broadcastPtyExit(sessionId, exitCode, signal)
      })
    })
  }

  stop(): void {
    log.info('stopping server')

    // Close all client streams
    for (const [clientId, clientStream] of this.clientStreams) {
      try {
        clientStream.stream.end()
      } catch (error) {
        log.error({ err: error, clientId }, 'error closing client stream')
      }
      this.clientStreams.delete(clientId)
    }

    // Shutdown exec manager
    execManager.shutdown()

    // Stop server
    this.server.forceShutdown()

    // Remove socket file
    if (fs.existsSync(this.socketPath)) {
      fs.unlinkSync(this.socketPath)
    }
  }

  private createServiceImpl(): grpc.UntypedServiceImplementation {
    return {
      createPty: this.handleCreatePty.bind(this),
      attachPty: this.handleAttachPty.bind(this),
      detachPty: this.handleDetachPty.bind(this),
      resizePty: this.handleResizePty.bind(this),
      killPty: this.handleKillPty.bind(this),
      listPtySessions: this.handleListPtySessions.bind(this),
      getScrollback: this.handleGetScrollback.bind(this),
      ptyStream: this.handlePtyStream.bind(this),
      execStream: this.handleExecStream.bind(this),
      createSession: this.handleCreateSession.bind(this),
      updateSession: this.handleUpdateSession.bind(this),
      getSession: this.handleGetSession.bind(this),
      deleteSession: this.handleDeleteSession.bind(this),
      listSessions: this.handleListSessions.bind(this),
      getDefaultSession: this.handleGetDefaultSession.bind(this),
      sessionWatch: this.handleSessionWatch.bind(this),
      shutdown: this.handleShutdown.bind(this),
      // Filesystem operations
      readDirectory: this.handleReadDirectory.bind(this),
      readFile: this.handleReadFile.bind(this),
      writeFile: this.handleWriteFile.bind(this),
      searchFiles: this.handleSearchFiles.bind(this)
    }
  }

  // PTY Management Handlers (Unary RPCs)

  private handleCreatePty(
    call: grpc.ServerUnaryCall<CreatePtyRequest, CreatePtyResponse>,
    callback: grpc.sendUnaryData<CreatePtyResponse>
  ): void {
    try {
      log.debug({ request: call.request }, 'createPty called')
      const sessionId = this.ptyManager.create({
        cwd: call.request.cwd,
        env: call.request.env || {},
        cols: call.request.cols,
        rows: call.request.rows,
        sandbox: call.request.sandbox,
        startupCommand: call.request.startupCommand
      })

      log.info({ sessionId }, 'PTY session created')
      callback(null, { sessionId })
    } catch (error) {
      log.error({ err: error }, 'createPty error')
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private handleAttachPty(
    call: grpc.ServerUnaryCall<AttachPtyRequest, AttachPtyResponse>,
    callback: grpc.sendUnaryData<AttachPtyResponse>
  ): void {
    try {
      const { sessionId } = call.request
      const clientId = this.getClientId(call.metadata)
      log.info({ sessionId, clientId }, 'attachPty called')

      // Attach client to PTY session
      this.ptyManager.attach(sessionId, clientId)

      // Get scrollback history
      const scrollback = this.ptyManager.getScrollback(sessionId)

      callback(null, { scrollback })
    } catch (error) {
      log.error({ err: error, sessionId: call.request.sessionId }, 'attachPty error')
      callback({
        code: grpc.status.NOT_FOUND,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private handleDetachPty(
    call: grpc.ServerUnaryCall<DetachPtyRequest, Empty>,
    callback: grpc.sendUnaryData<Empty>
  ): void {
    try {
      const { sessionId } = call.request
      const clientId = this.getClientId(call.metadata)
      log.info({ sessionId, clientId }, 'detachPty called')

      this.ptyManager.detach(sessionId, clientId)
      callback(null, {})
    } catch (error) {
      log.error({ err: error, sessionId: call.request.sessionId }, 'detachPty error')
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private handleResizePty(
    call: grpc.ServerUnaryCall<ResizePtyRequest, Empty>,
    callback: grpc.sendUnaryData<Empty>
  ): void {
    try {
      const { sessionId, cols, rows } = call.request
      log.debug({ sessionId, cols, rows }, 'resizePty called')

      this.ptyManager.resize(sessionId, cols, rows)
      callback(null, {})
    } catch (error) {
      log.error({ err: error }, 'resizePty error')
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private handleKillPty(
    call: grpc.ServerUnaryCall<KillPtyRequest, Empty>,
    callback: grpc.sendUnaryData<Empty>
  ): void {
    try {
      const { sessionId } = call.request
      log.info({ sessionId }, 'killPty called')

      this.ptyManager.kill(sessionId)
      callback(null, {})
    } catch (error) {
      log.error({ err: error }, 'killPty error')
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private handleListPtySessions(
    call: grpc.ServerUnaryCall<Empty, ListPtySessionsResponse>,
    callback: grpc.sendUnaryData<ListPtySessionsResponse>
  ): void {
    try {
      const sessions = this.ptyManager.listSessions()
      log.debug({ count: sessions.length }, 'listPtySessions called')

      // Convert SessionInfo to PtySessionInfo (they should have the same shape)
      const protoSessions: PtySessionInfo[] = sessions.map(s => ({
        id: s.id,
        cwd: s.cwd,
        cols: s.cols,
        rows: s.rows,
        createdAt: s.createdAt,
        lastActivity: s.lastActivity,
        attachedClients: s.attachedClients
      }))

      callback(null, { sessions: protoSessions })
    } catch (error) {
      log.error({ err: error }, 'listPtySessions error')
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private handleGetScrollback(
    call: grpc.ServerUnaryCall<GetScrollbackRequest, GetScrollbackResponse>,
    callback: grpc.sendUnaryData<GetScrollbackResponse>
  ): void {
    try {
      const { sessionId } = call.request
      log.debug({ sessionId }, 'getScrollback called')

      const scrollback = this.ptyManager.getScrollback(sessionId)
      callback(null, { scrollback })
    } catch (error) {
      log.error({ err: error }, 'getScrollback error')
      callback({
        code: grpc.status.NOT_FOUND,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  // PTY Streaming Handler (Bidirectional)

  private handlePtyStream(
    call: grpc.ServerDuplexStream<PtyInput, PtyOutput>
  ): void {
    const clientId = this.getClientId(call.metadata) || `client-${++this.clientCounter}`
    log.info({ clientId }, 'client stream connected')

    const clientStream: ClientStream = {
      clientId,
      stream: call,
      attachedSessions: new Set()
    }
    this.clientStreams.set(clientId, clientStream)

    // Handle incoming messages from client
    call.on('data', (input: PtyInput) => {
      try {
        if (input.write) {
          const { sessionId, data } = input.write
          this.ptyManager.write(sessionId, data.toString('utf-8'))
          clientStream.attachedSessions.add(sessionId)
        } else if (input.resize) {
          const { sessionId, cols, rows } = input.resize
          this.ptyManager.resize(sessionId, cols, rows)
        } else if (input.detach) {
          const { sessionId } = input.detach
          this.ptyManager.detach(sessionId, clientId)
          clientStream.attachedSessions.delete(sessionId)
        }
      } catch (error) {
        log.error({ err: error, clientId }, 'error processing client input')
      }
    })

    call.on('end', () => {
      log.info({ clientId }, 'client stream ended')
      this.handleClientDisconnect(clientId)
    })

    call.on('error', (error) => {
      log.error({ err: error, clientId }, 'client stream error')
      this.handleClientDisconnect(clientId)
    })
  }

  private handleClientDisconnect(clientId: string): void {
    const clientStream = this.clientStreams.get(clientId)
    if (!clientStream) return

    // Detach client from all PTY sessions
    const sessions = this.ptyManager.listSessions()
    for (const session of sessions) {
      this.ptyManager.detach(session.id, clientId)
    }

    // Detach client from all workspace sessions
    this.sessionStore.detachClient(clientId)

    this.clientStreams.delete(clientId)
  }

  // Exec Streaming Handler (Bidirectional)

  private handleExecStream(
    call: grpc.ServerDuplexStream<ExecInput, ExecOutput>
  ): void {
    const clientId = this.getClientId(call.metadata) || `client-${++this.clientCounter}`
    log.info({ clientId }, 'exec stream connected')

    let execId: string | null = null
    let started = false

    call.on('data', (input: ExecInput) => {
      try {
        if (!started && input.start) {
          started = true
          execId = `exec-${++this.clientCounter}-${Date.now()}`
          
          log.debug({ execId, command: input.start.command, args: input.start.args }, 'starting exec')
          
          execManager.start(execId, {
            cwd: input.start.cwd,
            command: input.start.command,
            args: input.start.args,
            env: input.start.env,
            timeoutMs: input.start.timeoutMs ?? 30000
          }, {
            onStdout: (data) => {
              call.write({ stdout: { data } })
            },
            onStderr: (data) => {
              call.write({ stderr: { data } })
            },
            onExit: (code, signal, error) => {
              log.debug({ execId, code, signal, error: error?.message }, 'exec completed')
              call.write({
                result: {
                  exitCode: code ?? -1,
                  error: error?.message
                }
              })
              call.end()
            }
          })
        } else if (execId && input.stdin) {
          execManager.writeStdin(execId, input.stdin)
        } else if (execId && input.signal) {
          execManager.kill(execId, input.signal.signal)
        }
      } catch (error) {
        log.error({ err: error, clientId }, 'error processing exec input')
        call.write({
          result: {
            exitCode: -1,
            error: error instanceof Error ? error.message : 'Unknown error'
          }
        })
        call.end()
      }
    })

    call.on('end', () => {
      log.info({ clientId }, 'exec stream ended')
      if (execId) {
        execManager.closeStdin(execId)
      }
    })

    call.on('error', (error) => {
      log.error({ err: error, clientId }, 'exec stream error')
      if (execId) {
        execManager.kill(execId, 15) // SIGTERM
      }
    })
  }

  // Workspace Session Handlers

  private handleCreateSession(
    call: grpc.ServerUnaryCall<CreateSessionRequest, ProtoSession>,
    callback: grpc.sendUnaryData<ProtoSession>
  ): void {
    try {
      const clientId = this.getClientId(call.metadata)
      log.debug({ clientId, workspaces: call.request.workspaces.length }, 'createSession called')

      // Convert proto WorkspaceInput to internal format
      const workspaces = this.convertWorkspaceInputs(call.request.workspaces)
      const session = this.sessionStore.createSession(clientId, workspaces)

      // Convert to proto format
      const protoSession = this.convertToProtoSession(session)
      callback(null, protoSession)
    } catch (error) {
      log.error({ err: error }, 'createSession error')
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private handleUpdateSession(
    call: grpc.ServerUnaryCall<UpdateSessionRequest, ProtoSession>,
    callback: grpc.sendUnaryData<ProtoSession>
  ): void {
    try {
      const clientId = this.getClientId(call.metadata)
      const { sessionId, workspaces, senderId } = call.request
      log.debug({ clientId, sessionId, senderId }, 'updateSession called')

      const convertedWorkspaces = this.convertWorkspaceInputs(workspaces)
      const session = this.sessionStore.updateSession(clientId, sessionId, convertedWorkspaces)

      if (!session) {
        callback({
          code: grpc.status.NOT_FOUND,
          message: `Session not found: ${sessionId}`
        })
        return
      }

      const protoSession = this.convertToProtoSession(session)
      callback(null, protoSession)

      // Broadcast to all watchers of this session except the sender
      if (senderId) {
        this.broadcastSessionUpdate(sessionId, protoSession, senderId)
      }
    } catch (error) {
      log.error({ err: error }, 'updateSession error')
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private handleSessionWatch(
    call: grpc.ServerWritableStream<SessionWatchRequest, SessionWatchEvent>
  ): void {
    const { sessionId, listenerId } = call.request
    log.info({ sessionId, listenerId }, 'sessionWatch registered')

    const watcher: SessionWatcher = { listenerId, sessionId, stream: call }
    this.sessionWatchers.set(listenerId, watcher)

    call.on('cancelled', () => {
      log.info({ listenerId }, 'sessionWatch cancelled')
      this.sessionWatchers.delete(listenerId)
    })

    call.on('error', (error) => {
      log.error({ err: error, listenerId }, 'sessionWatch error')
      this.sessionWatchers.delete(listenerId)
    })
  }

  private broadcastSessionUpdate(sessionId: string, protoSession: ProtoSession, senderId: string): void {
    const event: SessionWatchEvent = {
      sessionId,
      session: protoSession,
      senderId
    }

    for (const watcher of this.sessionWatchers.values()) {
      if (watcher.sessionId === sessionId && watcher.listenerId !== senderId) {
        try {
          watcher.stream.write(event)
        } catch (error) {
          log.error({ err: error, listenerId: watcher.listenerId }, 'error broadcasting session update')
          this.sessionWatchers.delete(watcher.listenerId)
        }
      }
    }
  }

  private handleGetSession(
    call: grpc.ServerUnaryCall<GetSessionRequest, ProtoSession>,
    callback: grpc.sendUnaryData<ProtoSession>
  ): void {
    try {
      const { sessionId } = call.request
      log.debug({ sessionId }, 'getSession called')

      const session = this.sessionStore.getSession(sessionId)
      if (!session) {
        callback({
          code: grpc.status.NOT_FOUND,
          message: `Session not found: ${sessionId}`
        })
        return
      }

      const protoSession = this.convertToProtoSession(session)
      callback(null, protoSession)
    } catch (error) {
      log.error({ err: error }, 'getSession error')
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private handleDeleteSession(
    call: grpc.ServerUnaryCall<DeleteSessionRequest, Empty>,
    callback: grpc.sendUnaryData<Empty>
  ): void {
    try {
      const { sessionId } = call.request
      log.info({ sessionId }, 'deleteSession called')

      this.sessionStore.deleteSession(sessionId)
      callback(null, {})
    } catch (error) {
      log.error({ err: error }, 'deleteSession error')
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private handleListSessions(
    call: grpc.ServerUnaryCall<Empty, ListSessionsResponse>,
    callback: grpc.sendUnaryData<ListSessionsResponse>
  ): void {
    try {
      const sessions = this.sessionStore.listSessions()
      log.debug({ count: sessions.length }, 'listSessions called')

      const protoSessions = sessions.map(s => this.convertToProtoSession(s))
      callback(null, { sessions: protoSessions })
    } catch (error) {
      log.error({ err: error }, 'listSessions error')
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private handleGetDefaultSession(
    call: grpc.ServerUnaryCall<Empty, ProtoSession>,
    callback: grpc.sendUnaryData<ProtoSession>
  ): void {
    try {
      // Get or create the default session for this client
      const clientId = call.metadata.get('client-id')[0]?.toString() || 'unknown'
      const session = this.sessionStore.getOrCreateDefaultSession(clientId)
      log.debug({ sessionId: session.id, clientId }, 'getDefaultSession called')

      callback(null, this.convertToProtoSession(session))
    } catch (error) {
      log.error({ err: error }, 'getDefaultSession error')
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  // Daemon Control

  private handleShutdown(
    call: grpc.ServerUnaryCall<Empty, Empty>,
    callback: grpc.sendUnaryData<Empty>
  ): void {
    log.info('shutdown requested via gRPC')
    callback(null, {})

    // Schedule shutdown after sending response
    setTimeout(() => {
      log.info('initiating shutdown')
      this.stop()
      this.ptyManager.shutdown()
      process.exit(0)
    }, 100)
  }

  // Helper Methods

  private getClientId(metadata: grpc.Metadata): string {
    const clientIds = metadata.get('client-id')
    if (clientIds.length > 0) {
      return clientIds[0].toString()
    }
    return `client-${++this.clientCounter}`
  }

  private broadcastPtyData(sessionId: string, data: string): void {
    const output: PtyOutput = {
      data: {
        sessionId,
        data: Buffer.from(data, 'utf-8')
      }
    }

    for (const clientStream of this.clientStreams.values()) {
      try {
        clientStream.stream.write(output)
      } catch (error) {
        log.error({ err: error, clientId: clientStream.clientId }, 'error broadcasting PTY data')
      }
    }
  }

  private broadcastPtyExit(sessionId: string, exitCode: number, signal?: number): void {
    const output: PtyOutput = {
      exit: {
        sessionId,
        exitCode,
        signal
      }
    }

    for (const clientStream of this.clientStreams.values()) {
      try {
        clientStream.stream.write(output)
      } catch (error) {
        log.error({ err: error, clientId: clientStream.clientId }, 'error broadcasting PTY exit')
      }
    }
  }

  private convertWorkspaceInputs(inputs: WorkspaceInput[]): Omit<Workspace, 'createdAt' | 'lastActivity' | 'attachedClients'>[] {
    return inputs.map(input => ({
      id: input.id,
      path: input.path,
      name: input.name,
      parentId: input.parentId || null,
      children: input.children || [],
      status: input.status as 'active' | 'merged' | 'abandoned',
      isGitRepo: input.isGitRepo,
      gitBranch: input.gitBranch || null,
      gitRootPath: input.gitRootPath || null,
      isWorktree: input.isWorktree,
      isDetached: input.isDetached,
      tabs: input.tabs.map(tab => ({
        id: tab.id,
        applicationId: tab.applicationId,
        title: tab.title,
        state: JSON.parse(tab.state.toString('utf-8'))
      })),
      activeTabId: input.activeTabId || null,
      metadata: input.metadata?.length ? JSON.parse(input.metadata.toString('utf-8')) : {}
    }))
  }

  private convertToProtoSession(session: Session): ProtoSession {
    return {
      id: session.id,
      workspaces: session.workspaces.map((w: Workspace) => ({
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
        activeTabId: w.activeTabId || undefined,
        metadata: Buffer.from(JSON.stringify(w.metadata ?? {}), 'utf-8'),
        createdAt: w.createdAt,
        lastActivity: w.lastActivity,
        attachedClients: w.attachedClients
      })),
      createdAt: session.createdAt,
      lastActivity: session.lastActivity,
      attachedClients: session.attachedClients
    }
  }

  // Filesystem Operation Handlers

  private async handleReadDirectory(
    call: grpc.ServerUnaryCall<ReadDirectoryRequest, ReadDirectoryResponse>,
    callback: grpc.sendUnaryData<ReadDirectoryResponse>
  ): Promise<void> {
    try {
      const result = await filesystem.readDirectory(
        call.request.workspacePath,
        call.request.dirPath
      )
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleReadFile(
    call: grpc.ServerWritableStream<ReadFileRequest, FileReadChunk>
  ): Promise<void> {
    try {
      const result = await filesystem.readFile(
        call.request.workspacePath,
        call.request.filePath
      )

      if (!result.success || !result.file) {
        call.write({ end: { success: false, error: result.error || 'Unknown error' } })
        call.end()
        return
      }

      const { path: filePath, content, size, language } = result.file

      // Send header with metadata
      call.write({ header: { path: filePath, size, language } })

      // Stream content in 64KB chunks
      const chunkSize = 64 * 1024
      for (let i = 0; i < content.length; i += chunkSize) {
        const chunk = content.slice(i, i + chunkSize)
        call.write({ data: { data: Buffer.from(chunk, 'utf-8') } })
      }

      call.write({ end: { success: true } })
      call.end()
    } catch (error) {
      call.write({ end: { success: false, error: error instanceof Error ? error.message : 'Unknown error' } })
      call.end()
    }
  }

  private handleWriteFile(
    call: grpc.ServerReadableStream<FileWriteChunk, WriteFileResponse>,
    callback: grpc.sendUnaryData<WriteFileResponse>
  ): void {
    let workspacePath: string = ''
    let filePath: string = ''
    const chunks: Buffer[] = []

    call.on('data', (chunk: FileWriteChunk) => {
      if (chunk.header) {
        workspacePath = chunk.header.workspacePath
        filePath = chunk.header.filePath
      } else if (chunk.data) {
        chunks.push(chunk.data.data)
      }
    })

    call.on('end', async () => {
      try {
        const content = Buffer.concat(chunks).toString('utf-8')
        const result = await filesystem.writeFile(workspacePath, filePath, content)
        callback(null, result)
      } catch (error) {
        callback({
          code: grpc.status.INTERNAL,
          message: error instanceof Error ? error.message : 'Unknown error'
        })
      }
    })

    call.on('error', (error) => {
      callback({
        code: grpc.status.INTERNAL,
        message: error.message
      })
    })
  }

  private handleSearchFiles(
    call: grpc.ServerUnaryCall<SearchFilesRequest, SearchFilesResponse>,
    callback: grpc.sendUnaryData<SearchFilesResponse>
  ): void {
    const { workspacePath, query } = call.request

    filesystem
      .searchFiles(workspacePath, query)
      .then((result) => {
        callback(null, {
          success: result.success,
          entries: result.entries || [],
          error: result.error
        })
      })
      .catch((error) => {
        callback({
          code: grpc.status.INTERNAL,
          message: error instanceof Error ? error.message : 'Unknown error'
        })
      })
  }
}
