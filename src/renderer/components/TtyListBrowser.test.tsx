// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import TtyListBrowser from './TtyListBrowser'
import type { Workspace, TerminalApi, TTYSessionInfo } from '../types'
import type { WorkspaceStoreState } from '../store/createWorkspaceStore'
import type { ApplicationRenderProps } from '../types'

const mockTerminalApi: TerminalApi = {
  create: vi.fn(),
  attach: vi.fn(),
  list: vi.fn(),
  write: vi.fn(),
  resize: vi.fn(),
  kill: vi.fn(),
  onEvent: vi.fn(() => () => {}),
  onActiveProcessesOpen: vi.fn(() => () => {}),
  createSession: vi.fn(),
} as unknown as TerminalApi

// Mutable set of session stores the component reads via useAppStore.getState() to
// figure out which TTYs are referenced by a tab. Tests push entries before render.
const mockSessionStores = new Map<string, { store: { getState: () => unknown } }>()

vi.mock('../store/app', () => {
  const useAppStore = <T,>(selector: (s: { terminal: TerminalApi }) => T): T =>
    selector({ terminal: mockTerminalApi })
  useAppStore.getState = (): { sessionStores: typeof mockSessionStores } => ({ sessionStores: mockSessionStores })
  return { useAppStore }
})

// Build a session store entry exposing the appStates the component scans.
function mockSessionEntry(connectionId: string, ptyIds: (string | null)[]): { store: { getState: () => unknown } } {
  const appStates = Object.fromEntries(
    ptyIds.map((ptyId, i) => [`tab-${String(i)}`, { applicationId: 'terminal', title: 'T', state: { ptyId } }])
  )
  return {
    store: {
      getState: () => ({
        connection: { id: connectionId },
        workspaces: new Map([['ws-1', { status: 'loaded', data: { appStates } }]]),
      }),
    },
  }
}

function makeWorkspaceStore(addTab: WorkspaceStoreState['addTab'], path = '/test'): ApplicationRenderProps['workspace'] {
  return createStore<WorkspaceStoreState>()(() => ({
    workspace: { id: 'ws-1', path } as Workspace,
    connectionId: 'local',
    addTab,
    appStates: {},
    metadata: {},
  } as unknown as WorkspaceStoreState))
}

const sampleTab = { id: 'tab-1', applicationId: 'tty-list', title: 'TTYs', state: {} }

