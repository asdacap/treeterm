// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'

import { createStore } from 'zustand/vanilla'
import GitHubBrowser from './GitHubBrowser'
import type { GitHubPrInfo } from '../types'

// Mock lucide-react icons to simple spans
vi.mock('lucide-react', () => ({
  RefreshCw: (props: any) => <span data-testid="refresh-icon" {...props} />,
  Loader2: (props: any) => <span data-testid="loader-icon" {...props} />,
  ExternalLink: (props: any) => <span data-testid="external-link-icon" {...props} />,
  CheckCircle2: (props: any) => <span data-testid="check-circle-icon" {...props} />,
  XCircle: (props: any) => <span data-testid="x-circle-icon" {...props} />,
  Clock: (props: any) => <span data-testid="clock-icon" {...props} />,
  AlertCircle: (props: any) => <span data-testid="alert-circle-icon" {...props} />,
  MessageSquare: (props: any) => <span data-testid="message-square-icon" {...props} />,
}))

function makePrInfo(overrides: Partial<GitHubPrInfo> = {}): GitHubPrInfo {
  return {
    number: 42,
    title: 'Add feature X',
    url: 'https://github.com/test/repo/pull/42',
    state: 'OPEN',
    reviews: [],
    checkRuns: [],
    unresolvedThreads: [],
    unresolvedCount: 0,
    ...overrides,
  }
}

function makeStores(prInfo: GitHubPrInfo | null = null) {
  const refreshPrStatus = vi.fn().mockResolvedValue(undefined)
  const gitControllerStore = createStore<any>()(() => ({
    prInfo,
    refreshPrStatus,
  }))
  const workspaceStore = createStore<any>()(() => ({
    gitController: gitControllerStore,
  }))
  return { workspaceStore, gitControllerStore, refreshPrStatus }
}

describe('GitHubBrowser', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders "No open pull request" when prInfo is null', () => {
    const { workspaceStore } = makeStores(null)
    render(<GitHubBrowser workspace={workspaceStore} isVisible={true} />)
    expect(screen.getByText('No open pull request found for this branch.')).toBeDefined()
  })

  it('returns null when not visible', () => {
    const { workspaceStore } = makeStores(null)
    const { container } = render(<GitHubBrowser workspace={workspaceStore} isVisible={false} />)
    expect(container.innerHTML).toBe('')
  })

  it('renders PR title and number', () => {
    const { workspaceStore } = makeStores(makePrInfo())
    render(<GitHubBrowser workspace={workspaceStore} isVisible={true} />)
    expect(screen.getByText('#42 Add feature X')).toBeDefined()
  })

  it('renders PR state badge', () => {
    const { workspaceStore } = makeStores(makePrInfo({ state: 'MERGED' }))
    render(<GitHubBrowser workspace={workspaceStore} isVisible={true} />)
    expect(screen.getByText('Merged')).toBeDefined()
  })

  it('renders reviews when present', () => {
    const { workspaceStore } = makeStores(makePrInfo({
      reviews: [{ author: 'reviewer1', state: 'APPROVED' }],
    }))
    render(<GitHubBrowser workspace={workspaceStore} isVisible={true} />)
    expect(screen.getByText('Reviews')).toBeDefined()
    expect(screen.getByText('reviewer1')).toBeDefined()
    expect(screen.getByText('Approved')).toBeDefined()
  })

  it('hides reviews section when empty', () => {
    const { workspaceStore } = makeStores(makePrInfo({ reviews: [] }))
    render(<GitHubBrowser workspace={workspaceStore} isVisible={true} />)
    expect(screen.queryByText('Reviews')).toBeNull()
  })

  it('renders check runs when present', () => {
    const { workspaceStore } = makeStores(makePrInfo({
      checkRuns: [{ name: 'CI Build', status: 'COMPLETED', conclusion: 'SUCCESS' }],
    }))
    render(<GitHubBrowser workspace={workspaceStore} isVisible={true} />)
    expect(screen.getByText('Checks')).toBeDefined()
    expect(screen.getByText('CI Build')).toBeDefined()
  })

  it('hides check runs section when empty', () => {
    const { workspaceStore } = makeStores(makePrInfo({ checkRuns: [] }))
    render(<GitHubBrowser workspace={workspaceStore} isVisible={true} />)
    expect(screen.queryByText('Checks')).toBeNull()
  })

  it('renders "No unresolved comments" when threads are empty', () => {
    const { workspaceStore } = makeStores(makePrInfo({ unresolvedThreads: [] }))
    render(<GitHubBrowser workspace={workspaceStore} isVisible={true} />)
    expect(screen.getByText('No unresolved comments')).toBeDefined()
  })

  it('renders unresolved threads', () => {
    const { workspaceStore } = makeStores(makePrInfo({
      unresolvedThreads: [{ path: 'src/app.ts', line: 42, author: 'reviewer', body: 'Fix this', isResolved: false }],
    }))
    render(<GitHubBrowser workspace={workspaceStore} isVisible={true} />)
    expect(screen.getByText('Unresolved Comments (1)')).toBeDefined()
    expect(screen.getByText('src/app.ts:42')).toBeDefined()
    expect(screen.getByText('Fix this')).toBeDefined()
  })

  it('refresh button calls refreshPrStatus', async () => {
    const { workspaceStore, refreshPrStatus } = makeStores(null)
    render(<GitHubBrowser workspace={workspaceStore} isVisible={true} />)

    const refreshBtn = screen.getByText('Refresh').closest('button')!
    fireEvent.click(refreshBtn)

    await waitFor(() => {
      expect(refreshPrStatus).toHaveBeenCalledTimes(1)
    })
  })
})
