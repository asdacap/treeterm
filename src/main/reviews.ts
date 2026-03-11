import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'fs'
import { join } from 'path'
import { randomUUID } from 'crypto'

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
  return join(worktreePath, TREETERM_DIR, REVIEWS_FILENAME)
}

export function loadReviews(worktreePath: string): ReviewsData {
  const reviewsPath = getReviewsPath(worktreePath)

  try {
    if (existsSync(reviewsPath)) {
      const data = readFileSync(reviewsPath, 'utf-8')
      return JSON.parse(data)
    }
  } catch (error) {
    console.error('Failed to load reviews:', error)
  }

  return { ...defaultReviewsData }
}

export function saveReviews(worktreePath: string, reviews: ReviewsData): void {
  const treetermDir = join(worktreePath, TREETERM_DIR)
  const reviewsPath = getReviewsPath(worktreePath)

  try {
    if (!existsSync(treetermDir)) {
      mkdirSync(treetermDir, { recursive: true })
    }
    writeFileSync(reviewsPath, JSON.stringify(reviews, null, 2), 'utf-8')
  } catch (error) {
    console.error('Failed to save reviews:', error)
    throw error
  }
}

export function addComment(
  worktreePath: string,
  comment: Omit<ReviewComment, 'id' | 'createdAt'>
): ReviewComment {
  const reviews = loadReviews(worktreePath)

  const newComment: ReviewComment = {
    ...comment,
    id: randomUUID(),
    createdAt: Date.now()
  }

  reviews.comments.push(newComment)
  saveReviews(worktreePath, reviews)

  return newComment
}

export function deleteComment(worktreePath: string, commentId: string): boolean {
  const reviews = loadReviews(worktreePath)
  const initialLength = reviews.comments.length

  reviews.comments = reviews.comments.filter(c => c.id !== commentId)

  if (reviews.comments.length < initialLength) {
    saveReviews(worktreePath, reviews)
    return true
  }

  return false
}

export function updateOutdatedComments(
  worktreePath: string,
  currentCommitHash: string
): ReviewsData {
  const reviews = loadReviews(worktreePath)

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
    saveReviews(worktreePath, reviews)
  }

  return reviews
}
