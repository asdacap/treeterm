// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { LoadedWorkspaceTreeItem } from './SessionPanel'
import type { WorkspaceStoreState } from '../store/createWorkspaceStore'
import type { GitHubPrInfo, Workspace } from '../types'
import { ActivityState } from '../types'
import { makeWorkspace } from '../../shared/test-fixtures/workspace'

vi.mock('../store/activityState', () => ({
  useActivityStateStore: vi.fn(() => ActivityState.Idle),
}))

vi.mock('../store/app', () => ({
  useAppStore: Object.assign(
    vi.fn(() => ({ getViewportSize: vi.fn(() => ({ width: 1024, height: 768 })) })),
    { getState: () => ({ getViewportSize: () => ({ width: 1024, height: 768 }) }) }
  ),
}))

// Use a simple in-memory state instead of creating a real Zustand store in the factory,
// since vi.mock factories are hoisted before variable declarations.
vi.mock('../store/contextMenu', async () => {
  const { create } = await import('zustand')
  const store = create<{
    activeMenuId: string | null
    position: { x: number; y: number }
    open: (menuId: string, x: number, y: number) => void
    close: () => void
  }>()((set) => ({
    activeMenuId: null,
    position: { x: 0, y: 0 },
    open: (menuId: string, x: number, y: number) => { set({ activeMenuId: menuId, position: { x, y } }); },
    close: () => { set({ activeMenuId: null }); },
  }))
  return {
    useContextMenuStore: store,
    handleClickOutside: vi.fn(),
    installClickListener: vi.fn(),
  }
})

function makeWorkspaceStore(
  metadataOverride: Record<string, string> = {},
  workspaceOverride: Partial<Workspace> = {},
  prInfo: GitHubPrInfo | null = null,
) {
  const toggleFavourite = vi.fn()
  const store = createStore<WorkspaceStoreState>()(() => ({
    workspace: makeWorkspace(workspaceOverride) as unknown as Workspace,
    metadata: { displayName: 'My WS', ...metadataOverride },
    appStates: {},
    addTab: vi.fn(), openOrFocusTab: vi.fn(), removeTab: vi.fn(), setActiveTab: vi.fn(),
    updateTabTitle: vi.fn(), updateTabState: vi.fn(),
    reviewComments: createStore<any>()(() => ({})),
    promptHarness: vi.fn(),
    quickForkWorkspace: vi.fn(), updateMetadata: vi.fn(), deleteMetadata: vi.fn(), toggleFavourite,
    updateStatus: vi.fn(), refreshGitInfo: vi.fn(),
    mergeAndRemove: vi.fn(), mergeAndKeep: vi.fn(),
    closeAndClean: vi.fn(), lookupWorkspace: vi.fn(),
    remove: vi.fn(), removeKeepBranch: vi.fn(), removeKeepBoth: vi.fn(),
    initTab: vi.fn(), getTabRef: vi.fn().mockReturnValue(null), disposeTabResources: vi.fn(),
    initAnalyzer: vi.fn(), createTty: vi.fn(), getTtyWriter: vi.fn(),
    connectionId: 'local', updateSettings: vi.fn(),
    settings: { defaultApplicationId: '' },
    setWorkspace: vi.fn(),
    // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
    gitApi: {} as any, filesystemApi: {} as any, runActionsApi: {} as any, execApi: {} as any,
    // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
    worktreeRegistryApi: {} as any, saveRegistryEntry: vi.fn(),
    gitController: createStore<{ prInfo: GitHubPrInfo | null }>()(() => ({ prInfo })),
  }) as unknown as WorkspaceStoreState)
  return { store, toggleFavourite }
}

function renderTreeItem(
  store: ReturnType<typeof makeWorkspaceStore>['store'],
  props: Partial<{ isActive: boolean; onToggleFavourite: (id: string) => void }> = {}
) {
  const onToggleFavourite = props.onToggleFavourite ?? vi.fn()
  const ws = makeWorkspace({ name: 'my-branch', isGitRepo: false, isWorktree: false })
  return render(
    <LoadedWorkspaceTreeItem
      id="ws-1"
      store={store}
      data={ws}
      depth={0}
      isActive={props.isActive ?? false}
      isFocused={false}
      isExpanded={false}
      onToggleExpand={vi.fn()}
      onClick={vi.fn()}
      onQuickFork={vi.fn()}
      onCreateChild={vi.fn()}
      onRemove={vi.fn()}
      onDismiss={vi.fn()}
      onOpenSettings={vi.fn()}
      onToggleFavourite={onToggleFavourite}
      children={[]}
      renderChild={() => null}
      isDragging={false}
      dragOverPosition={null}
      onDragStart={vi.fn()}
      onDragOver={vi.fn()}
      onDrop={vi.fn()}
      onDragEnd={vi.fn()}
    />
  )
}

