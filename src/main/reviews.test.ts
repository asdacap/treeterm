import { describe, it, expect, vi, beforeEach } from 'vitest'
import { ReviewsClient } from './reviews'
import type { GrpcDaemonClient } from './grpcClient'

vi.mock('os', () => ({
  homedir: () => '/home/testuser'
}))

vi.mock('human-id', () => ({
  humanId: () => 'test-review-id'
}))

const TREETERM_HOME = '/home/testuser/.treeterm'
const INDEX_FILE = '/home/testuser/.treeterm/reviews/index.json'
const REVIEW_FILE = '/home/testuser/.treeterm/reviews/test-review-id.json'

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
    it('returns default reviews when no files exist', async () => {
      const client = makeMockClient({
        readFile: vi.fn().mockResolvedValue({ success: false })
      })
      const reviews = new ReviewsClient(client)
      const result = await reviews.loadReviews(undefined, worktreePath)

      expect(result.reviews.version).toBe(1)
      expect(result.reviews.comments).toHaveLength(0)
      expect(result.reviewId).toBe('test-review-id')
    })

    it('returns parsed reviews from new location with provided reviewId', async () => {
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
      const readFile = vi.fn()
        // Read review file
        .mockResolvedValueOnce({
          success: true,
          file: { content: JSON.stringify(reviewsData) }
        })
      const client = makeMockClient({ readFile })
      const reviews = new ReviewsClient(client)
      const result = await reviews.loadReviews('my-review-id', worktreePath)

      expect(result.reviews.comments).toHaveLength(1)
      expect(result.reviews.comments[0].id).toBe('c1')
      expect(result.reviewId).toBe('my-review-id')
    })

    it('migrates from old location when new file does not exist', async () => {
      const oldReviewsData = {
        version: 1 as const,
        comments: [{
          id: 'old1',
          filePath: 'src/old.ts',
          lineNumber: 5,
          text: 'Old comment',
          commitHash: 'old123',
          createdAt: 500,
          isOutdated: false,
          side: 'modified' as const
        }]
      }
      const readFile = vi.fn()
        // Load index — not found (migration check)
        .mockResolvedValueOnce({ success: false })
        // Read new review file — not found
        .mockResolvedValueOnce({ success: false })
        // Read old review file — found
        .mockResolvedValueOnce({
          success: true,
          file: { content: JSON.stringify(oldReviewsData) }
        })

      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({ readFile, writeFile })
      const reviews = new ReviewsClient(client)
      const result = await reviews.loadReviews(undefined, worktreePath)

      expect(result.reviews.comments).toHaveLength(1)
      expect(result.reviews.comments[0].id).toBe('old1')
      // Should have written to new location (review file only, no index)
      expect(writeFile).toHaveBeenCalledWith(
        TREETERM_HOME,
        REVIEW_FILE,
        expect.any(String)
      )
    })

    it('migrates reviewId from old index when reviewId is undefined', async () => {
      const existingIndex = {
        version: 1,
        mappings: { [worktreePath]: 'existing-id' }
      }
      const readFile = vi.fn()
        // Load index — found
        .mockResolvedValueOnce({
          success: true,
          file: { content: JSON.stringify(existingIndex) }
        })
        // Read review file with existing-id
        .mockResolvedValueOnce({ success: false })
        // Read old file
        .mockResolvedValueOnce({ success: false })

      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({ readFile, writeFile })
      const reviews = new ReviewsClient(client)
      const result = await reviews.loadReviews(undefined, worktreePath)

      expect(result.reviewId).toBe('existing-id')
      // readFile should have tried to read existing-id.json
      expect(readFile).toHaveBeenCalledWith(
        TREETERM_HOME,
        '/home/testuser/.treeterm/reviews/existing-id.json'
      )
    })

    it('returns default reviews when readFile throws', async () => {
      const client = makeMockClient({
        readFile: vi.fn().mockRejectedValue(new Error('connection error'))
      })
      const reviews = new ReviewsClient(client)
      const result = await reviews.loadReviews(undefined, worktreePath)

      expect(result.reviews.version).toBe(1)
      expect(result.reviews.comments).toHaveLength(0)
    })
  })

  describe('resolveReviewFilePath', () => {
    it('returns the absolute path to the review file with provided reviewId', async () => {
      const client = makeMockClient()
      const reviews = new ReviewsClient(client)
      const result = await reviews.resolveReviewFilePath('my-id', worktreePath)

      expect(result.filePath).toBe('/home/testuser/.treeterm/reviews/my-id.json')
      expect(result.reviewId).toBe('my-id')
    })

    it('generates reviewId when not provided', async () => {
      const client = makeMockClient({
        readFile: vi.fn().mockResolvedValue({ success: false }),
      })
      const reviews = new ReviewsClient(client)
      const result = await reviews.resolveReviewFilePath(undefined, worktreePath)

      expect(result.filePath).toBe(REVIEW_FILE)
      expect(result.reviewId).toBe('test-review-id')
    })
  })

  describe('saveReviews', () => {
    it('writes JSON to the review file in ~/.treeterm/reviews/', async () => {
      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({ writeFile })
      const reviews = new ReviewsClient(client)
      const data = { version: 1 as const, comments: [] }

      await reviews.saveReviews('test-review-id', data)

      expect(writeFile).toHaveBeenCalledWith(
        TREETERM_HOME,
        REVIEW_FILE,
        expect.stringContaining('"version"')
      )
    })

    it('throws when writeFile returns failure', async () => {
      const writeFile = vi.fn()
        .mockResolvedValueOnce({ success: false, error: 'disk full' })
      const client = makeMockClient({ writeFile })
      const reviews = new ReviewsClient(client)

      await expect(reviews.saveReviews('test-review-id', { version: 1, comments: [] }))
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

      const result = await reviews.addComment(undefined, worktreePath, {
        filePath: 'src/main.ts',
        lineNumber: 5,
        text: 'Looks good',
        commitHash: 'def456',
        isOutdated: false,
        addressed: false,
        side: 'modified',
      })

      expect(result.comment.id).toBeDefined()
      expect(result.comment.createdAt).toBeGreaterThan(0)
      expect(result.comment.text).toBe('Looks good')
      expect(result.reviewId).toBe('test-review-id')
      expect(writeFile).toHaveBeenCalled()
    })
  })

  describe('deleteComment', () => {
    it('returns false when review file does not exist', async () => {
      const client = makeMockClient({
        readFile: vi.fn().mockResolvedValue({ success: false })
      })
      const reviews = new ReviewsClient(client)
      const deleted = await reviews.deleteComment('test-review-id', 'nonexistent-id')
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
      const readFile = vi.fn()
        .mockResolvedValueOnce({
          success: true,
          file: { content: JSON.stringify(existingReviews) }
        })
      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({ readFile, writeFile })
      const reviews = new ReviewsClient(client)
      const deleted = await reviews.deleteComment('test-review-id', 'c1')

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
      const readFile = vi.fn()
        .mockResolvedValueOnce({
          success: true,
          file: { content: JSON.stringify(existingReviews) }
        })
      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({ readFile, writeFile })
      const reviews = new ReviewsClient(client)
      const result = await reviews.updateOutdatedComments('my-review-id', worktreePath, 'new-hash')

      expect(result.reviews.comments[0].isOutdated).toBe(true)
      expect(result.reviewId).toBe('my-review-id')
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
      const readFile = vi.fn()
        .mockResolvedValueOnce({
          success: true,
          file: { content: JSON.stringify(existingReviews) }
        })
      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({ readFile, writeFile })
      const reviews = new ReviewsClient(client)
      await reviews.updateOutdatedComments('my-review-id', worktreePath, 'same-hash')

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
      const readFile = vi.fn()
        .mockResolvedValueOnce({
          success: true,
          file: { content: JSON.stringify(existingReviews) }
        })
      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({ readFile, writeFile })
      const reviews = new ReviewsClient(client)
      const result = await reviews.updateOutdatedComments('my-review-id', worktreePath, 'current-hash')

      expect(result.reviews.comments[0].isOutdated).toBe(false)
      expect(writeFile).toHaveBeenCalled()
    })
  })

  describe('cleanupReviews', () => {
    it('writes empty reviews data to the review file', async () => {
      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({ writeFile })
      const reviews = new ReviewsClient(client)

      await reviews.cleanupReviews('my-review-id')

      expect(writeFile).toHaveBeenCalledWith(
        TREETERM_HOME,
        '/home/testuser/.treeterm/reviews/my-review-id.json',
        expect.stringContaining('"comments": []')
      )
    })
  })
})
