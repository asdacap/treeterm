import { join } from 'path'
import { homedir } from 'os'
import { randomUUID } from 'crypto'
import { humanId } from 'human-id'
import type { GrpcDaemonClient } from './grpcClient'

const TREETERM_HOME = join(homedir(), '.treeterm')
const REVIEWS_DIR = join(TREETERM_HOME, 'reviews')
const INDEX_FILE = join(REVIEWS_DIR, 'index.json')

export interface ReviewComment {
  id: string
  filePath: string
  lineNumber: number
  text: string
  commitHash: string
  createdAt: number
  isOutdated: boolean
  addressed: boolean
  side: 'original' | 'modified'
}

export interface ReviewsData {
  version: 1
  comments: ReviewComment[]
}

interface ReviewIndex {
  version: 1
  mappings: Record<string, string> // worktreePath -> humanId
}

const defaultReviewsData: ReviewsData = {
  version: 1,
  comments: []
}

export class ReviewsClient {
  constructor(private daemonClient: GrpcDaemonClient) {}

  private generateReviewId(): string {
    return humanId({ separator: '-', capitalize: false })
  }

  private getReviewFilePath(reviewId: string): string {
    return join(REVIEWS_DIR, reviewId + '.json')
  }

  /**
   * One-time migration: if reviewId is undefined, check the old index file
   * for a mapping from worktreePath to reviewId.
   */
  private async migrateFromIndex(worktreePath: string): Promise<string | undefined> {
    try {
      const result = await this.daemonClient.readFile(TREETERM_HOME, INDEX_FILE)
      if (result.success && result.file) {
        const index: ReviewIndex = JSON.parse(result.file.content)
        return index.mappings[worktreePath]
      }
    } catch {
      // Index doesn't exist
    }
    return undefined
  }

  /**
   * Resolve or generate a reviewId. If reviewId is provided, use it.
   * Otherwise, try to migrate from the old index file, then generate a new one.
   */
  private async resolveReviewId(
    reviewId: string | undefined,
    worktreePath: string
  ): Promise<{ reviewId: string; isNew: boolean }> {
    if (reviewId) {
      return { reviewId, isNew: false }
    }

    // Try migration from old index
    const migratedId = await this.migrateFromIndex(worktreePath)
    if (migratedId) {
      return { reviewId: migratedId, isNew: true }
    }

    return { reviewId: this.generateReviewId(), isNew: true }
  }

  async resolveReviewFilePath(
    reviewId: string | undefined,
    worktreePath: string
  ): Promise<{ filePath: string; reviewId: string }> {
    const resolved = await this.resolveReviewId(reviewId, worktreePath)
    return {
      filePath: this.getReviewFilePath(resolved.reviewId),
      reviewId: resolved.reviewId
    }
  }

  async loadReviews(
    reviewId: string | undefined,
    worktreePath: string
  ): Promise<{ reviews: ReviewsData; reviewId: string }> {
    const resolved = await this.resolveReviewId(reviewId, worktreePath)
    const reviewFilePath = this.getReviewFilePath(resolved.reviewId)

    try {
      const result = await this.daemonClient.readFile(TREETERM_HOME, reviewFilePath)
      if (result.success && result.file) {
        return {
          reviews: JSON.parse(result.file.content),
          reviewId: resolved.reviewId
        }
      }
    } catch {
      // Review file doesn't exist at new location
    }

    // Attempt migration from old location
    const oldPath = join(worktreePath, '.treeterm', 'reviews.json')
    try {
      const oldResult = await this.daemonClient.readFile(worktreePath, oldPath)
      if (oldResult.success && oldResult.file) {
        const data: ReviewsData = JSON.parse(oldResult.file.content)
        // Save to new location
        await this.saveReviews(resolved.reviewId, data)
        return { reviews: data, reviewId: resolved.reviewId }
      }
    } catch {
      // Old file doesn't exist either
    }

    return { reviews: { ...defaultReviewsData }, reviewId: resolved.reviewId }
  }

  async saveReviews(reviewId: string, reviews: ReviewsData): Promise<void> {
    const reviewFilePath = this.getReviewFilePath(reviewId)
    const content = JSON.stringify(reviews, null, 2)

    const result = await this.daemonClient.writeFile(TREETERM_HOME, reviewFilePath, content)
    if (!result.success) {
      throw new Error(result.error || 'Failed to save reviews')
    }
  }

  async addComment(
    reviewId: string | undefined,
    worktreePath: string,
    comment: Omit<ReviewComment, 'id' | 'createdAt'>
  ): Promise<{ comment: ReviewComment; reviewId: string }> {
    const { reviews, reviewId: resolvedId } = await this.loadReviews(reviewId, worktreePath)

    const newComment: ReviewComment = {
      ...comment,
      id: randomUUID(),
      createdAt: Date.now(),
      addressed: comment.addressed ?? false
    }

    reviews.comments.push(newComment)
    await this.saveReviews(resolvedId, reviews)

    return { comment: newComment, reviewId: resolvedId }
  }

  async deleteComment(reviewId: string, commentId: string): Promise<boolean> {
    const reviewFilePath = this.getReviewFilePath(reviewId)

    let reviews: ReviewsData
    try {
      const result = await this.daemonClient.readFile(TREETERM_HOME, reviewFilePath)
      if (result.success && result.file) {
        reviews = JSON.parse(result.file.content)
      } else {
        return false
      }
    } catch {
      return false
    }

    const initialLength = reviews.comments.length
    reviews.comments = reviews.comments.filter(c => c.id !== commentId)

    if (reviews.comments.length < initialLength) {
      await this.saveReviews(reviewId, reviews)
      return true
    }

    return false
  }

  async toggleAddressed(reviewId: string, commentId: string): Promise<boolean> {
    const reviewFilePath = this.getReviewFilePath(reviewId)

    let reviews: ReviewsData
    try {
      const result = await this.daemonClient.readFile(TREETERM_HOME, reviewFilePath)
      if (result.success && result.file) {
        reviews = JSON.parse(result.file.content)
      } else {
        return false
      }
    } catch {
      return false
    }

    const comment = reviews.comments.find(c => c.id === commentId)
    if (!comment) return false

    comment.addressed = !comment.addressed
    await this.saveReviews(reviewId, reviews)
    return true
  }

  async updateOutdatedComments(
    reviewId: string | undefined,
    worktreePath: string,
    currentCommitHash: string
  ): Promise<{ reviews: ReviewsData; reviewId: string }> {
    const { reviews, reviewId: resolvedId } = await this.loadReviews(reviewId, worktreePath)

    let modified = false
    reviews.comments = reviews.comments.map(comment => {
      const shouldBeOutdated = comment.commitHash !== currentCommitHash
      if (comment.isOutdated !== shouldBeOutdated) {
        modified = true
        return { ...comment, isOutdated: shouldBeOutdated }
      }
      return comment
    })

    if (modified) {
      await this.saveReviews(resolvedId, reviews)
    }

    return { reviews, reviewId: resolvedId }
  }

  async cleanupReviews(reviewId: string): Promise<void> {
    await this.saveReviews(reviewId, { version: 1, comments: [] })
  }
}
