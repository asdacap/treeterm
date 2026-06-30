// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import SessionInfoPane from './SessionInfoPane'
import type { SessionState } from '../store/createSessionStore'
import {
  ConnectionStatus,
  ConnectionErrorKind,
  ConnectionTargetType,
} from '../../shared/types'

// Hoisted so the (hoisted) vi.mock factory can reference it without a TDZ error.
const h = vi.hoisted(() => {
  const sshApi = {
    connect: vi.fn(),
    watchBootstrapOutput: vi.fn().mockResolvedValue({ scrollback: [], unsubscribe: () => {} }),
    watchTunnelOutput: vi.fn().mockResolvedValue({ scrollback: [], unsubscribe: () => {} }),
    watchDaemonOutput: vi.fn().mockResolvedValue({ scrollback: [], unsubscribe: () => {} }),
    listPortForwards: vi.fn().mockResolvedValue([]),
    onPortForwardStatus: vi.fn(() => () => {}),
    forceReconnect: vi.fn(),
  }
  const appState = {
    ssh: sshApi,
    exec: {},
    disconnectSession: vi.fn(),
    sessionNamesStore: undefined as unknown,
    setSessionError: vi.fn(),
    addRemoteSession: vi.fn(),
  }
  return { sshApi, appState }
})
const sshApi = h.sshApi
const appState = h.appState

const sessionNamesStore = createStore<{ names: Map<string, { name: string }> }>()(() => ({
  names: new Map(),
}))
appState.sessionNamesStore = sessionNamesStore

// `useAppStore` is used both as a hook (`useAppStore(selector)`) and statically
// (`useAppStore.getState()`), so the mock is a callable with a `getState` property.
vi.mock('../store/app', () => ({
  useAppStore: Object.assign(
    (selector: (s: typeof appState) => unknown): unknown => selector(appState),
    { getState: () => appState },
  ),
}))

const REMOTE_CONFIG = { id: 'conn-1', host: 'h', user: 'u', port: 22, identityFile: undefined, label: 'h', portForwards: [] }

function makeMismatchStore(): StoreApi<SessionState> {
  return createStore<SessionState>()(() => ({
    sessionId: 'session-1',
    activeWorkspaceId: undefined,
    sessionVersion: 1,
    workspaces: new Map(),
    connection: {
      id: 'conn-1',
      status: ConnectionStatus.Error,
      error: 'Daemon binary hash mismatch (local=aaa... remote=bbb...).',
      errorKind: ConnectionErrorKind.DaemonHashMismatch,
      target: { type: ConnectionTargetType.Remote, config: REMOTE_CONFIG },
    },
  } as unknown as SessionState))
}

describe('SessionInfoPane — daemon hash mismatch', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    sshApi.connect.mockResolvedValue({
      info: { status: ConnectionStatus.Error, error: 'still broken', errorKind: ConnectionErrorKind.Generic },
      session: null,
    })
  })

  it('shows the Daemon Mismatch tab, active, with both recovery buttons', () => {
    render(<SessionInfoPane sessionStore={makeMismatchStore()} />)
    const tab = screen.getByText('Daemon Mismatch')
    expect(tab.className).toContain('active')
    expect(screen.getByText('Refresh daemon')).toBeDefined()
    expect(screen.getByText('Ignore mismatch & connect anyway')).toBeDefined()
    // Inline warning about dropping processes is shown next to Refresh
    expect(screen.getByText(/Drops all running processes/)).toBeDefined()
    // The mismatch error message is surfaced in the dedicated tab body
    const body = document.querySelector('.ssh-pane-mismatch-message')
    expect(body?.textContent).toContain('Daemon binary hash mismatch')
  })

  it('Refresh daemon reconnects with refreshDaemon=true', () => {
    render(<SessionInfoPane sessionStore={makeMismatchStore()} />)
    fireEvent.click(screen.getByText('Refresh daemon'))
    expect(sshApi.connect).toHaveBeenCalledWith(REMOTE_CONFIG, { refreshDaemon: true })
  })

  it('Ignore mismatch reconnects with allowOutdatedDaemon=true', () => {
    render(<SessionInfoPane sessionStore={makeMismatchStore()} />)
    fireEvent.click(screen.getByText('Ignore mismatch & connect anyway'))
    expect(sshApi.connect).toHaveBeenCalledWith(REMOTE_CONFIG, { allowOutdatedDaemon: true })
  })
})
