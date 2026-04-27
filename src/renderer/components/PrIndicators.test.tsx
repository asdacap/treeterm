// @vitest-environment jsdom
import { describe, it, expect } from 'vitest'

import { getPrSignal, PrSignal } from './PrIndicators'
import type { GitHubPrInfo } from '../types'

function makePrInfo(overrides: Partial<GitHubPrInfo> = {}): GitHubPrInfo {
  return {
    number: 1,
    url: 'https://example.com/pr/1',
    title: 'test',
    state: 'OPEN',
    reviews: [],
    checkRuns: [],
    unresolvedThreads: [],
    unresolvedCount: 0,
    ...overrides,
  }
}

describe('getPrSignal', () => {
  it('returns None when no prInfo and no conflicts', () => {
    expect(getPrSignal(null, false)).toBe(PrSignal.None)
    expect(getPrSignal(undefined, false)).toBe(PrSignal.None)
  })

  it('returns MergeConflict when hasConflictsWithParent is true even without prInfo', () => {
    expect(getPrSignal(null, true)).toBe(PrSignal.MergeConflict)
  })

  it('MergeConflict takes precedence over CI failure', () => {
    const prInfo = makePrInfo({
      checkRuns: [{ name: 'ci', status: 'COMPLETED', conclusion: 'FAILURE' }],
    })
    expect(getPrSignal(prInfo, true)).toBe(PrSignal.MergeConflict)
  })

  it('MergeConflict takes precedence over ReadyToMerge', () => {
    const prInfo = makePrInfo({
      reviews: [{ author: 'reviewer', state: 'APPROVED' }],
    })
    expect(getPrSignal(prInfo, true)).toBe(PrSignal.MergeConflict)
  })

  it('returns CiFailure when a check is COMPLETED with FAILURE', () => {
    const prInfo = makePrInfo({
      checkRuns: [{ name: 'ci', status: 'COMPLETED', conclusion: 'FAILURE' }],
    })
    expect(getPrSignal(prInfo, false)).toBe(PrSignal.CiFailure)
  })

  it('returns CiRunning when a check is not COMPLETED', () => {
    const prInfo = makePrInfo({
      checkRuns: [{ name: 'ci', status: 'IN_PROGRESS', conclusion: null }],
    })
    expect(getPrSignal(prInfo, false)).toBe(PrSignal.CiRunning)
  })

  it('returns ReadyToMerge when OPEN, approved, no changes requested, no unresolved threads', () => {
    const prInfo = makePrInfo({
      reviews: [{ author: 'reviewer', state: 'APPROVED' }],
    })
    expect(getPrSignal(prInfo, false)).toBe(PrSignal.ReadyToMerge)
  })

  it('returns None when approved but changes requested', () => {
    const prInfo = makePrInfo({
      reviews: [
        { author: 'a', state: 'APPROVED' },
        { author: 'b', state: 'CHANGES_REQUESTED' },
      ],
    })
    expect(getPrSignal(prInfo, false)).toBe(PrSignal.None)
  })

  it('returns None when approved but unresolved threads exist', () => {
    const prInfo = makePrInfo({
      reviews: [{ author: 'a', state: 'APPROVED' }],
      unresolvedCount: 1,
    })
    expect(getPrSignal(prInfo, false)).toBe(PrSignal.None)
  })

  it('returns None when PR is not OPEN', () => {
    const prInfo = makePrInfo({
      state: 'MERGED',
      reviews: [{ author: 'a', state: 'APPROVED' }],
    })
    expect(getPrSignal(prInfo, false)).toBe(PrSignal.None)
  })
})
