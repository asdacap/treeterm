/**
 * Git Client for Main Process
 * 
 * This module provides high-level git operations by executing git commands
 * through the daemon's ExecStream RPC. All git logic lives here in the main
 * process rather than in the daemon.
 */

import { GrpcDaemonClient } from './grpcClient'
import type { ExecInput, ExecOutput } from '../generated/treeterm'
import { readFile } from 'fs/promises'
import { join } from 'path'

export interface GitStatusEntry {
  path: string
  status: 'modified' | 'added' | 'deleted' | 'renamed' | 'untracked'
  staged: boolean
  originalPath?: string
}

export interface GitDiffFile {
  path: string
  status: 'added' | 'modified' | 'deleted' | 'renamed'
  additions: number
  deletions: number
}

export interface GitDiffResult {
  files: GitDiffFile[]
  totalAdditions: number
  totalDeletions: number
  baseBranch: string
  headBranch: string
}

export interface GitInfo {
  isRepo: boolean
  branch: string | null
  rootPath: string | null
}

export interface WorktreeInfo {
  path: string
  branch: string
}

export interface ChildWorktreeInfo extends WorktreeInfo {
  displayName: string
}

export class GitClient {
  constructor(private daemonClient: GrpcDaemonClient) {}

  /**
   * Execute a git command and return the result
   */
  private async exec(
    cwd: string,
    args: string[],
    options: { timeoutMs?: number; env?: Record<string, string> } = {}
  ): Promise<{ exitCode: number; stdout: string; stderr: string }> {
    return new Promise((resolve, reject) => {
      const stdout: Buffer[] = []
      const stderr: Buffer[] = []
      let resultReceived = false

      try {
        const stream = this.daemonClient.execStream()

        // Send start command
        const startInput: ExecInput = {
          start: {
            cwd,
            command: 'git',
            args,
            env: options.env || {},
            timeoutMs: options.timeoutMs || 30000
          }
        }
        stream.write(startInput)
        
        // Close stdin immediately for git commands (no input needed)
        stream.end()

        // Handle output
        stream.on('data', (output: ExecOutput) => {
          if (output.stdout) {
            stdout.push(output.stdout.data)
          } else if (output.stderr) {
            stderr.push(output.stderr.data)
          } else if (output.result) {
            resultReceived = true
            resolve({
              exitCode: output.result.exitCode,
              stdout: Buffer.concat(stdout).toString('utf-8'),
              stderr: Buffer.concat(stderr).toString('utf-8')
            })
          }
        })

        stream.on('error', (error) => {
          reject(new Error(`Exec stream error: ${error.message}`))
        })

        stream.on('end', () => {
          if (!resultReceived) {
            // Stream ended without result - treat as error
            resolve({
              exitCode: -1,
              stdout: Buffer.concat(stdout).toString('utf-8'),
              stderr: Buffer.concat(stderr).toString('utf-8') || 'Stream ended unexpectedly'
            })
          }
        })
      } catch (error) {
        reject(new Error(`Failed to start exec: ${error instanceof Error ? error.message : String(error)}`))
      }
    })
  }

  /**
   * Check if a directory is a git repository and get basic info
   */
  async getGitInfo(dirPath: string): Promise<GitInfo> {
    try {
      const result = await this.exec(dirPath, ['rev-parse', '--is-inside-work-tree'])
      
      if (result.exitCode !== 0) {
        return { isRepo: false, branch: null, rootPath: null }
      }

      const [branchResult, rootResult] = await Promise.all([
        this.exec(dirPath, ['rev-parse', '--abbrev-ref', 'HEAD']),
        this.exec(dirPath, ['rev-parse', '--show-toplevel'])
      ])

      return {
        isRepo: true,
        branch: branchResult.stdout.trim() || null,
        rootPath: rootResult.stdout.trim() || null
      }
    } catch (error) {
      console.warn('[git] getGitInfo failed, treating as non-repo:', error)
      return { isRepo: false, branch: null, rootPath: null }
    }
  }

