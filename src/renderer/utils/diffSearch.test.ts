import { describe, it, expect } from 'vitest'
import { findLinesInDiff, findInText, createLineMatcher, type DiffSearchOptions } from './diffSearch'
import type { FileDiffContents } from '../types'
import { DiffContentKind } from '../types'

function lines(count: number, label = (i: number) => `line ${String(i)}`): string {
  return Array.from({ length: count }, (_, i) => `${label(i + 1)}\n`).join('')
}

function contents(originalContent: string, modifiedContent: string): FileDiffContents {
  return { kind: DiffContentKind.Text, originalContent, modifiedContent, language: 'plaintext' }
}

const unified: DiffSearchOptions = { diffStyle: 'unified', expandUnchanged: false, ignoreWhitespace: false }

describe('createLineMatcher', () => {
  it('matches case-insensitively for a lowercase query', () => {
    expect(createLineMatcher('foo')('const FOO = 1')).toBe(true)
  })

  it('matches exactly once the query has a capital letter', () => {
    const matches = createLineMatcher('Foo')
    expect(matches('const Foo = 1')).toBe(true)
    expect(matches('const foo = 1')).toBe(false)
  })
})

describe('findLinesInDiff', () => {
  it('returns no hits for an empty query', () => {
    expect(findLinesInDiff(contents('a\n', 'b\n'), 'f.ts', '', unified)).toEqual([])
  })

  it('finds added and deleted lines on their own side', () => {
    const before = lines(10)
    const after = before.replace('line 5\n', 'line five\n')
    expect(findLinesInDiff(contents(before, after), 'f.ts', 'line 5', unified)).toEqual([
      { side: 'deletions', lineNumber: 5 },
    ])
    expect(findLinesInDiff(contents(before, after), 'f.ts', 'five', unified)).toEqual([
      { side: 'additions', lineNumber: 5 },
    ])
  })

  it('skips unchanged lines folded away between hunks', () => {
    const before = lines(40)
    const after = before.replace('line 2\n', 'changed\n').replace('line 38\n', 'changed\n')
    const hits = findLinesInDiff(contents(before, after), 'f.ts', 'line 20', unified)
    expect(hits).toEqual([])
  })

  it('includes every unchanged line when unchanged regions are expanded', () => {
    const before = lines(40)
    const after = before.replace('line 2\n', 'changed\n').replace('line 38\n', 'changed\n')
    const hits = findLinesInDiff(contents(before, after), 'f.ts', 'line 20', { ...unified, expandUnchanged: true })
    expect(hits).toEqual([{ side: 'additions', lineNumber: 20 }])
  })

  it('orders unified changes as all deletions then all additions', () => {
    const before = 'keep\nold a\nold b\nkeep\n'
    const after = 'keep\nnew a\nnew b\nkeep\n'
    const hits = findLinesInDiff(contents(before, after), 'f.ts', ' a', unified)
    expect(hits).toEqual([
      { side: 'deletions', lineNumber: 2 },
      { side: 'additions', lineNumber: 2 },
    ])
    expect(findLinesInDiff(contents(before, after), 'f.ts', ' ', unified).map((h) => h.side)).toEqual([
      'deletions', 'deletions', 'additions', 'additions',
    ])
  })

  it('orders split changes row by row', () => {
    const before = 'keep\nold a\nold b\nkeep\n'
    const after = 'keep\nnew a\nnew b\nkeep\n'
    const hits = findLinesInDiff(contents(before, after), 'f.ts', ' ', { ...unified, diffStyle: 'split' })
    expect(hits).toEqual([
      { side: 'deletions', lineNumber: 2 },
      { side: 'additions', lineNumber: 2 },
      { side: 'deletions', lineNumber: 3 },
      { side: 'additions', lineNumber: 3 },
    ])
  })

  it('returns no hits when both sides are identical', () => {
    expect(findLinesInDiff(contents('same\n', 'same\n'), 'f.ts', 'same', unified)).toEqual([])
  })
})

describe('findLinesInDiff on non-text diffs', () => {
  it('finds nothing in image or binary diffs', () => {
    const image: FileDiffContents = { kind: DiffContentKind.Image, originalBase64: 'aGk=', modifiedBase64: '' }
    expect(findLinesInDiff(image, 'a.png', 'a', unified)).toEqual([])
    expect(findLinesInDiff({ kind: DiffContentKind.Binary }, 'a.bin', 'a', unified)).toEqual([])
  })
})

describe('findInText', () => {
  it('finds every occurrence with 1-based line and column', () => {
    expect(findInText('foo bar foo\nbaz\n  foo', 'foo')).toEqual([
      { lineNumber: 1, column: 1 },
      { lineNumber: 1, column: 9 },
      { lineNumber: 3, column: 3 },
    ])
  })

  it('is case-insensitive for a lowercase query and case-sensitive otherwise', () => {
    expect(findInText('Foo foo', 'foo')).toHaveLength(2)
    expect(findInText('Foo foo', 'Foo')).toEqual([{ lineNumber: 1, column: 1 }])
  })

  it('finds nothing for an empty query', () => {
    expect(findInText('foo', '')).toEqual([])
  })
})
