import { describe, it, expect, vi } from 'vitest'
import {
  createReviewViewedFilesStore,
  parseViewedFiles,
  REVIEW_VIEWED_FILES_KEY,
} from './createReviewViewedFilesStore'
import type { ViewedFileStats } from '../types'

function setup(initial: Record<string, ViewedFileStats> = {}) {
  const metadata: Record<string, string> = Object.keys(initial).length
    ? { [REVIEW_VIEWED_FILES_KEY]: JSON.stringify(initial) }
    : {}
  const updateMetadata = vi.fn((key: string, value: string) => {
    metadata[key] = value
  })
  const store = createReviewViewedFilesStore({
    getMetadata: () => metadata,
    updateMetadata,
  })
  return { store: store.getState(), metadata, updateMetadata }
}

const file = (path: string, additions = 1, deletions = 2) => ({ path, additions, deletions })

describe('parseViewedFiles', () => {
  it('returns empty when key absent', () => {
    expect(parseViewedFiles({})).toEqual({})
  })

  it('returns empty on malformed JSON', () => {
    expect(parseViewedFiles({ [REVIEW_VIEWED_FILES_KEY]: '{not json' })).toEqual({})
  })

  it('parses stored entries', () => {
    const stored = { 'a.ts': { additions: 3, deletions: 4 } }
    expect(parseViewedFiles({ [REVIEW_VIEWED_FILES_KEY]: JSON.stringify(stored) })).toEqual(stored)
  })
})

describe('toggleViewedFile', () => {
  it('marks an unviewed file viewed with its current stats', () => {
    const { store, metadata } = setup()
    store.toggleViewedFile(file('a.ts', 5, 6))
    expect(parseViewedFiles(metadata)).toEqual({ 'a.ts': { additions: 5, deletions: 6 } })
  })

  it('unmarks an already-viewed file', () => {
    const { store, metadata } = setup({ 'a.ts': { additions: 5, deletions: 6 } })
    store.toggleViewedFile(file('a.ts', 5, 6))
    expect(parseViewedFiles(metadata)).toEqual({})
  })

  it('leaves other files untouched', () => {
    const { store, metadata } = setup({ 'a.ts': { additions: 1, deletions: 1 } })
    store.toggleViewedFile(file('b.ts', 2, 2))
    expect(parseViewedFiles(metadata)).toEqual({
      'a.ts': { additions: 1, deletions: 1 },
      'b.ts': { additions: 2, deletions: 2 },
    })
  })

  it('persists to workspace metadata, not tab state', () => {
    const { store, updateMetadata } = setup()
    store.toggleViewedFile(file('a.ts'))
    expect(updateMetadata).toHaveBeenCalledWith(
      REVIEW_VIEWED_FILES_KEY,
      expect.any(String),
      'toggleViewedFile'
    )
  })
})

describe('markFilesViewed', () => {
  it('adds every unviewed file', () => {
    const { store, metadata } = setup()
    store.markFilesViewed([file('a.ts', 1, 1), file('b.ts', 2, 2)])
    expect(Object.keys(parseViewedFiles(metadata))).toEqual(['a.ts', 'b.ts'])
  })

  it('keeps the original stats of already-viewed files', () => {
    const { store, metadata } = setup({ 'a.ts': { additions: 9, deletions: 9 } })
    store.markFilesViewed([file('a.ts', 1, 1)])
    expect(parseViewedFiles(metadata)['a.ts']).toEqual({ additions: 9, deletions: 9 })
  })

  it('does not write when nothing changes', () => {
    const { store, updateMetadata } = setup({ 'a.ts': { additions: 1, deletions: 1 } })
    store.markFilesViewed([file('a.ts', 1, 1)])
    expect(updateMetadata).not.toHaveBeenCalled()
  })

  it('does not write for an empty list', () => {
    const { store, updateMetadata } = setup()
    store.markFilesViewed([])
    expect(updateMetadata).not.toHaveBeenCalled()
  })
})

describe('reconcileViewedFiles', () => {
  it('drops entries whose stats changed', () => {
    const { store, metadata } = setup({ 'a.ts': { additions: 1, deletions: 1 } })
    store.reconcileViewedFiles([file('a.ts', 2, 1)])
    expect(parseViewedFiles(metadata)).toEqual({})
  })

  it('keeps entries whose stats match', () => {
    const { store, updateMetadata } = setup({ 'a.ts': { additions: 1, deletions: 1 } })
    store.reconcileViewedFiles([file('a.ts', 1, 1)])
    expect(updateMetadata).not.toHaveBeenCalled()
  })

  it('preserves entries absent from the list (other view mode)', () => {
    const { store, metadata } = setup({
      'a.ts': { additions: 1, deletions: 1 },
      'b.ts': { additions: 3, deletions: 3 },
    })
    store.reconcileViewedFiles([file('a.ts', 2, 2)])
    expect(parseViewedFiles(metadata)).toEqual({ 'b.ts': { additions: 3, deletions: 3 } })
  })

  it('does not write when nothing is invalidated', () => {
    const { store, updateMetadata } = setup({ 'b.ts': { additions: 3, deletions: 3 } })
    store.reconcileViewedFiles([file('a.ts', 1, 1)])
    expect(updateMetadata).not.toHaveBeenCalled()
  })
})

describe('getViewedFiles', () => {
  it('reads through to the latest metadata', () => {
    const { store } = setup()
    expect(store.getViewedFiles()).toEqual({})
    store.toggleViewedFile(file('a.ts', 7, 8))
    expect(store.getViewedFiles()).toEqual({ 'a.ts': { additions: 7, deletions: 8 } })
  })
})