describe('LoadedWorkspaceTreeItem — favourite feature', () => {
  beforeEach(async () => {
    vi.clearAllMocks()
    const { useContextMenuStore } = await import('../store/contextMenu')
    useContextMenuStore.getState().close()
  })

  it('shows "Mark as Favourite" in context menu when not favourited', () => {
    const { store } = makeWorkspaceStore({})
    renderTreeItem(store)

    const row = screen.getByText('My WS').closest('.tree-item') as HTMLElement
    fireEvent.contextMenu(row)

    expect(screen.getByText('Mark as Favourite')).toBeDefined()
  })

  it('shows "Unmark as Favourite" in context menu when favourited', () => {
    const { store } = makeWorkspaceStore({ isFavourite: 'true' })
    renderTreeItem(store)

    const row = screen.getByText('My WS').closest('.tree-item') as HTMLElement
    fireEvent.contextMenu(row)

    expect(screen.getByText('Unmark as Favourite')).toBeDefined()
  })

  it('calls onToggleFavourite when the favourite menu item is clicked', () => {
    const onToggleFavourite = vi.fn()
    const { store } = makeWorkspaceStore({})
    renderTreeItem(store, { onToggleFavourite })

    const row = screen.getByText('My WS').closest('.tree-item') as HTMLElement
    fireEvent.contextMenu(row)
    fireEvent.click(screen.getByText('Mark as Favourite'))

    expect(onToggleFavourite).toHaveBeenCalledWith('ws-1')
  })

  it('renders star icon when favourited', () => {
    const { store } = makeWorkspaceStore({ isFavourite: 'true' })
    const { container } = renderTreeItem(store)

    const favouriteIcon = container.querySelector('.tree-item-favourite-icon')
    expect(favouriteIcon).not.toBeNull()
  })

  it('does not render star icon when not favourited', () => {
    const { store } = makeWorkspaceStore({})
    const { container } = renderTreeItem(store)

    const favouriteIcon = container.querySelector('.tree-item-favourite-icon')
    expect(favouriteIcon).toBeNull()
  })
})

