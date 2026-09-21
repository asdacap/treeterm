// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, fireEvent, act } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import { FavouriteWorkspaceItem } from './TreePane'
import type { WorkspaceStoreState } from '../store/createWorkspaceStore'
import { WorkspaceEntryStatus, type SessionState } from '../store/createSessionStore'
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
    attentionPending: false,
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
    workspaces: new Map([['ws-1', { status: WorkspaceEntryStatus.Loaded, data: makeWorkspace(), store }]]),
    setActiveWorkspace: vi.fn(),
  }) as unknown as SessionState) as unknown as StoreApi<SessionState>
  const rendered = render(
    <FavouriteWorkspaceItem
      sessionId="s-1"
      sessionStore={sessionStore}
      workspaceId="ws-1"
      workspaceStore={store}
      data={makeWorkspace({ name: 'my-branch' })}
    />
  )
  return { ...rendered, sessionStore }
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


describe('favourite workspace unread attention', () => {
  it('tracks committed attention without acknowledging from the row itself', () => {
    const store = makeWorkspaceStore(null)
    store.setState({ metadata: { isFavourite: 'true', workspaceAttention: JSON.stringify({ revision: 'event-1', acknowledgedRevision: '' }) } })
    const { container, getByRole, queryByRole } = renderItem(store)
    const row = container.querySelector('.tree-item')!
    expect(row.classList.contains('workspace-unread')).toBe(true)
    fireEvent.click(row)
    expect(getByRole('img', { name: 'Unread workspace activity' })).toBeTruthy()
    act(() => { store.setState({ metadata: { isFavourite: 'true', workspaceAttention: JSON.stringify({ revision: 'event-1', acknowledgedRevision: 'event-1' }) } }) })
    expect(queryByRole('img', { name: 'Unread workspace activity' })).toBeNull()
    expect(row.classList.contains('workspace-unread')).toBe(false)
  })
})


describe('FavouriteWorkspaceItem — workspace attention state', () => {
  it('renders pending from the workspace store and removes the indicator when committed', () => {
    const store = makeWorkspaceStore(null)
    const { getByRole, queryByRole } = renderItem(store)
    act(() => { store.setState({ attentionPending: true }) })
    expect(getByRole('status', { name: 'Saving workspace attention' })).toBeTruthy()
    act(() => { store.setState({ attentionPending: false }) })
    expect(queryByRole('status', { name: 'Saving workspace attention' })).toBeNull()
  })

  it('renders nothing when its entry is no longer loaded or is removed', () => {
    const { sessionStore, container } = renderItem(makeWorkspaceStore(null))
    act(() => {
      sessionStore.setState({ workspaces: new Map([['ws-1', { status: WorkspaceEntryStatus.Error, name: 'failed', error: 'failure' }]]) })
    })
    expect(container.querySelector('.tree-item')).toBeNull()
    act(() => { sessionStore.setState({ workspaces: new Map() }) })
    expect(container.querySelector('.tree-item')).toBeNull()
  })
})
