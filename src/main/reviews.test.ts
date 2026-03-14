import { describe, it, expect, vi, beforeEach } from 'vitest'
import { ReviewsClient } from './reviews'
import type { GrpcDaemonClient } from './grpcClient'

function makeMockClient(overrides: Partial<{
  readFile: (workspace: string, path: string) => Promise<any>
  writeFile: (workspace: string, path: string, content: string) => Promise<any>
}> = {}): GrpcDaemonClient {
  return {
    readFile: vi.fn().mockResolvedValue({ success: false }),
    writeFile: vi.fn().mockResolvedValue({ success: true }),
    ...overrides,
  } as unknown as GrpcDaemonClient
}

const worktreePath = '/workspace/myrepo'

describe('ReviewsClient', () => {
  describe('loadReviews', () => {
    it('returns default reviews when file does not exist', async () => {
      const client = makeMockClient({
        readFile: vi.fn().mockResolvedValue({ success: false })
      })
      const reviews = new ReviewsClient(client)
      const result = await reviews.loadReviews(worktreePath)

      expect(result.version).toBe(1)
      expect(result.comments).toHaveLength(0)
    })

    it('returns parsed reviews when file exists', async () => {
      const reviewsData = {
        version: 1 as const,
        comments: [{
          id: 'c1',
          filePath: 'src/index.ts',
          lineNumber: 10,
          text: 'Fix this',
          commitHash: 'abc123',
          createdAt: 1000,
          isOutdated: false,
          side: 'modified' as const
        }]
      }
      const client = makeMockClient({
        readFile: vi.fn().mockResolvedValue({
          success: true,
          file: { content: JSON.stringify(reviewsData) }
        })
      })
      const reviews = new ReviewsClient(client)
      const result = await reviews.loadReviews(worktreePath)

      expect(result.comments).toHaveLength(1)
      expect(result.comments[0].id).toBe('c1')
    })

    it('returns default reviews when readFile throws', async () => {
      const client = makeMockClient({
        readFile: vi.fn().mockRejectedValue(new Error('connection error'))
      })
      const reviews = new ReviewsClient(client)
      const result = await reviews.loadReviews(worktreePath)

      expect(result.version).toBe(1)
      expect(result.comments).toHaveLength(0)
    })
  })

  describe('saveReviews', () => {
    it('writes JSON to the reviews file', async () => {
      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({ writeFile })
      const reviews = new ReviewsClient(client)
      const data = { version: 1 as const, comments: [] }

      await reviews.saveReviews(worktreePath, data)

      expect(writeFile).toHaveBeenCalledWith(
        worktreePath,
        expect.stringContaining('reviews.json'),
        expect.stringContaining('"version"')
      )
    })

    it('throws when writeFile returns failure', async () => {
      const client = makeMockClient({
        writeFile: vi.fn().mockResolvedValue({ success: false, error: 'disk full' })
      })
      const reviews = new ReviewsClient(client)

      await expect(reviews.saveReviews(worktreePath, { version: 1, comments: [] }))
        .rejects.toThrow('disk full')
    })
  })

  describe('addComment', () => {
    it('adds a comment and returns it with id and createdAt', async () => {
      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({
        readFile: vi.fn().mockResolvedValue({ success: false }),
        writeFile,
      })
      const reviews = new ReviewsClient(client)

      const comment = await reviews.addComment(worktreePath, {
        filePath: 'src/main.ts',
        lineNumber: 5,
        text: 'Looks good',
        commitHash: 'def456',
        isOutdated: false,
        side: 'modified',
      })

      expect(comment.id).toBeDefined()
      expect(comment.createdAt).toBeGreaterThan(0)
      expect(comment.text).toBe('Looks good')
      expect(writeFile).toHaveBeenCalled()
    })
  })

  describe('deleteComment', () => {
    it('returns false when comment does not exist', async () => {
      const client = makeMockClient({
        readFile: vi.fn().mockResolvedValue({ success: false })
      })
      const reviews = new ReviewsClient(client)
      const deleted = await reviews.deleteComment(worktreePath, 'nonexistent-id')
      expect(deleted).toBe(false)
    })

    it('removes existing comment and returns true', async () => {
      const existingReviews = {
        version: 1 as const,
        comments: [{
          id: 'c1',
          filePath: 'src/foo.ts',
          lineNumber: 1,
          text: 'note',
          commitHash: 'abc',
          createdAt: 1000,
          isOutdated: false,
          side: 'modified' as const
        }]
      }
      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({
        readFile: vi.fn().mockResolvedValue({
          success: true,
          file: { content: JSON.stringify(existingReviews) }
        }),
        writeFile,
      })
      const reviews = new ReviewsClient(client)
      const deleted = await reviews.deleteComment(worktreePath, 'c1')

      expect(deleted).toBe(true)
      expect(writeFile).toHaveBeenCalled()
    })
  })

  describe('updateOutdatedComments', () => {
    it('marks comments as outdated when commit hash differs', async () => {
      const existingReviews = {
        version: 1 as const,
        comments: [{
          id: 'c1',
          filePath: 'src/a.ts',
          lineNumber: 1,
          text: 'old',
          commitHash: 'old-hash',
          createdAt: 1000,
          isOutdated: false,
          side: 'modified' as const
        }]
      }
      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({
        readFile: vi.fn().mockResolvedValue({
          success: true,
          file: { content: JSON.stringify(existingReviews) }
        }),
        writeFile,
      })
      const reviews = new ReviewsClient(client)
      const result = await reviews.updateOutdatedComments(worktreePath, 'new-hash')

      expect(result.comments[0].isOutdated).toBe(true)
      expect(writeFile).toHaveBeenCalled()
    })

    it('does not save when nothing changed', async () => {
      const existingReviews = {
        version: 1 as const,
        comments: [{
          id: 'c1',
          filePath: 'src/a.ts',
          lineNumber: 1,
          text: 'current',
          commitHash: 'same-hash',
          createdAt: 1000,
          isOutdated: false,
          side: 'modified' as const
        }]
      }
      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({
        readFile: vi.fn().mockResolvedValue({
          success: true,
          file: { content: JSON.stringify(existingReviews) }
        }),
        writeFile,
      })
      const reviews = new ReviewsClient(client)
      await reviews.updateOutdatedComments(worktreePath, 'same-hash')

      expect(writeFile).not.toHaveBeenCalled()
    })

    it('marks outdated comment as current when commit hash matches', async () => {
      const existingReviews = {
        version: 1 as const,
        comments: [{
          id: 'c1',
          filePath: 'src/a.ts',
          lineNumber: 1,
          text: 'outdated',
          commitHash: 'current-hash',
          createdAt: 1000,
          isOutdated: true,
          side: 'original' as const
        }]
      }
      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({
        readFile: vi.fn().mockResolvedValue({
          success: true,
          file: { content: JSON.stringify(existingReviews) }
        }),
        writeFile,
      })
      const reviews = new ReviewsClient(client)
      const result = await reviews.updateOutdatedComments(worktreePath, 'current-hash')

      expect(result.comments[0].isOutdated).toBe(false)
      expect(writeFile).toHaveBeenCalled()
    })
  })
})
