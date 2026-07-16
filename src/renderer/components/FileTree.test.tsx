// @vitest-environment jsdom
/* eslint-disable custom/no-string-literal-comparison -- test fixture paths */
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { describe, expect, it, vi } from 'vitest'
import { makeWorkspace } from '../../shared/test-fixtures/workspace'
import type { FileEntry, WorkspaceFilesystemApi, WorkspaceStore } from '../types'
import { FileTree } from './FileTree'

const filesystem = vi.hoisted(() => ({ readDirectory: vi.fn() }))
vi.mock('../hooks/useWorkspaceApis', () => ({
  useFilesystemApi: () => filesystem as unknown as WorkspaceFilesystemApi,
}))

function entry(relativePath: string, isDirectory: boolean): FileEntry {
  return {
    name: relativePath.split('/').at(-1) ?? relativePath,
    path: `/repo/${relativePath}`,
    relativePath,
    isDirectory,
  }
}

function makeStore(favouritePaths: string[], localFavouritePaths: string[] = favouritePaths): {
  workspace: WorkspaceStore
  addFavouritePath: ReturnType<typeof vi.fn>
  removeFavouritePath: ReturnType<typeof vi.fn>
} {
  const addFavouritePath = vi.fn()
  const removeFavouritePath = vi.fn()
  const store = createStore(() => ({
    workspace: makeWorkspace({ path: '/repo', favouritePaths: localFavouritePaths }),
    favouritePathsRevision: 0,
    getFavouritePaths: () => favouritePaths,
    isFavouritePath: (path: string) => favouritePaths.some((favourite) => path === favourite || path.startsWith(`${favourite}/`)),
    addFavouritePath,
    removeFavouritePath,
  }))
  return { workspace: store as unknown as WorkspaceStore, addFavouritePath, removeFavouritePath }
}

describe('FileTree favourites', () => {
  it('renders files expanded from a favourite directory and selects them', async () => {
    filesystem.readDirectory.mockImplementation((path: string) => Promise.resolve({
      success: true,
      contents: {
        path,
        entries: path === '/repo'
          ? [entry('src', true)]
          : [entry('src/index.ts', false)],
      },
    }))
    const { workspace } = makeStore(['src'])
    const onSelectFile = vi.fn()

    render(<FileTree workspace={workspace} selectedPath={null} expandedDirs={[]} onSelectFile={onSelectFile} onToggleDir={vi.fn()} />)

    const favourite = await screen.findByTitle('src/index.ts')
    fireEvent.click(favourite)
    expect(onSelectFile).toHaveBeenCalledWith('/repo/src/index.ts')
  })

  it('adds and removes local favourites from entry star buttons', async () => {
    filesystem.readDirectory.mockResolvedValue({
      success: true,
      contents: { path: '/repo', entries: [entry('README.md', false)] },
    })
    const added = makeStore([])
    const firstRender = render(<FileTree workspace={added.workspace} selectedPath={null} expandedDirs={[]} onSelectFile={vi.fn()} onToggleDir={vi.fn()} />)
    fireEvent.click(await screen.findByRole('button', { name: 'Add file to favourites' }))
    expect(added.addFavouritePath).toHaveBeenCalledWith('README.md')
    firstRender.unmount()

    const removed = makeStore(['README.md'])
    render(<FileTree workspace={removed.workspace} selectedPath={null} expandedDirs={[]} onSelectFile={vi.fn()} onToggleDir={vi.fn()} />)
    await waitFor(() => { expect(screen.getByRole('button', { name: 'Remove from favourites' })).toBeDefined() })
    fireEvent.click(screen.getByRole('button', { name: 'Remove from favourites' }))
    expect(removed.removeFavouritePath).toHaveBeenCalledWith('README.md')
  })

  it('retries a failed favourite directory load', async () => {
    let nestedAttempts = 0
    filesystem.readDirectory.mockImplementation((path: string) => {
      if (path === '/repo') {
        return Promise.resolve({ success: true, contents: { path, entries: [entry('src', true)] } })
      }
      nestedAttempts++
      return nestedAttempts === 1
        ? Promise.resolve({ success: false, error: 'temporary failure' })
        : Promise.resolve({ success: true, contents: { path, entries: [entry('src/index.ts', false)] } })
    })
    const { workspace } = makeStore(['src'])

    render(<FileTree workspace={workspace} selectedPath={null} expandedDirs={[]} onSelectFile={vi.fn()} onToggleDir={vi.fn()} />)

    expect(await screen.findByText('temporary failure')).toBeDefined()
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(await screen.findByTitle('src/index.ts')).toBeDefined()
  })
})
