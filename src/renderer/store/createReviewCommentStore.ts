import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import type { ReviewComment } from '../types'

export interface ReviewCommentDeps {
  getMetadata: () => Record<string, string>
  updateMetadata: (key: string, value: string) => void
}

export interface ReviewCommentState {
  getReviewComments: () => ReviewComment[]
  addReviewComment: (comment: Omit<ReviewComment, 'id' | 'createdAt'>) => void
  deleteReviewComment: (commentId: string) => void
  toggleReviewCommentAddressed: (commentId: string) => void
  updateOutdatedReviewComments: (currentCommitHash: string) => void
  clearReviewComments: () => void
  markAllReviewCommentsAddressed: () => void
}

export type ReviewCommentStore = StoreApi<ReviewCommentState>

export function parseReviewComments(metadata: Record<string, string>): ReviewComment[] {
  if (!metadata.reviewComments) return []
  try {
    return JSON.parse(metadata.reviewComments) as ReviewComment[]
  } catch {
    return []
  }
}

function serializeReviewComments(comments: ReviewComment[]): string {
  return JSON.stringify(comments)
}

export function createReviewCommentStore(deps: ReviewCommentDeps): ReviewCommentStore {
  return createStore<ReviewCommentState>()(() => ({
    getReviewComments: (): ReviewComment[] => {
      return parseReviewComments(deps.getMetadata())
    },

    addReviewComment: (comment: Omit<ReviewComment, 'id' | 'createdAt'>): void => {
      const comments = parseReviewComments(deps.getMetadata())
      const newComment: ReviewComment = {
        ...comment,
        id: crypto.randomUUID(),
        createdAt: Date.now(),
      }
      comments.push(newComment)
      deps.updateMetadata('reviewComments', serializeReviewComments(comments))
    },

    deleteReviewComment: (commentId: string): void => {
      const comments = parseReviewComments(deps.getMetadata())
      const filtered = comments.filter(c => c.id !== commentId)
      deps.updateMetadata('reviewComments', serializeReviewComments(filtered))
    },

    toggleReviewCommentAddressed: (commentId: string): void => {
      const comments = parseReviewComments(deps.getMetadata())
      const updated = comments.map(c =>
        c.id === commentId ? { ...c, addressed: !c.addressed } : c
      )
      deps.updateMetadata('reviewComments', serializeReviewComments(updated))
    },

    updateOutdatedReviewComments: (currentCommitHash: string): void => {
      const comments = parseReviewComments(deps.getMetadata())
      if (comments.length === 0) return
      const updated = comments.map(comment => {
        const shouldBeOutdated = comment.commitHash !== null && comment.commitHash !== currentCommitHash
        if (comment.isOutdated !== shouldBeOutdated) {
          return { ...comment, isOutdated: shouldBeOutdated }
        }
        return comment
      })
      deps.updateMetadata('reviewComments', serializeReviewComments(updated))
    },

    clearReviewComments: (): void => {
      deps.updateMetadata('reviewComments', serializeReviewComments([]))
    },

    markAllReviewCommentsAddressed: (): void => {
      const comments = parseReviewComments(deps.getMetadata())
      const updated = comments.map(c => c.addressed ? c : { ...c, addressed: true })
      deps.updateMetadata('reviewComments', serializeReviewComments(updated))
    },
  }))
}
