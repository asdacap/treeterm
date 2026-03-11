/**
 * gRPC server for daemon-client communication
 */

import * as grpc from '@grpc/grpc-js'
import * as fs from 'fs'
import * as path from 'path'
import type { DaemonPtyManager } from './ptyManager'
import type { SessionStore } from './sessionStore'
import { createModuleLogger } from './logger'
import { getDefaultSocketPath } from './socketPath'
import * as git from './git'
import * as filesystem from './filesystem'
import * as reviews from './reviews'
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
  type DaemonSession as ProtoDaemonSession,
  type WorkspaceInput,
  type PtySessionInfo,
  type GitInfoRequest,
  type GitInfoResponse,
  type CreateWorktreeRequest,
  type CreateWorktreeResponse,
  type RemoveWorktreeRequest,
  type MergeWorktreeResponse,
  type ListWorktreesRequest,
  type ListWorktreesResponse,
  type GetChildWorktreesRequest,
  type GetChildWorktreesResponse,
  type GetDiffRequest,
  type GetDiffResponse,
  type GetFileDiffRequest,
  type GetFileDiffResponse,
  type MergeWorktreeRequest,
  type HasUncommittedChangesRequest,
  type HasUncommittedChangesResponse,
  type CommitAllRequest,
  type CommitAllResponse,
  type DeleteBranchRequest,
  type DeleteBranchResponse,
  type GetUncommittedChangesRequest,
  type GetUncommittedChangesResponse,
  type GetUncommittedFileDiffRequest,
  type StageFileRequest,
  type StageFileResponse,
  type StageAllRequest,
  type StageAllResponse,
  type CommitStagedRequest,
  type CommitStagedResponse,
  type CheckMergeConflictsRequest,
  type CheckMergeConflictsResponse,
  type GetFileContentsForDiffRequest,
  type GetFileContentsForDiffResponse,
  type GetUncommittedFileContentsForDiffRequest,
  type ListLocalBranchesRequest,
  type ListLocalBranchesResponse,
  type ListRemoteBranchesRequest,
  type ListRemoteBranchesResponse,
  type GetBranchesInWorktreesRequest,
  type GetBranchesInWorktreesResponse,
  type CreateWorktreeFromBranchRequest,
  type CreateWorktreeFromRemoteRequest,
  type GetHeadCommitHashRequest,
  type GetHeadCommitHashResponse,
  type LoadReviewsRequest,
  type LoadReviewsResponse,
  type SaveReviewsRequest,
  type SaveReviewsResponse,
  type AddReviewCommentRequest,
  type AddReviewCommentResponse,
  type DeleteReviewCommentRequest,
  type DeleteReviewCommentResponse,
  type UpdateOutdatedReviewsRequest,
  type UpdateOutdatedReviewsResponse,
  type ReadDirectoryRequest,
  type ReadDirectoryResponse,
  type ReadFileRequest,
  type ReadFileResponse,
  type WriteFileRequest,
  type WriteFileResponse
} from '../generated/treeterm'

const log = createModuleLogger('grpcServer')

export { getDefaultSocketPath }

// Track connected clients and their streams
interface ClientStream {
  clientId: string
  stream: grpc.ServerDuplexStream<PtyInput, PtyOutput>
  attachedSessions: Set<string>
}

export class GrpcServer {
  private server: grpc.Server
  private ptyManager: DaemonPtyManager
  private sessionStore: SessionStore
  private clientStreams: Map<string, ClientStream> = new Map()
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

