import { parseDiffFromFile, DEFAULT_COLLAPSED_CONTEXT_THRESHOLD } from '@pierre/diffs'
import type { FileDiffContents, TextDiffContents } from '../types'
import { DiffContentKind } from '../types'

// Searches a diff the way PierreDiffViewer shows it: the same hunks (parsed by the
// same parseDiffFromFile call the viewer makes), in on-screen row order. Lines
// folded away inside a collapsed unchanged region are skipped, since there is no
// rendered row to scroll to.

// Values match @pierre/diffs' SelectionSide so a hit can be passed as selectedLines.
export enum DiffSearchSide {
  Additions = 'additions',
  Deletions = 'deletions',
}

export interface DiffSearchHit {
  side: DiffSearchSide
  lineNumber: number
}

export interface DiffSearchOptions {
  diffStyle: 'split' | 'unified'
  expandUnchanged: boolean
  ignoreWhitespace: boolean
}

interface SearchableRow extends DiffSearchHit {
  text: string
}

// Parsing is the expensive part, so rows are cached per contents object and
// reused across keystrokes. Keyed weakly: a refreshed file gets a new object.
const rowCache = new WeakMap<TextDiffContents, Map<string, SearchableRow[]>>()

function buildRows(contents: TextDiffContents, filePath: string, opts: DiffSearchOptions): SearchableRow[] {
  let diff
  try {
    diff = parseDiffFromFile(
      { name: filePath, contents: contents.originalContent },
      { name: filePath, contents: contents.modifiedContent },
      { ignoreWhitespace: opts.ignoreWhitespace }
    )
  } catch {
    return []
  }
  if (diff.hunks.length === 0) return []

  const rows: SearchableRow[] = []
  const pushAddition = (index: number): void => {
    const text = diff.additionLines[index]
    if (text !== undefined) rows.push({ side: DiffSearchSide.Additions, lineNumber: index + 1, text })
  }
  const pushDeletion = (index: number): void => {
    const text = diff.deletionLines[index]
    if (text !== undefined) rows.push({ side: DiffSearchSide.Deletions, lineNumber: index + 1, text })
  }
  // Unchanged lines between hunks; the viewer shows them when expanded or when
  // the gap is too small to be worth collapsing.
  const pushGap = (from: number, to: number): void => {
    if (!opts.expandUnchanged && to - from > DEFAULT_COLLAPSED_CONTEXT_THRESHOLD) return
    for (let i = from; i < to; i++) pushAddition(i)
  }

  let nextAddition = 0
  for (const hunk of diff.hunks) {
    pushGap(nextAddition, hunk.additionLineIndex)
    for (const block of hunk.hunkContent) {
      // eslint-disable-next-line custom/no-string-literal-comparison -- @pierre/diffs' own union
      if (block.type === 'context') {
        for (let i = 0; i < block.lines; i++) pushAddition(block.additionLineIndex + i)
      // eslint-disable-next-line custom/no-string-literal-comparison -- @pierre/diffs' own union
      } else if (opts.diffStyle === 'split') {
        // Side by side: each row pairs the nth deletion with the nth addition.
        for (let i = 0; i < Math.max(block.deletions, block.additions); i++) {
          if (i < block.deletions) pushDeletion(block.deletionLineIndex + i)
          if (i < block.additions) pushAddition(block.additionLineIndex + i)
        }
      } else {
        for (let i = 0; i < block.deletions; i++) pushDeletion(block.deletionLineIndex + i)
        for (let i = 0; i < block.additions; i++) pushAddition(block.additionLineIndex + i)
      }
    }
    nextAddition = hunk.additionLineIndex + hunk.additionCount
  }
  pushGap(nextAddition, diff.additionLines.length)
  return rows
}

function getRows(contents: TextDiffContents, filePath: string, opts: DiffSearchOptions): SearchableRow[] {
  const key = `${filePath}|${opts.diffStyle}|${String(opts.expandUnchanged)}|${String(opts.ignoreWhitespace)}`
  let byKey = rowCache.get(contents)
  if (!byKey) {
    byKey = new Map()
    rowCache.set(contents, byKey)
  }
  let rows = byKey.get(key)
  if (!rows) {
    rows = buildRows(contents, filePath, opts)
    byKey.set(key, rows)
  }
  return rows
}

// Smart case: an all-lowercase query matches case-insensitively, any capital
// letter makes it exact.
export function createLineMatcher(query: string): (text: string) => boolean {
  if (query !== query.toLowerCase()) return (text) => text.includes(query)
  return (text) => text.toLowerCase().includes(query)
}

export function findLinesInDiff(
  contents: FileDiffContents,
  filePath: string,
  query: string,
  opts: DiffSearchOptions
): DiffSearchHit[] {
  // Image and binary diffs have no text lines to search.
  if (!query || contents.kind !== DiffContentKind.Text) return []
  const matches = createLineMatcher(query)
  return getRows(contents, filePath, opts)
    .filter((row) => matches(row.text))
    .map(({ side, lineNumber }) => ({ side, lineNumber }))
}
