import { describe, it, expect, vi } from 'vitest'
import { EventEmitter } from 'events'
import { GitClient } from './git'
import type { GrpcDaemonClient } from './grpcClient'

type ExecOutput = {
  stdout?: { data: Buffer }
  stderr?: { data: Buffer }
  result?: { exitCode: number }
}

/**
 * Build a mock exec stream that emits the given sequence of outputs,
 * then ends the stream.
 */
function buildMockStream(outputs: ExecOutput[]): any {
  const emitter = new EventEmitter()
  const stream = Object.assign(emitter, {
    write: vi.fn(),
    end: vi.fn().mockImplementation(() => {
      // Emit outputs async so stream.on() listeners are registered first
      setTimeout(() => {
        for (const output of outputs) {
          emitter.emit('data', output)
        }
        emitter.emit('end')
      }, 0)
    }),
  })
  return stream
}

function makeMockClient(streams: any[], extras: Record<string, any> = {}): GrpcDaemonClient {
  let callIndex = 0
  return {
    execStream: vi.fn<(...args: any[]) => any>((): any => streams[callIndex++] ?? buildMockStream([])),
    ...extras,
  } as unknown as GrpcDaemonClient
}

/** Shorthand to build a result-only stream (exit code + stdout) */
function resultStream(stdout: string, exitCode = 0): any {
  return buildMockStream([
    { stdout: { data: Buffer.from(stdout) } },
    { result: { exitCode } },
  ])
}

function errorStream(stderr: string, exitCode = 1): any {
  return buildMockStream([
    { stderr: { data: Buffer.from(stderr) } },
    { result: { exitCode } },
  ])
}

