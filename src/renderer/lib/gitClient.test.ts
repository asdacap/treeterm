/* eslint-disable custom/no-string-literal-comparison -- tests verify parsing of git porcelain characters which are external */
import { describe, it, expect, vi } from 'vitest'
import { createGitApi, parseStatus } from './gitClient'
import type { ExecApi, FilesystemApi } from '../types'
import { ExecEventType, type ExecEvent } from '../../shared/ipc-types'
import { FileChangeStatus } from '../../shared/types'

// ---------------------------------------------------------------------------
// Mock helpers (adapted from githubClient.test.ts pattern)
// ---------------------------------------------------------------------------

interface MockExecApi extends ExecApi {
  _complete: (execId: string, stdout: string, exitCode?: number) => void
  _completeWithStderr: (execId: string, stderr: string, exitCode?: number) => void
  _error: (execId: string, message: string) => void
}

function createMockExec(): MockExecApi {
  const eventCallbacks = new Map<string, (event: ExecEvent) => void>()
  let execCounter = 0

  return {
    start: vi.fn().mockImplementation(() => {
      execCounter++
      return Promise.resolve({ success: true, execId: `exec-${String(execCounter)}` })
    }),
    kill: vi.fn(),
    onEvent: vi.fn().mockImplementation((execId: string, cb: (event: ExecEvent) => void) => {
      eventCallbacks.set(execId, cb)
      return () => { eventCallbacks.delete(execId) }
    }),
    _complete: (execId: string, stdout: string, exitCode = 0) => {
      const cb = eventCallbacks.get(execId)
      if (cb) {
        if (stdout) cb({ type: ExecEventType.Stdout, data: stdout })
        cb({ type: ExecEventType.Exit, exitCode })
      }
    },
    _completeWithStderr: (execId: string, stderr: string, exitCode = 1) => {
      const cb = eventCallbacks.get(execId)
      if (cb) {
        if (stderr) cb({ type: ExecEventType.Stderr, data: stderr })
        cb({ type: ExecEventType.Exit, exitCode })
      }
    },
    _error: (execId: string, message: string) => {
      const cb = eventCallbacks.get(execId)
      if (cb) {
        cb({ type: ExecEventType.Error, message })
      }
    },
  }
}

function createMockFilesystem(overrides?: Partial<FilesystemApi>): FilesystemApi {
  return {
    readDirectory: vi.fn(),
    readFile: vi.fn().mockResolvedValue({ success: false, error: 'not found' }),
    writeFile: vi.fn(),
    searchFiles: vi.fn(),
    ...overrides,
  }
}

/**
 * Helper to set up a sequence of git command completions.
 * Each entry describes how to complete the Nth exec call.
 * Returns a helper that auto-completes calls in order.
 */
type CompletionEntry =
  | { stdout: string; exitCode?: number }
  | { stderr: string; exitCode?: number }

