// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import SessionTtyList from './SessionTtyList'
import type { SessionState, WorkspaceEntry } from '../store/createSessionStore'
import { WorkspaceEntryStatus } from '../store/createSessionStore'
import type { Workspace, TerminalApi, TTYSessionInfo } from '../types'

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

// The real PtyViewer instantiates an xterm Terminal (needs a real canvas); stub it
// so the dialog renders something assertable in jsdom.
vi.mock('./PtyViewer', () => ({
  default: ({ ptyId }: { ptyId: string }) => <div data-testid="pty-viewer">viewer:{ptyId}</div>,
}))

function loadedEntry(id: string, name: string, path: string): WorkspaceEntry {
  return { status: WorkspaceEntryStatus.Loaded, data: { id, name, path } as Workspace, store: {} } as unknown as WorkspaceEntry
}

function makeSessionStore(
  workspaces: Map<string, WorkspaceEntry>,
  connectionId = 'local'
): StoreApi<SessionState> {
  return createStore<SessionState>()(() => ({
    connection: { id: connectionId },
    workspaces,
  } as unknown as SessionState))
}

const TS = 1_000_000

describe('SessionTtyList', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('shows loading state immediately', () => {
    vi.mocked(mockTerminalApi.list).mockReturnValue(new Promise(() => {}))
    render(<SessionTtyList sessionStore={makeSessionStore(new Map())} />)
    expect(screen.getByText('Loading TTY sessions…')).toBeDefined()
  })

  it('shows empty state when there are no sessions', async () => {
    vi.mocked(mockTerminalApi.list).mockResolvedValue([])
    render(<SessionTtyList sessionStore={makeSessionStore(new Map())} />)
    await waitFor(() => {
      expect(screen.getByText('No TTY sessions')).toBeDefined()
    })
  })

  it('groups PTYs by workspace and buckets unmatched cwd under Unknown (last)', async () => {
    const workspaces = new Map<string, WorkspaceEntry>([
      ['ws-a', loadedEntry('ws-a', 'Alpha', '/a')],
      ['ws-b', loadedEntry('ws-b', 'Beta', '/b')],
    ])
    const sessions: TTYSessionInfo[] = [
      { id: 'pty-a1', cwd: '/a', cols: 80, rows: 24, createdAt: TS, lastActivity: TS },
      { id: 'pty-b1', cwd: '/b', cols: 80, rows: 24, createdAt: TS, lastActivity: TS },
      { id: 'pty-x1', cwd: '/elsewhere', cols: 80, rows: 24, createdAt: TS, lastActivity: TS },
    ]
    vi.mocked(mockTerminalApi.list).mockResolvedValue(sessions)
    const { container } = render(<SessionTtyList sessionStore={makeSessionStore(workspaces)} />)

    await waitFor(() => {
      expect(screen.getByText('Alpha (1)')).toBeDefined()
    })
    expect(screen.getByText('Beta (1)')).toBeDefined()
    expect(screen.getByText('Unknown (1)')).toBeDefined()

    const headers = Array.from(container.querySelectorAll('.tty-list-group-header')).map(h => h.textContent)
    expect(headers).toEqual(['Alpha (1)', 'Beta (1)', 'Unknown (1)'])
  })

  it('opens a viewer dialog when Open is clicked', async () => {
    const workspaces = new Map<string, WorkspaceEntry>([['ws-a', loadedEntry('ws-a', 'Alpha', '/a')]])
    vi.mocked(mockTerminalApi.list).mockResolvedValue([
      { id: 'pty-a1', cwd: '/a', cols: 80, rows: 24, createdAt: TS, lastActivity: TS },
    ])
    render(<SessionTtyList sessionStore={makeSessionStore(workspaces)} />)

    await waitFor(() => {
      expect(screen.getByText('Open')).toBeDefined()
    })
    expect(screen.queryByTestId('pty-viewer')).toBeNull()
    fireEvent.click(screen.getByText('Open'))
    expect(screen.getByTestId('pty-viewer').textContent).toBe('viewer:pty-a1')
  })

  it('kills a PTY and refreshes when Kill is clicked', async () => {
    const workspaces = new Map<string, WorkspaceEntry>([['ws-a', loadedEntry('ws-a', 'Alpha', '/a')]])
    vi.mocked(mockTerminalApi.list).mockResolvedValue([
      { id: 'pty-a1', cwd: '/a', cols: 80, rows: 24, createdAt: TS, lastActivity: TS },
    ])
    render(<SessionTtyList sessionStore={makeSessionStore(workspaces)} />)

    await waitFor(() => {
      expect(mockTerminalApi.list).toHaveBeenCalledTimes(1)
    })
    fireEvent.click(screen.getByText('Kill'))
    expect(mockTerminalApi.kill).toHaveBeenCalledWith('local', 'pty-a1')
    await waitFor(() => {
      expect(mockTerminalApi.list).toHaveBeenCalledTimes(2)
    })
  })

  it('shows an error message when list() rejects', async () => {
    vi.mocked(mockTerminalApi.list).mockRejectedValue(new Error('boom'))
    render(<SessionTtyList sessionStore={makeSessionStore(new Map())} />)
    await waitFor(() => {
      expect(screen.getByText('Failed to load TTY sessions')).toBeDefined()
    })
    expect(screen.getByText('boom')).toBeDefined()
  })
})
