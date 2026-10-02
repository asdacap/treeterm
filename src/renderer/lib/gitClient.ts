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
  FileChangeStats,
  UncommittedFile,
  ConflictCheckResult,
  UncommittedChanges,
  FileDiffContents,
  GitLogResult,
  ExecApi,
  FilesystemApi,
} from '../types'
/* eslint-disable custom/no-string-literal-comparison -- parses git porcelain output; status chars/tokens are git CLI conventions, not our domain */
import { DEFAULT_EXEC_TIMEOUT_MS, ExecEventType, type IpcResult } from '../../shared/ipc-types'
import { FileChangeStatus } from '../../shared/types'
import { FileStatKind } from '../types'
import { resolveHomedir } from './homedir'
import { withTimeout } from './withTimeout'
import { MAX_READ_FILE_BYTES } from './fileLimits'

// Backstop only — the daemon enforces the exec timeout, so this fires only if the result event
// is never delivered to the renderer.
const GIT_EXEC_BACKSTOP_MARGIN_MS = 5000

// `git worktree add` checks out the whole tree (plus LFS smudge and post-checkout hooks), which
// takes over 30s on large repos.
const WORKTREE_ADD_TIMEOUT_MS = 10 * 60_000

// `git merge` updates the parent's checkout (plus LFS smudge and hooks), which takes over 30s on large repos.
const MERGE_TIMEOUT_MS = 5 * 60_000

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
  isAncestor: (repoPath: string, ancestorRef: string, descendantRef: string) => Promise<boolean>
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
  const timeoutMs = options?.timeoutMs ?? DEFAULT_EXEC_TIMEOUT_MS
  const startResult = await exec.start(connectionId, cwd, 'git', args, timeoutMs)
  if (!startResult.success) throw new Error(startResult.error)
  const { execId } = startResult

  let unsub: () => void = () => undefined
  return withTimeout(new Promise<ExecResult>((resolve, reject) => {
    const stdout: string[] = []
    const stderr: string[] = []

    unsub = exec.onEvent(execId, (event) => {
      if (event.type === ExecEventType.Stdout) {
        stdout.push(event.data)
        options?.onProgress?.(event.data)
      } else if (event.type === ExecEventType.Stderr) {
        stderr.push(event.data)
        options?.onProgress?.(event.data)
      } else if (event.type === ExecEventType.Exit) {
        unsub()
        resolve({ exitCode: event.exitCode, stdout: stdout.join(''), stderr: stderr.join(''), args })
      } else {
        unsub()
        reject(new Error(event.message))
      }
    })
  }), timeoutMs + GIT_EXEC_BACKSTOP_MARGIN_MS, `git ${args.join(' ')}`, () => { unsub(); })
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

type ParsedNumstat = FileChangeStats | { kind: FileStatKind.Binary }

type ChangedPath = { path: string; originalPath?: string }

/** Preserve Git's -/- binary marker until its actual byte sizes are available. */
function parseNumstatCounts(line: string): ParsedNumstat {
  const [add, del] = line.split('\t')
  if (add === '-' && del === '-') return { kind: FileStatKind.Binary }
  if (!add || !del || !/^\d+$/.test(add) || !/^\d+$/.test(del)) {
    throw new Error(`Invalid git numstat counts: ${line}`)
  }
  return { kind: FileStatKind.Text, additions: Number(add), deletions: Number(del) }
}

/** `--numstat` uses a compact `old => new` path for renames. */
function numstatPath(path: string): string {
  const nested = /^(.*)\{[^{}]* => ([^{}]*)\}(.*)$/.exec(path)
  if (nested) return `${nested[1] ?? ''}${nested[2] ?? ''}${nested[3] ?? ''}`
  const arrow = path.indexOf(' => ')
  return arrow < 0 ? path : path.slice(arrow + 4)
}

function parseNumstat(stdout: string, knownPaths: ReadonlySet<string>): Map<string, ParsedNumstat> {
  const stats = new Map<string, ParsedNumstat>()
  for (const line of stdout.trim().split('\n').filter(Boolean)) {
    const columns = line.split('\t')
    const path = columns.slice(2).join('\t')
    if (!path) throw new Error(`Missing path in git numstat: ${line}`)
    stats.set(knownPaths.has(path) ? path : numstatPath(path), parseNumstatCounts(line))
  }
  return stats
}

/** Parse the only binary stat for one changed path, without interpreting the filename. */
function parseBinaryByteChange(stdout: string, path: string): number {
  const binLines = stdout.split('\n').filter(line => /\|\s*Bin\s/.test(line))
  if (binLines.length !== 1) throw new Error(`Missing or ambiguous binary byte sizes for ${path}`)
  const match = /\|\s*Bin\s+(\d+)\s+->\s+(\d+)\s+bytes\s*$/.exec(binLines[0] ?? '')
  if (!match) throw new Error(`Invalid binary byte sizes for ${path}`)
  const oldSize = Number(match[1])
  const newSize = Number(match[2])
  if (!Number.isSafeInteger(oldSize) || !Number.isSafeInteger(newSize)) {
    throw new Error(`Invalid binary byte sizes for ${path}`)
  }
  return newSize - oldSize
}

