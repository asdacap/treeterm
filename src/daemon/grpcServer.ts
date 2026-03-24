/**
 * gRPC server for daemon-client communication
 */

import * as grpc from '@grpc/grpc-js'
import * as fs from 'fs'
import * as path from 'path'
import { z } from 'zod'
import type { DaemonPtyManager } from './ptyManager'
import type { SessionStore } from './sessionStore'
import type { Session, Workspace, AppState } from '../shared/types'
import { createModuleLogger } from './logger'
import { getDefaultSocketPath } from './socketPath'
import * as filesystem from './filesystem'
import { execManager } from './execManager'
import {
  TreeTermDaemonService,
  type CreatePtyRequest,
  type CreatePtyResponse,
  type KillPtyRequest,
  type ListPtySessionsResponse,
  type PtyInput,
  type PtyOutput,
  type CreateSessionRequest,
  type UpdateSessionRequest,
  type DeleteSessionRequest,
  type ListSessionsResponse,
  type GetDefaultSessionIdResponse,
  type Empty,
  type Session as ProtoSession,
  type Workspace as ProtoWorkspaceInput,
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

// Zod schemas for runtime validation of JSON-encoded proto bytes fields
const appStateJsonSchema = z.record(z.string(), z.unknown())
const metadataSchema = z.record(z.string(), z.string())

function parseJsonBuffer(buf: Buffer, schema: z.ZodTypeAny, fieldName: string): unknown {
  const raw = JSON.parse(buf.toString('utf-8'))
  const result = schema.safeParse(raw)
  if (!result.success) {
    log.warn({ fieldName, error: result.error.message }, 'JSON buffer failed validation, using raw value')
    return raw
  }
  return result.data
}

export { getDefaultSocketPath }

interface SessionWatcher {
  listenerId: string
  sessionId: string
  stream: grpc.ServerWritableStream<SessionWatchRequest, SessionWatchEvent>
}

export class GrpcServer {
  private server: grpc.Server
  private ptyManager: DaemonPtyManager
  private sessionStore: SessionStore
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

    })
  }

  stop(): void {
    log.info('stopping server')

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
      killPty: this.handleKillPty.bind(this),
      listPtySessions: this.handleListPtySessions.bind(this),
      ptyStream: this.handlePtyStream.bind(this),
      execStream: this.handleExecStream.bind(this),
      createSession: this.handleCreateSession.bind(this),
      updateSession: this.handleUpdateSession.bind(this),
      deleteSession: this.handleDeleteSession.bind(this),
      listSessions: this.handleListSessions.bind(this),
      getDefaultSessionId: this.handleGetDefaultSessionId.bind(this),
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
        lastActivity: s.lastActivity
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

  // PTY Streaming Handler (Bidirectional, per-session)

  private handlePtyStream(
    call: grpc.ServerDuplexStream<PtyInput, PtyOutput>
  ): void {
    let sessionId: string | null = null
    let dataUnsubscribe: (() => void) | null = null
    let exitUnsubscribe: (() => void) | null = null
    let resizeUnsubscribe: (() => void) | null = null

    const cleanup = (): void => {
      dataUnsubscribe?.()
      exitUnsubscribe?.()
      resizeUnsubscribe?.()
      dataUnsubscribe = null
      exitUnsubscribe = null
      resizeUnsubscribe = null
    }

    call.on('data', (input: PtyInput) => {
      try {
        if (input.start && !sessionId) {
          sessionId = input.start.sessionId
          log.info({ sessionId }, 'ptyStream started for session')

          // Send scrollback and check if already exited
          const result = this.ptyManager.attach(sessionId)
          for (const line of result.scrollback) {
            call.write({ data: { data: Buffer.from(line, 'utf-8') } })
          }

          if (result.exitCode !== undefined) {
            call.write({ exit: { exitCode: result.exitCode } })
            call.end()
            return
          }

          // Subscribe to live data/exit for this session
          dataUnsubscribe = this.ptyManager.onSessionData(sessionId, (data) => {
            call.write({ data: { data: Buffer.from(data, 'utf-8') } })
          })

          exitUnsubscribe = this.ptyManager.onSessionExit(sessionId, (exitCode, signal) => {
            call.write({ exit: { exitCode, signal } })
            call.end()
            cleanup()
          })

          resizeUnsubscribe = this.ptyManager.onSessionResize(sessionId, (cols, rows) => {
            call.write({ resize: { cols, rows } })
          })
        } else if (sessionId && input.write) {
          this.ptyManager.write(sessionId, input.write.data.toString('utf-8'))
        } else if (sessionId && input.resize) {
          this.ptyManager.resize(sessionId, input.resize.cols, input.resize.rows)
        }
      } catch (error) {
        log.error({ err: error, sessionId }, 'error processing pty stream input')
        call.destroy(error instanceof Error ? error : new Error(String(error)))
      }
    })

    call.on('end', () => {
      log.info({ sessionId }, 'pty stream ended')
      cleanup()
    })

    call.on('error', (error) => {
      log.error({ err: error, sessionId }, 'pty stream error')
      cleanup()
    })
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

      // Convert proto Workspace to internal format (timestamps are optional on input)
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

      if (!senderId) {
        callback({
          code: grpc.status.INVALID_ARGUMENT,
          message: 'senderId is required for session updates'
        })
        return
      }

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
      this.broadcastSessionUpdate(sessionId, protoSession, senderId)
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

    if (!listenerId) {
      call.destroy(new Error('listenerId is required for session watch'))
      return
    }

    log.info({ sessionId, listenerId }, 'sessionWatch registered')

    // Emit initial session state as first event
    const session = this.sessionStore.getSession(sessionId)
    if (!session) {
      call.destroy(Object.assign(new Error(`Session not found: ${sessionId}`), { code: grpc.status.NOT_FOUND }))
      return
    }

    const protoSession = this.convertToProtoSession(session)
    call.write({
      sessionId,
      session: protoSession,
      senderId: ''
    })

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

  private handleGetDefaultSessionId(
    call: grpc.ServerUnaryCall<Empty, GetDefaultSessionIdResponse>,
    callback: grpc.sendUnaryData<GetDefaultSessionIdResponse>
  ): void {
    try {
      // Get or create the default session for this client
      const clientId = call.metadata.get('client-id')[0]?.toString() || 'unknown'
      const session = this.sessionStore.getOrCreateDefaultSession(clientId)
      log.debug({ sessionId: session.id, clientId }, 'getDefaultSessionId called')

      callback(null, { sessionId: session.id })
    } catch (error) {
      log.error({ err: error }, 'getDefaultSessionId error')
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

  private convertWorkspaceInputs(inputs: ProtoWorkspaceInput[]): Omit<Workspace, 'createdAt' | 'lastActivity'>[] {
    return inputs.map(input => {
      const appStates: Record<string, AppState> = {}
      for (const [key, value] of Object.entries(input.appStates || {})) {
        appStates[key] = {
          applicationId: value.applicationId,
          title: value.title,
          state: parseJsonBuffer(value.state, appStateJsonSchema, `appStates[${key}].state`)
        }
      }
      return {
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
        appStates,
        activeTabId: input.activeTabId || null,
        metadata: input.metadata?.length
          ? parseJsonBuffer(input.metadata, metadataSchema, 'metadata') as Record<string, string>
          : {}
      }
    })
  }

  private convertToProtoSession(session: Session): ProtoSession {
    return {
      id: session.id,
      workspaces: session.workspaces.map((w: Workspace) => {
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
          children: w.children || [],
          status: w.status,
          isGitRepo: w.isGitRepo,
          gitBranch: w.gitBranch || undefined,
          gitRootPath: w.gitRootPath || undefined,
          isWorktree: w.isWorktree,
          isDetached: w.isDetached,
          appStates: protoAppStates,
          activeTabId: w.activeTabId || undefined,
          metadata: Buffer.from(JSON.stringify(w.metadata ?? {}), 'utf-8'),
          createdAt: w.createdAt,
          lastActivity: w.lastActivity
        }
      }),
      createdAt: session.createdAt,
      lastActivity: session.lastActivity
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
