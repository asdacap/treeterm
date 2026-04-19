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

vi.mock('../store/app', () => ({
  useAppStore: <T,>(selector: (s: { terminal: TerminalApi }) => T): T =>
    selector({ terminal: mockTerminalApi }),
}))

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
