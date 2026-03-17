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
      const result = await reviews.loadReviews(worktreePath)

      expect(result.version).toBe(1)
      expect(result.comments).toHaveLength(0)
    })

    it('returns parsed reviews from new location', async () => {
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
        // First call: load index — not found
        .mockResolvedValueOnce({ success: false })
        // Second call: writeFile for index (handled separately)
        // Third call: read review file
        .mockResolvedValueOnce({
          success: true,
          file: { content: JSON.stringify(reviewsData) }
        })
      const client = makeMockClient({ readFile })
      const reviews = new ReviewsClient(client)
      const result = await reviews.loadReviews(worktreePath)

      expect(result.comments).toHaveLength(1)
      expect(result.comments[0].id).toBe('c1')
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
        // Load index — not found
        .mockResolvedValueOnce({ success: false })
        // Read new review file — not found
        .mockResolvedValueOnce({ success: false })
        // Read old review file — found
        .mockResolvedValueOnce({
          success: true,
          file: { content: JSON.stringify(oldReviewsData) }
        })
        // Index read for saveReviews -> getOrCreateReviewId (cached, won't be called)

      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({ readFile, writeFile })
      const reviews = new ReviewsClient(client)
      const result = await reviews.loadReviews(worktreePath)

      expect(result.comments).toHaveLength(1)
      expect(result.comments[0].id).toBe('old1')
      // Should have written to new location (index + review file)
      expect(writeFile).toHaveBeenCalledWith(
        TREETERM_HOME,
        INDEX_FILE,
        expect.any(String)
      )
      expect(writeFile).toHaveBeenCalledWith(
        TREETERM_HOME,
        REVIEW_FILE,
        expect.any(String)
      )
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

  describe('index management', () => {
    it('creates index with new ID on first access', async () => {
      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({
        readFile: vi.fn().mockResolvedValue({ success: false }),
        writeFile,
      })
      const reviews = new ReviewsClient(client)
      await reviews.loadReviews(worktreePath)

      // Should have written the index
      expect(writeFile).toHaveBeenCalledWith(
        TREETERM_HOME,
        INDEX_FILE,
        expect.stringContaining('test-review-id')
      )
    })

    it('reuses existing ID from index', async () => {
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
        // Read review file
        .mockResolvedValueOnce({ success: false })
        // Read old file
        .mockResolvedValueOnce({ success: false })

      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({ readFile, writeFile })
      const reviews = new ReviewsClient(client)
      await reviews.loadReviews(worktreePath)

      // readFile should have tried to read existing-id.json, not test-review-id.json
      expect(readFile).toHaveBeenCalledWith(
        TREETERM_HOME,
        '/home/testuser/.treeterm/reviews/existing-id.json'
      )
    })
  })

  describe('resolveReviewFilePath', () => {
    it('returns the absolute path to the review file', async () => {
      const client = makeMockClient({
        readFile: vi.fn().mockResolvedValue({ success: false }),
      })
      const reviews = new ReviewsClient(client)
      const path = await reviews.resolveReviewFilePath(worktreePath)

      expect(path).toBe(REVIEW_FILE)
    })
  })

  describe('saveReviews', () => {
    it('writes JSON to the review file in ~/.treeterm/reviews/', async () => {
      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({
        readFile: vi.fn().mockResolvedValue({ success: false }),
        writeFile,
      })
      const reviews = new ReviewsClient(client)
      const data = { version: 1 as const, comments: [] }

      await reviews.saveReviews(worktreePath, data)

      expect(writeFile).toHaveBeenCalledWith(
        TREETERM_HOME,
        REVIEW_FILE,
        expect.stringContaining('"version"')
      )
    })

    it('throws when writeFile returns failure', async () => {
      const readFile = vi.fn().mockResolvedValue({ success: false })
      const writeFile = vi.fn()
        // First write: index save — success
        .mockResolvedValueOnce({ success: true })
        // Second write: review file — failure
        .mockResolvedValueOnce({ success: false, error: 'disk full' })
      const client = makeMockClient({ readFile, writeFile })
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
      const existingIndex = {
        version: 1,
        mappings: { [worktreePath]: 'test-review-id' }
      }
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
        // Load index
        .mockResolvedValueOnce({
          success: true,
          file: { content: JSON.stringify(existingIndex) }
        })
        // Read review file
        .mockResolvedValueOnce({
          success: true,
          file: { content: JSON.stringify(existingReviews) }
        })
      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({ readFile, writeFile })
      const reviews = new ReviewsClient(client)
      const deleted = await reviews.deleteComment(worktreePath, 'c1')

      expect(deleted).toBe(true)
      expect(writeFile).toHaveBeenCalled()
    })
  })

  describe('updateOutdatedComments', () => {
    it('marks comments as outdated when commit hash differs', async () => {
      const existingIndex = {
        version: 1,
        mappings: { [worktreePath]: 'test-review-id' }
      }
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
          file: { content: JSON.stringify(existingIndex) }
        })
        .mockResolvedValueOnce({
          success: true,
          file: { content: JSON.stringify(existingReviews) }
        })
      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({ readFile, writeFile })
      const reviews = new ReviewsClient(client)
      const result = await reviews.updateOutdatedComments(worktreePath, 'new-hash')

      expect(result.comments[0].isOutdated).toBe(true)
      expect(writeFile).toHaveBeenCalled()
    })

    it('does not save when nothing changed', async () => {
      const existingIndex = {
        version: 1,
        mappings: { [worktreePath]: 'test-review-id' }
      }
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
          file: { content: JSON.stringify(existingIndex) }
        })
        .mockResolvedValueOnce({
          success: true,
          file: { content: JSON.stringify(existingReviews) }
        })
      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({ readFile, writeFile })
      const reviews = new ReviewsClient(client)
      await reviews.updateOutdatedComments(worktreePath, 'same-hash')

      expect(writeFile).not.toHaveBeenCalled()
    })

    it('marks outdated comment as current when commit hash matches', async () => {
      const existingIndex = {
        version: 1,
        mappings: { [worktreePath]: 'test-review-id' }
      }
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
          file: { content: JSON.stringify(existingIndex) }
        })
        .mockResolvedValueOnce({
          success: true,
          file: { content: JSON.stringify(existingReviews) }
        })
      const writeFile = vi.fn().mockResolvedValue({ success: true })
      const client = makeMockClient({ readFile, writeFile })
      const reviews = new ReviewsClient(client)
      const result = await reviews.updateOutdatedComments(worktreePath, 'current-hash')

      expect(result.comments[0].isOutdated).toBe(false)
      expect(writeFile).toHaveBeenCalled()
    })
  })
})
