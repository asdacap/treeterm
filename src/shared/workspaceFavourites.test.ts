import { describe, expect, it } from 'vitest'
import { makeWorkspace } from './test-fixtures/workspace'
import { getWorkspaceFavouritePaths, isFavouritePath, normalizeFavouritePath, normalizeFileEntryRelativePath } from './workspaceFavourites'

describe('workspace favourites', () => {
  it('normalizes surrounding relative markers without corrupting Unix backslashes', () => {
    expect(normalizeFavouritePath('./src/components/')).toBe('src/components')
    expect(normalizeFavouritePath('src\\literal.txt')).toBe('src\\literal.txt')
    expect(normalizeFavouritePath('///README.md')).toBe('README.md')
  })

  it('normalizes native Windows FileEntry separators at the daemon boundary', () => {
    expect(normalizeFileEntryRelativePath('src\\components\\App.tsx', 'C:\\repo')).toBe('src/components/App.tsx')
    expect(normalizeFileEntryRelativePath('src\\components\\App.tsx', 'C:/repo')).toBe('src/components/App.tsx')
    expect(normalizeFileEntryRelativePath('src\\components\\App.tsx', '\\\\server\\repo')).toBe('src/components/App.tsx')
    expect(normalizeFileEntryRelativePath('src\\literal.txt', '/repo\\literal')).toBe('src\\literal.txt')
  })

  it('returns child favourites before inherited ancestor favourites without duplicates', () => {
    const root = makeWorkspace({ id: 'root', favouritePaths: ['src', 'README.md'] })
    const parent = makeWorkspace({ id: 'parent', parentId: 'root', favouritePaths: ['docs', 'src'] })
    const child = makeWorkspace({ id: 'child', parentId: 'parent', favouritePaths: ['package.json'] })
    const workspaces = new Map([root, parent, child].map((workspace) => [workspace.id, workspace]))

    expect(getWorkspaceFavouritePaths(child, (id) => workspaces.get(id))).toEqual([
      'package.json',
      'docs',
      'src',
      'README.md',
    ])
  })

  it('stops at a cyclic parent relationship', () => {
    const first = makeWorkspace({ id: 'first', parentId: 'second', favouritePaths: ['first.txt'] })
    const second = makeWorkspace({ id: 'second', parentId: 'first', favouritePaths: ['second.txt'] })
    const workspaces = new Map([first, second].map((workspace) => [workspace.id, workspace]))

    expect(getWorkspaceFavouritePaths(first, (id) => workspaces.get(id))).toEqual(['first.txt', 'second.txt'])
  })

  it('treats every file below a favourite directory as favourite', () => {
    expect(isFavouritePath('src/components/App.tsx', ['src'])).toBe(true)
    expect(isFavouritePath('source/App.tsx', ['src'])).toBe(false)
    expect(isFavouritePath('README.md', ['README.md'])).toBe(true)
  })
})