    this.server = new grpc.Server()
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
      createSession: this.handleCreateSession.bind(this),
      updateSession: this.handleUpdateSession.bind(this),
      getSession: this.handleGetSession.bind(this),
      deleteSession: this.handleDeleteSession.bind(this),
      listSessions: this.handleListSessions.bind(this),
      shutdown: this.handleShutdown.bind(this),
      // Git operations
      getGitInfo: this.handleGetGitInfo.bind(this),
      createWorktree: this.handleCreateWorktree.bind(this),
      removeWorktree: this.handleRemoveWorktree.bind(this),
      listWorktrees: this.handleListWorktrees.bind(this),
      getChildWorktrees: this.handleGetChildWorktrees.bind(this),
      getDiff: this.handleGetDiff.bind(this),
      getFileDiff: this.handleGetFileDiff.bind(this),
      getDiffAgainstHead: this.handleGetDiffAgainstHead.bind(this),
      getFileDiffAgainstHead: this.handleGetFileDiffAgainstHead.bind(this),
      mergeWorktree: this.handleMergeWorktree.bind(this),
      hasUncommittedChanges: this.handleHasUncommittedChanges.bind(this),
      commitAll: this.handleCommitAll.bind(this),
      deleteBranch: this.handleDeleteBranch.bind(this),
      getUncommittedChanges: this.handleGetUncommittedChanges.bind(this),
      getUncommittedFileDiff: this.handleGetUncommittedFileDiff.bind(this),
      stageFile: this.handleStageFile.bind(this),
      unstageFile: this.handleUnstageFile.bind(this),
      stageAll: this.handleStageAll.bind(this),
      unstageAll: this.handleUnstageAll.bind(this),
      commitStaged: this.handleCommitStaged.bind(this),
      checkMergeConflicts: this.handleCheckMergeConflicts.bind(this),
      getFileContentsForDiff: this.handleGetFileContentsForDiff.bind(this),
      getFileContentsForDiffAgainstHead: this.handleGetFileContentsForDiffAgainstHead.bind(this),
      getUncommittedFileContentsForDiff: this.handleGetUncommittedFileContentsForDiff.bind(this),
      listLocalBranches: this.handleListLocalBranches.bind(this),
      listRemoteBranches: this.handleListRemoteBranches.bind(this),
      getBranchesInWorktrees: this.handleGetBranchesInWorktrees.bind(this),
      createWorktreeFromBranch: this.handleCreateWorktreeFromBranch.bind(this),
      createWorktreeFromRemote: this.handleCreateWorktreeFromRemote.bind(this),
      getHeadCommitHash: this.handleGetHeadCommitHash.bind(this),
      // Reviews operations
      loadReviews: this.handleLoadReviews.bind(this),
      saveReviews: this.handleSaveReviews.bind(this),
      addReviewComment: this.handleAddReviewComment.bind(this),
      deleteReviewComment: this.handleDeleteReviewComment.bind(this),
      updateOutdatedReviews: this.handleUpdateOutdatedReviews.bind(this),
      // Filesystem operations
      readDirectory: this.handleReadDirectory.bind(this),
      readFile: this.handleReadFile.bind(this),
      writeFile: this.handleWriteFile.bind(this)
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
      log.debug({ sessionId }, 'attachPty called')

      // Get client ID from metadata
      const clientId = this.getClientId(call.metadata)