  /**
   * Parse git status --porcelain output
   */
  private parseStatus(output: string): GitStatusEntry[] {
    const entries: GitStatusEntry[] = []
    const lines = output.split('\n').filter(line => line.length > 0)

    for (const line of lines) {
      if (line.length < 3) continue

      const stagedChar = line[0]
      const unstagedChar = line[1]
      const afterStatus = line.slice(2).trim()
      
      // Handle renames: "R  new.txt -> old.txt"
      let path = afterStatus
      let originalPath: string | undefined
      
      if (stagedChar === 'R' || unstagedChar === 'R') {
        const arrowIndex = afterStatus.indexOf(' -> ')
        if (arrowIndex > -1) {
          originalPath = afterStatus.slice(0, arrowIndex)
          path = afterStatus.slice(arrowIndex + 4)
        }
      }

      // Helper to determine status from status char
      const getStatusFromChar = (c: string): GitStatusEntry['status'] => {
        if (c === 'A') return 'added'
        if (c === 'D') return 'deleted'
        if (c === 'R') return 'renamed'
        return 'modified'
      }

      // Untracked files
      if (stagedChar === '?' && unstagedChar === '?') {
        entries.push({
          path,
          status: 'untracked',
          staged: false,
          originalPath
        })
        continue
      }

      // Create entry for staged changes if any
      if (stagedChar !== ' ' && stagedChar !== '?') {
        entries.push({
          path,
          status: getStatusFromChar(stagedChar),
          staged: true,
          originalPath
        })
      }

      // Create entry forunstaged changes if any
      if (unstagedChar !== ' ' && unstagedChar !== '?') {
        entries.push({
          path,
          status: getStatusFromChar(unstagedChar),
          staged: false,
          originalPath
        })
      }
    }

    return entries
  }

  /**
   * Get repository status
   */
  async getStatus(dirPath: string): Promise<GitStatusEntry[]> {
    const result = await this.exec(dirPath, ['status', '--porcelain'])
    
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }

    return this.parseStatus(result.stdout)
  }

  /**
   * Check if there are uncommitted changes
   */
  async hasUncommittedChanges(dirPath: string): Promise<boolean> {
    const result = await this.exec(dirPath, ['status', '--porcelain'])
    return result.exitCode === 0 && result.stdout.trim().length > 0
  }

  /**
   * Stage a single file
   */
  async stageFile(dirPath: string, filePath: string): Promise<void> {
    const result = await this.exec(dirPath, ['add', filePath])
    
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }
  }

  /**
   * Unstage a single file
   */
  async unstageFile(dirPath: string, filePath: string): Promise<void> {
    const result = await this.exec(dirPath, ['reset', 'HEAD', filePath])
    
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }
  }

  /**
   * Stage all changes
   */
  async stageAll(dirPath: string): Promise<void> {
    const result = await this.exec(dirPath, ['add', '.'])
    
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }
  }

  /**
   * Unstage all changes
   */
  async unstageAll(dirPath: string): Promise<void> {
    const result = await this.exec(dirPath, ['reset', 'HEAD'])
    
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }
  }

  /**
   * Commit staged changes
   */
  async commitStaged(dirPath: string, message: string): Promise<string> {
    const result = await this.exec(dirPath, ['commit', '-m', message])
    
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }

    // Parse commit hash from output like "[main abc1234] message"
    const match = result.stdout.match(/\[.+\s([a-f0-9]+)\]/)
    return match?.[1] || ''
  }

  /**
   * Stage all and commit
   */
  async commitAll(dirPath: string, message: string): Promise<string> {
    const result = await this.exec(dirPath, ['commit', '-am', message])
    
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }

    const match = result.stdout.match(/\[.+\s([a-f0-9]+)\]/)
    return match?.[1] || ''
  }

  /**
   * Get list of local branches
   */
  async listLocalBranches(dirPath: string): Promise<string[]> {
    const result = await this.exec(dirPath, ['branch', '--format=%(refname:short)'])
    
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }

    return result.stdout.trim().split('\n').filter(Boolean)
  }

  /**
   * Get list of remote branches
   */
  async listRemoteBranches(dirPath: string): Promise<string[]> {
    const result = await this.exec(dirPath, ['branch', '-r', '--format=%(refname:short)'])
    
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }

    return result.stdout.trim().split('\n').filter(Boolean)
  }

  /**
   * Delete a branch
   */
  async deleteBranch(dirPath: string, branchName: string, force: boolean = false): Promise<void> {
    const args = force ? ['branch', '-D', branchName] : ['branch', '-d', branchName]
    const result = await this.exec(dirPath, args)
    
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }
  }

  /**
   * Get HEAD commit hash
   */
  async getHeadCommitHash(dirPath: string): Promise<string> {
    const result = await this.exec(dirPath, ['rev-parse', 'HEAD'])
    
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }

    return result.stdout.trim()
  }

  /**
   * Get current branch name
   */
  async getCurrentBranch(dirPath: string): Promise<string> {
    const result = await this.exec(dirPath, ['rev-parse', '--abbrev-ref', 'HEAD'])
    
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }

    return result.stdout.trim()
  }

  /**
   * Create a new worktree with a new branch
   */
  async createWorktree(
    repoPath: string,
    worktreeName: string,
    baseBranch?: string
  ): Promise<{ path: string; branch: string }> {
    // Get repo root
    const rootResult = await this.exec(repoPath, ['rev-parse', '--show-toplevel'])
    if (rootResult.exitCode !== 0) {
      throw this.interpretError(rootResult)
    }
    const rootPath = rootResult.stdout.trim()

    // Determine worktree path
    let worktreePath: string
    if (await this.isWorktreesDirInGitignore(rootPath)) {
      const worktreesDir = `${rootPath}/.worktrees`
      worktreePath = `${worktreesDir}/${worktreeName}`
    } else {
      const parentDir = rootPath.substring(0, rootPath.lastIndexOf('/'))
      const repoName = rootPath.substring(rootPath.lastIndexOf('/') + 1)
      worktreePath = `${parentDir}/${repoName}-${worktreeName}`
    }

    // Determine branch name (hierarchical if base has '/')
    let branchName: string
    if (baseBranch && baseBranch.includes('/')) {
      branchName = `${baseBranch}/${worktreeName}`
    } else {
      branchName = worktreeName
    }

    // Create worktree with new branch
    const args = ['worktree', 'add', '-b', branchName, worktreePath]
    if (baseBranch) {
      args.push(baseBranch)
    }

    const result = await this.exec(repoPath, args)
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }

    return { path: worktreePath, branch: branchName }
  }

  /**
   * Create a worktree from an existing local branch
   */
  async createWorktreeFromBranch(
    repoPath: string,
    branch: string,
    worktreeName: string
  ): Promise<{ path: string; branch: string }> {
    // Get repo root
    const rootResult = await this.exec(repoPath, ['rev-parse', '--show-toplevel'])
    if (rootResult.exitCode !== 0) {
      throw this.interpretError(rootResult)
    }
    const rootPath = rootResult.stdout.trim()

    // Determine worktree path
    let worktreePath: string
    if (await this.isWorktreesDirInGitignore(rootPath)) {
      const worktreesDir = `${rootPath}/.worktrees`
      worktreePath = `${worktreesDir}/${worktreeName}`
    } else {
      const parentDir = rootPath.substring(0, rootPath.lastIndexOf('/'))
      const repoName = rootPath.substring(rootPath.lastIndexOf('/') + 1)
      worktreePath = `${parentDir}/${repoName}-${worktreeName}`
    }

    // Create worktree from existing branch
    const result = await this.exec(repoPath, ['worktree', 'add', worktreePath, branch])
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }

    return { path: worktreePath, branch }
  }

  /**
   * Create a worktree from a remote branch
   */
  async createWorktreeFromRemote(
    repoPath: string,
    remoteBranch: string,
    worktreeName: string
  ): Promise<{ path: string; branch: string }> {
    // Extract branch name from remote (e.g., "origin/main" -> "main")
    const branchName = remoteBranch.replace(/^[^/]+\//, '')
    
    // Get repo root
    const rootResult = await this.exec(repoPath, ['rev-parse', '--show-toplevel'])
    if (rootResult.exitCode !== 0) {
      throw this.interpretError(rootResult)
    }
    const rootPath = rootResult.stdout.trim()

    // Determine worktree path
    let worktreePath: string
    if (await this.isWorktreesDirInGitignore(rootPath)) {
      const worktreesDir = `${rootPath}/.worktrees`
      worktreePath = `${worktreesDir}/${worktreeName}`
    } else {
      const parentDir = rootPath.substring(0, rootPath.lastIndexOf('/'))
      const repoName = rootPath.substring(rootPath.lastIndexOf('/') + 1)
      worktreePath = `${parentDir}/${repoName}-${worktreeName}`
    }

    // Create worktree with new branch tracking remote
    const result = await this.exec(repoPath, ['worktree', 'add', '-b', branchName, worktreePath, remoteBranch])
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }

    return { path: worktreePath, branch: branchName }
  }

  /**
   * Remove a worktree
   */
  async removeWorktree(
    repoPath: string,
    worktreePath: string,
    deleteBranch: boolean = false
  ): Promise<void> {
    let branchName: string | null = null

    // Get branch name before removing if needed
    if (deleteBranch) {
      try {
        const branchResult = await this.exec(worktreePath, ['rev-parse', '--abbrev-ref', 'HEAD'])
        if (branchResult.exitCode === 0) {
          branchName = branchResult.stdout.trim()
        }
      } catch (error) {
        console.warn('[git] could not get branch name before worktree removal:', error)
      }
    }

    // Remove worktree
    const result = await this.exec(repoPath, ['worktree', 'remove', worktreePath, '--force'])
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }

    // Delete branch if requested
    if (deleteBranch && branchName) {
      try {
        await this.deleteBranch(repoPath, branchName, true)
      } catch (error) {
        console.warn('[git] branch deletion after worktree removal failed:', error)
      }
    }
  }

  /**
   * List all worktrees
   */
  async listWorktrees(repoPath: string): Promise<WorktreeInfo[]> {
    const result = await this.exec(repoPath, ['worktree', 'list', '--porcelain'])
    
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }

    const worktrees: WorktreeInfo[] = []
    const lines = result.stdout.split('\n')
    let currentPath = ''
    let currentBranch = ''

    for (const line of lines) {
      if (line.startsWith('worktree ')) {
        currentPath = line.slice(9)
      } else if (line.startsWith('branch ')) {
        currentBranch = line.slice(7).replace('refs/heads/', '')
      } else if (line === '' && currentPath && currentBranch) {
        worktrees.push({ path: currentPath, branch: currentBranch })
        currentPath = ''
        currentBranch = ''
      }
    }

    return worktrees
  }

  /**
   * Get child worktrees of a parent branch
   */
  async getChildWorktrees(
    repoPath: string,
    parentBranch: string | null
  ): Promise<ChildWorktreeInfo[]> {
    const allWorktrees = await this.listWorktrees(repoPath)

    if (!parentBranch) {
      // Return top-level worktrees (branches without '/')
      return allWorktrees
        .filter(wt => !wt.branch.includes('/'))
        .map(wt => ({ ...wt, displayName: wt.branch }))
    }

    // Find child worktrees
    const branchPrefix = `${parentBranch}/`
    
    return allWorktrees
      .filter(wt => {
        if (!wt.branch.startsWith(branchPrefix)) return false
        const remainder = wt.branch.slice(branchPrefix.length)
        return remainder.length > 0 && !remainder.includes('/')
      })
      .map(wt => ({
        ...wt,
        displayName: wt.branch.slice(branchPrefix.length)
      }))
  }

  /**
   * Get branches currently in worktrees
   */
  async getBranchesInWorktrees(repoPath: string): Promise<string[]> {
    const worktrees = await this.listWorktrees(repoPath)
    return worktrees.map(wt => wt.branch)
  }

  /**
   * Get diff between current branch and parent branch
   */
  async getDiff(worktreePath: string, parentBranch: string): Promise<GitDiffResult> {
    // Get current branch
    const currentBranchResult = await this.exec(worktreePath, ['rev-parse', '--abbrev-ref', 'HEAD'])
    if (currentBranchResult.exitCode !== 0) {
      throw this.interpretError(currentBranchResult)
    }
    const currentBranch = currentBranchResult.stdout.trim()

    // Get merge base
    const mergeBaseResult = await this.exec(worktreePath, ['merge-base', parentBranch, currentBranch])
    if (mergeBaseResult.exitCode !== 0) {
      throw this.interpretError(mergeBaseResult)
    }
    const mergeBase = mergeBaseResult.stdout.trim()

    // Get diff stat and name status
    const [statResult, nameStatusResult] = await Promise.all([
      this.exec(worktreePath, ['diff', '--numstat', mergeBase, currentBranch]),
      this.exec(worktreePath, ['diff', '--name-status', mergeBase, currentBranch])
    ])

    const files: GitDiffFile[] = []
    let totalAdditions = 0
    let totalDeletions = 0

    // Parse name status
    const statusMap = new Map<string, GitDiffFile['status']>()
    const nameStatusLines = nameStatusResult.stdout.trim().split('\n').filter(Boolean)
    
    for (const line of nameStatusLines) {
      const [status, ...pathParts] = line.split('\t')
      const filePath = pathParts[pathParts.length - 1] // Handle renames
      
      if (status.startsWith('A')) statusMap.set(filePath, 'added')
      else if (status.startsWith('M')) statusMap.set(filePath, 'modified')
      else if (status.startsWith('D')) statusMap.set(filePath, 'deleted')
      else if (status.startsWith('R')) statusMap.set(filePath, 'renamed')
    }

    // Parse numstat
    const statLines = statResult.stdout.trim().split('\n').filter(Boolean)
    
    for (const line of statLines) {
      const [add, del, filePath] = line.split('\t')
      const additions = add === '-' ? 0 : parseInt(add, 10) || 0
      const deletions = del === '-' ? 0 : parseInt(del, 10) || 0

      files.push({
        path: filePath,
        status: statusMap.get(filePath) || 'modified',
        additions,
        deletions
      })

      totalAdditions += additions
      totalDeletions += deletions
    }

    return {
      files,
      totalAdditions,
      totalDeletions,
      baseBranch: parentBranch,
      headBranch: currentBranch
    }
  }

  /**
   * Get diff for a specific file
   */
  async getFileDiff(worktreePath: string, parentBranch: string, filePath: string): Promise<string> {
    // Get merge base
    const currentBranchResult = await this.exec(worktreePath, ['rev-parse', '--abbrev-ref', 'HEAD'])
    if (currentBranchResult.exitCode !== 0) {
      throw this.interpretError(currentBranchResult)
    }
    const currentBranch = currentBranchResult.stdout.trim()

    const mergeBaseResult = await this.exec(worktreePath, ['merge-base', parentBranch, currentBranch])
    if (mergeBaseResult.exitCode !== 0) {
      throw this.interpretError(mergeBaseResult)
    }
    const mergeBase = mergeBaseResult.stdout.trim()

    // Get file diff
    const result = await this.exec(worktreePath, ['diff', mergeBase, currentBranch, '--', filePath])
    
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }

    return result.stdout
  }

  /**
   * Merge a worktree branch into target branch
   */
  async mergeWorktree(
    mainRepoPath: string,
    worktreeBranch: string,
    targetBranch: string,
    squash: boolean = false
  ): Promise<void> {
    // Switch to target branch
    const checkoutResult = await this.exec(mainRepoPath, ['checkout', targetBranch])
    if (checkoutResult.exitCode !== 0) {
      throw this.interpretError(checkoutResult)
    }

    // Merge
    const args = squash 
      ? ['merge', '--squash', worktreeBranch]
      : ['merge', worktreeBranch]
    
    const result = await this.exec(mainRepoPath, args)
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }
  }

  /**
   * Check for merge conflicts between two branches
   */
  async checkMergeConflicts(
    repoPath: string,
    sourceBranch: string,
    targetBranch: string
  ): Promise<{ hasConflicts: boolean; conflictedFiles: string[]; messages: string[] }> {
    // Try a dry-run merge
    const result = await this.exec(repoPath, ['merge-tree', targetBranch, sourceBranch])
    
    const conflictedFiles: string[] = []
    const lines = result.stdout.split('\n')
    
    for (const line of lines) {
      // Look for conflict markers in merge-tree output
      if (line.includes('conflict') || line.startsWith('<<<<<<<')) {
        // Extract filename from line
        const parts = line.split(/\s+/)
        for (const part of parts) {
          if (part.includes('.') && !part.startsWith('<') && !part.startsWith('=') && !part.startsWith('>')) {
            if (!conflictedFiles.includes(part)) {
              conflictedFiles.push(part)
            }
          }
        }
      }
    }

    return {
      hasConflicts: conflictedFiles.length > 0,
      conflictedFiles,
      messages: conflictedFiles.length > 0 ? [`${conflictedFiles.length} conflicting files`] : []
    }
  }

  /**
   * Get uncommitted changes
   */
  async getUncommittedChanges(repoPath: string): Promise<{
    files: (GitStatusEntry & { additions: number; deletions: number })[]
    totalAdditions: number
    totalDeletions: number
  }> {
    // Get status
    const statusResult = await this.exec(repoPath, ['status', '--porcelain'])
    if (statusResult.exitCode !== 0) {
      throw this.interpretError(statusResult)
    }

    const status = this.parseStatus(statusResult.stdout)

    // Get staged diff stats (cached changes)
    const stagedStatResult = await this.exec(repoPath, ['diff', '--cached', '--numstat'])
    const stagedStatMap = new Map<string, { additions: number; deletions: number }>()
    
    for (const line of stagedStatResult.stdout.trim().split('\n').filter(Boolean)) {
      const [add, del, filePath] = line.split('\t')
      stagedStatMap.set(filePath, {
        additions: add === '-' ? 0 : parseInt(add, 10) || 0,
        deletions: del === '-' ? 0 : parseInt(del, 10) || 0
      })
    }

    // Get unstaged diff stats (working tree changes)
    const unstagedStatResult = await this.exec(repoPath, ['diff', '--numstat'])
    const unstagedStatMap = new Map<string, { additions: number; deletions: number }>()
    
    for (const line of unstagedStatResult.stdout.trim().split('\n').filter(Boolean)) {
      const [add, del, filePath] = line.split('\t')
      unstagedStatMap.set(filePath, {
        additions: add === '-' ? 0 : parseInt(add, 10) || 0,
        deletions: del === '-' ? 0 : parseInt(del, 10) || 0
      })
    }

    const files = status.map(s => {
      const statMap = s.staged ? stagedStatMap : unstagedStatMap
      return {
        ...s,
        additions: statMap.get(s.path)?.additions || 0,
        deletions: statMap.get(s.path)?.deletions || 0
      }
    })

    const totalAdditions = files.reduce((sum, f) => sum + f.additions, 0)
    const totalDeletions = files.reduce((sum, f) => sum + f.deletions, 0)

    return { files, totalAdditions, totalDeletions }
  }

  /**
   * Get diff for uncommitted file
   */
  async getUncommittedFileDiff(
    repoPath: string,
    filePath: string,
    staged: boolean
  ): Promise<string> {
    const args = staged 
      ? ['diff', '--cached', '--', filePath]
      : ['diff', '--', filePath]
    
    const result = await this.exec(repoPath, args)
    
    if (result.exitCode !== 0) {
      throw this.interpretError(result)
    }

    return result.stdout
  }

  /**
   * Get file contents for diff (original vs modified)
   */
  async getFileContentsForDiff(
    worktreePath: string,
    parentBranch: string,
    filePath: string
  ): Promise<{ originalContent: string; modifiedContent: string; language: string }> {
    // Get merge base
    const currentBranchResult = await this.exec(worktreePath, ['rev-parse', '--abbrev-ref', 'HEAD'])
    if (currentBranchResult.exitCode !== 0) {
      throw this.interpretError(currentBranchResult)
    }
    const currentBranch = currentBranchResult.stdout.trim()

    const mergeBaseResult = await this.exec(worktreePath, ['merge-base', parentBranch, currentBranch])
    if (mergeBaseResult.exitCode !== 0) {
      throw this.interpretError(mergeBaseResult)
    }
    const mergeBase = mergeBaseResult.stdout.trim()

    // Get original content from merge base
    const originalResult = await this.exec(worktreePath, ['show', `${mergeBase}:${filePath}`])
    const originalContent = originalResult.exitCode === 0 ? originalResult.stdout : ''

    // Get modified content from working tree
    const modifiedResult = await this.exec(worktreePath, ['show', `HEAD:${filePath}`])
    const modifiedContent = modifiedResult.exitCode === 0 ? modifiedResult.stdout : ''

    // Detect language from file extension
    const ext = filePath.split('.').pop() || ''
    const language = this.detectLanguage(ext)

    return { originalContent, modifiedContent, language }
  }

  /**
   * Get file contents for uncommitted diff
   */
  async getUncommittedFileContentsForDiff(
    repoPath: string,
    filePath: string,
    staged: boolean
  ): Promise<{ originalContent: string; modifiedContent: string; language: string }> {
    const ext = filePath.split('.').pop() || ''
    const language = this.detectLanguage(ext)

    if (staged) {
      // For staged files:
      // Original = HEAD version
      // Modified = index (staged) version
      const originalResult = await this.exec(repoPath, ['show', `HEAD:${filePath}`])
      const modifiedResult = await this.exec(repoPath, ['show', `:${filePath}`])
      
      const originalContent = originalResult.exitCode === 0 ? originalResult.stdout : ''
      const modifiedContent = modifiedResult.exitCode === 0 ? modifiedResult.stdout : ''
      
      return { originalContent, modifiedContent, language }
    } else {
      // For unstaged files:
      // Original = index version
      // Modified = working tree version
      const originalResult = await this.exec(repoPath, ['show', `:${filePath}`])
      const originalContent = originalResult.exitCode === 0 ? originalResult.stdout : ''
      
      // Read working tree file directly
      let modifiedContent = ''
      try {
        const fullPath = join(repoPath, filePath)
        modifiedContent = await readFile(fullPath, 'utf-8')
      } catch {
        // File might not exist in working tree (deleted)
        modifiedContent = ''
      }
      
      return { originalContent, modifiedContent, language }
    }
  }

  // Helper methods

  private async isWorktreesDirInGitignore(rootPath: string): Promise<boolean> {
    try {
      const result = await this.exec(rootPath, ['check-ignore', '.worktrees'])
      return result.exitCode === 0 // Exit code 0 means it's ignored
    } catch (error) {
      console.warn('[git] gitignore check failed:', error)
      return false
    }
  }

  private detectLanguage(ext: string): string {
    const langMap: Record<string, string> = {
      'ts': 'typescript',
      'tsx': 'typescript',
      'js': 'javascript',
      'jsx': 'javascript',
      'py': 'python',
      'rs': 'rust',
      'go': 'go',
      'java': 'java',
      'cpp': 'cpp',
      'c': 'c',
      'h': 'c',
      'hpp': 'cpp',
      'md': 'markdown',
      'json': 'json',
      'yaml': 'yaml',
      'yml': 'yaml',
      'toml': 'toml',
      'proto': 'protobuf',
      'sh': 'shell',
      'bash': 'shell',
      'zsh': 'shell',
      'html': 'html',
      'css': 'css',
      'scss': 'scss',
      'less': 'less',
      'sql': 'sql',
      'rb': 'ruby',
      'php': 'php',
      'swift': 'swift',
      'kt': 'kotlin',
      'scala': 'scala',
      'r': 'r',
      'm': 'objective-c',
      'mm': 'objective-cpp',
      'vue': 'vue',
      'svelte': 'svelte'
    }
    return langMap[ext.toLowerCase()] || 'plaintext'
  }

  private interpretError(result: { exitCode: number; stdout: string; stderr: string }): Error {
    const stderr = result.stderr.toLowerCase()
    
    if (stderr.includes('nothing to commit')) {
      return new Error('No changes to commit')
    }
    if (stderr.includes('merge conflict')) {
      return new Error('Merge conflict detected')
    }
    if (stderr.includes('already exists')) {
      return new Error('Already exists')
    }
    if (stderr.includes('not a git repository')) {
      return new Error('Not a git repository')
    }
    if (stderr.includes('pathspec') && stderr.includes('did not match')) {
      return new Error('File not found')
    }
    if (stderr.includes('failed to merge')) {
      return new Error('Merge failed')
    }
    if (stderr.includes('could not resolve')) {
      return new Error('Could not resolve reference')
    }
    if (result.exitCode !== 0 && result.stderr) {
      return new Error(`Git error: ${result.stderr}`)
    }
    
    return new Error(`Git command failed with exit code ${result.exitCode}`)
  }
}