describe('TtyListBrowser', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockSessionStores.clear()
  })

  it('shows loading state immediately', () => {
    vi.mocked(mockTerminalApi.list).mockReturnValue(new Promise(() => {}))
    const addTab = vi.fn()
    render(
      <TtyListBrowser tab={sampleTab} workspace={makeWorkspaceStore(addTab)} isVisible />
    )
    expect(screen.getByText('Loading TTY sessions…')).toBeDefined()
  })

  it('shows empty state when no sessions match the workspace path', async () => {
    const sessions: TTYSessionInfo[] = [
      { id: 'pty-other', cwd: '/other', cols: 80, rows: 24, createdAt: Date.now(), lastActivity: Date.now() }
    ]
    vi.mocked(mockTerminalApi.list).mockResolvedValue(sessions)
    const addTab = vi.fn()
    render(
      <TtyListBrowser tab={sampleTab} workspace={makeWorkspaceStore(addTab)} isVisible />
    )
    await waitFor(() => {
      expect(screen.getByText('No TTY sessions for this workspace')).toBeDefined()
    })
  })

  it('lists sessions matching the workspace path', async () => {
    const now = Date.now()
    const sessions: TTYSessionInfo[] = [
      { id: 'pty-aaaaaaaa-bbbb', cwd: '/test', cols: 120, rows: 30, createdAt: now, lastActivity: now },
      { id: 'pty-other', cwd: '/other', cols: 80, rows: 24, createdAt: now, lastActivity: now },
    ]
    vi.mocked(mockTerminalApi.list).mockResolvedValue(sessions)
    const addTab = vi.fn()
    render(
      <TtyListBrowser tab={sampleTab} workspace={makeWorkspaceStore(addTab)} isVisible />
    )
    await waitFor(() => {
      expect(screen.getByText('pty-aaaa')).toBeDefined()
    })
    expect(screen.getByText('120×30')).toBeDefined()
    expect(screen.queryByText('pty-othe')).toBeNull()
  })

  it('flags a TTY that no tab references as an orphan', async () => {
    const now = Date.now()
    const sessions: TTYSessionInfo[] = [
      { id: 'pty-orphan-1', cwd: '/test', cols: 80, rows: 24, createdAt: now, lastActivity: now },
    ]
    vi.mocked(mockTerminalApi.list).mockResolvedValue(sessions)
    mockSessionStores.set('s1', mockSessionEntry('local', ['pty-other']))
    render(
      <TtyListBrowser tab={sampleTab} workspace={makeWorkspaceStore(vi.fn())} isVisible />
    )
    await waitFor(() => {
      expect(screen.getByText('pty-orph')).toBeDefined()
    })
    expect(screen.getByText('orphan')).toBeDefined()
  })

  it('does not flag a TTY referenced by a terminal tab', async () => {
    const now = Date.now()
    const sessions: TTYSessionInfo[] = [
      { id: 'pty-live-1', cwd: '/test', cols: 80, rows: 24, createdAt: now, lastActivity: now },
    ]
    vi.mocked(mockTerminalApi.list).mockResolvedValue(sessions)
    mockSessionStores.set('s1', mockSessionEntry('local', ['pty-live-1']))
    render(
      <TtyListBrowser tab={sampleTab} workspace={makeWorkspaceStore(vi.fn())} isVisible />
    )
    await waitFor(() => {
      expect(screen.getByText('pty-live')).toBeDefined()
    })
    expect(screen.queryByText('orphan')).toBeNull()
  })

  it('does not flag a TTY referenced by an aiharness tab', async () => {
    const now = Date.now()
    const sessions: TTYSessionInfo[] = [
      { id: 'pty-harness-1', cwd: '/test', cols: 80, rows: 24, createdAt: now, lastActivity: now },
    ]
    vi.mocked(mockTerminalApi.list).mockResolvedValue(sessions)
    // aiharness tab state is a TerminalState with a ptyId — same shape isTerminalState matches.
    const entry = mockSessionEntry('local', [])
    entry.store.getState = () => ({
      connection: { id: 'local' },
      workspaces: new Map([['ws-1', {
        status: 'loaded',
        data: { appStates: { 'tab-0': { applicationId: 'aiharness-claude', title: 'AI', state: { ptyId: 'pty-harness-1', sandbox: {} } } } },
      }]]),
    })
    mockSessionStores.set('s1', entry)
    render(
      <TtyListBrowser tab={sampleTab} workspace={makeWorkspaceStore(vi.fn())} isVisible />
    )
    await waitFor(() => {
      expect(screen.getByText('pty-harn')).toBeDefined()
    })
    expect(screen.queryByText('orphan')).toBeNull()
  })

  it('opens an existing TTY in a new terminal tab on click', async () => {
    const session: TTYSessionInfo = {
      id: 'pty-existing-1', cwd: '/test', cols: 80, rows: 24, createdAt: Date.now(), lastActivity: Date.now()
    }
    vi.mocked(mockTerminalApi.list).mockResolvedValue([session])
    const addTab = vi.fn()
    render(
      <TtyListBrowser tab={sampleTab} workspace={makeWorkspaceStore(addTab)} isVisible />
    )
    await waitFor(() => {
      expect(screen.getByText('Open')).toBeDefined()
    })
    fireEvent.click(screen.getByText('Open'))
    expect(addTab).toHaveBeenCalledWith('terminal', { ptyId: 'pty-existing-1', connectionId: 'local' })
  })

  it('kills a TTY when Kill is clicked', async () => {
    const session: TTYSessionInfo = {
      id: 'pty-existing-1', cwd: '/test', cols: 80, rows: 24, createdAt: Date.now(), lastActivity: Date.now()
    }
    vi.mocked(mockTerminalApi.list).mockResolvedValue([session])
    const addTab = vi.fn()
    render(
      <TtyListBrowser tab={sampleTab} workspace={makeWorkspaceStore(addTab)} isVisible />
    )
    await waitFor(() => {
      expect(screen.getByText('Kill')).toBeDefined()
    })
    fireEvent.click(screen.getByText('Kill'))
    expect(mockTerminalApi.kill).toHaveBeenCalledWith('local', 'pty-existing-1')
  })

  it('shows an error message when list() rejects', async () => {
    vi.mocked(mockTerminalApi.list).mockRejectedValue(new Error('boom'))
    const addTab = vi.fn()
    render(
      <TtyListBrowser tab={sampleTab} workspace={makeWorkspaceStore(addTab)} isVisible />
    )
    await waitFor(() => {
      expect(screen.getByText('Failed to load TTY sessions')).toBeDefined()
    })
    expect(screen.getByText('boom')).toBeDefined()
  })

  it('refreshes the list when Refresh is clicked', async () => {
    const session: TTYSessionInfo = {
      id: 'pty-1', cwd: '/test', cols: 80, rows: 24, createdAt: Date.now(), lastActivity: Date.now()
    }
    vi.mocked(mockTerminalApi.list).mockResolvedValue([session])
    const addTab = vi.fn()
    render(
      <TtyListBrowser tab={sampleTab} workspace={makeWorkspaceStore(addTab)} isVisible />
    )
    await waitFor(() => {
      expect(mockTerminalApi.list).toHaveBeenCalledTimes(1)
    })
    fireEvent.click(screen.getByText('Refresh'))
    await waitFor(() => {
      expect(mockTerminalApi.list).toHaveBeenCalledTimes(2)
    })
  })
})
