import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import type { IpcResult, ReviewComment } from '../types'

export interface ReviewCommentDeps {
  getLineCommits: (filePath: string) => Promise<IpcResult<{ lineCommits: string[] }>>
  getMetadata: () => Record<string, string>
  updateMetadata: (key: string, value: string, reason: string) => void
}

export interface ReviewCommentState {
  getReviewComments: () => ReviewComment[]
  addReviewComment: (comment: Omit<ReviewComment, 'id' | 'createdAt' | 'lineCommitHash'>) => Promise<void>
  deleteReviewComment: (commentId: string) => void
  toggleReviewCommentAddressed: (commentId: string) => void
  updateOutdatedReviewComments: (currentCommitHash: string) => Promise<void>
  clearReviewComments: () => void
  markReviewCommentsAddressed: (ids: string[]) => void
}

export type ReviewCommentStore = StoreApi<ReviewCommentState>

export function parseReviewComments(metadata: Record<string, string>): ReviewComment[] {
  if (!metadata.reviewComments) return []
  try {
    // Comments saved before line tracking have no lineCommitHash.
    return (JSON.parse(metadata.reviewComments) as Partial<ReviewComment>[])
      .map(c => ({ ...c, lineCommitHash: c.lineCommitHash ?? null }) as ReviewComment)
  } catch {
    return []
  }
}

function serializeReviewComments(comments: ReviewComment[]): string {
  return JSON.stringify(comments)
}

export function createReviewCommentStore(deps: ReviewCommentDeps): ReviewCommentStore {
  const getLineCommits = async (filePath: string): Promise<string[]> => {
    const result = await deps.getLineCommits(filePath)
    if (!result.success) throw new Error(`Failed to blame ${filePath}: ${result.error}`)
    return result.lineCommits
  }

  return createStore<ReviewCommentState>()(() => ({
    getReviewComments: (): ReviewComment[] => {
      return parseReviewComments(deps.getMetadata())
    },

    addReviewComment: async (comment: Omit<ReviewComment, 'id' | 'createdAt' | 'lineCommitHash'>): Promise<void> => {
      // Only the modified side is the working-tree file that blame describes; a null
      // commitHash means the file is not in a git repo.
      // eslint-disable-next-line custom/no-string-literal-comparison -- ReviewComment.side is a string-literal union
      const tracksLine = comment.side === 'modified' && comment.commitHash !== null
      const lineCommitHash = tracksLine
        ? (await getLineCommits(comment.filePath))[comment.lineNumber - 1] ?? null
        : null
      const comments = parseReviewComments(deps.getMetadata())
      const newComment: ReviewComment = {
        ...comment,
        lineCommitHash,
        id: crypto.randomUUID(),
        createdAt: Date.now(),
      }
      comments.push(newComment)
      deps.updateMetadata('reviewComments', serializeReviewComments(comments), 'addReviewComment')
    },

    deleteReviewComment: (commentId: string): void => {
      const comments = parseReviewComments(deps.getMetadata())
      const filtered = comments.filter(c => c.id !== commentId)
      deps.updateMetadata('reviewComments', serializeReviewComments(filtered), 'deleteReviewComment')
    },

    toggleReviewCommentAddressed: (commentId: string): void => {
      const comments = parseReviewComments(deps.getMetadata())
      const updated = comments.map(c =>
        c.id === commentId ? { ...c, addressed: !c.addressed } : c
      )
      deps.updateMetadata('reviewComments', serializeReviewComments(updated), 'toggleReviewCommentAddressed')
    },

    updateOutdatedReviewComments: async (currentCommitHash: string): Promise<void> => {
      const comments = parseReviewComments(deps.getMetadata())
      if (comments.length === 0) return

      // Blame each file that has a tracked comment once.
      const trackedPaths = Array.from(new Set(comments.filter(c => c.lineCommitHash !== null).map(c => c.filePath)))
      const blames = new Map(await Promise.all(
        trackedPaths.map(async (filePath): Promise<[string, string[]]> => [filePath, await getLineCommits(filePath)])
      ))

      const outdatedById = new Map(comments.map(comment => [
        comment.id,
        comment.lineCommitHash !== null
          ? blames.get(comment.filePath)?.[comment.lineNumber - 1] !== comment.lineCommitHash
          : comment.commitHash !== null && comment.commitHash !== currentCommitHash,
      ]))

      // Comments may have been added or removed while blame ran — apply to the latest list.
      const latest = parseReviewComments(deps.getMetadata())
      const updated = latest.map(comment => {
        const shouldBeOutdated = outdatedById.get(comment.id)
        if (shouldBeOutdated === undefined || comment.isOutdated === shouldBeOutdated) return comment
        return { ...comment, isOutdated: shouldBeOutdated }
      })
      deps.updateMetadata('reviewComments', serializeReviewComments(updated), 'updateOutdatedReviewComments')
    },

    clearReviewComments: (): void => {
      deps.updateMetadata('reviewComments', serializeReviewComments([]), 'clearReviewComments')
    },

    markReviewCommentsAddressed: (ids: string[]): void => {
      if (ids.length === 0) return
      const idSet = new Set(ids)
      const comments = parseReviewComments(deps.getMetadata())
      const updated = comments.map(c => (idSet.has(c.id) && !c.addressed ? { ...c, addressed: true } : c))
      deps.updateMetadata('reviewComments', serializeReviewComments(updated), 'markReviewCommentsAddressed')
    },
  }))
}
