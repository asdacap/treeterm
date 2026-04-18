/**
 * Git Client for Renderer Process
 *
 * Migrated from main/git.ts — all git operations now run directly in the
 * renderer using the IPC-based ExecApi instead of gRPC ExecStream. This keeps
 * business logic in the renderer as required by the architecture (AGENTS.md).
 */

import type {
  GitInfo,
  WorktreeResult,
  WorktreeInfo,
  DiffResult,
  DiffFile,
  ConflictCheckResult,
  UncommittedChanges,
  FileDiffContents,
  GitLogResult,
  ExecApi,
  FilesystemApi,
} from '../types'
import type { IpcResult } from '../../shared/ipc-types'
import { FileChangeStatus } from '../../shared/types'
import { resolveHomedir } from './homedir'

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface GitStatusEntry {
  path: string
  status: FileChangeStatus
  staged: boolean
  originalPath?: string
}

/** The GitApi contract that createGitApi returns (matches the interface in renderer/types) */
export interface GitApi {
  getInfo: (dirPath: string) => Promise<GitInfo>
  createWorktree: (repoPath: string, name: string, baseBranch?: string, onProgress?: (data: string) => void) => Promise<WorktreeResult>
  removeWorktree: (repoPath: string, worktreePath: string, deleteBranch?: boolean, onProgress?: (data: string) => void) => Promise<IpcResult>
  listWorktrees: (repoPath: string) => Promise<WorktreeInfo[]>
  listLocalBranches: (repoPath: string) => Promise<string[]>
  listRemoteBranches: (repoPath: string) => Promise<string[]>
  getBranchesInWorktrees: (repoPath: string) => Promise<string[]>
  createWorktreeFromBranch: (repoPath: string, branch: string, worktreeName: string, onProgress?: (data: string) => void) => Promise<WorktreeResult>
  createWorktreeFromRemote: (repoPath: string, remoteBranch: string, worktreeName: string, onProgress?: (data: string) => void) => Promise<WorktreeResult>
  getDiff: (worktreePath: string, parentBranch: string) => Promise<IpcResult<{ diff: DiffResult }>>
  getFileDiff: (worktreePath: string, parentBranch: string, filePath: string) => Promise<IpcResult<{ diff: string }>>
  merge: (targetWorktreePath: string, worktreeBranch: string, squash?: boolean, onProgress?: (data: string) => void) => Promise<IpcResult>
  checkMergeConflicts: (repoPath: string, sourceBranch: string, targetBranch: string) => Promise<ConflictCheckResult>
  hasUncommittedChanges: (repoPath: string) => Promise<boolean>
  commitAll: (repoPath: string, message: string) => Promise<IpcResult>
  deleteBranch: (repoPath: string, branchName: string, onProgress?: (data: string) => void) => Promise<IpcResult>
  renameBranch: (repoPath: string, oldName: string, newName: string) => Promise<IpcResult>
  getUncommittedChanges: (repoPath: string) => Promise<IpcResult<{ changes: UncommittedChanges }>>
  getUncommittedFileDiff: (repoPath: string, filePath: string, staged: boolean) => Promise<IpcResult<{ diff: string }>>
  stageFile: (repoPath: string, filePath: string) => Promise<IpcResult>
  unstageFile: (repoPath: string, filePath: string) => Promise<IpcResult>
  stageAll: (repoPath: string) => Promise<IpcResult>
  unstageAll: (repoPath: string) => Promise<IpcResult>
  commitStaged: (repoPath: string, message: string) => Promise<IpcResult>
  getFileContentsForDiff: (worktreePath: string, parentBranch: string, filePath: string) => Promise<IpcResult<{ contents: FileDiffContents }>>
  getUncommittedFileContentsForDiff: (repoPath: string, filePath: string, staged: boolean) => Promise<IpcResult<{ contents: FileDiffContents }>>
  getHeadCommitHash: (repoPath: string) => Promise<IpcResult<{ hash: string }>>
  getLog: (repoPath: string, parentBranch: string | null, skip: number, limit: number) => Promise<IpcResult<{ result: GitLogResult }>>
  getCommitDiff: (repoPath: string, commitHash: string) => Promise<IpcResult<{ files: DiffFile[] }>>
  getCommitFileDiff: (repoPath: string, commitHash: string, filePath: string) => Promise<IpcResult<{ contents: FileDiffContents }>>
  fetch: (repoPath: string) => Promise<IpcResult>
  pull: (repoPath: string) => Promise<IpcResult>
  getBehindCount: (repoPath: string) => Promise<number>
  getRemoteUrl: (repoPath: string) => Promise<IpcResult<{ url: string }>>
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

type ExecResult = { exitCode: number; stdout: string; stderr: string; args: string[] }

async function execGit(
  exec: ExecApi,
  connectionId: string,
  cwd: string,
  args: string[],
  options?: { timeoutMs?: number; onProgress?: (data: string) => void },
): Promise<ExecResult> {
  const startResult = await exec.start(connectionId, cwd, 'git', args)
  if (!startResult.success) throw new Error(startResult.error)
  const { execId } = startResult

  return new Promise((resolve, reject) => {
    const stdout: string[] = []
    const stderr: string[] = []

    const unsub = exec.onEvent(execId, (event) => {
      if (event.type === 'stdout') {
        stdout.push(event.data)
        options?.onProgress?.(event.data)
      } else if (event.type === 'stderr') {
        stderr.push(event.data)
        options?.onProgress?.(event.data)
      } else if (event.type === 'exit') {
        unsub()
        resolve({ exitCode: event.exitCode, stdout: stdout.join(''), stderr: stderr.join(''), args })
      } else {
        unsub()
        reject(new Error(event.message))
      }
    })
  })
}

export function parseStatus(output: string): GitStatusEntry[] {
  const entries: GitStatusEntry[] = []
  const lines = output.split('\n').filter((line) => line.length > 0)

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

    const getStatusFromChar = (c: string): GitStatusEntry['status'] => {
      if (c === 'A') return FileChangeStatus.Added
      if (c === 'D') return FileChangeStatus.Deleted
      if (c === 'R') return FileChangeStatus.Renamed
      return FileChangeStatus.Modified
    }

    // Untracked files
    if (stagedChar === '?' && unstagedChar === '?') {
      entries.push({ path, status: FileChangeStatus.Untracked, staged: false, originalPath })
      continue
    }

    // Staged changes
    if (stagedChar !== ' ' && stagedChar !== '?') {
      entries.push({ path, status: getStatusFromChar(stagedChar ?? ''), staged: true, originalPath })
    }

    // Unstaged changes
    if (unstagedChar !== ' ' && unstagedChar !== '?') {
      entries.push({ path, status: getStatusFromChar(unstagedChar ?? ''), staged: false, originalPath })
    }
  }

