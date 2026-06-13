// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import CreateChildDialog, { TabMode } from './CreateChildDialog'
import type { WorkspaceStoreState } from '../store/createWorkspaceStore'
import type { WorkspaceStore } from '../types'
import { makeWorkspace } from '../../shared/test-fixtures/workspace'
import { createMockGitApi, createMockWorktreeRegistryApi } from '../../shared/mockApis'

vi.mock('../store/app', () => ({
  useAppStore: vi.fn((selector: (s: { applications: Map<string, unknown> }) => unknown) =>
    selector({ applications: new Map() })),
}))

function makeParentStore(): { store: WorkspaceStore; gitApi: ReturnType<typeof createMockGitApi>; registry: ReturnType<typeof createMockWorktreeRegistryApi> } {
  const gitApi = createMockGitApi()
  const registry = createMockWorktreeRegistryApi()
  const store = createStore<WorkspaceStoreState>()(() => ({
    workspace: makeWorkspace({ id: 'ws-1', path: '/repo', isGitRepo: true, gitRootPath: '/repo' }),
    settings: { defaultApplicationId: '' },
    gitApi,
    worktreeRegistryApi: registry,
  } as WorkspaceStoreState))
  return { store, gitApi, registry }
}

function renderDialog(store: WorkspaceStore, openWorktreePaths: string[], initialMode: TabMode) {
  return render(
    <CreateChildDialog
      parentWorkspace={store}
      onCreate={vi.fn()}
      onAdopt={vi.fn()}
      onCreateFromBranch={vi.fn()}
      onCreateFromRemote={vi.fn()}
      onCancel={vi.fn()}
      openWorktreePaths={openWorktreePaths}
      initialMode={initialMode}
    />
  )
}

describe('CreateChildDialog loaders', () => {
  beforeEach(() => { vi.clearAllMocks() })

  it('Existing tab renders fetched worktrees filtered by openWorktreePaths', async () => {
    const { store, gitApi } = makeParentStore()
    vi.mocked(gitApi.listWorktrees).mockResolvedValue([
      { path: '/wt/a', branch: 'feat-a' },
      { path: '/wt/b', branch: 'feat-b' },
    ])

    renderDialog(store, ['/wt/b'], TabMode.Existing)

    await waitFor(() => { expect(screen.getAllByText('feat-a').length).toBeGreaterThan(0) })
    expect(screen.queryAllByText('feat-b')).toEqual([])
  })

  it('re-rendering with a new openWorktreePaths identity re-filters WITHOUT refetching', async () => {
    const { store, gitApi } = makeParentStore()
    vi.mocked(gitApi.listWorktrees).mockResolvedValue([
      { path: '/wt/a', branch: 'feat-a' },
      { path: '/wt/b', branch: 'feat-b' },
    ])

    const { rerender } = renderDialog(store, ['/wt/b'], TabMode.Existing)
    await waitFor(() => { expect(screen.getAllByText('feat-a').length).toBeGreaterThan(0) })

    // New array identity each parent render — previously this re-ran the fetch effect.
    rerender(
      <CreateChildDialog
        parentWorkspace={store}
        onCreate={vi.fn()}
        onAdopt={vi.fn()}
        onCreateFromBranch={vi.fn()}
        onCreateFromRemote={vi.fn()}
        onCancel={vi.fn()}
        openWorktreePaths={['/wt/a']}
        initialMode={TabMode.Existing}
      />
    )

    await waitFor(() => { expect(screen.getAllByText('feat-b').length).toBeGreaterThan(0) })
    expect(screen.queryAllByText('feat-a')).toEqual([])
    expect(gitApi.listWorktrees).toHaveBeenCalledTimes(1)
  })

  it('Recent tab intersects registry with worktrees, sorts by lastUsedAt, filters open ones at render', async () => {
    const { store, gitApi, registry } = makeParentStore()
    vi.mocked(gitApi.listWorktrees).mockResolvedValue([
      { path: '/wt/a', branch: 'feat-a' },
      { path: '/wt/b', branch: 'feat-b' },
      { path: '/wt/c', branch: 'feat-c' },
    ])
    vi.mocked(registry.list).mockResolvedValue([
      { path: '/wt/a', branch: 'feat-a', displayName: 'Alpha', description: null, lastUsedAt: 1 },
      { path: '/wt/b', branch: 'feat-b', displayName: 'Beta', description: null, lastUsedAt: 2 },
      { path: '/wt/c', branch: 'feat-c', displayName: 'Gamma', description: null, lastUsedAt: 3 },
    ])

    renderDialog(store, ['/wt/c'], TabMode.Recent)

    await waitFor(() => { expect(screen.getByText('Beta')).toBeTruthy() })
    expect(screen.getByText('Alpha')).toBeTruthy()
    expect(screen.queryByText('Gamma')).toBeNull()

    // Sorted most-recently-used first: Beta (2) before Alpha (1).
    const names = screen.getAllByText(/Alpha|Beta/).map(el => el.textContent)
    expect(names).toEqual(['Beta', 'Alpha'])
  })

  it('a fetch resolving after unmount does not update state', async () => {
    const { store, gitApi } = makeParentStore()
    let resolveList: (wts: { path: string; branch: string }[]) => void = () => undefined
    vi.mocked(gitApi.listWorktrees).mockReturnValue(new Promise(resolve => { resolveList = resolve }))

    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    const { unmount } = renderDialog(store, [], TabMode.Existing)
    unmount()

    resolveList([{ path: '/wt/a', branch: 'feat-a' }])
    await Promise.resolve()

    const actWarnings = errorSpy.mock.calls.filter(args => String(args[0]).includes('not wrapped in act'))
    expect(actWarnings).toEqual([])
    errorSpy.mockRestore()
  })
})