function autoComplete(exec: MockExecApi, completions: CompletionEntry[]): void {
  let callIndex = 0
  const origStart = exec.start as ReturnType<typeof vi.fn>
  // eslint-disable-next-line @typescript-eslint/no-misused-promises
  origStart.mockImplementation(() => {
    callIndex++
    const execId = `exec-${String(callIndex)}`
    const completion = completions[callIndex - 1]

    // Schedule completion on macrotask queue so onEvent listener registers first
    if (completion) {
      setTimeout(() => {
        if ('stderr' in completion) {
          exec._completeWithStderr(execId, completion.stderr, completion.exitCode ?? 1)
        } else {
          exec._complete(execId, completion.stdout, completion.exitCode ?? 0)
        }
      })
    }

    return Promise.resolve({ success: true, execId })
  })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('createGitApi', () => {
  describe('parseStatus (via getUncommittedChanges)', () => {
    it('parses modified unstaged file', () => {
      const entries = parseStatus(' M src/app.ts\n')

      expect(entries).toHaveLength(1)
      expect(entries[0]!.path).toBe('src/app.ts')
      expect(entries[0]!.status).toBe(FileChangeStatus.Modified)
      expect(entries[0]!.staged).toBe(false)
    })

    it('parses added staged file', () => {
      const entries = parseStatus('A  src/new.ts')

      expect(entries[0]!.status).toBe(FileChangeStatus.Added)
      expect(entries[0]!.staged).toBe(true)
    })

    it('parses deleted file', () => {
      const entries = parseStatus('D  src/old.ts')

      expect(entries[0]!.status).toBe(FileChangeStatus.Deleted)
    })

    it('parses untracked file', () => {
      const entries = parseStatus('?? src/new.ts')

      expect(entries[0]!.status).toBe(FileChangeStatus.Untracked)
      expect(entries[0]!.staged).toBe(false)
    })

    it('parses renamed file', () => {
      const entries = parseStatus('R  new.ts -> old.ts')

      expect(entries[0]!.status).toBe(FileChangeStatus.Renamed)
      expect(entries[0]!.path).toBe('old.ts')
      expect(entries[0]!.originalPath).toBe('new.ts')
    })

    it('parses file with both staged and unstaged changes', () => {
      const entries = parseStatus('MM src/app.ts')

      expect(entries).toHaveLength(2)
      const staged = entries.find(e => e.path === 'src/app.ts' && e.staged)
      const unstaged = entries.find(e => e.path === 'src/app.ts' && !e.staged)
      expect(staged).toBeDefined()
      expect(unstaged).toBeDefined()
      expect(staged?.status).toBe(FileChangeStatus.Modified)
      expect(unstaged?.status).toBe(FileChangeStatus.Modified)
    })

    it('parses file with only staged changes', () => {
      const entries = parseStatus('M  src/app.ts')

      expect(entries).toHaveLength(1)
      expect(entries[0]!.staged).toBe(true)
      expect(entries[0]!.status).toBe(FileChangeStatus.Modified)
    })

    it('returns empty array for empty status output', () => {
      const entries = parseStatus('')

      expect(entries).toHaveLength(0)
    })
  })

  describe('interpretError (via hasUncommittedChanges and stageFile)', () => {
    it('detects not a git repository error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'not a git repository', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.stageFile('/not-a-repo', 'file.ts')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('Not a git repository')
      }
    })

    it('detects merge conflict error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'merge conflict detected', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.stageFile('/repo', 'file.ts')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('Merge conflict detected')
      }
    })

    it('detects already exists error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'already exists', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.stageFile('/repo', 'file.ts')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('Already exists')
      }
    })

    it('detects pathspec error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'pathspec did not match any files', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.stageFile('/repo', 'missing.ts')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('File not found')
      }
    })

    it('detects failed to merge error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'failed to merge', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.stageFile('/repo', 'file.ts')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('Merge failed')
      }
    })

    it('detects could not resolve error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'could not resolve reference', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.stageFile('/repo', 'file.ts')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('Could not resolve reference')
      }
    })

    it('falls back to generic git error with stderr', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'some unknown error message', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.stageFile('/repo', 'file.ts')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('Git error [git add file.ts]:')
      }
    })

    it('detects nothing to commit in stderr', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'nothing to commit, working tree clean', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.commitStaged('/repo', 'msg')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('No changes to commit')
      }
    })

    it('falls back to exit code message when stderr is empty', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '', exitCode: 128 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.stageFile('/repo', 'file.ts')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('Git command failed [git add file.ts] with exit code 128')
      }
    })
  })

  describe('detectLanguage (via getFileContentsForDiff)', () => {
    it('detects typescript from ts extension', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      // Stream sequence: rev-parse HEAD, merge-base, show original, show HEAD (modified)
      autoComplete(exec, [
        { stdout: 'feature-branch' },
        { stdout: 'abc123' },
        { stdout: 'original content' },
        { stdout: 'modified content' },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getFileContentsForDiff('/repo', 'main', 'src/app.ts')
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.contents.language).toBe('typescript')
      }
    })

    it('detects python from py extension', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'feature-branch' },
        { stdout: 'abc123' },
        { stdout: 'print("hi")' },
        { stdout: 'print("world")' },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getFileContentsForDiff('/repo', 'main', 'script.py')
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.contents.language).toBe('python')
      }
    })

    it('returns plaintext for unknown extension', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'feature-branch' },
        { stdout: 'abc123' },
        { stdout: 'data' },
        { stdout: 'data' },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getFileContentsForDiff('/repo', 'main', 'data.xyz')
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.contents.language).toBe('plaintext')
      }
    })
  })

  describe('listWorktrees', () => {
    it('parses porcelain worktree output', async () => {
      const output = [
        'worktree /repo',
        'HEAD abc123',
        'branch refs/heads/main',
        '',
        'worktree /repo/.worktrees/feature',
        'HEAD def456',
        'branch refs/heads/feature',
        '',
      ].join('\n')

      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: output }])

      const git = createGitApi(exec, fs, 'conn-1')
      const worktrees = await git.listWorktrees('/repo')

      expect(worktrees).toHaveLength(2)
      expect(worktrees[0]!.path).toBe('/repo')
      expect(worktrees[0]!.branch).toBe('main')
      expect(worktrees[1]!.path).toBe('/repo/.worktrees/feature')
      expect(worktrees[1]!.branch).toBe('feature')
    })

    it('throws when worktree list fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'not a git repository', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      await expect(git.listWorktrees('/not-repo')).rejects.toThrow()
    })
  })

  describe('getDiff', () => {
    it('parses numstat and name-status output', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'feature' },                          // current branch
        { stdout: 'base123' },                          // merge base
        { stdout: '10\t5\tsrc/app.ts\n3\t0\tsrc/new.ts' }, // numstat
        { stdout: 'M\tsrc/app.ts\nA\tsrc/new.ts' },    // name-status
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getDiff('/repo', 'main')

      expect(result.success).toBe(true)
      if (result.success) {
        const diff = result.diff
        expect(diff.headBranch).toBe('feature')
        expect(diff.baseBranch).toBe('main')
        expect(diff.totalAdditions).toBe(13)
        expect(diff.totalDeletions).toBe(5)
        expect(diff.files).toHaveLength(2)

        const appFile = diff.files.find(f => f.path === 'src/app.ts')
        expect(appFile?.status).toBe(FileChangeStatus.Modified)
        expect(appFile?.additions).toBe(10)
        expect(appFile?.deletions).toBe(5)

        const newFile = diff.files.find(f => f.path === 'src/new.ts')
        expect(newFile?.status).toBe(FileChangeStatus.Added)
        expect(newFile?.additions).toBe(3)
      }
    })

    it('handles binary files (- in numstat)', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'feature' },
        { stdout: 'base123' },
        { stdout: '-\t-\timage.png' },
        { stdout: 'M\timage.png' },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getDiff('/repo', 'main')

      expect(result.success).toBe(true)
      if (result.success) {
        const imageFile = result.diff.files.find(f => f.path === 'image.png')
        expect(imageFile?.additions).toBe(0)
        expect(imageFile?.deletions).toBe(0)
      }
    })
  })

  describe('getInfo', () => {
    it('returns non-repo info on failure', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'not a git repository', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const info = await git.getInfo('/not-repo')

      expect(info.isRepo).toBe(false)
    })

    it('returns repo info on success', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'true' },    // is-inside-work-tree
        { stdout: 'main' },    // abbrev-ref HEAD (parallel)
        { stdout: '/repo' },   // show-toplevel (parallel)
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const info = await git.getInfo('/repo')

      expect(info.isRepo).toBe(true)
      expect(info).toMatchObject({ isRepo: true, branch: 'main', rootPath: '/repo' })
    })
  })

  describe('hasUncommittedChanges', () => {
    it('returns true when there are uncommitted changes', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: 'M  src/app.ts' }])

      const git = createGitApi(exec, fs, 'conn-1')
      expect(await git.hasUncommittedChanges('/repo')).toBe(true)
    })

    it('returns false when working tree is clean', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '' }])

      const git = createGitApi(exec, fs, 'conn-1')
      expect(await git.hasUncommittedChanges('/repo')).toBe(false)
    })
  })

  describe('stageFile', () => {
    it('returns success on success', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.stageFile('/repo', 'src/app.ts')
      expect(result.success).toBe(true)
    })

    it('returns failure on error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'pathspec did not match any files', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.stageFile('/repo', 'missing.ts')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('File not found')
      }
    })
  })

  describe('unstageFile', () => {
    it('returns success on success', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.unstageFile('/repo', 'src/app.ts')
      expect(result.success).toBe(true)
    })

    it('returns failure on error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'not a git repository', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.unstageFile('/not-repo', 'src/app.ts')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('Not a git repository')
      }
    })
  })

  describe('stageAll', () => {
    it('returns success on success', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.stageAll('/repo')
      expect(result.success).toBe(true)
    })

    it('returns failure on error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'not a git repository', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.stageAll('/not-repo')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('Not a git repository')
      }
    })
  })

  describe('unstageAll', () => {
    it('returns success on success', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.unstageAll('/repo')
      expect(result.success).toBe(true)
    })

    it('returns failure on error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'not a git repository', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.unstageAll('/not-repo')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('Not a git repository')
      }
    })
  })

  describe('commitStaged', () => {
    it('returns success on commit', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '[main abc1234] add feature\n 1 file changed' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.commitStaged('/repo', 'add feature')
      expect(result.success).toBe(true)
    })

    it('returns failure on nothing to commit', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'nothing to commit, working tree clean', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.commitStaged('/repo', 'msg')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('No changes to commit')
      }
    })
  })

  describe('commitAll', () => {
    it('returns success on commit', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '[feature def5678] fix bug\n 2 files changed' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.commitAll('/repo', 'fix bug')
      expect(result.success).toBe(true)
    })

    it('returns failure on nothing to commit', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'nothing to commit', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.commitAll('/repo', 'msg')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('No changes to commit')
      }
    })
  })

  describe('listLocalBranches', () => {
    it('returns list of local branches', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: 'main\nfeature\ndevelop' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const branches = await git.listLocalBranches('/repo')
      expect(branches).toEqual(['main', 'feature', 'develop'])
    })

    it('returns empty array when no branches', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const branches = await git.listLocalBranches('/repo')
      expect(branches).toHaveLength(0)
    })

    it('throws on failure', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'not a git repository', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      await expect(git.listLocalBranches('/not-repo')).rejects.toThrow('Not a git repository')
    })
  })

  describe('listRemoteBranches', () => {
    it('returns list of remote branches', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: 'origin/main\norigin/feature' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const branches = await git.listRemoteBranches('/repo')
      expect(branches).toEqual(['origin/main', 'origin/feature'])
    })

    it('throws on failure', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'not a git repository', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      await expect(git.listRemoteBranches('/not-repo')).rejects.toThrow('Not a git repository')
    })
  })

  describe('deleteBranch', () => {
    it('returns success on deletion', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.deleteBranch('/repo', 'feature')
      expect(result.success).toBe(true)
    })

    it('returns failure on error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'not a git repository', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.deleteBranch('/not-repo', 'feature')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('Not a git repository')
      }
    })
  })

  describe('getHeadCommitHash', () => {
    it('returns trimmed commit hash', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: 'abc123def456\n' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getHeadCommitHash('/repo')
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.hash).toBe('abc123def456')
      }
    })

    it('returns failure on error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'not a git repository', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getHeadCommitHash('/not-repo')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('Not a git repository')
      }
    })
  })

  describe('getUncommittedChanges', () => {
    it('returns files with add/delete stats', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'M  src/app.ts' },      // status (staged)
        { stdout: '5\t2\tsrc/app.ts' },    // staged numstat
        { stdout: '' },                     // unstaged numstat
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getUncommittedChanges('/repo')
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.changes.files).toHaveLength(1)
        expect(result.changes.files[0]!.path).toBe('src/app.ts')
        expect(result.changes.files[0]!.additions).toBe(5)
        expect(result.changes.files[0]!.deletions).toBe(2)
        expect(result.changes.totalAdditions).toBe(5)
        expect(result.changes.totalDeletions).toBe(2)
      }
    })

    it('returns empty result for clean repo', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: '' },  // status
        { stdout: '' },  // staged numstat
        { stdout: '' },  // unstaged numstat
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getUncommittedChanges('/repo')
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.changes.files).toHaveLength(0)
        expect(result.changes.totalAdditions).toBe(0)
        expect(result.changes.totalDeletions).toBe(0)
      }
    })

    it('returns failure when status fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'not a git repository', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getUncommittedChanges('/not-repo')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('Not a git repository')
      }
    })
  })

  describe('getUncommittedFileDiff', () => {
    it('returns diff for staged file', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '@@ -1 +1 @@\n-old\n+new' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getUncommittedFileDiff('/repo', 'src/app.ts', true)
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.diff).toContain('@@ -1 +1 @@')
      }
    })

    it('returns diff for unstaged file', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '@@ -1 +1 @@\n-old\n+new' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getUncommittedFileDiff('/repo', 'src/app.ts', false)
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.diff).toContain('@@ -1 +1 @@')
      }
    })

    it('returns failure on error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'not a git repository', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getUncommittedFileDiff('/not-repo', 'src/app.ts', true)
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('Not a git repository')
      }
    })
  })

  describe('getFileDiff', () => {
    it('returns diff output for file', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'feature' },                      // current branch
        { stdout: 'base123' },                      // merge base
        { stdout: '@@ -1 +1 @@\n-old\n+new' },     // diff output
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getFileDiff('/repo', 'main', 'src/app.ts')
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.diff).toContain('@@ -1 +1 @@')
      }
    })

    it('returns failure when current branch fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'not a git repository', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getFileDiff('/not-repo', 'main', 'src/app.ts')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('Not a git repository')
      }
    })
  })

  describe('fetch', () => {
    it('returns success on success', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.fetch('/repo')
      expect(result.success).toBe(true)
    })

    it('returns failure on error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'could not resolve host', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.fetch('/repo')
      expect(result.success).toBe(false)
    })
  })

  describe('pull', () => {
    it('returns success on exit code 0', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: 'Already up to date.' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.pull('/repo')
      expect(result).toEqual({ success: true })
    })

    it('returns failure with error message on non-zero exit', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'merge conflict', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.pull('/repo')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('merge conflict')
      }
    })

    it('uses fallback message when stderr is empty', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.pull('/repo')
      expect(result).toMatchObject({ success: false, error: 'git pull failed' })
    })
  })

  describe('getBehindCount', () => {
    it('returns parsed count on success', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '5\n' }])

      const git = createGitApi(exec, fs, 'conn-1')
      expect(await git.getBehindCount('/repo')).toBe(5)
    })

    it('returns 0 on failure', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'no upstream', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      expect(await git.getBehindCount('/repo')).toBe(0)
    })

    it('returns 0 for NaN output', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: 'not-a-number\n' }])

      const git = createGitApi(exec, fs, 'conn-1')
      expect(await git.getBehindCount('/repo')).toBe(0)
    })
  })

  describe('getRemoteUrl', () => {
    it('returns trimmed URL on success', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: 'git@github.com:user/repo.git\n' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getRemoteUrl('/repo')
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.url).toBe('git@github.com:user/repo.git')
      }
    })

    it('returns failure on error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'No such remote', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getRemoteUrl('/repo')
      expect(result.success).toBe(false)
    })
  })

  describe('renameBranch', () => {
    it('returns success on success', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.renameBranch('/repo', 'old', 'new')
      expect(result.success).toBe(true)
    })

    it('returns failure on error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'branch not found', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.renameBranch('/repo', 'old', 'new')
      expect(result.success).toBe(false)
    })
  })

  describe('getLog', () => {
    it('parses commits with parentBranch', async () => {
      const logOutput = [
        'abc123\x1eabc\x1eAuthor\x1e2024-01-01T00:00:00Z\x1ecommit msg\x1edef456',
      ].join('\n')
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: logOutput }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getLog('/repo', 'main', 0, 10)

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.result.commits).toHaveLength(1)
        expect(result.result.commits[0]!.hash).toBe('abc123')
        expect(result.result.commits[0]!.shortHash).toBe('abc')
        expect(result.result.commits[0]!.author).toBe('Author')
        expect(result.result.commits[0]!.message).toBe('commit msg')
        expect(result.result.commits[0]!.parentHashes).toEqual(['def456'])
        expect(result.result.hasMore).toBe(false)
      }
    })

    it('parses commits without parentBranch (null)', async () => {
      const logOutput = 'abc\x1ea\x1eAuthor\x1e2024-01-01\x1emsg\x1e'
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: logOutput }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getLog('/repo', null, 0, 10)

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.result.commits).toHaveLength(1)
        expect(result.result.commits[0]!.parentHashes).toEqual([])
      }
    })

    it('detects hasMore when results exceed limit', async () => {
      const lines = Array.from({ length: 3 }, (_, i) =>
        `hash${String(i)}\x1eh${String(i)}\x1eAuthor\x1e2024-01-01\x1emsg${String(i)}\x1e`
      ).join('\n')
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: lines }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getLog('/repo', null, 0, 2)

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.result.hasMore).toBe(true)
        expect(result.result.commits).toHaveLength(2)
      }
    })

    it('returns failure on error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'not a git repository', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getLog('/repo', 'main', 0, 10)
      expect(result.success).toBe(false)
    })
  })

  describe('getCommitDiff', () => {
    it('parses A/M/D/R status types', async () => {
      const nameStatus = 'A\tnew.ts\nM\tmod.ts\nD\tdel.ts\nR100\trenamed.ts'
      const numstat = '10\t0\tnew.ts\n5\t3\tmod.ts\n0\t10\tdel.ts\n2\t1\trenamed.ts'
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: numstat },
        { stdout: nameStatus },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getCommitDiff('/repo', 'abc123')

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.files).toHaveLength(4)
        expect(result.files.find(f => f.path === 'new.ts')?.status).toBe(FileChangeStatus.Added)
        expect(result.files.find(f => f.path === 'mod.ts')?.status).toBe(FileChangeStatus.Modified)
        expect(result.files.find(f => f.path === 'del.ts')?.status).toBe(FileChangeStatus.Deleted)
        expect(result.files.find(f => f.path === 'renamed.ts')?.status).toBe(FileChangeStatus.Renamed)
      }
    })

    it('handles binary files', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: '-\t-\timage.png' },
        { stdout: 'M\timage.png' },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getCommitDiff('/repo', 'abc123')
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.files[0]!.additions).toBe(0)
        expect(result.files[0]!.deletions).toBe(0)
      }
    })

    it('returns failure on error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stderr: 'bad object', exitCode: 1 },
        { stderr: 'bad object', exitCode: 1 },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getCommitDiff('/repo', 'bad')
      expect(result.success).toBe(false)
    })
  })

  describe('getCommitFileDiff', () => {
    it('returns file contents from commit and parent', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'modified content' },
        { stdout: 'original content' },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getCommitFileDiff('/repo', 'abc123', 'file.ts')

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.contents.modifiedContent).toBe('modified content')
        expect(result.contents.originalContent).toBe('original content')
        expect(result.contents.language).toBe('typescript')
      }
    })

    it('returns empty content when show fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stderr: 'path not found', exitCode: 128 },
        { stderr: 'path not found', exitCode: 128 },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getCommitFileDiff('/repo', 'abc123', 'file.ts')

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.contents.modifiedContent).toBe('')
        expect(result.contents.originalContent).toBe('')
      }
    })
  })

  describe('createWorktree', () => {
    it('creates worktree in .worktrees when gitignored', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: '/repo' },   // rev-parse --show-toplevel
        { stdout: '' },        // check-ignore .worktrees (exit 0 = ignored)
        { stdout: '' },        // worktree add
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.createWorktree('/repo', 'feature')

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.path).toContain('.worktrees/feature')
        expect(result.branch).toBe('feature')
      }
    })

    it('creates worktree in home dir when not gitignored', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: '/repo' },                       // rev-parse
        { stderr: '.worktrees is not ignored', exitCode: 1 }, // check-ignore (exit 1 = not ignored)
        { stdout: '/home/user' },                  // resolveHomedir (echo $HOME)
        { stdout: '' },                            // worktree add
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.createWorktree('/repo', 'feature')

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.path).toContain('.treeterm/worktrees')
        expect(result.branch).toBe('feature')
      }
    })

    it('adds baseBranch to args when provided', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: '/repo' },
        { stdout: '' },        // gitignored
        { stdout: '' },        // worktree add
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      await git.createWorktree('/repo', 'feature', 'main')

      // Verify exec.start was called 3 times
      expect(exec.start).toHaveBeenCalledTimes(3)
    })

    it('returns failure when rev-parse fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'not a git repository', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.createWorktree('/repo', 'feature')
      expect(result.success).toBe(false)
    })
  })

  describe('createWorktreeFromBranch', () => {
    it('creates worktree from existing branch', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: '/repo' },   // rev-parse
        { stdout: '' },        // check-ignore
        { stdout: '' },        // worktree add
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.createWorktreeFromBranch('/repo', 'develop', 'develop-wt')

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.branch).toBe('develop')
      }
    })
  })

  describe('createWorktreeFromRemote', () => {
    it('strips remote prefix from branch name', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: '/repo' },   // rev-parse
        { stdout: '' },        // check-ignore
        { stdout: '' },        // worktree add
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.createWorktreeFromRemote('/repo', 'origin/feature', 'feature-wt')

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.branch).toBe('feature')
      }
    })
  })

  describe('removeWorktree', () => {
    it('removes worktree without branch deletion', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: '' },  // worktree remove
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.removeWorktree('/repo', '/repo/.worktrees/feature')
      expect(result.success).toBe(true)
    })

    it('removes worktree with branch deletion', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'feature\n' },  // rev-parse --abbrev-ref HEAD
        { stdout: '' },           // worktree remove
        { stdout: '' },           // branch -D
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.removeWorktree('/repo', '/repo/.worktrees/feature', true)
      expect(result.success).toBe(true)
    })

    it('handles branch deletion failure gracefully', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'feature\n' },                 // rev-parse
        { stdout: '' },                          // worktree remove
        { stderr: 'branch deletion failed', exitCode: 1 },     // branch -D fails
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      // Should not fail despite branch deletion failure
      const result = await git.removeWorktree('/repo', '/repo/.worktrees/feature', true)
      expect(result.success).toBe(true)
    })

    it('handles get-branch failure gracefully when deleteBranch is true', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stderr: 'cannot get branch', exitCode: 1 },  // rev-parse fails
        { stdout: '' },                                  // worktree remove still succeeds
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.removeWorktree('/repo', '/repo/.worktrees/feature', true)
      expect(result.success).toBe(true)
    })
  })

  describe('checkMergeConflicts', () => {
    it('returns no conflicts for clean merge', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.checkMergeConflicts('/repo', 'feature', 'main')

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.conflicts.hasConflicts).toBe(false)
        expect(result.conflicts.conflictedFiles).toEqual([])
        expect(result.conflicts.messages).toEqual([])
      }
    })

    it('detects conflicts', async () => {
      const output = 'conflict in src/app.ts\n<<<<<<< HEAD\nsome content'
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: output }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.checkMergeConflicts('/repo', 'feature', 'main')

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.conflicts.hasConflicts).toBe(true)
        expect(result.conflicts.conflictedFiles.length).toBeGreaterThan(0)
      }
    })
  })

  describe('merge', () => {
    it('merges without squash', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.merge('/repo', 'feature')
      expect(result.success).toBe(true)
    })

    it('merges with squash', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: '' }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.merge('/repo', 'feature', true)
      expect(result.success).toBe(true)
    })

    it('returns failure on error', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'merge conflict detected', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.merge('/repo', 'feature')
      expect(result.success).toBe(false)
    })
  })

  describe('getUncommittedFileContentsForDiff', () => {
    it('returns staged file contents', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'original from HEAD' },
        { stdout: 'modified from index' },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getUncommittedFileContentsForDiff('/repo', 'src/app.ts', true)

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.contents.originalContent).toBe('original from HEAD')
        expect(result.contents.modifiedContent).toBe('modified from index')
        expect(result.contents.language).toBe('typescript')
      }
    })

    it('returns unstaged file contents with filesystem read', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem({
        readFile: vi.fn().mockResolvedValue({ success: true, file: { content: 'working tree content' } }),
      })
      autoComplete(exec, [
        { stdout: 'index content' },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getUncommittedFileContentsForDiff('/repo', 'src/app.ts', false)

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.contents.originalContent).toBe('index content')
        expect(result.contents.modifiedContent).toBe('working tree content')
      }
    })

    it('returns empty content when unstaged filesystem read fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem({
        readFile: vi.fn().mockRejectedValue(new Error('file not found')),
      })
      autoComplete(exec, [
        { stdout: 'index content' },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getUncommittedFileContentsForDiff('/repo', 'src/deleted.ts', false)

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.contents.originalContent).toBe('index content')
        expect(result.contents.modifiedContent).toBe('')
      }
    })

    it('returns empty original when HEAD show fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stderr: 'path not found', exitCode: 128 },
        { stdout: 'new staged content' },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getUncommittedFileContentsForDiff('/repo', 'new-file.ts', true)

      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.contents.originalContent).toBe('')
        expect(result.contents.modifiedContent).toBe('new staged content')
      }
    })
  })

  describe('getBranchesInWorktrees', () => {
    it('returns branch names from worktrees', async () => {
      const output = [
        'worktree /repo', 'HEAD abc', 'branch refs/heads/main', '',
        'worktree /repo/.wt/f', 'HEAD def', 'branch refs/heads/feature', '',
      ].join('\n')
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stdout: output }])

      const git = createGitApi(exec, fs, 'conn-1')
      const branches = await git.getBranchesInWorktrees('/repo')
      expect(branches).toEqual(['main', 'feature'])
    })
  })

  describe('removeWorktree (failure branches)', () => {
    it('returns failure when worktree remove fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stderr: 'worktree is dirty', exitCode: 1 },  // worktree remove fails
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.removeWorktree('/repo', '/repo/.worktrees/feature')
      expect(result.success).toBe(false)
    })

    it('returns failure when worktree remove fails with deleteBranch', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'feature\n' },                   // rev-parse
        { stderr: 'worktree is dirty', exitCode: 1 }, // worktree remove fails
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.removeWorktree('/repo', '/repo/.worktrees/feature', true)
      expect(result.success).toBe(false)
    })

    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('connection lost'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.removeWorktree('/repo', '/repo/.worktrees/feature')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('connection lost')
      }
    })
  })

  describe('createWorktreeFromBranch (failure branches)', () => {
    it('returns failure when rev-parse fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'not a git repository', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.createWorktreeFromBranch('/repo', 'develop', 'wt')
      expect(result.success).toBe(false)
    })

    it('returns failure when worktree add fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: '/repo' },   // rev-parse
        { stdout: '' },        // check-ignore
        { stderr: 'already exists', exitCode: 1 }, // worktree add fails
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.createWorktreeFromBranch('/repo', 'develop', 'wt')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('Already exists')
      }
    })

    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon crash'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.createWorktreeFromBranch('/repo', 'develop', 'wt')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon crash')
      }
    })
  })

  describe('createWorktreeFromRemote (failure branches)', () => {
    it('returns failure when rev-parse fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'not a git repository', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.createWorktreeFromRemote('/repo', 'origin/feature', 'wt')
      expect(result.success).toBe(false)
    })

    it('returns failure when worktree add fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: '/repo' },   // rev-parse
        { stdout: '' },        // check-ignore
        { stderr: 'already exists', exitCode: 1 }, // worktree add fails
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.createWorktreeFromRemote('/repo', 'origin/feature', 'wt')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('Already exists')
      }
    })

    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon crash'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.createWorktreeFromRemote('/repo', 'origin/feature', 'wt')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon crash')
      }
    })
  })

  describe('getDiff (failure branches)', () => {
    it('returns failure when current branch rev-parse fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'not a git repo', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getDiff('/repo', 'main')
      expect(result.success).toBe(false)
    })

    it('returns failure when merge-base fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'feature' },                          // current branch
        { stderr: 'no common ancestor', exitCode: 1 },  // merge-base fails
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getDiff('/repo', 'main')
      expect(result.success).toBe(false)
    })

    it('parses D and R status types', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'feature' },
        { stdout: 'base123' },
        { stdout: '0\t10\tdel.ts\n2\t1\trenamed.ts' },
        { stdout: 'D\tdel.ts\nR100\trenamed.ts' },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getDiff('/repo', 'main')
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.diff.files.find(f => f.path === 'del.ts')?.status).toBe(FileChangeStatus.Deleted)
        expect(result.diff.files.find(f => f.path === 'renamed.ts')?.status).toBe(FileChangeStatus.Renamed)
      }
    })

    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('connection lost'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getDiff('/repo', 'main')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('connection lost')
      }
    })
  })

  describe('getFileDiff (failure branches)', () => {
    it('returns failure when merge-base fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'feature' },
        { stderr: 'no common ancestor', exitCode: 1 },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getFileDiff('/repo', 'main', 'src/app.ts')
      expect(result.success).toBe(false)
    })

    it('returns failure when diff command fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'feature' },
        { stdout: 'base123' },
        { stderr: 'diff failed', exitCode: 1 },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getFileDiff('/repo', 'main', 'src/app.ts')
      expect(result.success).toBe(false)
    })

    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('connection lost'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getFileDiff('/repo', 'main', 'src/app.ts')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('connection lost')
      }
    })
  })

  describe('getFileContentsForDiff (failure branches)', () => {
    it('returns empty content when original show fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'feature' },
        { stdout: 'base123' },
        { stderr: 'path not found', exitCode: 128 }, // original fails
        { stdout: 'modified content' },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getFileContentsForDiff('/repo', 'main', 'new-file.ts')
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.contents.originalContent).toBe('')
        expect(result.contents.modifiedContent).toBe('modified content')
      }
    })

    it('returns empty content when modified show fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'feature' },
        { stdout: 'base123' },
        { stdout: 'original content' },
        { stderr: 'path not found', exitCode: 128 }, // modified fails
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getFileContentsForDiff('/repo', 'main', 'deleted-file.ts')
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.contents.originalContent).toBe('original content')
        expect(result.contents.modifiedContent).toBe('')
      }
    })

    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('connection lost'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getFileContentsForDiff('/repo', 'main', 'file.ts')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('connection lost')
      }
    })
  })

  describe('getInfo (catch branch)', () => {
    it('returns non-repo when exec throws', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon crash'))

      const git = createGitApi(exec, fs, 'conn-1')
      const info = await git.getInfo('/repo')
      expect(info.isRepo).toBe(false)
    })
  })

  describe('checkMergeConflicts (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.checkMergeConflicts('/repo', 'feature', 'main')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('merge (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.merge('/repo', 'feature')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('hasUncommittedChanges (edge cases)', () => {
    it('returns false when exit code is non-zero', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [{ stderr: 'error', exitCode: 1 }])

      const git = createGitApi(exec, fs, 'conn-1')
      expect(await git.hasUncommittedChanges('/repo')).toBe(false)
    })
  })

  describe('getUncommittedFileContentsForDiff (edge cases)', () => {
    it('returns empty modified when unstaged readFile returns failure', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem({
        readFile: vi.fn().mockResolvedValue({ success: false, error: 'not found' }),
      })
      autoComplete(exec, [
        { stdout: 'index content' },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getUncommittedFileContentsForDiff('/repo', 'src/app.ts', false)
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.contents.originalContent).toBe('index content')
        expect(result.contents.modifiedContent).toBe('')
      }
    })

    it('returns empty original when unstaged show fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem({
        readFile: vi.fn().mockResolvedValue({ success: true, file: { content: 'working' } }),
      })
      autoComplete(exec, [
        { stderr: 'path not found', exitCode: 128 }, // show :filePath fails
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getUncommittedFileContentsForDiff('/repo', 'new-file.ts', false)
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.contents.originalContent).toBe('')
        expect(result.contents.modifiedContent).toBe('working')
      }
    })

    it('returns empty modified when staged show fails', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'original from HEAD' },
        { stderr: 'path not found', exitCode: 128 }, // show :filePath fails
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getUncommittedFileContentsForDiff('/repo', 'deleted.ts', true)
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.contents.originalContent).toBe('original from HEAD')
        expect(result.contents.modifiedContent).toBe('')
      }
    })
  })

  describe('commitAll (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.commitAll('/repo', 'msg')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('commitStaged (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.commitStaged('/repo', 'msg')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('stageFile (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.stageFile('/repo', 'file.ts')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('unstageFile (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.unstageFile('/repo', 'file.ts')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('stageAll (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.stageAll('/repo')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('unstageAll (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.unstageAll('/repo')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('deleteBranch (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.deleteBranch('/repo', 'feature')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('renameBranch (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.renameBranch('/repo', 'old', 'new')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('getHeadCommitHash (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getHeadCommitHash('/repo')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('getLog (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getLog('/repo', 'main', 0, 10)
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('getCommitDiff (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getCommitDiff('/repo', 'abc123')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('getCommitFileDiff (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getCommitFileDiff('/repo', 'abc123', 'file.ts')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('fetch (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.fetch('/repo')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('pull (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.pull('/repo')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('getRemoteUrl (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getRemoteUrl('/repo')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('getUncommittedChanges (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getUncommittedChanges('/repo')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('getUncommittedFileDiff (catch branch)', () => {
    it('catches thrown errors', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('daemon down'))

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getUncommittedFileDiff('/repo', 'file.ts', true)
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })
  })

  describe('createWorktree (catch branch)', () => {
    it('catches thrown errors from worktree add', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      // rev-parse succeeds, check-ignore succeeds, but worktree add throws
      let callCount = 0
      vi.mocked(exec.start).mockImplementation(() => {
        callCount++
        if (callCount === 3) return Promise.reject(new Error('disk full'))
        const execId = `exec-${String(callCount)}`
        setTimeout(() => {
          if (callCount === 1) exec._complete(execId, '/repo')
          if (callCount === 2) exec._complete(execId, '')
        })
        return Promise.resolve({ success: true, execId })
      })

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.createWorktree('/repo', 'feature')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('disk full')
      }
    })
  })

  describe('getUncommittedChanges (unstaged stats)', () => {
    it('returns unstaged file stats', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: ' M src/app.ts' },        // status (unstaged)
        { stdout: '' },                       // staged numstat
        { stdout: '3\t1\tsrc/app.ts' },      // unstaged numstat
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getUncommittedChanges('/repo')
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.changes.files).toHaveLength(1)
        expect(result.changes.files[0]!.additions).toBe(3)
        expect(result.changes.files[0]!.deletions).toBe(1)
      }
    })

    it('handles binary files in stats', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      autoComplete(exec, [
        { stdout: 'M  image.png' },
        { stdout: '-\t-\timage.png' },  // binary in staged numstat
        { stdout: '' },
      ])

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.getUncommittedChanges('/repo')
      expect(result.success).toBe(true)
      if (result.success) {
        expect(result.changes.files[0]!.additions).toBe(0)
        expect(result.changes.files[0]!.deletions).toBe(0)
      }
    })
  })

  describe('exec edge cases', () => {
    it('handles exec.start failure', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      ;(exec.start as ReturnType<typeof vi.fn>).mockResolvedValue({ success: false, error: 'daemon down' })

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.fetch('/repo')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('daemon down')
      }
    })

    it('handles error event from exec', async () => {
      const exec = createMockExec()
      const fs = createMockFilesystem()
      // eslint-disable-next-line @typescript-eslint/no-misused-promises
      ;(exec.start as ReturnType<typeof vi.fn>).mockImplementation(() => {
        const execId = 'exec-err'
        setTimeout(() => {
          exec._error(execId, 'stream broke')
        }, 0)
        return Promise.resolve({ success: true, execId })
      })

      const git = createGitApi(exec, fs, 'conn-1')
      const result = await git.fetch('/repo')
      expect(result.success).toBe(false)
      if (!result.success) {
        expect(result.error).toContain('stream broke')
      }
    })
  })
})