      const result = this.ptyManager.attach(sessionId, clientId)
      callback(null, { scrollback: result.scrollback })
    } catch (error) {
      log.error({ err: error }, 'attachPty error')
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

      log.debug({ sessionId, clientId }, 'detachPty called')
      this.ptyManager.detach(sessionId, clientId)
      callback(null, {})
    } catch (error) {
      log.error({ err: error }, 'detachPty error')
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

  // Workspace Session Handlers

  private handleCreateSession(
    call: grpc.ServerUnaryCall<CreateSessionRequest, ProtoDaemonSession>,
    callback: grpc.sendUnaryData<ProtoDaemonSession>
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
    call: grpc.ServerUnaryCall<UpdateSessionRequest, ProtoDaemonSession>,
    callback: grpc.sendUnaryData<ProtoDaemonSession>
  ): void {
    try {
      const clientId = this.getClientId(call.metadata)
      const { sessionId, workspaces } = call.request
      log.debug({ clientId, sessionId }, 'updateSession called')

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
    } catch (error) {
      log.error({ err: error }, 'updateSession error')
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private handleGetSession(
    call: grpc.ServerUnaryCall<GetSessionRequest, ProtoDaemonSession>,
    callback: grpc.sendUnaryData<ProtoDaemonSession>
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

  private convertWorkspaceInputs(inputs: WorkspaceInput[]): any[] {
    return inputs.map(input => ({
      path: input.path,
      name: input.name,
      parentPath: input.parentPath || null,
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
      activeTabId: input.activeTabId || null
    }))
  }

  private convertToProtoSession(session: any): ProtoDaemonSession {
    return {
      id: session.id,
      workspaces: session.workspaces.map((w: any) => ({
        path: w.path,
        name: w.name,
        parentPath: w.parentPath,
        status: w.status,
        isGitRepo: w.isGitRepo,
        gitBranch: w.gitBranch,
        gitRootPath: w.gitRootPath,
        isWorktree: w.isWorktree,
        isDetached: w.isDetached,
        tabs: w.tabs.map((t: any) => ({
          id: t.id,
          applicationId: t.applicationId,
          title: t.title,
          state: Buffer.from(JSON.stringify(t.state), 'utf-8')
        })),
        activeTabId: w.activeTabId,
        createdAt: w.createdAt,
        lastActivity: w.lastActivity,
        attachedClients: w.attachedClients
      })),
      createdAt: session.createdAt,
      lastActivity: session.lastActivity,
      attachedClients: session.attachedClients
    }
  }

  // Git Operation Handlers

  private async handleGetGitInfo(
    call: grpc.ServerUnaryCall<GitInfoRequest, GitInfoResponse>,
    callback: grpc.sendUnaryData<GitInfoResponse>
  ): Promise<void> {
    try {
      const info = await git.getGitInfo(call.request.dirPath)
      callback(null, {
        isRepo: info.isRepo,
        branch: info.branch ?? undefined,
        rootPath: info.rootPath ?? undefined
      })
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleCreateWorktree(
    call: grpc.ServerUnaryCall<CreateWorktreeRequest, CreateWorktreeResponse>,
    callback: grpc.sendUnaryData<CreateWorktreeResponse>
  ): Promise<void> {
    try {
      const result = await git.createWorktree(
        call.request.repoPath,
        call.request.worktreeName,
        call.request.baseBranch
      )
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleRemoveWorktree(
    call: grpc.ServerUnaryCall<RemoveWorktreeRequest, MergeWorktreeResponse>,
    callback: grpc.sendUnaryData<MergeWorktreeResponse>
  ): Promise<void> {
    try {
      const result = await git.removeWorktree(
        call.request.repoPath,
        call.request.worktreePath,
        call.request.deleteBranch
      )
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleListWorktrees(
    call: grpc.ServerUnaryCall<ListWorktreesRequest, ListWorktreesResponse>,
    callback: grpc.sendUnaryData<ListWorktreesResponse>
  ): Promise<void> {
    try {
      const worktrees = await git.listWorktrees(call.request.repoPath)
      callback(null, { worktrees })
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleGetChildWorktrees(
    call: grpc.ServerUnaryCall<GetChildWorktreesRequest, GetChildWorktreesResponse>,
    callback: grpc.sendUnaryData<GetChildWorktreesResponse>
  ): Promise<void> {
    try {
      const worktrees = await git.getChildWorktrees(
        call.request.repoPath,
        call.request.parentBranch ?? null
      )
      callback(null, { worktrees })
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleGetDiff(
    call: grpc.ServerUnaryCall<GetDiffRequest, GetDiffResponse>,
    callback: grpc.sendUnaryData<GetDiffResponse>
  ): Promise<void> {
    try {
      const result = await git.getDiff(call.request.worktreePath, call.request.parentBranch)
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleGetFileDiff(
    call: grpc.ServerUnaryCall<GetFileDiffRequest, GetFileDiffResponse>,
    callback: grpc.sendUnaryData<GetFileDiffResponse>
  ): Promise<void> {
    try {
      const result = await git.getFileDiff(
        call.request.worktreePath,
        call.request.parentBranch,
        call.request.filePath
      )
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleGetDiffAgainstHead(
    call: grpc.ServerUnaryCall<GetDiffRequest, GetDiffResponse>,
    callback: grpc.sendUnaryData<GetDiffResponse>
  ): Promise<void> {
    try {
      const result = await git.getDiffAgainstHead(
        call.request.worktreePath,
        call.request.parentBranch
      )
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleGetFileDiffAgainstHead(
    call: grpc.ServerUnaryCall<GetFileDiffRequest, GetFileDiffResponse>,
    callback: grpc.sendUnaryData<GetFileDiffResponse>
  ): Promise<void> {
    try {
      const result = await git.getFileDiffAgainstHead(
        call.request.worktreePath,
        call.request.parentBranch,
        call.request.filePath
      )
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleMergeWorktree(
    call: grpc.ServerUnaryCall<MergeWorktreeRequest, MergeWorktreeResponse>,
    callback: grpc.sendUnaryData<MergeWorktreeResponse>
  ): Promise<void> {
    try {
      const result = await git.mergeWorktree(
        call.request.mainRepoPath,
        call.request.worktreeBranch,
        call.request.targetBranch,
        call.request.squash
      )
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleHasUncommittedChanges(
    call: grpc.ServerUnaryCall<HasUncommittedChangesRequest, HasUncommittedChangesResponse>,
    callback: grpc.sendUnaryData<HasUncommittedChangesResponse>
  ): Promise<void> {
    try {
      const hasChanges = await git.hasUncommittedChanges(call.request.repoPath)
      callback(null, { hasChanges })
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleCommitAll(
    call: grpc.ServerUnaryCall<CommitAllRequest, CommitAllResponse>,
    callback: grpc.sendUnaryData<CommitAllResponse>
  ): Promise<void> {
    try {
      const result = await git.commitAll(call.request.repoPath, call.request.message)
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleDeleteBranch(
    call: grpc.ServerUnaryCall<DeleteBranchRequest, DeleteBranchResponse>,
    callback: grpc.sendUnaryData<DeleteBranchResponse>
  ): Promise<void> {
    try {
      const result = await git.deleteBranch(call.request.repoPath, call.request.branchName)
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleGetUncommittedChanges(
    call: grpc.ServerUnaryCall<GetUncommittedChangesRequest, GetUncommittedChangesResponse>,
    callback: grpc.sendUnaryData<GetUncommittedChangesResponse>
  ): Promise<void> {
    try {
      const result = await git.getUncommittedChanges(call.request.repoPath)
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleGetUncommittedFileDiff(
    call: grpc.ServerUnaryCall<GetUncommittedFileDiffRequest, GetFileDiffResponse>,
    callback: grpc.sendUnaryData<GetFileDiffResponse>
  ): Promise<void> {
    try {
      const result = await git.getUncommittedFileDiff(
        call.request.repoPath,
        call.request.filePath,
        call.request.staged
      )
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleStageFile(
    call: grpc.ServerUnaryCall<StageFileRequest, StageFileResponse>,
    callback: grpc.sendUnaryData<StageFileResponse>
  ): Promise<void> {
    try {
      const result = await git.stageFile(call.request.repoPath, call.request.filePath)
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleUnstageFile(
    call: grpc.ServerUnaryCall<StageFileRequest, StageFileResponse>,
    callback: grpc.sendUnaryData<StageFileResponse>
  ): Promise<void> {
    try {
      const result = await git.unstageFile(call.request.repoPath, call.request.filePath)
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleStageAll(
    call: grpc.ServerUnaryCall<StageAllRequest, StageAllResponse>,
    callback: grpc.sendUnaryData<StageAllResponse>
  ): Promise<void> {
    try {
      const result = await git.stageAll(call.request.repoPath)
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleUnstageAll(
    call: grpc.ServerUnaryCall<StageAllRequest, StageAllResponse>,
    callback: grpc.sendUnaryData<StageAllResponse>
  ): Promise<void> {
    try {
      const result = await git.unstageAll(call.request.repoPath)
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleCommitStaged(
    call: grpc.ServerUnaryCall<CommitStagedRequest, CommitStagedResponse>,
    callback: grpc.sendUnaryData<CommitStagedResponse>
  ): Promise<void> {
    try {
      const result = await git.commitStaged(call.request.repoPath, call.request.message)
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleCheckMergeConflicts(
    call: grpc.ServerUnaryCall<CheckMergeConflictsRequest, CheckMergeConflictsResponse>,
    callback: grpc.sendUnaryData<CheckMergeConflictsResponse>
  ): Promise<void> {
    try {
      const result = await git.checkMergeConflicts(
        call.request.repoPath,
        call.request.sourceBranch,
        call.request.targetBranch
      )
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleGetFileContentsForDiff(
    call: grpc.ServerUnaryCall<GetFileContentsForDiffRequest, GetFileContentsForDiffResponse>,
    callback: grpc.sendUnaryData<GetFileContentsForDiffResponse>
  ): Promise<void> {
    try {
      const result = await git.getFileContentsForDiff(
        call.request.worktreePath,
        call.request.parentBranch,
        call.request.filePath
      )
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleGetFileContentsForDiffAgainstHead(
    call: grpc.ServerUnaryCall<GetFileContentsForDiffRequest, GetFileContentsForDiffResponse>,
    callback: grpc.sendUnaryData<GetFileContentsForDiffResponse>
  ): Promise<void> {
    try {
      const result = await git.getFileContentsForDiffAgainstHead(
        call.request.worktreePath,
        call.request.parentBranch,
        call.request.filePath
      )
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleGetUncommittedFileContentsForDiff(
    call: grpc.ServerUnaryCall<GetUncommittedFileContentsForDiffRequest, GetFileContentsForDiffResponse>,
    callback: grpc.sendUnaryData<GetFileContentsForDiffResponse>
  ): Promise<void> {
    try {
      const result = await git.getUncommittedFileContentsForDiff(
        call.request.repoPath,
        call.request.filePath,
        call.request.staged
      )
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleListLocalBranches(
    call: grpc.ServerUnaryCall<ListLocalBranchesRequest, ListLocalBranchesResponse>,
    callback: grpc.sendUnaryData<ListLocalBranchesResponse>
  ): Promise<void> {
    try {
      const branches = await git.listLocalBranches(call.request.repoPath)
      callback(null, { branches })
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleListRemoteBranches(
    call: grpc.ServerUnaryCall<ListRemoteBranchesRequest, ListRemoteBranchesResponse>,
    callback: grpc.sendUnaryData<ListRemoteBranchesResponse>
  ): Promise<void> {
    try {
      const branches = await git.listRemoteBranches(call.request.repoPath)
      callback(null, { branches })
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleGetBranchesInWorktrees(
    call: grpc.ServerUnaryCall<GetBranchesInWorktreesRequest, GetBranchesInWorktreesResponse>,
    callback: grpc.sendUnaryData<GetBranchesInWorktreesResponse>
  ): Promise<void> {
    try {
      const branches = await git.getBranchesInWorktrees(call.request.repoPath)
      callback(null, { branches })
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleCreateWorktreeFromBranch(
    call: grpc.ServerUnaryCall<CreateWorktreeFromBranchRequest, CreateWorktreeResponse>,
    callback: grpc.sendUnaryData<CreateWorktreeResponse>
  ): Promise<void> {
    try {
      const result = await git.createWorktreeFromBranch(
        call.request.repoPath,
        call.request.branch,
        call.request.worktreeName
      )
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleCreateWorktreeFromRemote(
    call: grpc.ServerUnaryCall<CreateWorktreeFromRemoteRequest, CreateWorktreeResponse>,
    callback: grpc.sendUnaryData<CreateWorktreeResponse>
  ): Promise<void> {
    try {
      const result = await git.createWorktreeFromRemote(
        call.request.repoPath,
        call.request.remoteBranch,
        call.request.worktreeName
      )
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleGetHeadCommitHash(
    call: grpc.ServerUnaryCall<GetHeadCommitHashRequest, GetHeadCommitHashResponse>,
    callback: grpc.sendUnaryData<GetHeadCommitHashResponse>
  ): Promise<void> {
    try {
      const result = await git.getHeadCommitHash(call.request.repoPath)
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  // Reviews Operation Handlers

  private async handleLoadReviews(
    call: grpc.ServerUnaryCall<LoadReviewsRequest, LoadReviewsResponse>,
    callback: grpc.sendUnaryData<LoadReviewsResponse>
  ): Promise<void> {
    try {
      const reviewsData = reviews.loadReviews(call.request.worktreePath)
      callback(null, { success: true, reviews: reviewsData })
    } catch (error) {
      callback(null, {
        success: false,
        error: error instanceof Error ? error.message : 'Unknown error loading reviews'
      })
    }
  }

  private async handleSaveReviews(
    call: grpc.ServerUnaryCall<SaveReviewsRequest, SaveReviewsResponse>,
    callback: grpc.sendUnaryData<SaveReviewsResponse>
  ): Promise<void> {
    try {
      if (!call.request.reviews) {
        callback(null, { success: false, error: 'Reviews data is required' })
        return
      }
      reviews.saveReviews(call.request.worktreePath, call.request.reviews as any)
      callback(null, { success: true })
    } catch (error) {
      callback(null, {
        success: false,
        error: error instanceof Error ? error.message : 'Unknown error saving reviews'
      })
    }
  }

  private async handleAddReviewComment(
    call: grpc.ServerUnaryCall<AddReviewCommentRequest, AddReviewCommentResponse>,
    callback: grpc.sendUnaryData<AddReviewCommentResponse>
  ): Promise<void> {
    try {
      const comment = reviews.addComment(call.request.worktreePath, {
        filePath: call.request.filePath,
        lineNumber: call.request.lineNumber,
        text: call.request.text,
        commitHash: call.request.commitHash,
        isOutdated: call.request.isOutdated,
        side: call.request.side as 'original' | 'modified'
      })
      callback(null, { success: true, comment })
    } catch (error) {
      callback(null, {
        success: false,
        error: error instanceof Error ? error.message : 'Unknown error adding comment'
      })
    }
  }

  private async handleDeleteReviewComment(
    call: grpc.ServerUnaryCall<DeleteReviewCommentRequest, DeleteReviewCommentResponse>,
    callback: grpc.sendUnaryData<DeleteReviewCommentResponse>
  ): Promise<void> {
    try {
      const success = reviews.deleteComment(call.request.worktreePath, call.request.commentId)
      if (success) {
        callback(null, { success: true })
      } else {
        callback(null, { success: false, error: 'Comment not found' })
      }
    } catch (error) {
      callback(null, {
        success: false,
        error: error instanceof Error ? error.message : 'Unknown error deleting comment'
      })
    }
  }

  private async handleUpdateOutdatedReviews(
    call: grpc.ServerUnaryCall<UpdateOutdatedReviewsRequest, UpdateOutdatedReviewsResponse>,
    callback: grpc.sendUnaryData<UpdateOutdatedReviewsResponse>
  ): Promise<void> {
    try {
      const reviewsData = reviews.updateOutdatedComments(
        call.request.worktreePath,
        call.request.currentCommitHash
      )
      callback(null, { success: true, reviews: reviewsData })
    } catch (error) {
      callback(null, {
        success: false,
        error: error instanceof Error ? error.message : 'Unknown error updating outdated reviews'
      })
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
    call: grpc.ServerUnaryCall<ReadFileRequest, ReadFileResponse>,
    callback: grpc.sendUnaryData<ReadFileResponse>
  ): Promise<void> {
    try {
      const result = await filesystem.readFile(
        call.request.workspacePath,
        call.request.filePath
      )
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }

  private async handleWriteFile(
    call: grpc.ServerUnaryCall<WriteFileRequest, WriteFileResponse>,
    callback: grpc.sendUnaryData<WriteFileResponse>
  ): Promise<void> {
    try {
      const result = await filesystem.writeFile(
        call.request.workspacePath,
        call.request.filePath,
        call.request.content
      )
      callback(null, result)
    } catch (error) {
      callback({
        code: grpc.status.INTERNAL,
        message: error instanceof Error ? error.message : 'Unknown error'
      })
    }
  }
}
