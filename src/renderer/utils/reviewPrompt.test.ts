import { describe, it, expect } from 'vitest'
import { generateReviewPrompt } from './reviewPrompt'
import type { ReviewComment } from '../types'

function makeComment(overrides: Partial<ReviewComment> = {}): ReviewComment {
  return {
    id: 'c1',
    filePath: 'src/foo.ts',
    lineNumber: 10,
    text: 'fix this',
    commitHash: 'abc123',
    createdAt: Date.now(),
    isOutdated: false,
    addressed: false,
    side: 'modified',
    ...overrides
  }
}

describe('generateReviewPrompt', () => {
  it('returns empty string for empty array', () => {
    expect(generateReviewPrompt([])).toBe('')
  })

  it('returns empty string when all comments are addressed', () => {
    const comments = [
      makeComment({ addressed: true }),
      makeComment({ id: 'c2', addressed: true })
    ]
    expect(generateReviewPrompt(comments)).toBe('')
  })

  it('formats a single unaddressed comment', () => {
    const result = generateReviewPrompt([makeComment()])
    expect(result).toContain('Please address the following review comments:')
    expect(result).toContain('## src/foo.ts')
    expect(result).toContain('- Line 10 (modified): "fix this"')
  })

  it('groups comments by filePath', () => {
    const comments = [
      makeComment({ id: 'c1', filePath: 'a.ts', lineNumber: 1, text: 'one' }),
      makeComment({ id: 'c2', filePath: 'b.ts', lineNumber: 2, text: 'two' }),
      makeComment({ id: 'c3', filePath: 'a.ts', lineNumber: 5, text: 'three' })
    ]
    const result = generateReviewPrompt(comments)
    expect(result).toContain('## a.ts')
    expect(result).toContain('## b.ts')
    // a.ts should have both comments
    const aSection = result.split('## b.ts')[0]
    expect(aSection).toContain('"one"')
    expect(aSection).toContain('"three"')
  })

  it('adds [OUTDATED] suffix to outdated comments', () => {
    const result = generateReviewPrompt([makeComment({ isOutdated: true })])
    expect(result).toContain('[OUTDATED]')
  })

  it('does not add [OUTDATED] to non-outdated comments', () => {
    const result = generateReviewPrompt([makeComment({ isOutdated: false })])
    expect(result).not.toContain('[OUTDATED]')
  })

  it('filters out addressed comments and formats unaddressed ones', () => {
    const comments = [
      makeComment({ id: 'c1', addressed: true, text: 'done' }),
      makeComment({ id: 'c2', addressed: false, text: 'pending' }),
      makeComment({ id: 'c3', addressed: false, isOutdated: true, text: 'old' })
    ]
    const result = generateReviewPrompt(comments)
    expect(result).not.toContain('"done"')
    expect(result).toContain('"pending"')
    expect(result).toContain('"old" [OUTDATED]')
  })
})