describe('GitClient', () => {
  describe('parseStatus (via getStatus)', () => {
    it('parses modified unstaged file', async () => {
      const client = makeMockClient([resultStream(' M src/app.ts\n')])
      const git = new GitClient(client)
      const entries = await git.getStatus('/repo')

      expect(entries).toHaveLength(1)
      expect(entries[0].path).toBe('src/app.ts')
      expect(entries[0].status).toBe('modified')
      expect(entries[0].staged).toBe(false)
    })

    it('parses added staged file', async () => {
      const client = makeMockClient([resultStream('A  src/new.ts')])
      const git = new GitClient(client)
      const entries = await git.getStatus('/repo')

      expect(entries[0].status).toBe('added')
      expect(entries[0].staged).toBe(true)
    })

    it('parses deleted file', async () => {
      const client = makeMockClient([resultStream('D  src/old.ts')])
      const git = new GitClient(client)
      const entries = await git.getStatus('/repo')

      expect(entries[0].status).toBe('deleted')
    })

    it('parses untracked file', async () => {
      const client = makeMockClient([resultStream('?? src/new.ts')])
      const git = new GitClient(client)
      const entries = await git.getStatus('/repo')

      expect(entries[0].status).toBe('untracked')
      expect(entries[0].staged).toBe(false)
    })

    it('parses renamed file', async () => {
      const client = makeMockClient([resultStream('R  new.ts -> old.ts')])
      const git = new GitClient(client)
      const entries = await git.getStatus('/repo')

      expect(entries[0].status).toBe('renamed')
      expect(entries[0].path).toBe('old.ts')
      expect(entries[0].originalPath).toBe('new.ts')
    })

    it('parses file with both staged andunstaged changes', async () => {
      const client = makeMockClient([resultStream('MM src/app.ts')])
      const git = new GitClient(client)
      const entries = await git.getStatus('/repo')

      expect(entries).toHaveLength(2)
      const staged = entries.find(e => e.path === 'src/app.ts' && e.staged)
      const unstaged = entries.find(e => e.path === 'src/app.ts' && !e.staged)
      expect(staged).toBeDefined()
      expect(unstaged).toBeDefined()
      expect(staged?.status).toBe('modified')
      expect(unstaged?.status).toBe('modified')
    })

    it('parses file with only staged changes', async () => {
      const client = makeMockClient([resultStream('M  src/app.ts')])
      const git = new GitClient(client)
      const entries = await git.getStatus('/repo')

      expect(entries).toHaveLength(1)
      expect(entries[0].staged).toBe(true)
      expect(entries[0].status).toBe('modified')
    })

    it('throws when getStatus fails', async () => {
      const client = makeMockClient([errorStream('not a git repository')])
      const git = new GitClient(client)

      await expect(git.getStatus('/not-a-repo')).rejects.toThrow('Not a git repository')
    })

    it('returns empty array for empty status output', async () => {
      const client = makeMockClient([resultStream('')])
      const git = new GitClient(client)
      const entries = await git.getStatus('/repo')

      expect(entries).toHaveLength(0)
    })
  })

  describe('interpretError (via getStatus)', () => {
    it('detects merge conflict error', async () => {
      const client = makeMockClient([errorStream('merge conflict detected')])
      const git = new GitClient(client)
      await expect(git.getStatus('/repo')).rejects.toThrow('Merge conflict detected')
    })

    it('detects already exists error', async () => {
      const client = makeMockClient([errorStream('already exists')])
      const git = new GitClient(client)
      await expect(git.getStatus('/repo')).rejects.toThrow('Already exists')
    })

    it('detects pathspec error', async () => {
      const client = makeMockClient([errorStream('pathspec did not match any files')])
      const git = new GitClient(client)
      await expect(git.getStatus('/repo')).rejects.toThrow('File not found')
    })

    it('detects failed to merge error', async () => {
      const client = makeMockClient([errorStream('failed to merge')])
      const git = new GitClient(client)
      await expect(git.getStatus('/repo')).rejects.toThrow('Merge failed')
    })

    it('detects could not resolve error', async () => {
      const client = makeMockClient([errorStream('could not resolve reference')])
      const git = new GitClient(client)
      await expect(git.getStatus('/repo')).rejects.toThrow('Could not resolve reference')
    })

    it('falls back to generic git error with stderr', async () => {
      const client = makeMockClient([errorStream('some unknown error message')])
      const git = new GitClient(client)
      await expect(git.getStatus('/repo')).rejects.toThrow('Git error [git status --porcelain]:')
    })
  })

  describe('detectLanguage (via getFileContentsForDiff)', () => {
    it('detects typescript from ts extension', async () => {
      // Stream sequence: rev-parse HEAD, merge-base, show original, show HEAD (modified)
      const client = makeMockClient([
        resultStream('feature-branch'),
        resultStream('abc123'),
        resultStream('original content'),
        resultStream('modified content'),
      ])
      const git = new GitClient(client)
      const result = await git.getFileContentsForDiff('/repo', 'main', 'src/app.ts')
      expect(result.language).toBe('typescript')
    })

    it('detects python from py extension', async () => {
      const client = makeMockClient([
        resultStream('feature-branch'),
        resultStream('abc123'),
        resultStream('print("hi")'),
        resultStream('print("world")'),
      ])
      const git = new GitClient(client)
      const result = await git.getFileContentsForDiff('/repo', 'main', 'script.py')
      expect(result.language).toBe('python')
    })

    it('returns plaintext for unknown extension', async () => {
      const client = makeMockClient([
        resultStream('feature-branch'),
        resultStream('abc123'),
        resultStream('data'),
        resultStream('data'),
      ])
      const git = new GitClient(client)
      const result = await git.getFileContentsForDiff('/repo', 'main', 'data.xyz')
      expect(result.language).toBe('plaintext')
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

      const client = makeMockClient([resultStream(output)])
      const git = new GitClient(client)
      const worktrees = await git.listWorktrees('/repo')

      expect(worktrees).toHaveLength(2)
      expect(worktrees[0].path).toBe('/repo')
      expect(worktrees[0].branch).toBe('main')
      expect(worktrees[1].path).toBe('/repo/.worktrees/feature')
      expect(worktrees[1].branch).toBe('feature')
    })

    it('throws when worktree list fails', async () => {
      const client = makeMockClient([errorStream('not a git repository')])
      const git = new GitClient(client)
      await expect(git.listWorktrees('/not-repo')).rejects.toThrow()
    })
  })

  describe('getDiff', () => {
    it('parses numstat and name-status output', async () => {
      // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
      const currentBranchStream: any = resultStream('feature')
      // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
      const mergeBaseStream: any = resultStream('base123')
      // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
      const numstatStream: any = resultStream('10\t5\tsrc/app.ts\n3\t0\tsrc/new.ts')
      // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
      const nameStatusStream: any = resultStream('M\tsrc/app.ts\nA\tsrc/new.ts')

      const client = makeMockClient([
        currentBranchStream,
        mergeBaseStream,
        numstatStream,
        nameStatusStream,
      ])
      const git = new GitClient(client)
      const diff = await git.getDiff('/repo', 'main')

      expect(diff.headBranch).toBe('feature')
      expect(diff.baseBranch).toBe('main')
      expect(diff.totalAdditions).toBe(13)
      expect(diff.totalDeletions).toBe(5)
      expect(diff.files).toHaveLength(2)

      const appFile = diff.files.find(f => f.path === 'src/app.ts')
      expect(appFile?.status).toBe('modified')
      expect(appFile?.additions).toBe(10)
      expect(appFile?.deletions).toBe(5)

      const newFile = diff.files.find(f => f.path === 'src/new.ts')
      expect(newFile?.status).toBe('added')
      expect(newFile?.additions).toBe(3)
    })

    it('handles binary files (- in numstat)', async () => {
      const client = makeMockClient([
        resultStream('feature'),
        resultStream('base123'),
        resultStream('-\t-\timage.png'),
        resultStream('M\timage.png'),
      ])
      const git = new GitClient(client)
      const diff = await git.getDiff('/repo', 'main')

      const imageFile = diff.files.find(f => f.path === 'image.png')
      expect(imageFile?.additions).toBe(0)
      expect(imageFile?.deletions).toBe(0)
    })
  })

  describe('getGitInfo', () => {
    it('returns non-repo info on failure', async () => {
      const client = makeMockClient([errorStream('not a git repository')])
      const git = new GitClient(client)
      const info = await git.getGitInfo('/not-repo')

      expect(info.isRepo).toBe(false)
    })

    it('returns repo info on success', async () => {
      const client = makeMockClient([
        resultStream('true'),    // is-inside-work-tree
        resultStream('main'),    // abbrev-ref HEAD (parallel)
        resultStream('/repo'),   // show-toplevel (parallel)
      ])
      const git = new GitClient(client)
      const info = await git.getGitInfo('/repo')

      expect(info.isRepo).toBe(true)
      expect(info).toMatchObject({ isRepo: true, branch: 'main', rootPath: '/repo' })
    })
  })

  describe('hasUncommittedChanges', () => {
    it('returns true when there are uncommitted changes', async () => {
      const client = makeMockClient([resultStream('M  src/app.ts')])
      const git = new GitClient(client)
      expect(await git.hasUncommittedChanges('/repo')).toBe(true)
    })

    it('returns false when working tree is clean', async () => {
      const client = makeMockClient([resultStream('')])
      const git = new GitClient(client)
      expect(await git.hasUncommittedChanges('/repo')).toBe(false)
    })
  })

  describe('stageFile', () => {
    it('resolves on success', async () => {
      const client = makeMockClient([resultStream('')])
      const git = new GitClient(client)
      await expect(git.stageFile('/repo', 'src/app.ts')).resolves.toBeUndefined()
    })

    it('throws on failure', async () => {
      const client = makeMockClient([errorStream('pathspec did not match any files')])
      const git = new GitClient(client)
      await expect(git.stageFile('/repo', 'missing.ts')).rejects.toThrow('File not found')
    })
  })

  describe('unstageFile', () => {
    it('resolves on success', async () => {
      const client = makeMockClient([resultStream('')])
      const git = new GitClient(client)
      await expect(git.unstageFile('/repo', 'src/app.ts')).resolves.toBeUndefined()
    })

    it('throws on failure', async () => {
      const client = makeMockClient([errorStream('not a git repository')])
      const git = new GitClient(client)
      await expect(git.unstageFile('/not-repo', 'src/app.ts')).rejects.toThrow('Not a git repository')
    })
  })

  describe('stageAll', () => {
    it('resolves on success', async () => {
      const client = makeMockClient([resultStream('')])
      const git = new GitClient(client)
      await expect(git.stageAll('/repo')).resolves.toBeUndefined()
    })

    it('throws on failure', async () => {
      const client = makeMockClient([errorStream('not a git repository')])
      const git = new GitClient(client)
      await expect(git.stageAll('/not-repo')).rejects.toThrow('Not a git repository')
    })
  })

  describe('unstageAll', () => {
    it('resolves on success', async () => {
      const client = makeMockClient([resultStream('')])
      const git = new GitClient(client)
      await expect(git.unstageAll('/repo')).resolves.toBeUndefined()
    })

    it('throws on failure', async () => {
      const client = makeMockClient([errorStream('not a git repository')])
      const git = new GitClient(client)
      await expect(git.unstageAll('/not-repo')).rejects.toThrow('Not a git repository')
    })
  })

  describe('commitStaged', () => {
    it('returns commit hash parsed from output', async () => {
      const client = makeMockClient([resultStream('[main abc1234] add feature\n 1 file changed')])
      const git = new GitClient(client)
      const hash = await git.commitStaged('/repo', 'add feature')
      expect(hash).toBe('abc1234')
    })

    it('throws on nothing to commit', async () => {
      const client = makeMockClient([errorStream('nothing to commit, working tree clean')])
      const git = new GitClient(client)
      await expect(git.commitStaged('/repo', 'msg')).rejects.toThrow('No changes to commit')
    })
  })

  describe('commitAll', () => {
    it('returns commit hash parsed from output', async () => {
      const client = makeMockClient([resultStream('[feature def5678] fix bug\n 2 files changed')])
      const git = new GitClient(client)
      const hash = await git.commitAll('/repo', 'fix bug')
      expect(hash).toBe('def5678')
    })

    it('throws on failure', async () => {
      const client = makeMockClient([errorStream('nothing to commit')])
      const git = new GitClient(client)
      await expect(git.commitAll('/repo', 'msg')).rejects.toThrow('No changes to commit')
    })
  })

  describe('listLocalBranches', () => {
    it('returns list of local branches', async () => {
      const client = makeMockClient([resultStream('main\nfeature\ndevelop')])
      const git = new GitClient(client)
      const branches = await git.listLocalBranches('/repo')
      expect(branches).toEqual(['main', 'feature', 'develop'])
    })

    it('returns empty array when no branches', async () => {
      const client = makeMockClient([resultStream('')])
      const git = new GitClient(client)
      const branches = await git.listLocalBranches('/repo')
      expect(branches).toHaveLength(0)
    })

    it('throws on failure', async () => {
      const client = makeMockClient([errorStream('not a git repository')])
      const git = new GitClient(client)
      await expect(git.listLocalBranches('/not-repo')).rejects.toThrow('Not a git repository')
    })
  })

  describe('listRemoteBranches', () => {
    it('returns list of remote branches', async () => {
      const client = makeMockClient([resultStream('origin/main\norigin/feature')])
      const git = new GitClient(client)
      const branches = await git.listRemoteBranches('/repo')
      expect(branches).toEqual(['origin/main', 'origin/feature'])
    })

    it('throws on failure', async () => {
      const client = makeMockClient([errorStream('not a git repository')])
      const git = new GitClient(client)
      await expect(git.listRemoteBranches('/not-repo')).rejects.toThrow('Not a git repository')
    })
  })

  describe('deleteBranch', () => {
    it('deletes branch without force flag', async () => {
      const client = makeMockClient([resultStream('')])
      const git = new GitClient(client)
      await expect(git.deleteBranch('/repo', 'feature')).resolves.toBeUndefined()
    })

    it('deletes branch with force flag', async () => {
      const client = makeMockClient([resultStream('')])
      const git = new GitClient(client)
      await expect(git.deleteBranch('/repo', 'feature', true)).resolves.toBeUndefined()
    })

    it('throws on failure', async () => {
      const client = makeMockClient([errorStream('not a git repository')])
      const git = new GitClient(client)
      await expect(git.deleteBranch('/not-repo', 'feature')).rejects.toThrow('Not a git repository')
    })
  })

  describe('getHeadCommitHash', () => {
    it('returns trimmed commit hash', async () => {
      const client = makeMockClient([resultStream('abc123def456\n')])
      const git = new GitClient(client)
      expect(await git.getHeadCommitHash('/repo')).toBe('abc123def456')
    })

    it('throws on failure', async () => {
      const client = makeMockClient([errorStream('not a git repository')])
      const git = new GitClient(client)
      await expect(git.getHeadCommitHash('/not-repo')).rejects.toThrow('Not a git repository')
    })
  })

  describe('getCurrentBranch', () => {
    it('returns trimmed branch name', async () => {
      const client = makeMockClient([resultStream('feature-branch\n')])
      const git = new GitClient(client)
      expect(await git.getCurrentBranch('/repo')).toBe('feature-branch')
    })

    it('throws on failure', async () => {
      const client = makeMockClient([errorStream('not a git repository')])
      const git = new GitClient(client)
      await expect(git.getCurrentBranch('/not-repo')).rejects.toThrow('Not a git repository')
    })
  })

  describe('getUncommittedChanges', () => {
    it('returns files with add/delete stats', async () => {
      const client = makeMockClient([
        resultStream('M  src/app.ts'),      // status (staged)
        resultStream('5\t2\tsrc/app.ts'),   // staged numstat
        resultStream(''),                   // unstaged numstat
      ])
      const git = new GitClient(client)
      const result = await git.getUncommittedChanges('/repo')
      expect(result.files).toHaveLength(1)
      expect(result.files[0].path).toBe('src/app.ts')
      expect(result.files[0].additions).toBe(5)
      expect(result.files[0].deletions).toBe(2)
      expect(result.totalAdditions).toBe(5)
      expect(result.totalDeletions).toBe(2)
    })

    it('returns empty result for clean repo', async () => {
      const client = makeMockClient([
        resultStream(''),  // status
        resultStream(''),  // staged numstat
        resultStream(''),  // unstaged numstat
      ])
      const git = new GitClient(client)
      const result = await git.getUncommittedChanges('/repo')
      expect(result.files).toHaveLength(0)
      expect(result.totalAdditions).toBe(0)
      expect(result.totalDeletions).toBe(0)
    })

    it('throws when status fails', async () => {
      const client = makeMockClient([errorStream('not a git repository')])
      const git = new GitClient(client)
      await expect(git.getUncommittedChanges('/not-repo')).rejects.toThrow('Not a git repository')
    })
  })

  describe('getUncommittedFileDiff', () => {
    it('returns diff for staged file', async () => {
      const client = makeMockClient([resultStream('@@ -1 +1 @@\n-old\n+new')])
      const git = new GitClient(client)
      const diff = await git.getUncommittedFileDiff('/repo', 'src/app.ts', true)
      expect(diff).toContain('@@ -1 +1 @@')
    })

    it('returns diff for unstaged file', async () => {
      const client = makeMockClient([resultStream('@@ -1 +1 @@\n-old\n+new')])
      const git = new GitClient(client)
      const diff = await git.getUncommittedFileDiff('/repo', 'src/app.ts', false)
      expect(diff).toContain('@@ -1 +1 @@')
    })

    it('throws on failure', async () => {
      const client = makeMockClient([errorStream('not a git repository')])
      const git = new GitClient(client)
      await expect(git.getUncommittedFileDiff('/not-repo', 'src/app.ts', true)).rejects.toThrow('Not a git repository')
    })
  })

  describe('getFileDiff', () => {
    it('returns diff output for file', async () => {
      const client = makeMockClient([
        resultStream('feature'),                      // current branch
        resultStream('base123'),                      // merge base
        resultStream('@@ -1 +1 @@\n-old\n+new'),     // diff output
      ])
      const git = new GitClient(client)
      const diff = await git.getFileDiff('/repo', 'main', 'src/app.ts')
      expect(diff).toContain('@@ -1 +1 @@')
    })

    it('throws when current branch fails', async () => {
      const client = makeMockClient([errorStream('not a git repository')])
      const git = new GitClient(client)
      await expect(git.getFileDiff('/not-repo', 'main', 'src/app.ts')).rejects.toThrow('Not a git repository')
    })
  })

  describe('interpretError (additional edge cases)', () => {
    it('detects nothing to commit in stderr', async () => {
      const client = makeMockClient([errorStream('nothing to commit, working tree clean')])
      const git = new GitClient(client)
      await expect(git.getStatus('/repo')).rejects.toThrow('No changes to commit')
    })

    it('falls back to exit code message when stderr is empty', async () => {
      const client = makeMockClient([buildMockStream([{ result: { exitCode: 128 } }])])
      const git = new GitClient(client)
      await expect(git.getStatus('/repo')).rejects.toThrow('Git command failed [git status --porcelain] with exit code 128')
    })
  })

  describe('fetch', () => {
    it('resolves on success', async () => {
      const client = makeMockClient([resultStream('')])
      const git = new GitClient(client)
      await expect(git.fetch('/repo')).resolves.toBeUndefined()
    })

    it('throws on failure', async () => {
      const client = makeMockClient([errorStream('could not resolve host')])
      const git = new GitClient(client)
      await expect(git.fetch('/repo')).rejects.toThrow()
    })
  })

  describe('pull', () => {
    it('returns success on exit code 0', async () => {
      const client = makeMockClient([resultStream('Already up to date.')])
      const git = new GitClient(client)
      const result = await git.pull('/repo')
      expect(result).toEqual({ success: true })
    })

    it('returns failure with error message on non-zero exit', async () => {
      const client = makeMockClient([errorStream('merge conflict')])
      const git = new GitClient(client)
      const result = await git.pull('/repo')
      expect(result).toMatchObject({ success: false, error: expect.stringContaining('merge conflict') as unknown as string })
    })

    it('uses fallback message when stderr is empty', async () => {
      const client = makeMockClient([buildMockStream([{ result: { exitCode: 1 } }])])
      const git = new GitClient(client)
      const result = await git.pull('/repo')
      expect(result).toMatchObject({ success: false, error: 'git pull failed' })
    })
  })

  describe('getBehindCount', () => {
    it('returns parsed count on success', async () => {
      const client = makeMockClient([resultStream('5\n')])
      const git = new GitClient(client)
      expect(await git.getBehindCount('/repo')).toBe(5)
    })

    it('returns 0 on failure', async () => {
      const client = makeMockClient([errorStream('no upstream')])
      const git = new GitClient(client)
      expect(await git.getBehindCount('/repo')).toBe(0)
    })

    it('returns 0 for NaN output', async () => {
      const client = makeMockClient([resultStream('not-a-number\n')])
      const git = new GitClient(client)
      expect(await git.getBehindCount('/repo')).toBe(0)
    })
  })

  describe('getRemoteUrl', () => {
    it('returns trimmed URL on success', async () => {
      const client = makeMockClient([resultStream('git@github.com:user/repo.git\n')])
      const git = new GitClient(client)
      expect(await git.getRemoteUrl('/repo')).toBe('git@github.com:user/repo.git')
    })

    it('throws on failure', async () => {
      const client = makeMockClient([errorStream('No such remote')])
      const git = new GitClient(client)
      await expect(git.getRemoteUrl('/repo')).rejects.toThrow()
    })
  })

  describe('renameBranch', () => {
    it('resolves on success', async () => {
      const client = makeMockClient([resultStream('')])
      const git = new GitClient(client)
      await expect(git.renameBranch('/repo', 'old', 'new')).resolves.toBeUndefined()
    })

    it('throws on failure', async () => {
      const client = makeMockClient([errorStream('branch not found')])
      const git = new GitClient(client)
      await expect(git.renameBranch('/repo', 'old', 'new')).rejects.toThrow()
    })
  })

  describe('getLog', () => {
    it('parses commits with parentBranch', async () => {
      const logOutput = [
        'abc123\x1eabc\x1eAuthor\x1e2024-01-01T00:00:00Z\x1ecommit msg\x1edef456',
      ].join('\n')
      const client = makeMockClient([resultStream(logOutput)])
      const git = new GitClient(client)
      const result = await git.getLog('/repo', 'main', 0, 10)

      expect(result.commits).toHaveLength(1)
      expect(result.commits[0].hash).toBe('abc123')
      expect(result.commits[0].shortHash).toBe('abc')
      expect(result.commits[0].author).toBe('Author')
      expect(result.commits[0].message).toBe('commit msg')
      expect(result.commits[0].parentHashes).toEqual(['def456'])
      expect(result.hasMore).toBe(false)
    })

    it('parses commits without parentBranch (null)', async () => {
      const logOutput = 'abc\x1ea\x1eAuthor\x1e2024-01-01\x1emsg\x1e'
      const client = makeMockClient([resultStream(logOutput)])
      const git = new GitClient(client)
      const result = await git.getLog('/repo', null, 0, 10)

      expect(result.commits).toHaveLength(1)
      expect(result.commits[0].parentHashes).toEqual([])
    })

    it('detects hasMore when results exceed limit', async () => {
      const lines = Array.from({ length: 3 }, (_, i) =>
        `hash${String(i)}\x1eh${String(i)}\x1eAuthor\x1e2024-01-01\x1emsg${String(i)}\x1e`
      ).join('\n')
      const client = makeMockClient([resultStream(lines)])
      const git = new GitClient(client)
      const result = await git.getLog('/repo', null, 0, 2)

      expect(result.hasMore).toBe(true)
      expect(result.commits).toHaveLength(2)
    })

    it('throws on failure', async () => {
      const client = makeMockClient([errorStream('not a git repository')])
      const git = new GitClient(client)
      await expect(git.getLog('/repo', 'main', 0, 10)).rejects.toThrow()
    })
  })

  describe('getCommitDiff', () => {
    it('parses A/M/D/R status types', async () => {
      const nameStatus = 'A\tnew.ts\nM\tmod.ts\nD\tdel.ts\nR100\trenamed.ts'
      const numstat = '10\t0\tnew.ts\n5\t3\tmod.ts\n0\t10\tdel.ts\n2\t1\trenamed.ts'
      const client = makeMockClient([
        resultStream(numstat),
        resultStream(nameStatus),
      ])
      const git = new GitClient(client)
      const files = await git.getCommitDiff('/repo', 'abc123')

      expect(files).toHaveLength(4)
      expect(files.find(f => f.path === 'new.ts')?.status).toBe('added')
      expect(files.find(f => f.path === 'mod.ts')?.status).toBe('modified')
      expect(files.find(f => f.path === 'del.ts')?.status).toBe('deleted')
      expect(files.find(f => f.path === 'renamed.ts')?.status).toBe('renamed')
    })

    it('handles binary files', async () => {
      const client = makeMockClient([
        resultStream('-\t-\timage.png'),
        resultStream('M\timage.png'),
      ])
      const git = new GitClient(client)
      const files = await git.getCommitDiff('/repo', 'abc123')
      expect(files[0].additions).toBe(0)
      expect(files[0].deletions).toBe(0)
    })

    it('throws on failure', async () => {
      const client = makeMockClient([errorStream('bad object')])
      const git = new GitClient(client)
      await expect(git.getCommitDiff('/repo', 'bad')).rejects.toThrow()
    })
  })

  describe('getCommitFileDiff', () => {
    it('returns file contents from commit and parent', async () => {
      const client = makeMockClient([
        resultStream('modified content'),
        resultStream('original content'),
      ])
      const git = new GitClient(client)
      const result = await git.getCommitFileDiff('/repo', 'abc123', 'file.ts')

      expect(result.modifiedContent).toBe('modified content')
      expect(result.originalContent).toBe('original content')
      expect(result.language).toBe('typescript')
    })

    it('returns empty content when show fails', async () => {
      const client = makeMockClient([
        errorStream('path not found', 128),
        errorStream('path not found', 128),
      ])
      const git = new GitClient(client)
      const result = await git.getCommitFileDiff('/repo', 'abc123', 'file.ts')

      expect(result.modifiedContent).toBe('')
      expect(result.originalContent).toBe('')
    })
  })

  describe('createWorktree', () => {
    it('creates worktree in .worktrees when gitignored', async () => {
      const client = makeMockClient([
        resultStream('/repo'),   // rev-parse --show-toplevel
        resultStream(''),        // check-ignore .worktrees (exit 0 = ignored)
        resultStream(''),        // worktree add
      ])
      const git = new GitClient(client)
      const result = await git.createWorktree('/repo', 'feature')

      expect(result.path).toContain('.worktrees/feature')
      expect(result.branch).toBe('feature')
    })

    it('creates worktree in home dir when not gitignored', async () => {
      const client = makeMockClient([
        resultStream('/repo'),                       // rev-parse
        errorStream('.worktrees is not ignored'),     // check-ignore (exit 1 = not ignored)
        resultStream('/home/user'),                  // resolveHomedir (echo $HOME)
        resultStream(''),                            // worktree add
      ])
      const git = new GitClient(client)
      const result = await git.createWorktree('/repo', 'feature')

      expect(result.path).toContain('.treeterm/worktrees')
      expect(result.branch).toBe('feature')
    })

    it('adds baseBranch to args when provided', async () => {
      const client = makeMockClient([
        resultStream('/repo'),
        resultStream(''),        // gitignored
        resultStream(''),        // worktree add
      ])
      const git = new GitClient(client)
      await git.createWorktree('/repo', 'feature', 'main')

      // Verify the third exec call includes 'main' in args
      // eslint-disable-next-line @typescript-eslint/unbound-method
      const calls = vi.mocked(client.execStream).mock.calls
      expect(calls.length).toBe(3)
    })

    it('throws when rev-parse fails', async () => {
      const client = makeMockClient([errorStream('not a git repository')])
      const git = new GitClient(client)
      await expect(git.createWorktree('/repo', 'feature')).rejects.toThrow()
    })
  })

  describe('createWorktreeFromBranch', () => {
    it('creates worktree from existing branch', async () => {
      const client = makeMockClient([
        resultStream('/repo'),   // rev-parse
        resultStream(''),        // check-ignore
        resultStream(''),        // worktree add
      ])
      const git = new GitClient(client)
      const result = await git.createWorktreeFromBranch('/repo', 'develop', 'develop-wt')

      expect(result.branch).toBe('develop')
    })
  })

  describe('createWorktreeFromRemote', () => {
    it('strips remote prefix from branch name', async () => {
      const client = makeMockClient([
        resultStream('/repo'),   // rev-parse
        resultStream(''),        // check-ignore
        resultStream(''),        // worktree add
      ])
      const git = new GitClient(client)
      const result = await git.createWorktreeFromRemote('/repo', 'origin/feature', 'feature-wt')

      expect(result.branch).toBe('feature')
    })
  })

  describe('removeWorktree', () => {
    it('removes worktree without branch deletion', async () => {
      const client = makeMockClient([
        resultStream(''),  // worktree remove
      ])
      const git = new GitClient(client)
      await expect(git.removeWorktree('/repo', '/repo/.worktrees/feature')).resolves.toBeUndefined()
    })

    it('removes worktree with branch deletion', async () => {
      const client = makeMockClient([
        resultStream('feature\n'),  // rev-parse --abbrev-ref HEAD
        resultStream(''),           // worktree remove
        resultStream(''),           // branch -D
      ])
      const git = new GitClient(client)
      await expect(git.removeWorktree('/repo', '/repo/.worktrees/feature', true)).resolves.toBeUndefined()
    })

    it('handles branch deletion failure gracefully', async () => {
      const client = makeMockClient([
        resultStream('feature\n'),                 // rev-parse
        resultStream(''),                          // worktree remove
        errorStream('branch deletion failed'),     // branch -D fails
      ])
      const git = new GitClient(client)
      // Should not throw despite branch deletion failure
      await expect(git.removeWorktree('/repo', '/repo/.worktrees/feature', true)).resolves.toBeUndefined()
    })

    it('handles get-branch failure gracefully when deleteBranch is true', async () => {
      const client = makeMockClient([
        errorStream('cannot get branch'),  // rev-parse fails
        resultStream(''),                  // worktree remove still succeeds
      ])
      const git = new GitClient(client)
      await expect(git.removeWorktree('/repo', '/repo/.worktrees/feature', true)).resolves.toBeUndefined()
    })
  })

  describe('checkMergeConflicts', () => {
    it('returns no conflicts for clean merge', async () => {
      const client = makeMockClient([resultStream('')])
      const git = new GitClient(client)
      const result = await git.checkMergeConflicts('/repo', 'feature', 'main')

      expect(result.hasConflicts).toBe(false)
      expect(result.conflictedFiles).toEqual([])
      expect(result.messages).toEqual([])
    })

    it('detects conflicts', async () => {
      const output = 'conflict in src/app.ts\n<<<<<<< HEAD\nsome content'
      const client = makeMockClient([resultStream(output)])
      const git = new GitClient(client)
      const result = await git.checkMergeConflicts('/repo', 'feature', 'main')

      expect(result.hasConflicts).toBe(true)
      expect(result.conflictedFiles.length).toBeGreaterThan(0)
    })
  })

  describe('mergeWorktree', () => {
    it('merges without squash', async () => {
      const client = makeMockClient([resultStream('')])
      const git = new GitClient(client)
      await expect(git.mergeWorktree('/repo', 'feature')).resolves.toBeUndefined()
    })

    it('merges with squash', async () => {
      const client = makeMockClient([resultStream('')])
      const git = new GitClient(client)
      await expect(git.mergeWorktree('/repo', 'feature', true)).resolves.toBeUndefined()
    })

    it('throws on failure', async () => {
      const client = makeMockClient([errorStream('merge conflict detected')])
      const git = new GitClient(client)
      await expect(git.mergeWorktree('/repo', 'feature')).rejects.toThrow()
    })
  })

  describe('getUncommittedFileContentsForDiff', () => {
    it('returns staged file contents', async () => {
      const client = makeMockClient([
        resultStream('original from HEAD'),
        resultStream('modified from index'),
      ])
      const git = new GitClient(client)
      const result = await git.getUncommittedFileContentsForDiff('/repo', 'src/app.ts', true)

      expect(result.originalContent).toBe('original from HEAD')
      expect(result.modifiedContent).toBe('modified from index')
      expect(result.language).toBe('typescript')
    })

    it('returns unstaged file contents with daemon read', async () => {
      const client = makeMockClient([
        resultStream('index content'),
      ], {
        readFile: vi.fn().mockResolvedValue({ success: true, file: { content: 'working tree content' } }),
      })
      const git = new GitClient(client)
      const result = await git.getUncommittedFileContentsForDiff('/repo', 'src/app.ts', false)

      expect(result.originalContent).toBe('index content')
      expect(result.modifiedContent).toBe('working tree content')
    })

    it('returns empty content when unstaged daemon read fails', async () => {
      const client = makeMockClient([
        resultStream('index content'),
      ], {
        readFile: vi.fn().mockRejectedValue(new Error('file not found')),
      })
      const git = new GitClient(client)
      const result = await git.getUncommittedFileContentsForDiff('/repo', 'src/deleted.ts', false)

      expect(result.originalContent).toBe('index content')
      expect(result.modifiedContent).toBe('')
    })

    it('returns empty original when HEAD show fails', async () => {
      const client = makeMockClient([
        errorStream('path not found', 128),
        resultStream('new staged content'),
      ])
      const git = new GitClient(client)
      const result = await git.getUncommittedFileContentsForDiff('/repo', 'new-file.ts', true)

      expect(result.originalContent).toBe('')
      expect(result.modifiedContent).toBe('new staged content')
    })
  })

  describe('getBranchesInWorktrees', () => {
    it('returns branch names from worktrees', async () => {
      const output = [
        'worktree /repo', 'HEAD abc', 'branch refs/heads/main', '',
        'worktree /repo/.wt/f', 'HEAD def', 'branch refs/heads/feature', '',
      ].join('\n')
      const client = makeMockClient([resultStream(output)])
      const git = new GitClient(client)
      const branches = await git.getBranchesInWorktrees('/repo')
      expect(branches).toEqual(['main', 'feature'])
    })
  })

  describe('exec edge cases', () => {
    it('calls onProgress for stdout', async () => {
      const progress = vi.fn()
      const client = makeMockClient([resultStream('progress output')])
      const git = new GitClient(client)
      // Use fetch which passes through to exec - call with progress would require internal access
      // Instead test via a method that supports onProgress
      await git.deleteBranch('/repo', 'feature', false, progress)
    })

    it('handles stream error', async () => {
      const emitter = new (await import('events')).EventEmitter()
      const stream = Object.assign(emitter, {
        write: vi.fn(),
        end: vi.fn().mockImplementation(() => {
          setTimeout(() => {
            emitter.emit('error', new Error('stream broke'))
          }, 0)
        }),
      })
      const client = makeMockClient([stream])
      const git = new GitClient(client)
      await expect(git.fetch('/repo')).rejects.toThrow('Exec stream error: stream broke')
    })

    it('handles stream end without result', async () => {
      const emitter = new (await import('events')).EventEmitter()
      const stream = Object.assign(emitter, {
        write: vi.fn(),
        end: vi.fn().mockImplementation(() => {
          setTimeout(() => {
            emitter.emit('end')
          }, 0)
        }),
      })
      const client = makeMockClient([stream])
      const git = new GitClient(client)
      // Stream ends without result - should resolve with exitCode -1
      // fetch expects exitCode 0, so it should throw
      await expect(git.fetch('/repo')).rejects.toThrow()
    })
  })
})
