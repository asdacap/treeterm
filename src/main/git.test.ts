import { describe, it, expect, vi, beforeEach } from 'vitest'
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

function makeMockClient(streams: any[]): GrpcDaemonClient {
  let callIndex = 0
  return {
    execStream: vi.fn(() => streams[callIndex++] ?? buildMockStream([])),
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
      // Use two lines so trim() doesn't eat the leading space on the first entry.
      // When the only output line starts with ' M', trim() removes that space.
      // Adding a staged file first keeps the unstaged line intact.
      const client = makeMockClient([resultStream('A  src/staged.ts\n M src/app.ts\n')])
      const git = new GitClient(client)
      const entries = await git.getStatus('/repo')

      expect(entries).toHaveLength(2)
      const unstaged = entries.find(e => e.path === 'src/app.ts')!
      expect(unstaged.status).toBe('modified')
      expect(unstaged.staged).toBe(false)
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
      expect(staged!.status).toBe('modified')
      expect(unstaged!.status).toBe('modified')
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
      await expect(git.getStatus('/repo')).rejects.toThrow('Git error:')
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
      const currentBranchStream = resultStream('feature')
      const mergeBaseStream = resultStream('base123')
      const numstatStream = resultStream('10\t5\tsrc/app.ts\n3\t0\tsrc/new.ts')
      const nameStatusStream = resultStream('M\tsrc/app.ts\nA\tsrc/new.ts')

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
      expect(appFile!.status).toBe('modified')
      expect(appFile!.additions).toBe(10)
      expect(appFile!.deletions).toBe(5)

      const newFile = diff.files.find(f => f.path === 'src/new.ts')
      expect(newFile!.status).toBe('added')
      expect(newFile!.additions).toBe(3)
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
      expect(imageFile!.additions).toBe(0)
      expect(imageFile!.deletions).toBe(0)
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
      expect(info.branch).toBe('main')
      expect(info.rootPath).toBe('/repo')
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

  describe('getChildWorktrees', () => {
    const worktreeOutput = [
      'worktree /repo',
      'HEAD abc123',
      'branch refs/heads/main',
      '',
      'worktree /repo/.worktrees/feature/sub1',
      'HEAD def456',
      'branch refs/heads/feature/sub1',
      '',
      'worktree /repo/.worktrees/other',
      'HEAD ghi789',
      'branch refs/heads/other',
      '',
    ].join('\n')

    it('returns top-level worktrees when parentBranch is null', async () => {
      const client = makeMockClient([resultStream(worktreeOutput)])
      const git = new GitClient(client)
      const children = await git.getChildWorktrees('/repo', null)
      const branches = children.map(c => c.branch)
      expect(branches).toContain('main')
      expect(branches).toContain('other')
      expect(branches).not.toContain('feature/sub1')
    })

    it('returns children of parent branch with displayName', async () => {
      const client = makeMockClient([resultStream(worktreeOutput)])
      const git = new GitClient(client)
      const children = await git.getChildWorktrees('/repo', 'feature')
      expect(children).toHaveLength(1)
      expect(children[0].branch).toBe('feature/sub1')
      expect(children[0].displayName).toBe('sub1')
    })

    it('returns empty array when parent has no children', async () => {
      const client = makeMockClient([resultStream(worktreeOutput)])
      const git = new GitClient(client)
      const children = await git.getChildWorktrees('/repo', 'nonexistent')
      expect(children).toHaveLength(0)
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
      await expect(git.getStatus('/repo')).rejects.toThrow('Git command failed with exit code 128')
    })
  })
})
