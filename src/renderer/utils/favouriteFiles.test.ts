/* eslint-disable custom/no-string-literal-comparison -- test fixture paths */
import { describe, expect, it, vi } from 'vitest'
import type { FileEntry, WorkspaceFilesystemApi } from '../types'
import { resolveFavouriteFiles } from './favouriteFiles'

function entry(relativePath: string, isDirectory: boolean): FileEntry {
  return {
    name: relativePath.split('/').at(-1) ?? relativePath,
    path: `/repo/${relativePath}`,
    relativePath,
    isDirectory,
  }
}

function filesystem(entriesByPath: Record<string, FileEntry[]>): WorkspaceFilesystemApi {
  return {
    readDirectory: vi.fn((path: string) => Promise.resolve({
      success: true as const,
      contents: { path, entries: entriesByPath[path] ?? [] },
    })),
    readFile: vi.fn(),
    writeFile: vi.fn(),
    searchFiles: vi.fn(),
  }
}

describe('resolveFavouriteFiles', () => {
  it('resolves files and recursively expands favourite directories', async () => {
    const fs = filesystem({
      '/repo': [entry('README.md', false), entry('src', true)],
      '/repo/src': [entry('src/index.ts', false), entry('src/components', true)],
      '/repo/src/components': [entry('src/components/App.tsx', false)],
    })

    await expect(resolveFavouriteFiles('/repo', ['README.md', 'src'], fs)).resolves.toEqual([
      { path: '/repo/README.md', relativePath: 'README.md' },
      { path: '/repo/src/components/App.tsx', relativePath: 'src/components/App.tsx' },
      { path: '/repo/src/index.ts', relativePath: 'src/index.ts' },
    ])
  })

  it('deduplicates overlapping file and directory favourites', async () => {
    const fs = filesystem({
      '/repo': [entry('src', true)],
      '/repo/src': [entry('src/index.ts', false)],
    })

    await expect(resolveFavouriteFiles('/repo', ['src', 'src/index.ts'], fs)).resolves.toEqual([
      { path: '/repo/src/index.ts', relativePath: 'src/index.ts' },
    ])
  })

  it('ignores inherited paths whose file or parent directory does not exist in the current workspace', async () => {
    const fs = filesystem({ '/repo': [entry('present.ts', false)] })
    await expect(resolveFavouriteFiles('/repo', ['missing.ts', 'missing/child.ts', 'present.ts'], fs)).resolves.toEqual([
      { path: '/repo/present.ts', relativePath: 'present.ts' },
    ])
  })

  it('fails loudly when a favourite directory cannot be read', async () => {
    const fs = filesystem({ '/repo': [entry('src', true)] })
    vi.mocked(fs.readDirectory).mockImplementation((path: string) => Promise.resolve(path === '/repo/src'
      ? { success: false, error: 'permission denied' }
      : { success: true, contents: { path, entries: [entry('src', true)] } }))

    await expect(resolveFavouriteFiles('/repo', ['src'], fs)).rejects.toThrow('permission denied')
  })

  it('stops scheduling directory reads after cancellation', async () => {
    const fs = filesystem({})
    let cancelled = false
    vi.mocked(fs.readDirectory).mockImplementation((path: string) => {
      if (path === '/repo/src/first') cancelled = true
      const entries = path === '/repo'
        ? [entry('src', true)]
        : path === '/repo/src'
          ? [entry('src/first', true), entry('src/second', true)]
          : []
      return Promise.resolve({ success: true, contents: { path, entries } })
    })

    await resolveFavouriteFiles('/repo', ['src'], fs, () => cancelled)

    expect(fs.readDirectory).not.toHaveBeenCalledWith('/repo/src/second')
  })
})
