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
  })
})
