import { describe, it, expect, vi } from 'vitest'
import { createReviewCommentStore, parseReviewComments } from './createReviewCommentStore'
import type { ReviewCommentDeps } from './createReviewCommentStore'
import type { ReviewComment } from '../types'

function makeDeps(
  metadata: Record<string, string> = {},
  blames: Record<string, string[]> = {},
): ReviewCommentDeps {
  const state = { metadata }
  return {
    getLineCommits: (filePath: string) => Promise.resolve({ success: true, lineCommits: blames[filePath] ?? [] }),
    getMetadata: () => state.metadata,
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    updateMetadata: (key: string, value: string, _reason: string) => { state.metadata[key] = value },
  }
}

describe('parseReviewComments', () => {
  it('returns empty array when no reviewComments key', () => {
    expect(parseReviewComments({})).toEqual([])
  })

  it('returns empty array for invalid JSON', () => {
    expect(parseReviewComments({ reviewComments: 'not-json{{' })).toEqual([])
  })

  it('parses valid JSON comments, defaulting a missing lineCommitHash to null', () => {
    const comments = [{ id: 'c1', text: 'fix' }, { id: 'c2', text: 'x', lineCommitHash: 'L1' }]
    expect(parseReviewComments({ reviewComments: JSON.stringify(comments) })).toEqual([
      { id: 'c1', text: 'fix', lineCommitHash: null },
      { id: 'c2', text: 'x', lineCommitHash: 'L1' },
    ])
  })
})

