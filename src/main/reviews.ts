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

const defaultIndex: ReviewIndex = {
  version: 1,
  mappings: {}
}

export class ReviewsClient {
  private cachedIndex: ReviewIndex | null = null

  constructor(private daemonClient: GrpcDaemonClient) {}

  private async loadIndex(): Promise<ReviewIndex> {
    if (this.cachedIndex) return this.cachedIndex

    try {
      const result = await this.daemonClient.readFile(TREETERM_HOME, INDEX_FILE)
      if (result.success && result.file) {
        this.cachedIndex = JSON.parse(result.file.content)
        return this.cachedIndex!
      }
    } catch {
      // Index doesn't exist yet
    }

    this.cachedIndex = { ...defaultIndex, mappings: {} }
    return this.cachedIndex
  }

  private async saveIndex(index: ReviewIndex): Promise<void> {
    const content = JSON.stringify(index, null, 2)
    const result = await this.daemonClient.writeFile(TREETERM_HOME, INDEX_FILE, content)
    if (!result.success) {
      throw new Error(result.error || 'Failed to save review index')
    }
    this.cachedIndex = index
  }

  private async getOrCreateReviewId(worktreePath: string): Promise<string> {
    const index = await this.loadIndex()

    if (index.mappings[worktreePath]) {
      return index.mappings[worktreePath]
    }

    const id = humanId({ separator: '-', capitalize: false })
    index.mappings[worktreePath] = id
    await this.saveIndex(index)
    return id
  }

  private getReviewFilePath(reviewId: string): string {
    return join(REVIEWS_DIR, reviewId + '.json')
  }

  async resolveReviewFilePath(worktreePath: string): Promise<string> {
    const id = await this.getOrCreateReviewId(worktreePath)
    return this.getReviewFilePath(id)
  }

  async loadReviews(worktreePath: string): Promise<ReviewsData> {
    const id = await this.getOrCreateReviewId(worktreePath)
    const reviewFilePath = this.getReviewFilePath(id)

    try {
      const result = await this.daemonClient.readFile(TREETERM_HOME, reviewFilePath)
      if (result.success && result.file) {
        return JSON.parse(result.file.content)
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
        await this.saveReviews(worktreePath, data)
        return data
      }
    } catch {
      // Old file doesn't exist either
    }

    return { ...defaultReviewsData }
  }

  async saveReviews(worktreePath: string, reviews: ReviewsData): Promise<void> {
    const id = await this.getOrCreateReviewId(worktreePath)
    const reviewFilePath = this.getReviewFilePath(id)
    const content = JSON.stringify(reviews, null, 2)

    const result = await this.daemonClient.writeFile(TREETERM_HOME, reviewFilePath, content)
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
