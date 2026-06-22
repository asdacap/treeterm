// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor, fireEvent } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import CreateChildDialog, { TabMode } from './CreateChildDialog'
import type { WorkspaceStoreState } from '../store/createWorkspaceStore'
import type { WorkspaceStore } from '../types'
import { makeWorkspace } from '../../shared/test-fixtures/workspace'
import { createMockGitApi, createMockGitHubApi, createMockWorktreeRegistryApi } from '../../shared/mockApis'

vi.mock('../store/app', () => ({
  useAppStore: vi.fn((selector: (s: { applications: Map<string, unknown> }) => unknown) =>
    selector({ applications: new Map() })),
}))

function makeParentStore(): { store: WorkspaceStore; gitApi: ReturnType<typeof createMockGitApi>; github: ReturnType<typeof createMockGitHubApi>; registry: ReturnType<typeof createMockWorktreeRegistryApi> } {
  const gitApi = createMockGitApi()
  const github = createMockGitHubApi()
  const registry = createMockWorktreeRegistryApi()
  const store = createStore<WorkspaceStoreState>()(() => ({
    workspace: makeWorkspace({ id: 'ws-1', path: '/repo', isGitRepo: true, gitRootPath: '/repo' }),
    settings: { defaultApplicationId: '' },
    gitApi,
    gitHubApi: github,
    worktreeRegistryApi: registry,
  } as WorkspaceStoreState))
  return { store, gitApi, github, registry }
}

function renderDialog(store: WorkspaceStore, openWorktreePaths: string[], initialMode: TabMode) {
  return render(
    <CreateChildDialog
      parentWorkspace={store}
      onCreate={vi.fn()}
      onAdopt={vi.fn()}
      onCreateFromBranch={vi.fn()}
      onCreateFromRemote={vi.fn()}
      onCreateFromPr={vi.fn()}
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
        onCreateFromPr={vi.fn()}
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

  it('PR tab lists open PRs and opening one passes head branch + title (not description)', async () => {
    const { store, github } = makeParentStore()
    vi.mocked(github.listOpenPrs).mockResolvedValue({
      prs: [
        { number: 12, title: 'Add login flow', author: 'alice', headRefName: 'feat/login', isCrossRepo: false },
        { number: 7, title: 'Fork change', author: 'bob', headRefName: 'patch-1', isCrossRepo: true },
      ],
    })
    const onCreateFromPr = vi.fn<(...args: any[]) => any>().mockReturnValue({ success: true })

    render(
      <CreateChildDialog
        parentWorkspace={store}
        onCreate={vi.fn()}
        onAdopt={vi.fn()}
        onCreateFromBranch={vi.fn()}
        onCreateFromRemote={vi.fn()}
        onCreateFromPr={onCreateFromPr}
        onCancel={vi.fn()}
        openWorktreePaths={[]}
        initialMode={TabMode.Pr}
      />
    )

    const sameRepoPr = await screen.findByText('#12 Add login flow')
    // Fork PR is shown but disabled (cannot be opened directly)
    expect(screen.getByText('#7 Fork change')).toBeTruthy()
    expect(screen.getByText('Fork')).toBeTruthy()

    fireEvent.click(sameRepoPr)
    fireEvent.click(screen.getByText('Open'))

    expect(onCreateFromPr).toHaveBeenCalledWith('feat/login', 'Add login flow', false, undefined)
  })

  it('PR tab does not open a fork PR when clicked', async () => {
    const { store, github } = makeParentStore()
    vi.mocked(github.listOpenPrs).mockResolvedValue({
      prs: [{ number: 7, title: 'Fork change', author: 'bob', headRefName: 'patch-1', isCrossRepo: true }],
    })
    const onCreateFromPr = vi.fn<(...args: any[]) => any>().mockReturnValue({ success: true })

    render(
      <CreateChildDialog
        parentWorkspace={store}
        onCreate={vi.fn()}
        onAdopt={vi.fn()}
        onCreateFromBranch={vi.fn()}
        onCreateFromRemote={vi.fn()}
        onCreateFromPr={onCreateFromPr}
        onCancel={vi.fn()}
        openWorktreePaths={[]}
        initialMode={TabMode.Pr}
      />
    )

    const forkPr = await screen.findByText('#7 Fork change')
    fireEvent.click(forkPr)
    fireEvent.click(screen.getByText('Open'))
    expect(onCreateFromPr).not.toHaveBeenCalled()
  })

  it('PR tab surfaces a load error', async () => {
    const { store, github } = makeParentStore()
    vi.mocked(github.listOpenPrs).mockResolvedValue({ error: 'No GitHub PAT configured.' })

    render(
      <CreateChildDialog
        parentWorkspace={store}
        onCreate={vi.fn()}
        onAdopt={vi.fn()}
        onCreateFromBranch={vi.fn()}
        onCreateFromRemote={vi.fn()}
        onCreateFromPr={vi.fn()}
        onCancel={vi.fn()}
        openWorktreePaths={[]}
        initialMode={TabMode.Pr}
      />
    )

    await waitFor(() => { expect(screen.getByText(/Failed to load pull requests: No GitHub PAT configured\./)).toBeTruthy() })
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
