import { describe, it, expect } from 'vitest'
import { createReviewCommentStore, parseReviewComments } from './createReviewCommentStore'
import type { ReviewCommentDeps } from './createReviewCommentStore'

function makeDeps(metadata: Record<string, string> = {}): ReviewCommentDeps {
  const state = { metadata }
  return {
    getMetadata: () => state.metadata,
    updateMetadata: (key: string, value: string) => { state.metadata[key] = value },
  }
}

describe('parseReviewComments', () => {
  it('returns empty array when no reviewComments key', () => {
    expect(parseReviewComments({})).toEqual([])
  })

  it('returns empty array for invalid JSON', () => {
    expect(parseReviewComments({ reviewComments: 'not-json{{' })).toEqual([])
  })

  it('parses valid JSON comments', () => {
    const comments = [{ id: 'c1', text: 'fix' }]
    expect(parseReviewComments({ reviewComments: JSON.stringify(comments) })).toEqual(comments)
  })
})

describe('createReviewCommentStore', () => {
  it('getReviewComments returns parsed comments from metadata', () => {
    const comments = [
      { id: 'c1', filePath: 'a.ts', lineNumber: 1, text: 'A', commitHash: 'h1', createdAt: 1, isOutdated: false, addressed: false, side: 'modified' },
    ]
    const deps = makeDeps({ reviewComments: JSON.stringify(comments) })
    const store = createReviewCommentStore(deps)

    expect(store.getState().getReviewComments()).toEqual(comments)
  })

  it('getReviewComments returns empty when no metadata', () => {
    const store = createReviewCommentStore(makeDeps())
    expect(store.getState().getReviewComments()).toEqual([])
  })

  it('markAllReviewCommentsAddressed sets all to addressed=true', () => {
    const comments = [
      { id: 'c1', filePath: 'a.ts', lineNumber: 1, text: 'A', commitHash: 'h1', createdAt: 1, isOutdated: false, addressed: false, side: 'modified' },
      { id: 'c2', filePath: 'b.ts', lineNumber: 2, text: 'B', commitHash: 'h1', createdAt: 2, isOutdated: false, addressed: false, side: 'modified' },
    ]
    const deps = makeDeps({ reviewComments: JSON.stringify(comments) })
    const store = createReviewCommentStore(deps)

    store.getState().markAllReviewCommentsAddressed()

    const result = JSON.parse(deps.getMetadata().reviewComments)
    expect(result[0].addressed).toBe(true)
    expect(result[1].addressed).toBe(true)
  })

  it('markAllReviewCommentsAddressed skips already-addressed comments', () => {
    const comments = [
      { id: 'c1', filePath: 'a.ts', lineNumber: 1, text: 'A', commitHash: 'h1', createdAt: 1, isOutdated: false, addressed: true, side: 'modified' },
      { id: 'c2', filePath: 'b.ts', lineNumber: 2, text: 'B', commitHash: 'h1', createdAt: 2, isOutdated: false, addressed: false, side: 'modified' },
    ]
    const deps = makeDeps({ reviewComments: JSON.stringify(comments) })
    const store = createReviewCommentStore(deps)

    store.getState().markAllReviewCommentsAddressed()

    const result = JSON.parse(deps.getMetadata().reviewComments)
    expect(result[0].addressed).toBe(true)
    expect(result[1].addressed).toBe(true)
  })
})
