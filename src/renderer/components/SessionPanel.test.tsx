// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { LoadedWorkspaceTreeItem } from './SessionPanel'
import type { WorkspaceStoreState } from '../store/createWorkspaceStore'
import type { Workspace } from '../types'
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

function makeWorkspaceStore(metadataOverride: Record<string, string> = {}, workspaceOverride: Partial<Workspace> = {}) {
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
    gitController: createStore<any>()(() => ({})),
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
