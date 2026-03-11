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
  type CreateSessionRequest,
  type UpdateSessionRequest,
  type GetSessionRequest,
  type DeleteSessionRequest,
  type DaemonSession as ProtoDaemonSession,
  type DaemonWorkspace as ProtoDaemonWorkspace,
  type WorkspaceInput
} from '../generated/treeterm'
import { getDefaultSocketPath } from '../daemon/socketPath'
import type {
  CreateSessionConfig,
  SessionInfo,
  DaemonWorkspace,
  DaemonSession,
  DaemonTab
} from '../daemon/protocol'

type DataListener = (data: string) => void
type ExitListener = (exitCode: number, signal?: number) => void

export class GrpcDaemonClient {
  private client: TreeTermDaemonClient | null = null
  private stream: grpc.ClientDuplexStream<PtyInput, PtyOutput> | null = null
  private connected: boolean = false
  private dataListeners: Map<string, Set<DataListener>> = new Map()
  private exitListeners: Map<string, Set<ExitListener>> = new Map()
  private clientId: string = `client-${Date.now()}`

  constructor(private socketPath: string = getDefaultSocketPath()) {}

  async connect(): Promise<void> {
    if (this.connected) {
      return
    }

    return new Promise((resolve, reject) => {
      const socketUri = `unix://${this.socketPath}`
      const credentials = grpc.credentials.createInsecure()

      this.client = new TreeTermDaemonClient(socketUri, credentials)

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
    })

    this.stream.on('end', () => {
      console.log('[grpcDaemonClient] stream ended')
      this.connected = false
      this.stream = null
    })
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

  async createSession(workspaces: Omit<DaemonWorkspace, 'createdAt' | 'lastActivity' | 'attachedClients'>[]): Promise<DaemonSession> {
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

  async updateSession(sessionId: string, workspaces: Omit<DaemonWorkspace, 'createdAt' | 'lastActivity' | 'attachedClients'>[]): Promise<DaemonSession> {
    if (!this.client) {
      throw new Error('Not connected to daemon')
    }

    return new Promise((resolve, reject) => {
      const request: UpdateSessionRequest = {
        sessionId,
        workspaces: this.convertToProtoWorkspaceInputs(workspaces)
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

  async listSessions(): Promise<DaemonSession[]> {
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

  async getSession(sessionId: string): Promise<DaemonSession | null> {
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

  // Git Operations

  async getGitInfo(dirPath: string): Promise<{ isRepo: boolean; branch: string | null; rootPath: string | null }> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.getGitInfo({ dirPath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve({
          isRepo: response.isRepo,
          branch: response.branch ?? null,
          rootPath: response.rootPath ?? null
        })
        else reject(new Error('No response from server'))
      })
    })
  }

  async createWorktree(repoPath: string, worktreeName: string, baseBranch?: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.createWorktree({ repoPath, worktreeName, baseBranch }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async removeWorktree(repoPath: string, worktreePath: string, deleteBranch: boolean = false): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.removeWorktree({ repoPath, worktreePath, deleteBranch }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async listWorktrees(repoPath: string): Promise<any[]> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.listWorktrees({ repoPath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response.worktrees)
        else resolve([])
      })
    })
  }

  async getChildWorktrees(repoPath: string, parentBranch: string | null): Promise<any[]> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.getChildWorktrees({ repoPath, parentBranch: parentBranch ?? undefined }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response.worktrees)
        else resolve([])
      })
    })
  }

  async getDiff(worktreePath: string, parentBranch: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.getDiff({ worktreePath, parentBranch }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async getFileDiff(worktreePath: string, parentBranch: string, filePath: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.getFileDiff({ worktreePath, parentBranch, filePath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async getDiffAgainstHead(worktreePath: string, parentBranch: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.getDiffAgainstHead({ worktreePath, parentBranch }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async getFileDiffAgainstHead(worktreePath: string, parentBranch: string, filePath: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.getFileDiffAgainstHead({ worktreePath, parentBranch, filePath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async mergeWorktree(mainRepoPath: string, worktreeBranch: string, targetBranch: string, squash: boolean = false): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.mergeWorktree({ mainRepoPath, worktreeBranch, targetBranch, squash }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async hasUncommittedChanges(repoPath: string): Promise<boolean> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.hasUncommittedChanges({ repoPath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response.hasChanges)
        else resolve(false)
      })
    })
  }

  async commitAll(repoPath: string, message: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.commitAll({ repoPath, message }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async deleteBranch(repoPath: string, branchName: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.deleteBranch({ repoPath, branchName }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async getUncommittedChanges(repoPath: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.getUncommittedChanges({ repoPath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async getUncommittedFileDiff(repoPath: string, filePath: string, staged: boolean): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.getUncommittedFileDiff({ repoPath, filePath, staged }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async stageFile(repoPath: string, filePath: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.stageFile({ repoPath, filePath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async unstageFile(repoPath: string, filePath: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.unstageFile({ repoPath, filePath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async stageAll(repoPath: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.stageAll({ repoPath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async unstageAll(repoPath: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.unstageAll({ repoPath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async commitStaged(repoPath: string, message: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.commitStaged({ repoPath, message }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async checkMergeConflicts(repoPath: string, sourceBranch: string, targetBranch: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.checkMergeConflicts({ repoPath, sourceBranch, targetBranch }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async getFileContentsForDiff(worktreePath: string, parentBranch: string, filePath: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.getFileContentsForDiff({ worktreePath, parentBranch, filePath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async getFileContentsForDiffAgainstHead(worktreePath: string, parentBranch: string, filePath: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.getFileContentsForDiffAgainstHead({ worktreePath, parentBranch, filePath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async getUncommittedFileContentsForDiff(repoPath: string, filePath: string, staged: boolean): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.getUncommittedFileContentsForDiff({ repoPath, filePath, staged }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async listLocalBranches(repoPath: string): Promise<string[]> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.listLocalBranches({ repoPath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response.branches)
        else resolve([])
      })
    })
  }

  async listRemoteBranches(repoPath: string): Promise<string[]> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.listRemoteBranches({ repoPath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response.branches)
        else resolve([])
      })
    })
  }

  async getBranchesInWorktrees(repoPath: string): Promise<string[]> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.getBranchesInWorktrees({ repoPath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response.branches)
        else resolve([])
      })
    })
  }

  async createWorktreeFromBranch(repoPath: string, branch: string, worktreeName: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.createWorktreeFromBranch({ repoPath, branch, worktreeName }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async createWorktreeFromRemote(repoPath: string, remoteBranch: string, worktreeName: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.createWorktreeFromRemote({ repoPath, remoteBranch, worktreeName }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async getHeadCommitHash(repoPath: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.getHeadCommitHash({ repoPath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  // Reviews Operations

  async loadReviews(worktreePath: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.loadReviews({ worktreePath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async saveReviews(worktreePath: string, reviews: any): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.saveReviews({ worktreePath, reviews }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async addReviewComment(worktreePath: string, comment: any): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.addReviewComment({
        worktreePath,
        filePath: comment.filePath,
        lineNumber: comment.lineNumber,
        text: comment.text,
        commitHash: comment.commitHash,
        isOutdated: comment.isOutdated,
        side: comment.side
      }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async deleteReviewComment(worktreePath: string, commentId: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.deleteReviewComment({ worktreePath, commentId }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async updateOutdatedReviews(worktreePath: string, currentCommitHash: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.updateOutdatedReviews({ worktreePath, currentCommitHash }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
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
      this.client!.readFile({ workspacePath, filePath }, (error, response) => {
        if (error) reject(new Error(error.message))
        else if (response) resolve(response)
        else reject(new Error('No response from server'))
      })
    })
  }

  async writeFile(workspacePath: string, filePath: string, content: string): Promise<any> {
    if (!this.client) throw new Error('Not connected to daemon')
    return new Promise((resolve, reject) => {
      this.client!.writeFile({ workspacePath, filePath, content }, (error, response) => {
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
    workspaces: Omit<DaemonWorkspace, 'createdAt' | 'lastActivity' | 'attachedClients'>[]
  ): WorkspaceInput[] {
    return workspaces.map(w => ({
      path: w.path,
      name: w.name,
      parentPath: w.parentPath || undefined,
      status: w.status,
      isGitRepo: w.isGitRepo,
      gitBranch: w.gitBranch || undefined,
      gitRootPath: w.gitRootPath || undefined,
      isWorktree: w.isWorktree,
      isDetached: w.isDetached,
      tabs: w.tabs.map(t => ({
        id: t.id,
        applicationId: t.applicationId,
        title: t.title,
        state: Buffer.from(JSON.stringify(t.state), 'utf-8')
      })),
      activeTabId: w.activeTabId || undefined
    }))
  }

  private convertFromProtoSession(protoSession: ProtoDaemonSession): DaemonSession {
    return {
      id: protoSession.id,
      workspaces: protoSession.workspaces.map(w => this.convertFromProtoWorkspace(w)),
      createdAt: protoSession.createdAt,
      lastActivity: protoSession.lastActivity,
      attachedClients: protoSession.attachedClients
    }
  }

  private convertFromProtoWorkspace(protoWorkspace: ProtoDaemonWorkspace): DaemonWorkspace {
    return {
      path: protoWorkspace.path,
      name: protoWorkspace.name,
      parentPath: protoWorkspace.parentPath || null,
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

    console.log('[grpcDaemonClient] spawning daemon at', daemonPath)

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
