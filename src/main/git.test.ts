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
    it('finds child worktrees when parent is on non-treeterm branch (main)', async () => {
      const porcelainOutput = `worktree /path/to/repo
HEAD abc123
branch refs/heads/main

worktree /path/to/repo/.worktrees/feature
HEAD def456
branch refs/heads/treeterm/feature

`
      vi.mocked(simpleGit).mockReturnValue({
        raw: vi.fn().mockResolvedValue(porcelainOutput)
      } as any)

      const result = await getChildWorktrees('/path/to/repo', 'main')

      expect(result).toHaveLength(1)
      expect(result[0].branch).toBe('treeterm/feature')
      expect(result[0].displayName).toBe('feature')
    })

    it('finds child worktrees when parent is on any non-treeterm branch', async () => {
      const porcelainOutput = `worktree /path/to/repo
HEAD abc123
branch refs/heads/code-knights-features

worktree /path/to/repo-rankingwebserver
HEAD def456
branch refs/heads/treeterm/rankingwebserver

`
      vi.mocked(simpleGit).mockReturnValue({
        raw: vi.fn().mockResolvedValue(porcelainOutput)
      } as any)

      const result = await getChildWorktrees('/path/to/repo', 'code-knights-features')

      expect(result).toHaveLength(1)
      expect(result[0].branch).toBe('treeterm/rankingwebserver')
      expect(result[0].displayName).toBe('rankingwebserver')
    })

    it('finds nested child worktrees when parent is on treeterm/* branch', async () => {
      const porcelainOutput = `worktree /path/to/repo
HEAD abc123
branch refs/heads/main

worktree /path/to/repo/.worktrees/feature
HEAD def456
branch refs/heads/treeterm/feature

worktree /path/to/repo/.worktrees/feature-sub
HEAD ghi789
branch refs/heads/treeterm/feature/sub

`
      vi.mocked(simpleGit).mockReturnValue({
        raw: vi.fn().mockResolvedValue(porcelainOutput)
      } as any)

      const result = await getChildWorktrees('/path/to/repo', 'treeterm/feature')

      expect(result).toHaveLength(1)
      expect(result[0].branch).toBe('treeterm/feature/sub')
      expect(result[0].displayName).toBe('sub')
    })

    it('uses treeterm/ prefix when parentBranch is null', async () => {
      const porcelainOutput = `worktree /path/to/repo
HEAD abc123
branch refs/heads/main

worktree /path/to/repo/.worktrees/feature
HEAD def456
branch refs/heads/treeterm/feature

`
      vi.mocked(simpleGit).mockReturnValue({
        raw: vi.fn().mockResolvedValue(porcelainOutput)
      } as any)

      const result = await getChildWorktrees('/path/to/repo', null)

      // Should still use 'treeterm/' prefix when parentBranch is null
      expect(result).toHaveLength(1)
      expect(result[0].branch).toBe('treeterm/feature')
    })

    it('excludes worktrees with nested slashes (grandchildren)', async () => {
      const porcelainOutput = `worktree /path/to/repo
HEAD abc123
branch refs/heads/main

worktree /path/to/repo/.worktrees/feature
HEAD def456
branch refs/heads/treeterm/feature

worktree /path/to/repo/.worktrees/feature-sub
HEAD ghi789
branch refs/heads/treeterm/feature/sub

`
      vi.mocked(simpleGit).mockReturnValue({
        raw: vi.fn().mockResolvedValue(porcelainOutput)
      } as any)

      const result = await getChildWorktrees('/path/to/repo', 'main')

      // Should only include treeterm/feature, not treeterm/feature/sub
      expect(result).toHaveLength(1)
      expect(result[0].branch).toBe('treeterm/feature')
    })
  })
})