function parseNameStatuses(stdout: string): Map<string, { status: FileChangeStatus; originalPath?: string }> {
  const statuses = new Map<string, { status: FileChangeStatus; originalPath?: string }>()
  for (const line of stdout.trim().split('\n').filter(Boolean)) {
    const [change, ...paths] = line.split('\t')
    const path = paths[paths.length - 1]
    if (!path || !change) continue
    const status = change.startsWith('A') ? FileChangeStatus.Added
      : change.startsWith('D') ? FileChangeStatus.Deleted
        : change.startsWith('R') ? FileChangeStatus.Renamed
          : FileChangeStatus.Modified
    statuses.set(path, {
      status,
      ...(change.startsWith('R') && paths.length > 1 ? { originalPath: paths[0] } : {}),
    })
  }
  return statuses
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

  async function fileStats(
    repoPath: string,
    parsed: ParsedNumstat,
    args: string[],
    changed: ChangedPath,
    noIndex = false,
  ): Promise<FileChangeStats> {
    if (parsed.kind === FileStatKind.Text) return parsed
    const paths = changed.originalPath ? [changed.originalPath, changed.path] : [changed.path]
    const result = await git(repoPath, [...args, '--', ...(noIndex ? ['/dev/null', changed.path] : paths)])
    if (result.exitCode !== 0 && !(noIndex && result.exitCode === 1)) throw interpretError(result)
    return { kind: FileStatKind.Binary, byteChange: parseBinaryByteChange(result.stdout, changed.path) }
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

  // Resolve the MAIN worktree root for a repo, even when `dirPath` is itself a
  // linked worktree. `git rev-parse --show-toplevel` returns the linked
  // worktree's own path, which would anchor new worktrees inside a child
  // workspace instead of at the repo root. The first entry of
  // `git worktree list --porcelain` is always the main worktree.
  async function resolveRepoRoot(dirPath: string): Promise<string | null> {
    const result = await git(dirPath, ['worktree', 'list', '--porcelain'])
    if (result.exitCode !== 0) return null
    const line = result.stdout.split('\n').find((l) => l.startsWith('worktree '))
    if (!line) return null
    return line.slice('worktree '.length).trim() || null
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

        const [branchResult, repoRoot, topLevelResult] = await Promise.all([
          git(dirPath, ['rev-parse', '--abbrev-ref', 'HEAD']),
          resolveRepoRoot(dirPath),
          git(dirPath, ['rev-parse', '--show-toplevel']),
        ])

        return {
          isRepo: true,
          branch: branchResult.stdout.trim() || 'HEAD',
          rootPath: repoRoot || topLevelResult.stdout.trim() || dirPath,
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
        // Resolve to the MAIN worktree root so forking a child workspace creates
        // the new worktree at the repo root, not nested inside the child.
        const rootPath = await resolveRepoRoot(repoPath)
        if (!rootPath) {
          return { success: false, error: 'Failed to resolve repository root' }
        }

        const worktreePath = await resolveWorktreePath(rootPath, name)
        const branchName = name

        const args = ['worktree', 'add', '-b', branchName, worktreePath]
        if (baseBranch) args.push(baseBranch)

        const result = await git(repoPath, args, { onProgress, timeoutMs: WORKTREE_ADD_TIMEOUT_MS })
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
        // Resolve to the MAIN worktree root so forking a child workspace creates
        // the new worktree at the repo root, not nested inside the child.
        const rootPath = await resolveRepoRoot(repoPath)
        if (!rootPath) {
          return { success: false, error: 'Failed to resolve repository root' }
        }

        const worktreePath = await resolveWorktreePath(rootPath, worktreeName)

        const result = await git(repoPath, ['worktree', 'add', worktreePath, branch], { onProgress, timeoutMs: WORKTREE_ADD_TIMEOUT_MS })
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

        // Resolve to the MAIN worktree root so forking a child workspace creates
        // the new worktree at the repo root, not nested inside the child.
        const rootPath = await resolveRepoRoot(repoPath)
        if (!rootPath) {
          return { success: false, error: 'Failed to resolve repository root' }
        }

        const worktreePath = await resolveWorktreePath(rootPath, worktreeName)

        const result = await git(repoPath, ['worktree', 'add', '-b', branchName, worktreePath, remoteBranch], { onProgress, timeoutMs: WORKTREE_ADD_TIMEOUT_MS })
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

        if (statResult.exitCode !== 0) throw interpretError(statResult)
        if (nameStatusResult.exitCode !== 0) throw interpretError(nameStatusResult)
        const statusMap = parseNameStatuses(nameStatusResult.stdout)
        const files: DiffFile[] = []
        let totalAdditions = 0
        let totalDeletions = 0

        for (const [path, parsed] of Array.from(parseNumstat(statResult.stdout, new Set(statusMap.keys())))) {
          const change = statusMap.get(path)
          const stats = await fileStats(worktreePath, parsed,
            ['diff', '--stat', mergeBase, currentBranch], { path, originalPath: change?.originalPath })
          files.push({ path, status: change?.status ?? FileChangeStatus.Modified, ...stats })
          if (stats.kind === FileStatKind.Text) {
            totalAdditions += stats.additions
            totalDeletions += stats.deletions
          }
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

        const result = await git(targetWorktreePath, args, { onProgress, timeoutMs: MERGE_TIMEOUT_MS })
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
        // `--untracked-files=all` is required, not a nicety: git's default
        // (`normal`) collapses a wholly-untracked directory into one `dir/` entry.
        // That entry is a directory, so it has no blob to diff against and cannot
        // be read as a file — the review pane renders it as an empty diff and the
        // tree shows a nameless row. Listing every untracked file individually is
        // what makes new files inside new directories reviewable at all.
        const statusResult = await git(repoPath, ['status', '--porcelain', '--untracked-files=all'])
        if (statusResult.exitCode !== 0) throw interpretError(statusResult)

        const status = parseStatus(statusResult.stdout)

        const stagedStatResult = await git(repoPath, ['diff', '--cached', '--numstat'])
        if (stagedStatResult.exitCode !== 0) throw interpretError(stagedStatResult)
        const stagedStatMap = parseNumstat(stagedStatResult.stdout, new Set(status.map(entry => entry.path)))

        const unstagedStatResult = await git(repoPath, ['diff', '--numstat'])
        if (unstagedStatResult.exitCode !== 0) throw interpretError(unstagedStatResult)
        const unstagedStatMap = parseNumstat(unstagedStatResult.stdout, new Set(status.map(entry => entry.path)))

        // `git diff` omits untracked files. Diff each against /dev/null; exit 1
        // means they differ, while exit codes above 1 are errors.
        const untrackedStatMap = new Map<string, ParsedNumstat>()
        await Promise.all(
          status
            .filter((entry) => entry.status === FileChangeStatus.Untracked)
            .map(async (entry) => {
              const result = await git(repoPath, ['diff', '--numstat', '--no-index', '--', '/dev/null', entry.path])
              if (result.exitCode > 1) throw interpretError(result)
              const line = result.stdout.trim().split('\n').filter(Boolean)[0]
              if (line) untrackedStatMap.set(entry.path, parseNumstatCounts(line))
            })
        )

        const files: UncommittedFile[] = await Promise.all(status.map(async (entry) => {
          const statMap = entry.status === FileChangeStatus.Untracked
            ? untrackedStatMap
            : entry.staged ? stagedStatMap : unstagedStatMap
          const parsed = statMap.get(entry.path) ?? { kind: FileStatKind.Text as const, additions: 0, deletions: 0 }
          const args = entry.status === FileChangeStatus.Untracked
            ? ['diff', '--stat', '--no-index']
            : entry.staged ? ['diff', '--cached', '--stat'] : ['diff', '--stat']
          const stats = await fileStats(repoPath, parsed, args, entry, entry.status === FileChangeStatus.Untracked)
          return { ...entry, ...stats }
        }))

        const totalAdditions = files.reduce((sum, file) => sum + (file.kind === FileStatKind.Text ? file.additions : 0), 0)
        const totalDeletions = files.reduce((sum, file) => sum + (file.kind === FileStatKind.Text ? file.deletions : 0), 0)

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
            const fileResult = await filesystem.readFile(repoPath, filePath, MAX_READ_FILE_BYTES)
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

        if (nameStatusResult.exitCode !== 0) throw interpretError(nameStatusResult)
        const statusMap = parseNameStatuses(nameStatusResult.stdout)
        const files: DiffFile[] = []
        for (const [path, parsed] of Array.from(parseNumstat(statResult.stdout, new Set(statusMap.keys())))) {
          const change = statusMap.get(path)
          const stats = await fileStats(repoPath, parsed,
            ['diff-tree', '--no-commit-id', '--root', '-r', '--stat', commitHash],
            { path, originalPath: change?.originalPath })
          files.push({ path, status: change?.status ?? FileChangeStatus.Modified, ...stats })
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

    // ----- isAncestor -----
    async isAncestor(repoPath: string, ancestorRef: string, descendantRef: string): Promise<boolean> {
      // exit 0 = ancestor, exit 1 = not ancestor, other codes (bad ref) also treated as not ancestor.
      const result = await git(repoPath, ['merge-base', '--is-ancestor', ancestorRef, descendantRef])
      return result.exitCode === 0
    },
  }
}