  return entries
}

function interpretError(result: ExecResult): Error {
  const stderr = result.stderr.toLowerCase()
  const cmd = `git ${result.args.join(' ')}`

  if (stderr.includes('nothing to commit')) return new Error(`No changes to commit (${cmd})`)
  if (stderr.includes('merge conflict')) return new Error(`Merge conflict detected (${cmd})`)
  if (stderr.includes('already exists')) return new Error(`Already exists (${cmd})`)
  if (stderr.includes('not a git repository')) return new Error(`Not a git repository (${cmd})`)
  if (stderr.includes('pathspec') && stderr.includes('did not match')) return new Error(`File not found (${cmd})`)
  if (stderr.includes('failed to merge')) return new Error(`Merge failed (${cmd})`)
  if (stderr.includes('could not resolve')) return new Error(`Could not resolve reference (${cmd})`)
  if (result.exitCode !== 0 && result.stderr) return new Error(`Git error [${cmd}]: ${result.stderr}`)

  return new Error(`Git command failed [${cmd}] with exit code ${String(result.exitCode)}`)
}

function detectLanguage(ext: string): string {
  const langMap: Record<string, string> = {
    ts: 'typescript',
    tsx: 'typescript',
    js: 'javascript',
    jsx: 'javascript',
    py: 'python',
    rs: 'rust',
    go: 'go',
    java: 'java',
    cpp: 'cpp',
    c: 'c',
    h: 'c',
    hpp: 'cpp',
    md: 'markdown',
    json: 'json',
    yaml: 'yaml',
    yml: 'yaml',
    toml: 'toml',
    proto: 'protobuf',
    sh: 'shell',
    bash: 'shell',
    zsh: 'shell',
    html: 'html',
    css: 'css',
    scss: 'scss',
    less: 'less',
    sql: 'sql',
    rb: 'ruby',
    php: 'php',
    swift: 'swift',
    kt: 'kotlin',
    scala: 'scala',
    r: 'r',
    m: 'objective-c',
    mm: 'objective-cpp',
    vue: 'vue',
    svelte: 'svelte',
  }
  return langMap[ext.toLowerCase()] || 'plaintext'
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

export function createGitApi(exec: ExecApi, filesystem: FilesystemApi, connectionId: string): GitApi {
  // Convenience wrapper — runs `git <args>` in the given cwd.
  async function git(
    cwd: string,
    args: string[],
    options?: { timeoutMs?: number; onProgress?: (data: string) => void },
  ): Promise<ExecResult> {
    return execGit(exec, connectionId, cwd, args, options)
  }

  async function isWorktreesDirInGitignore(rootPath: string): Promise<boolean> {
    try {
      const result = await git(rootPath, ['check-ignore', '.worktrees'])
      return result.exitCode === 0
    } catch (error) {
      console.warn('[git] gitignore check failed:', error)
      return false
    }
  }

  async function resolveWorktreePath(rootPath: string, worktreeName: string): Promise<string> {
    if (await isWorktreesDirInGitignore(rootPath)) {
      return `${rootPath}/.worktrees/${worktreeName}`
    }
    const repoName = rootPath.substring(rootPath.lastIndexOf('/') + 1)
    const home = await resolveHomedir(exec, connectionId)
    return `${home}/.treeterm/worktrees/${repoName}/${worktreeName}`
  }

  // -----------------------------------------------------------------------
  // API implementation
  // -----------------------------------------------------------------------

  return {
    // ----- getInfo -----
    async getInfo(dirPath: string): Promise<GitInfo> {
      try {
        const result = await git(dirPath, ['rev-parse', '--is-inside-work-tree'])
        if (result.exitCode !== 0) return { isRepo: false }

        const [branchResult, rootResult] = await Promise.all([
          git(dirPath, ['rev-parse', '--abbrev-ref', 'HEAD']),
          git(dirPath, ['rev-parse', '--show-toplevel']),
        ])

        return {
          isRepo: true,
          branch: branchResult.stdout.trim() || 'HEAD',
          rootPath: rootResult.stdout.trim() || dirPath,
        }
      } catch (error) {
        console.warn('[git] getGitInfo failed, treating as non-repo:', error)
        return { isRepo: false }
      }
    },

    // ----- createWorktree -----
    async createWorktree(
      repoPath: string,
      name: string,
      baseBranch?: string,
      onProgress?: (data: string) => void,
    ): Promise<WorktreeResult> {
      try {
        const rootResult = await git(repoPath, ['rev-parse', '--show-toplevel'])
        if (rootResult.exitCode !== 0) {
          return { success: false, error: interpretError(rootResult).message }
        }
        const rootPath = rootResult.stdout.trim()

        const worktreePath = await resolveWorktreePath(rootPath, name)
        const branchName = name

        const args = ['worktree', 'add', '-b', branchName, worktreePath]
        if (baseBranch) args.push(baseBranch)

        const result = await git(repoPath, args, { onProgress })
        if (result.exitCode !== 0) {
          return { success: false, error: interpretError(result).message }
        }

        return { success: true, path: worktreePath, branch: branchName }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- removeWorktree -----
    async removeWorktree(
      repoPath: string,
      worktreePath: string,
      deleteBranch?: boolean,
      onProgress?: (data: string) => void,
    ): Promise<IpcResult> {
      try {
        let branchName: string | null = null

        if (deleteBranch) {
          try {
            const branchResult = await git(worktreePath, ['rev-parse', '--abbrev-ref', 'HEAD'])
            if (branchResult.exitCode === 0) {
              branchName = branchResult.stdout.trim()
            }
          } catch (error) {
            console.warn('[git] could not get branch name before worktree removal:', error)
          }
        }

        const result = await git(repoPath, ['worktree', 'remove', worktreePath, '--force'], { onProgress })
        if (result.exitCode !== 0) {
          return { success: false, error: interpretError(result).message }
        }

        if (deleteBranch && branchName) {
          try {
            const delResult = await git(repoPath, ['branch', '-D', branchName], { onProgress })
            if (delResult.exitCode !== 0) {
              console.warn('[git] branch deletion after worktree removal failed:', delResult.stderr)
            }
          } catch (error) {
            console.warn('[git] branch deletion after worktree removal failed:', error)
          }
        }

        return { success: true }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- listWorktrees -----
    async listWorktrees(repoPath: string): Promise<WorktreeInfo[]> {
      const result = await git(repoPath, ['worktree', 'list', '--porcelain'])
      if (result.exitCode !== 0) throw interpretError(result)

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
    },

    // ----- listLocalBranches -----
    async listLocalBranches(repoPath: string): Promise<string[]> {
      const result = await git(repoPath, ['branch', '--format=%(refname:short)'])
      if (result.exitCode !== 0) throw interpretError(result)
      return result.stdout.trim().split('\n').filter(Boolean)
    },

    // ----- listRemoteBranches -----
    async listRemoteBranches(repoPath: string): Promise<string[]> {
      const result = await git(repoPath, ['branch', '-r', '--format=%(refname:short)'])
      if (result.exitCode !== 0) throw interpretError(result)
      return result.stdout.trim().split('\n').filter(Boolean)
    },

    // ----- getBranchesInWorktrees -----
    async getBranchesInWorktrees(repoPath: string): Promise<string[]> {
      const worktrees = await this.listWorktrees(repoPath)
      return worktrees.map((wt) => wt.branch)
    },

    // ----- createWorktreeFromBranch -----
    async createWorktreeFromBranch(
      repoPath: string,
      branch: string,
      worktreeName: string,
      onProgress?: (data: string) => void,
    ): Promise<WorktreeResult> {
      try {
        const rootResult = await git(repoPath, ['rev-parse', '--show-toplevel'])
        if (rootResult.exitCode !== 0) {
          return { success: false, error: interpretError(rootResult).message }
        }
        const rootPath = rootResult.stdout.trim()

        const worktreePath = await resolveWorktreePath(rootPath, worktreeName)

        const result = await git(repoPath, ['worktree', 'add', worktreePath, branch], { onProgress })
        if (result.exitCode !== 0) {
          return { success: false, error: interpretError(result).message }
        }

        return { success: true, path: worktreePath, branch }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- createWorktreeFromRemote -----
    async createWorktreeFromRemote(
      repoPath: string,
      remoteBranch: string,
      worktreeName: string,
      onProgress?: (data: string) => void,
    ): Promise<WorktreeResult> {
      try {
        const branchName = remoteBranch.replace(/^[^/]+\//, '')

        const rootResult = await git(repoPath, ['rev-parse', '--show-toplevel'])
        if (rootResult.exitCode !== 0) {
          return { success: false, error: interpretError(rootResult).message }
        }
        const rootPath = rootResult.stdout.trim()

        const worktreePath = await resolveWorktreePath(rootPath, worktreeName)

        const result = await git(repoPath, ['worktree', 'add', '-b', branchName, worktreePath, remoteBranch], { onProgress })
        if (result.exitCode !== 0) {
          return { success: false, error: interpretError(result).message }
        }

        return { success: true, path: worktreePath, branch: branchName }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- getDiff -----
    async getDiff(worktreePath: string, parentBranch: string): Promise<IpcResult<{ diff: DiffResult }>> {
      try {
        const currentBranchResult = await git(worktreePath, ['rev-parse', '--abbrev-ref', 'HEAD'])
        if (currentBranchResult.exitCode !== 0) throw interpretError(currentBranchResult)
        const currentBranch = currentBranchResult.stdout.trim()

        const mergeBaseResult = await git(worktreePath, ['merge-base', parentBranch, currentBranch])
        if (mergeBaseResult.exitCode !== 0) throw interpretError(mergeBaseResult)
        const mergeBase = mergeBaseResult.stdout.trim()

        const [statResult, nameStatusResult] = await Promise.all([
          git(worktreePath, ['diff', '--numstat', mergeBase, currentBranch]),
          git(worktreePath, ['diff', '--name-status', mergeBase, currentBranch]),
        ])

        const files: DiffFile[] = []
        let totalAdditions = 0
        let totalDeletions = 0

        // Parse name status
        const statusMap = new Map<string, DiffFile['status']>()
        const nameStatusLines = nameStatusResult.stdout.trim().split('\n').filter(Boolean)

        for (const line of nameStatusLines) {
          const [status, ...pathParts] = line.split('\t')
          const filePath = pathParts[pathParts.length - 1] ?? ''
          if (!status) continue
          if (status.startsWith('A')) statusMap.set(filePath, FileChangeStatus.Added)
          else if (status.startsWith('M')) statusMap.set(filePath, FileChangeStatus.Modified)
          else if (status.startsWith('D')) statusMap.set(filePath, FileChangeStatus.Deleted)
          else if (status.startsWith('R')) statusMap.set(filePath, FileChangeStatus.Renamed)
        }

        // Parse numstat
        const statLines = statResult.stdout.trim().split('\n').filter(Boolean)

        for (const line of statLines) {
          const [add, del, filePath] = line.split('\t')
          const additions = (add ?? '') === '-' ? 0 : parseInt(add ?? '', 10) || 0
          const deletions = (del ?? '') === '-' ? 0 : parseInt(del ?? '', 10) || 0
          const resolvedPath = filePath ?? ''

          files.push({
            path: resolvedPath,
            status: statusMap.get(resolvedPath) || FileChangeStatus.Modified,
            additions,
            deletions,
          })

          totalAdditions += additions
          totalDeletions += deletions
        }

        return {
          success: true,
          diff: { files, totalAdditions, totalDeletions, baseBranch: parentBranch, headBranch: currentBranch },
        }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- getFileDiff -----
    async getFileDiff(
      worktreePath: string,
      parentBranch: string,
      filePath: string,
    ): Promise<IpcResult<{ diff: string }>> {
      try {
        const currentBranchResult = await git(worktreePath, ['rev-parse', '--abbrev-ref', 'HEAD'])
        if (currentBranchResult.exitCode !== 0) throw interpretError(currentBranchResult)
        const currentBranch = currentBranchResult.stdout.trim()

        const mergeBaseResult = await git(worktreePath, ['merge-base', parentBranch, currentBranch])
        if (mergeBaseResult.exitCode !== 0) throw interpretError(mergeBaseResult)
        const mergeBase = mergeBaseResult.stdout.trim()

        const result = await git(worktreePath, ['diff', mergeBase, currentBranch, '--', filePath])
        if (result.exitCode !== 0) throw interpretError(result)

        return { success: true, diff: result.stdout }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- merge -----
    async merge(
      targetWorktreePath: string,
      worktreeBranch: string,
      squash?: boolean,
      onProgress?: (data: string) => void,
    ): Promise<IpcResult> {
      try {
        const args = squash
          ? ['merge', '--squash', worktreeBranch]
          : ['merge', worktreeBranch]

        const result = await git(targetWorktreePath, args, { onProgress })
        if (result.exitCode !== 0) {
          return { success: false, error: interpretError(result).message }
        }

        return { success: true }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- checkMergeConflicts -----
    async checkMergeConflicts(
      repoPath: string,
      sourceBranch: string,
      targetBranch: string,
    ): Promise<ConflictCheckResult> {
      try {
        const result = await git(repoPath, ['merge-tree', targetBranch, sourceBranch])

        const conflictedFiles: string[] = []
        const lines = result.stdout.split('\n')

        for (const line of lines) {
          if (line.includes('conflict') || line.startsWith('<<<<<<<')) {
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
          success: true,
          conflicts: {
            hasConflicts: conflictedFiles.length > 0,
            conflictedFiles,
            messages: conflictedFiles.length > 0 ? [`${String(conflictedFiles.length)} conflicting files`] : [],
          },
        }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- hasUncommittedChanges -----
    async hasUncommittedChanges(repoPath: string): Promise<boolean> {
      const result = await git(repoPath, ['status', '--porcelain'])
      return result.exitCode === 0 && result.stdout.trim().length > 0
    },

    // ----- commitAll -----
    async commitAll(repoPath: string, message: string): Promise<IpcResult> {
      try {
        const result = await git(repoPath, ['commit', '-am', message])
        if (result.exitCode !== 0) {
          return { success: false, error: interpretError(result).message }
        }
        return { success: true }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- deleteBranch -----
    async deleteBranch(
      repoPath: string,
      branchName: string,
      onProgress?: (data: string) => void,
    ): Promise<IpcResult> {
      try {
        const result = await git(repoPath, ['branch', '-D', branchName], { onProgress })
        if (result.exitCode !== 0) {
          return { success: false, error: interpretError(result).message }
        }
        return { success: true }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- renameBranch -----
    async renameBranch(repoPath: string, oldName: string, newName: string): Promise<IpcResult> {
      try {
        const result = await git(repoPath, ['branch', '-m', oldName, newName])
        if (result.exitCode !== 0) {
          return { success: false, error: interpretError(result).message }
        }
        return { success: true }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- getUncommittedChanges -----
    async getUncommittedChanges(repoPath: string): Promise<IpcResult<{ changes: UncommittedChanges }>> {
      try {
        const statusResult = await git(repoPath, ['status', '--porcelain'])
        if (statusResult.exitCode !== 0) throw interpretError(statusResult)

        const status = parseStatus(statusResult.stdout)

        // Get staged diff stats
        const stagedStatResult = await git(repoPath, ['diff', '--cached', '--numstat'])
        const stagedStatMap = new Map<string, { additions: number; deletions: number }>()

        for (const line of stagedStatResult.stdout.trim().split('\n').filter(Boolean)) {
          const [add, del, filePath] = line.split('\t')
          if (!filePath) continue
          stagedStatMap.set(filePath, {
            additions: (add ?? '') === '-' ? 0 : parseInt(add ?? '', 10) || 0,
            deletions: (del ?? '') === '-' ? 0 : parseInt(del ?? '', 10) || 0,
          })
        }

        // Get unstaged diff stats
        const unstagedStatResult = await git(repoPath, ['diff', '--numstat'])
        const unstagedStatMap = new Map<string, { additions: number; deletions: number }>()

        for (const line of unstagedStatResult.stdout.trim().split('\n').filter(Boolean)) {
          const [add, del, filePath] = line.split('\t')
          if (!filePath) continue
          unstagedStatMap.set(filePath, {
            additions: (add ?? '') === '-' ? 0 : parseInt(add ?? '', 10) || 0,
            deletions: (del ?? '') === '-' ? 0 : parseInt(del ?? '', 10) || 0,
          })
        }

        const files = status.map((s) => {
          const statMap = s.staged ? stagedStatMap : unstagedStatMap
          return {
            ...s,
            additions: statMap.get(s.path)?.additions || 0,
            deletions: statMap.get(s.path)?.deletions || 0,
          }
        })

        const totalAdditions = files.reduce((sum, f) => sum + f.additions, 0)
        const totalDeletions = files.reduce((sum, f) => sum + f.deletions, 0)

        return { success: true, changes: { files, totalAdditions, totalDeletions } }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- getUncommittedFileDiff -----
    async getUncommittedFileDiff(
      repoPath: string,
      filePath: string,
      staged: boolean,
    ): Promise<IpcResult<{ diff: string }>> {
      try {
        const args = staged ? ['diff', '--cached', '--', filePath] : ['diff', '--', filePath]
        const result = await git(repoPath, args)
        if (result.exitCode !== 0) throw interpretError(result)
        return { success: true, diff: result.stdout }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- stageFile -----
    async stageFile(repoPath: string, filePath: string): Promise<IpcResult> {
      try {
        const result = await git(repoPath, ['add', filePath])
        if (result.exitCode !== 0) {
          return { success: false, error: interpretError(result).message }
        }
        return { success: true }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- unstageFile -----
    async unstageFile(repoPath: string, filePath: string): Promise<IpcResult> {
      try {
        const result = await git(repoPath, ['reset', 'HEAD', filePath])
        if (result.exitCode !== 0) {
          return { success: false, error: interpretError(result).message }
        }
        return { success: true }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- stageAll -----
    async stageAll(repoPath: string): Promise<IpcResult> {
      try {
        const result = await git(repoPath, ['add', '.'])
        if (result.exitCode !== 0) {
          return { success: false, error: interpretError(result).message }
        }
        return { success: true }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- unstageAll -----
    async unstageAll(repoPath: string): Promise<IpcResult> {
      try {
        const result = await git(repoPath, ['reset', 'HEAD'])
        if (result.exitCode !== 0) {
          return { success: false, error: interpretError(result).message }
        }
        return { success: true }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- commitStaged -----
    async commitStaged(repoPath: string, message: string): Promise<IpcResult> {
      try {
        const result = await git(repoPath, ['commit', '-m', message])
        if (result.exitCode !== 0) {
          return { success: false, error: interpretError(result).message }
        }
        return { success: true }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- getFileContentsForDiff -----
    async getFileContentsForDiff(
      worktreePath: string,
      parentBranch: string,
      filePath: string,
    ): Promise<IpcResult<{ contents: FileDiffContents }>> {
      try {
        const currentBranchResult = await git(worktreePath, ['rev-parse', '--abbrev-ref', 'HEAD'])
        if (currentBranchResult.exitCode !== 0) throw interpretError(currentBranchResult)
        const currentBranch = currentBranchResult.stdout.trim()

        const mergeBaseResult = await git(worktreePath, ['merge-base', parentBranch, currentBranch])
        if (mergeBaseResult.exitCode !== 0) throw interpretError(mergeBaseResult)
        const mergeBase = mergeBaseResult.stdout.trim()

        const originalResult = await git(worktreePath, ['show', `${mergeBase}:${filePath}`])
        const originalContent = originalResult.exitCode === 0 ? originalResult.stdout : ''

        const modifiedResult = await git(worktreePath, ['show', `HEAD:${filePath}`])
        const modifiedContent = modifiedResult.exitCode === 0 ? modifiedResult.stdout : ''

        const ext = filePath.split('.').pop() || ''
        const language = detectLanguage(ext)

        return { success: true, contents: { originalContent, modifiedContent, language } }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- getUncommittedFileContentsForDiff -----
    async getUncommittedFileContentsForDiff(
      repoPath: string,
      filePath: string,
      staged: boolean,
    ): Promise<IpcResult<{ contents: FileDiffContents }>> {
      try {
        const ext = filePath.split('.').pop() || ''
        const language = detectLanguage(ext)

        if (staged) {
          const originalResult = await git(repoPath, ['show', `HEAD:${filePath}`])
          const modifiedResult = await git(repoPath, ['show', `:${filePath}`])

          const originalContent = originalResult.exitCode === 0 ? originalResult.stdout : ''
          const modifiedContent = modifiedResult.exitCode === 0 ? modifiedResult.stdout : ''

          return { success: true, contents: { originalContent, modifiedContent, language } }
        } else {
          const originalResult = await git(repoPath, ['show', `:${filePath}`])
          const originalContent = originalResult.exitCode === 0 ? originalResult.stdout : ''

          // Read working tree file via FilesystemApi
          let modifiedContent = ''
          try {
            const fileResult = await filesystem.readFile(repoPath, filePath)
            if (fileResult.success) {
              modifiedContent = fileResult.file.content
            }
          } catch {
            // File might not exist in working tree (deleted)
            modifiedContent = ''
          }

          return { success: true, contents: { originalContent, modifiedContent, language } }
        }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- getHeadCommitHash -----
    async getHeadCommitHash(repoPath: string): Promise<IpcResult<{ hash: string }>> {
      try {
        const result = await git(repoPath, ['rev-parse', 'HEAD'])
        if (result.exitCode !== 0) throw interpretError(result)
        return { success: true, hash: result.stdout.trim() }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- getLog -----
    async getLog(
      repoPath: string,
      parentBranch: string | null,
      skip: number,
      limit: number,
    ): Promise<IpcResult<{ result: GitLogResult }>> {
      try {
        const format = '%H%x1e%h%x1e%an%x1e%aI%x1e%s%x1e%P'
        const args = ['log', `--format=${format}`, `--skip=${String(skip)}`, `--max-count=${String(limit + 1)}`]

        if (parentBranch) {
          args.push(`${parentBranch}..HEAD`)
        }

        const gitResult = await git(repoPath, args)
        if (gitResult.exitCode !== 0) throw interpretError(gitResult)

        const lines = gitResult.stdout.trim().split('\n').filter(Boolean)
        const hasMore = lines.length > limit
        const commitLines = hasMore ? lines.slice(0, limit) : lines

        const commits = commitLines.map((line) => {
          const [hash, shortHash, author, date, message, parents] = line.split('\x1e')
          return {
            hash: hash ?? '',
            shortHash: shortHash ?? '',
            author: author ?? '',
            date: date ?? '',
            message: message ?? '',
            parentHashes: parents ? parents.split(' ').filter(Boolean) : [],
          }
        })

        return { success: true, result: { commits, hasMore } }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- getCommitDiff -----
    async getCommitDiff(repoPath: string, commitHash: string): Promise<IpcResult<{ files: DiffFile[] }>> {
      try {
        const [statResult, nameStatusResult] = await Promise.all([
          git(repoPath, ['diff-tree', '--no-commit-id', '--root', '-r', '--numstat', commitHash]),
          git(repoPath, ['diff-tree', '--no-commit-id', '--root', '-r', '--name-status', commitHash]),
        ])

        if (statResult.exitCode !== 0) throw interpretError(statResult)

        const statusMap = new Map<string, DiffFile['status']>()
        for (const line of nameStatusResult.stdout.trim().split('\n').filter(Boolean)) {
          const [status, ...pathParts] = line.split('\t')
          const filePath = pathParts[pathParts.length - 1] ?? ''
          if (!status) continue
          if (status.startsWith('A')) statusMap.set(filePath, FileChangeStatus.Added)
          else if (status.startsWith('M')) statusMap.set(filePath, FileChangeStatus.Modified)
          else if (status.startsWith('D')) statusMap.set(filePath, FileChangeStatus.Deleted)
          else if (status.startsWith('R')) statusMap.set(filePath, FileChangeStatus.Renamed)
        }

        const files: DiffFile[] = []
        for (const line of statResult.stdout.trim().split('\n').filter(Boolean)) {
          const [add, del, filePath] = line.split('\t')
          const resolvedPath = filePath ?? ''
          files.push({
            path: resolvedPath,
            status: statusMap.get(resolvedPath) || FileChangeStatus.Modified,
            additions: (add ?? '') === '-' ? 0 : parseInt(add ?? '', 10) || 0,
            deletions: (del ?? '') === '-' ? 0 : parseInt(del ?? '', 10) || 0,
          })
        }

        return { success: true, files }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- getCommitFileDiff -----
    async getCommitFileDiff(
      repoPath: string,
      commitHash: string,
      filePath: string,
    ): Promise<IpcResult<{ contents: FileDiffContents }>> {
      try {
        const [modifiedResult, originalResult] = await Promise.all([
          git(repoPath, ['show', `${commitHash}:${filePath}`]),
          git(repoPath, ['show', `${commitHash}~1:${filePath}`]),
        ])

        const ext = filePath.split('.').pop() || ''
        const language = detectLanguage(ext)

        return {
          success: true,
          contents: {
            originalContent: originalResult.exitCode === 0 ? originalResult.stdout : '',
            modifiedContent: modifiedResult.exitCode === 0 ? modifiedResult.stdout : '',
            language,
          },
        }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- fetch -----
    async fetch(repoPath: string): Promise<IpcResult> {
      try {
        const result = await git(repoPath, ['fetch'], { timeoutMs: 60000 })
        if (result.exitCode !== 0) {
          return { success: false, error: `git fetch failed: ${result.stderr}` }
        }
        return { success: true }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- pull -----
    async pull(repoPath: string): Promise<IpcResult> {
      try {
        const result = await git(repoPath, ['pull'], { timeoutMs: 60000 })
        if (result.exitCode !== 0) {
          return { success: false, error: result.stderr.trim() || 'git pull failed' }
        }
        return { success: true }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },

    // ----- getBehindCount -----
    async getBehindCount(repoPath: string): Promise<number> {
      const result = await git(repoPath, ['rev-list', '--count', 'HEAD..@{upstream}'])
      if (result.exitCode !== 0) return 0
      return parseInt(result.stdout.trim(), 10) || 0
    },

    // ----- getRemoteUrl -----
    async getRemoteUrl(repoPath: string): Promise<IpcResult<{ url: string }>> {
      try {
        const result = await git(repoPath, ['remote', 'get-url', 'origin'])
        if (result.exitCode !== 0) {
          return { success: false, error: `Failed to get remote URL: ${result.stderr}` }
        }
        return { success: true, url: result.stdout.trim() }
      } catch (error) {
        return { success: false, error: error instanceof Error ? error.message : String(error) }
      }
    },
  }
}
