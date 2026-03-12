import { join } from 'path'
import { randomUUID } from 'crypto'
import type { GrpcDaemonClient } from './grpcClient'

const REVIEWS_FILENAME = 'reviews.json'
const TREETERM_DIR = '.treeterm'

export interface ReviewComment {
  id: string
  filePath: string
  lineNumber: number
  text: string
  commitHash: string
  createdAt: number
  isOutdated: boolean
  side: 'original' | 'modified'
}

export interface ReviewsData {
  version: 1
  comments: ReviewComment[]
}

const defaultReviewsData: ReviewsData = {
  version: 1,
  comments: []
}

function getReviewsPath(worktreePath: string): string {
  return join(TREETERM_DIR, REVIEWS_FILENAME)
}

export class ReviewsClient {
  constructor(private daemonClient: GrpcDaemonClient) {}

  async loadReviews(worktreePath: string): Promise<ReviewsData> {
    const reviewsPath = getReviewsPath(worktreePath)
    
    try {
      const result = await this.daemonClient.readFile(worktreePath, reviewsPath)
      if (result.success && result.file) {
        return JSON.parse(result.file.content)
      }
    } catch (error) {
      // File doesn't exist or is unreadable - return default
      console.log('[reviews] No existing reviews file, returning defaults')
    }

    return { ...defaultReviewsData }
  }

  async saveReviews(worktreePath: string, reviews: ReviewsData): Promise<void> {
    const reviewsPath = getReviewsPath(worktreePath)
    const content = JSON.stringify(reviews, null, 2)
    
    const result = await this.daemonClient.writeFile(worktreePath, reviewsPath, content)
    if (!result.success) {
      throw new Error(result.error || 'Failed to save reviews')
    }
  }

  async addComment(
    worktreePath: string,
    comment: Omit<ReviewComment, 'id' | 'createdAt'>
  ): Promise<ReviewComment> {
    const reviews = await this.loadReviews(worktreePath)

    const newComment: ReviewComment = {
      ...comment,
      id: randomUUID(),
      createdAt: Date.now()
    }

    reviews.comments.push(newComment)
    await this.saveReviews(worktreePath, reviews)

    return newComment
  }

  async deleteComment(worktreePath: string, commentId: string): Promise<boolean> {
    const reviews = await this.loadReviews(worktreePath)
    const initialLength = reviews.comments.length

    reviews.comments = reviews.comments.filter(c => c.id !== commentId)

    if (reviews.comments.length < initialLength) {
      await this.saveReviews(worktreePath, reviews)
      return true
    }

    return false
  }

  async updateOutdatedComments(
    worktreePath: string,
    currentCommitHash: string
  ): Promise<ReviewsData> {
    const reviews = await this.loadReviews(worktreePath)

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
      await this.saveReviews(worktreePath, reviews)
    }

    return reviews
  }
}

// Export standalone functions for backward compatibility during migration
// These will be removed once index.ts is fully updated
export async function loadReviews(
  daemonClient: GrpcDaemonClient,
  worktreePath: string
): Promise<ReviewsData> {
  const client = new ReviewsClient(daemonClient)
  return client.loadReviews(worktreePath)
}

export async function saveReviews(
  daemonClient: GrpcDaemonClient,
  worktreePath: string,
  reviews: ReviewsData
): Promise<void> {
  const client = new ReviewsClient(daemonClient)
  return client.saveReviews(worktreePath, reviews)
}

export async function addComment(
  daemonClient: GrpcDaemonClient,
  worktreePath: string,
  comment: Omit<ReviewComment, 'id' | 'createdAt'>
): Promise<ReviewComment> {
  const client = new ReviewsClient(daemonClient)
  return client.addComment(worktreePath, comment)
}

export async function deleteComment(
  daemonClient: GrpcDaemonClient,
  worktreePath: string,
  commentId: string
): Promise<boolean> {
  const client = new ReviewsClient(daemonClient)
  return client.deleteComment(worktreePath, commentId)
}

export async function updateOutdatedComments(
  daemonClient: GrpcDaemonClient,
  worktreePath: string,
  currentCommitHash: string
): Promise<ReviewsData> {
  const client = new ReviewsClient(daemonClient)
  return client.updateOutdatedComments(worktreePath, currentCommitHash)
}