describe('createReviewCommentStore', () => {
  it('getReviewComments returns parsed comments from metadata', () => {
    const comments = [
      { id: 'c1', filePath: 'a.ts', lineNumber: 1, text: 'A', commitHash: 'h1', lineCommitHash: 'L1', createdAt: 1, isOutdated: false, addressed: false, side: 'modified' },
    ]
    const deps = makeDeps({ reviewComments: JSON.stringify(comments) })
    const store = createReviewCommentStore(deps)

    expect(store.getState().getReviewComments()).toEqual(comments)
  })

  it('getReviewComments returns empty when no metadata', () => {
    const store = createReviewCommentStore(makeDeps())
    expect(store.getState().getReviewComments()).toEqual([])
  })

  it('markReviewCommentsAddressed marks only the given ids', () => {
    const comments = [
      { id: 'c1', filePath: 'a.ts', lineNumber: 1, text: 'A', commitHash: 'h1', createdAt: 1, isOutdated: false, addressed: false, side: 'modified' },
      { id: 'c2', filePath: 'b.ts', lineNumber: 2, text: 'B', commitHash: 'h1', createdAt: 2, isOutdated: false, addressed: false, side: 'modified' },
    ]
    const deps = makeDeps({ reviewComments: JSON.stringify(comments) })
    const store = createReviewCommentStore(deps)

    store.getState().markReviewCommentsAddressed(['c1'])

    const result = JSON.parse(deps.getMetadata().reviewComments!) as { addressed: boolean }[]
    expect(result[0]!.addressed).toBe(true)
    expect(result[1]!.addressed).toBe(false)
  })

  it('markReviewCommentsAddressed with empty ids is a no-op', () => {
    const comments = [
      { id: 'c1', filePath: 'a.ts', lineNumber: 1, text: 'A', commitHash: 'h1', createdAt: 1, isOutdated: false, addressed: false, side: 'modified' },
    ]
    const deps = makeDeps({ reviewComments: JSON.stringify(comments) })
    const store = createReviewCommentStore(deps)

    store.getState().markReviewCommentsAddressed([])

    const result = JSON.parse(deps.getMetadata().reviewComments!) as { addressed: boolean }[]
    expect(result[0]!.addressed).toBe(false)
  })

  describe('addReviewComment', () => {
    const base = { filePath: 'a.ts', lineNumber: 2, text: 'T', commitHash: 'head', isOutdated: false, addressed: false }

    it('records the blamed commit of a modified-side line', async () => {
      const deps = makeDeps({}, { 'a.ts': ['L1', 'L2'] })
      const store = createReviewCommentStore(deps)
      await store.getState().addReviewComment({ ...base, side: 'modified' })
      expect(store.getState().getReviewComments()[0]!.lineCommitHash).toBe('L2')
    })

    it('records null when the line is beyond the blamed file', async () => {
      const deps = makeDeps({}, { 'a.ts': ['L1'] })
      const store = createReviewCommentStore(deps)
      await store.getState().addReviewComment({ ...base, side: 'modified' })
      expect(store.getState().getReviewComments()[0]!.lineCommitHash).toBeNull()
    })

    it('does not blame original-side or non-git comments', async () => {
      const deps = makeDeps()
      const spy = vi.spyOn(deps, 'getLineCommits')
      const store = createReviewCommentStore(deps)
      await store.getState().addReviewComment({ ...base, side: 'original' })
      await store.getState().addReviewComment({ ...base, commitHash: null, side: 'modified' })
      expect(spy).not.toHaveBeenCalled()
      expect(store.getState().getReviewComments().map(c => c.lineCommitHash)).toEqual([null, null])
    })

    it('throws and adds nothing when blame fails', async () => {
      const deps = makeDeps()
      deps.getLineCommits = () => Promise.resolve({ success: false, error: 'boom' })
      const store = createReviewCommentStore(deps)
      await expect(store.getState().addReviewComment({ ...base, side: 'modified' })).rejects.toThrow('Failed to blame a.ts: boom')
      expect(store.getState().getReviewComments()).toEqual([])
    })
  })

  describe('updateOutdatedReviewComments', () => {
    const make = (over: Partial<ReviewComment>): ReviewComment => ({
      id: 'c', filePath: 'a.ts', lineNumber: 1, text: 'T', commitHash: 'old-head', lineCommitHash: 'L1',
      createdAt: 1, isOutdated: false, addressed: false, side: 'modified', ...over,
    })

    const outdatedFlags = (deps: ReviewCommentDeps): Record<string, boolean> =>
      Object.fromEntries((JSON.parse(deps.getMetadata().reviewComments!) as ReviewComment[]).map(c => [c.id, c.isOutdated]))

    it('is a no-op without comments', async () => {
      const deps = makeDeps()
      await createReviewCommentStore(deps).getState().updateOutdatedReviewComments('head')
      expect(deps.getMetadata().reviewComments).toBeUndefined()
    })

    it('keeps a comment current across new commits while its line is unchanged', async () => {
      const deps = makeDeps({ reviewComments: JSON.stringify([make({ id: 'c1' })]) }, { 'a.ts': ['L1'] })
      await createReviewCommentStore(deps).getState().updateOutdatedReviewComments('new-head')
      expect(outdatedFlags(deps)).toEqual({ c1: false })
    })

    it('marks outdated when the line blame changed or the line is gone, and clears when restored', async () => {
      const comments = [
        make({ id: 'changed', lineNumber: 1 }),
        make({ id: 'gone', lineNumber: 5 }),
        make({ id: 'restored', filePath: 'b.ts', lineCommitHash: 'B1', isOutdated: true }),
      ]
      const deps = makeDeps({ reviewComments: JSON.stringify(comments) }, { 'a.ts': ['L9'], 'b.ts': ['B1'] })
      await createReviewCommentStore(deps).getState().updateOutdatedReviewComments('new-head')
      expect(outdatedFlags(deps)).toEqual({ changed: true, gone: true, restored: false })
    })

    it('falls back to HEAD comparison for untracked comments', async () => {
      const comments = [
        make({ id: 'stale', lineCommitHash: null, commitHash: 'old-head' }),
        make({ id: 'fresh', lineCommitHash: null, commitHash: 'new-head' }),
        make({ id: 'nogit', lineCommitHash: null, commitHash: null }),
      ]
      const deps = makeDeps({ reviewComments: JSON.stringify(comments) })
      const spy = vi.spyOn(deps, 'getLineCommits')
      await createReviewCommentStore(deps).getState().updateOutdatedReviewComments('new-head')
      expect(outdatedFlags(deps)).toEqual({ stale: true, fresh: false, nogit: false })
      expect(spy).not.toHaveBeenCalled()
    })

    it('blames each file once', async () => {
      const comments = [make({ id: 'c1' }), make({ id: 'c2', lineNumber: 2 })]
      const deps = makeDeps({ reviewComments: JSON.stringify(comments) }, { 'a.ts': ['L1', 'L2'] })
      const spy = vi.spyOn(deps, 'getLineCommits')
      await createReviewCommentStore(deps).getState().updateOutdatedReviewComments('new-head')
      expect(spy).toHaveBeenCalledTimes(1)
    })

    it('applies results to the latest list, keeping comments added during blame', async () => {
      const deps = makeDeps({ reviewComments: JSON.stringify([make({ id: 'c1' })]) })
      deps.getLineCommits = () => {
        deps.updateMetadata('reviewComments', JSON.stringify([make({ id: 'c1' }), make({ id: 'added' })]), 'test')
        return Promise.resolve({ success: true, lineCommits: ['L2'] })
      }
      await createReviewCommentStore(deps).getState().updateOutdatedReviewComments('new-head')
      expect(outdatedFlags(deps)).toEqual({ c1: true, added: false })
    })

    it('throws when blame fails', async () => {
      const deps = makeDeps({ reviewComments: JSON.stringify([make({ id: 'c1' })]) })
      deps.getLineCommits = () => Promise.resolve({ success: false, error: 'boom' })
      await expect(createReviewCommentStore(deps).getState().updateOutdatedReviewComments('h')).rejects.toThrow('boom')
    })
  })
})
