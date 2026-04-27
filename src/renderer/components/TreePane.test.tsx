// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import { FavouriteWorkspaceItem } from './TreePane'
import type { WorkspaceStoreState } from '../store/createWorkspaceStore'
import type { SessionState } from '../store/createSessionStore'
import type { GitHubPrInfo, Workspace } from '../types'
import { ActivityState } from '../types'
import { makeWorkspace } from '../../shared/test-fixtures/workspace'

vi.mock('../store/activityState', () => ({
  useActivityStateStore: vi.fn(() => ActivityState.Idle),
}))

vi.mock('../store/app', () => ({
  useAppStore: Object.assign(
    vi.fn(() => ({})),
    { getState: () => ({}) }
  ),
}))

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
    open: (menuId, x, y) => { set({ activeMenuId: menuId, position: { x, y } }); },
    close: () => { set({ activeMenuId: null }); },
  }))
  return {
    useContextMenuStore: store,
    handleClickOutside: vi.fn(),
    installClickListener: vi.fn(),
  }
})

function makeWorkspaceStore(prInfo: GitHubPrInfo | null) {
  return createStore<WorkspaceStoreState>()(() => ({
    workspace: makeWorkspace() as unknown as Workspace,
    metadata: { displayName: 'My WS', isFavourite: 'true' },
    appStates: {},
    addTab: vi.fn(),
    toggleFavourite: vi.fn(),
    gitController: createStore<{ prInfo: GitHubPrInfo | null }>()(() => ({ prInfo })),
  }) as unknown as WorkspaceStoreState)
}

function makePrInfo(
  state: GitHubPrInfo['state'],
  overrides: Partial<GitHubPrInfo> = {},
): GitHubPrInfo {
  return {
    number: 99,
    url: 'https://github.com/x/y/pull/99',
    title: 'My PR',
    state,
    reviews: [],
    checkRuns: [],
    unresolvedThreads: [],
    unresolvedCount: 0,
    ...overrides,
  }
}

function renderItem(store: ReturnType<typeof makeWorkspaceStore>) {
  const sessionStore = createStore<SessionState>()(() => ({
    activeWorkspaceId: null,
    setActiveWorkspace: vi.fn(),
  }) as unknown as SessionState) as unknown as StoreApi<SessionState>
  return render(
    <FavouriteWorkspaceItem
      sessionId="s-1"
      sessionStore={sessionStore}
      workspaceId="ws-1"
      workspaceStore={store}
      data={makeWorkspace({ name: 'my-branch' })}
    />
  )
}

describe('FavouriteWorkspaceItem — PR indicators', () => {
  beforeEach(async () => {
    vi.clearAllMocks()
    const { useContextMenuStore } = await import('../store/contextMenu')
    useContextMenuStore.getState().close()
  })

  it('renders the PR number prefix when prInfo is set', () => {
    const store = makeWorkspaceStore(makePrInfo('OPEN'))
    const { container } = renderItem(store)

    const prSpan = container.querySelector('.tree-item-pr-number')
    expect(prSpan).not.toBeNull()
    expect(prSpan?.textContent).toBe('#99')
  })

  it('applies the open state class for OPEN PRs', () => {
    const store = makeWorkspaceStore(makePrInfo('OPEN'))
    const { container } = renderItem(store)

    expect(container.querySelector('.tree-item-pr-number--open')).not.toBeNull()
  })

  it('applies the merged state class for MERGED PRs', () => {
    const store = makeWorkspaceStore(makePrInfo('MERGED'))
    const { container } = renderItem(store)

    expect(container.querySelector('.tree-item-pr-number--merged')).not.toBeNull()
  })

  it('does not render the PR number when prInfo is null', () => {
    const store = makeWorkspaceStore(null)
    const { container } = renderItem(store)

    expect(container.querySelector('.tree-item-pr-number')).toBeNull()
  })

  it('renders the CI failure icon when a check has failed', () => {
    const store = makeWorkspaceStore(makePrInfo('OPEN', {
      checkRuns: [{ name: 'test', status: 'COMPLETED', conclusion: 'FAILURE' }],
    }))
    const { container } = renderItem(store)

    expect(container.querySelector('.tree-item-pr-signal--ci-failure')).not.toBeNull()
  })

  it('renders the ready-to-merge icon for an approved open PR with passing checks', () => {
    const store = makeWorkspaceStore(makePrInfo('OPEN', {
      checkRuns: [{ name: 'lint', status: 'COMPLETED', conclusion: 'SUCCESS' }],
      reviews: [{ author: 'r', state: 'APPROVED' }],
    }))
    const { container } = renderItem(store)

    expect(container.querySelector('.tree-item-pr-signal--ready')).not.toBeNull()
  })
})
