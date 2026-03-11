import { describe, it, expect, vi, beforeEach } from 'vitest'
import { simpleGit } from 'simple-git'

vi.mock('simple-git')

// Import after mocks are set up
import { getGitInfo, listWorktrees, getChildWorktrees } from './git'

describe('git', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('getGitInfo', () => {
    it('returns isRepo: false for non-git directory', async () => {
      vi.mocked(simpleGit).mockReturnValue({
        checkIsRepo: vi.fn().mockResolvedValue(false)
      } as any)

      const result = await getGitInfo('/some/path')

      expect(result.isRepo).toBe(false)
      expect(result.branch).toBeNull()
      expect(result.rootPath).toBeNull()
    })

    it('returns branch and rootPath for valid repo', async () => {
      vi.mocked(simpleGit).mockReturnValue({
        checkIsRepo: vi.fn().mockResolvedValue(true),
        revparse: vi
          .fn()
          .mockResolvedValueOnce('feature-branch\n')
          .mockResolvedValueOnce('/repo/root\n')
      } as any)

      const result = await getGitInfo('/repo/root')

      expect(result).toEqual({
        isRepo: true,
        branch: 'feature-branch',
        rootPath: '/repo/root'
      })
    })

    it('returns isRepo: false on error', async () => {
      vi.mocked(simpleGit).mockReturnValue({
        checkIsRepo: vi.fn().mockRejectedValue(new Error('Git error'))
      } as any)

      const result = await getGitInfo('/some/path')

      expect(result.isRepo).toBe(false)
    })
  })

  describe('listWorktrees', () => {
    it('parses porcelain output correctly', async () => {
      const porcelainOutput = `worktree /path/to/main
branch refs/heads/main

worktree /path/to/feature
branch refs/heads/feature

`
      vi.mocked(simpleGit).mockReturnValue({
        raw: vi.fn().mockResolvedValue(porcelainOutput)
      } as any)

      const result = await listWorktrees('/path/to/main')

      expect(result).toHaveLength(2)
      expect(result[0]).toEqual({ path: '/path/to/main', branch: 'main' })
      expect(result[1]).toEqual({ path: '/path/to/feature', branch: 'feature' })
    })

    it('returns empty array on error', async () => {
      vi.mocked(simpleGit).mockReturnValue({
        raw: vi.fn().mockRejectedValue(new Error('Git error'))
      } as any)

      const result = await listWorktrees('/some/path')

      expect(result).toEqual([])
    })
  })

  describe('getChildWorktrees', () => {
    it('finds child worktrees when parent is on top-level branch (main)', async () => {
      const porcelainOutput = `worktree /path/to/repo
HEAD abc123
branch refs/heads/main

worktree /path/to/repo/.worktrees/feature
HEAD def456
branch refs/heads/main/feature

`
      vi.mocked(simpleGit).mockReturnValue({
        raw: vi.fn().mockResolvedValue(porcelainOutput)
      } as any)

      const result = await getChildWorktrees('/path/to/repo', 'main')

      expect(result).toHaveLength(1)
      expect(result[0].branch).toBe('main/feature')
      expect(result[0].displayName).toBe('feature')
    })

    it('finds child worktrees when parent is on any top-level branch', async () => {
      const porcelainOutput = `worktree /path/to/repo
HEAD abc123
branch refs/heads/code-knights-features

worktree /path/to/repo-rankingwebserver
HEAD def456
branch refs/heads/code-knights-features/rankingwebserver

`
      vi.mocked(simpleGit).mockReturnValue({
        raw: vi.fn().mockResolvedValue(porcelainOutput)
      } as any)

      const result = await getChildWorktrees('/path/to/repo', 'code-knights-features')

      expect(result).toHaveLength(1)
      expect(result[0].branch).toBe('code-knights-features/rankingwebserver')
      expect(result[0].displayName).toBe('rankingwebserver')
    })

    it('finds nested child worktrees when parent is on hierarchical branch', async () => {
      const porcelainOutput = `worktree /path/to/repo
HEAD abc123
branch refs/heads/main

worktree /path/to/repo/.worktrees/feature
HEAD def456
branch refs/heads/feature

worktree /path/to/repo/.worktrees/feature-sub
HEAD ghi789
branch refs/heads/feature/sub

`
      vi.mocked(simpleGit).mockReturnValue({
        raw: vi.fn().mockResolvedValue(porcelainOutput)
      } as any)

      const result = await getChildWorktrees('/path/to/repo', 'feature')

      expect(result).toHaveLength(1)
      expect(result[0].branch).toBe('feature/sub')
      expect(result[0].displayName).toBe('sub')
    })

    it('finds top-level worktrees when parentBranch is null', async () => {
      const porcelainOutput = `worktree /path/to/repo
HEAD abc123
branch refs/heads/main

worktree /path/to/repo/.worktrees/feature
HEAD def456
branch refs/heads/feature

worktree /path/to/repo/.worktrees/bugfix
HEAD ghi789
branch refs/heads/bugfix

worktree /path/to/repo/.worktrees/feature-sub
HEAD jkl012
branch refs/heads/feature/sub

`
      vi.mocked(simpleGit).mockReturnValue({
        raw: vi.fn().mockResolvedValue(porcelainOutput)
      } as any)

      const result = await getChildWorktrees('/path/to/repo', null)

      // Should find only top-level branches (no '/')
      expect(result).toHaveLength(3)
      expect(result[0].branch).toBe('main')
      expect(result[1].branch).toBe('feature')
      expect(result[2].branch).toBe('bugfix')
    })

    it('excludes worktrees with nested slashes (grandchildren)', async () => {
      const porcelainOutput = `worktree /path/to/repo
HEAD abc123
branch refs/heads/main

worktree /path/to/repo/.worktrees/feature
HEAD def456
branch refs/heads/main/feature

worktree /path/to/repo/.worktrees/feature-sub
HEAD ghi789
branch refs/heads/main/feature/sub

`
      vi.mocked(simpleGit).mockReturnValue({
        raw: vi.fn().mockResolvedValue(porcelainOutput)
      } as any)

      const result = await getChildWorktrees('/path/to/repo', 'main')

      // Should only include main/feature, not main/feature/sub
      expect(result).toHaveLength(1)
      expect(result[0].branch).toBe('main/feature')
    })
  })
})