describe('LoadedWorkspaceTreeItem — PR number', () => {
  beforeEach(async () => {
    vi.clearAllMocks()
    const { useContextMenuStore } = await import('../store/contextMenu')
    useContextMenuStore.getState().close()
  })

  function makePrInfo(
    state: GitHubPrInfo['state'],
    overrides: Partial<GitHubPrInfo> = {},
  ): GitHubPrInfo {
    return {
      number: 1234,
      url: 'https://github.com/x/y/pull/1234',
      title: 'My PR',
      state,
      reviews: [],
      checkRuns: [],
      unresolvedThreads: [],
      unresolvedCount: 0,
      ...overrides,
    }
  }

  it('renders the PR number before the workspace name when prInfo is set', () => {
    const { store } = makeWorkspaceStore({}, {}, makePrInfo('OPEN'))
    const { container } = renderTreeItem(store)

    const prSpan = container.querySelector('.tree-item-pr-number')
    expect(prSpan).not.toBeNull()
    expect(prSpan?.textContent).toBe('#1234')
    expect(screen.getByText('My WS')).toBeDefined()
  })

  it('applies the open state class for OPEN PRs', () => {
    const { store } = makeWorkspaceStore({}, {}, makePrInfo('OPEN'))
    const { container } = renderTreeItem(store)

    const prSpan = container.querySelector('.tree-item-pr-number')
    expect(prSpan?.classList.contains('tree-item-pr-number--open')).toBe(true)
  })

  it('applies the closed state class for CLOSED PRs', () => {
    const { store } = makeWorkspaceStore({}, {}, makePrInfo('CLOSED'))
    const { container } = renderTreeItem(store)

    const prSpan = container.querySelector('.tree-item-pr-number')
    expect(prSpan?.classList.contains('tree-item-pr-number--closed')).toBe(true)
  })

  it('applies the merged state class for MERGED PRs', () => {
    const { store } = makeWorkspaceStore({}, {}, makePrInfo('MERGED'))
    const { container } = renderTreeItem(store)

    const prSpan = container.querySelector('.tree-item-pr-number')
    expect(prSpan?.classList.contains('tree-item-pr-number--merged')).toBe(true)
  })

  it('does not render the PR number element when prInfo is null', () => {
    const { store } = makeWorkspaceStore({}, {}, null)
    const { container } = renderTreeItem(store)

    const prSpan = container.querySelector('.tree-item-pr-number')
    expect(prSpan).toBeNull()
  })

  it('renders the CI failure icon when any check run has failed', () => {
    const prInfo = makePrInfo('OPEN', {
      checkRuns: [
        { name: 'lint', status: 'COMPLETED', conclusion: 'SUCCESS' },
        { name: 'test', status: 'COMPLETED', conclusion: 'FAILURE' },
      ],
    })
    const { store } = makeWorkspaceStore({}, {}, prInfo)
    const { container } = renderTreeItem(store)

    expect(container.querySelector('.tree-item-pr-signal--ci-failure')).not.toBeNull()
  })

  it('renders the CI running icon when any check is in progress and none have failed', () => {
    const prInfo = makePrInfo('OPEN', {
      checkRuns: [
        { name: 'lint', status: 'COMPLETED', conclusion: 'SUCCESS' },
        { name: 'test', status: 'IN_PROGRESS', conclusion: null },
      ],
    })
    const { store } = makeWorkspaceStore({}, {}, prInfo)
    const { container } = renderTreeItem(store)

    expect(container.querySelector('.tree-item-pr-signal--ci-running')).not.toBeNull()
    expect(container.querySelector('.tree-item-pr-signal--ci-failure')).toBeNull()
  })

  it('prefers the failure icon over running when both failure and in-progress checks exist', () => {
    const prInfo = makePrInfo('OPEN', {
      checkRuns: [
        { name: 'lint', status: 'IN_PROGRESS', conclusion: null },
        { name: 'test', status: 'COMPLETED', conclusion: 'FAILURE' },
      ],
    })
    const { store } = makeWorkspaceStore({}, {}, prInfo)
    const { container } = renderTreeItem(store)

    expect(container.querySelector('.tree-item-pr-signal--ci-failure')).not.toBeNull()
    expect(container.querySelector('.tree-item-pr-signal--ci-running')).toBeNull()
  })

  it('renders the ready-to-merge icon for an open PR with passing checks, an approval, and no unresolved threads', () => {
    const prInfo = makePrInfo('OPEN', {
      checkRuns: [{ name: 'lint', status: 'COMPLETED', conclusion: 'SUCCESS' }],
      reviews: [{ author: 'reviewer', state: 'APPROVED' }],
    })
    const { store } = makeWorkspaceStore({}, {}, prInfo)
    const { container } = renderTreeItem(store)

    expect(container.querySelector('.tree-item-pr-signal--ready')).not.toBeNull()
  })

  it('does not render the ready-to-merge icon when there are unresolved threads', () => {
    const prInfo = makePrInfo('OPEN', {
      checkRuns: [{ name: 'lint', status: 'COMPLETED', conclusion: 'SUCCESS' }],
      reviews: [{ author: 'reviewer', state: 'APPROVED' }],
      unresolvedCount: 2,
    })
    const { store } = makeWorkspaceStore({}, {}, prInfo)
    const { container } = renderTreeItem(store)

    expect(container.querySelector('.tree-item-pr-signal--ready')).toBeNull()
  })

  it('does not render the ready-to-merge icon when a review requested changes', () => {
    const prInfo = makePrInfo('OPEN', {
      checkRuns: [{ name: 'lint', status: 'COMPLETED', conclusion: 'SUCCESS' }],
      reviews: [
        { author: 'a', state: 'APPROVED' },
        { author: 'b', state: 'CHANGES_REQUESTED' },
      ],
    })
    const { store } = makeWorkspaceStore({}, {}, prInfo)
    const { container } = renderTreeItem(store)

    expect(container.querySelector('.tree-item-pr-signal--ready')).toBeNull()
  })

  it('does not render any signal icon when checks pass but the PR has no approving review', () => {
    const prInfo = makePrInfo('OPEN', {
      checkRuns: [{ name: 'lint', status: 'COMPLETED', conclusion: 'SUCCESS' }],
    })
    const { store } = makeWorkspaceStore({}, {}, prInfo)
    const { container } = renderTreeItem(store)

    expect(container.querySelector('.tree-item-pr-signal')).toBeNull()
  })
})
