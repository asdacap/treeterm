// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, act } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import WebAppPane from './WebAppPane'
import type { PageSsh } from './PageViews'
import { WebAppPhase } from '../../applications/webApp/runtime'
import type { WebAppRef, WebAppRuntime } from '../../applications/webApp/runtime'
import type { PortForwardInfo, WorkspaceStore } from '../types'
import type { WorkspaceStoreState } from '../store/createWorkspaceStore'

vi.mock('./Terminal', () => ({
  default: ({ isVisible }: { isVisible: boolean }) => <div data-testid="terminal">{isVisible ? 'visible' : 'hidden'}</div>,
}))

function makeSsh(initial: PortForwardInfo[] = []) {
  let statusCb: (info: PortForwardInfo) => void = () => {}
  let outputCb: (line: string) => void = () => {}
  const unsubscribeOutput = vi.fn()
  const ssh = {
    listPortForwards: vi.fn(() => Promise.resolve(initial)),
    onPortForwardStatus: vi.fn((cb: (info: PortForwardInfo) => void) => { statusCb = cb; return vi.fn() }),
    watchPortForwardOutput: vi.fn((_id: string, cb: (line: string) => void) => {
      outputCb = cb
      return Promise.resolve({ scrollback: ['[portfwd] started'], unsubscribe: unsubscribeOutput })
    }),
  } as unknown as PageSsh
  return { ssh, emitStatus: (i: PortForwardInfo) => { statusCb(i) }, emitOutput: (l: string) => { outputCb(l) }, unsubscribeOutput }
}

function makeRef(runtime: WebAppRuntime): WebAppRef {
  return {
    runtime: createStore<WebAppRuntime>(() => runtime),
    retry: vi.fn(),
    restartForward: vi.fn(() => Promise.resolve()),
    pageUrl: vi.fn((home: string) => home),
    rememberPage: vi.fn(),
    cachedTerminal: null,
    disposeCachedTerminal: vi.fn(),
    close: vi.fn(),
    dispose: vi.fn(),
  }
}

function makeWorkspace(ref: WebAppRef, isRemote: boolean): WorkspaceStore {
  return createStore<WorkspaceStoreState>()(() => ({
    isRemote,
    connectionId: 'c',
    workspace: { path: '/proj' },
    getTabRef: () => ref,
  }) as unknown as WorkspaceStoreState)
}

beforeEach(() => { vi.clearAllMocks() })

describe('WebAppPane', () => {
  it('shows Browser and Shell sub-tabs locally and switches between them', () => {
    const ref = makeRef({ phase: WebAppPhase.AllocatingPort })
    render(<WebAppPane workspace={makeWorkspace(ref, false)} tabId="tab-1" isVisible ssh={makeSsh().ssh} openExternal={vi.fn()} />)
    expect(screen.queryByText('Port Forward')).toBeNull()
    expect(screen.getByTestId('terminal').textContent).toBe('hidden')
    fireEvent.click(screen.getByText('Shell'))
    expect(screen.getByTestId('terminal').textContent).toBe('visible')
    fireEvent.click(screen.getByText('Browser'))
    fireEvent.click(screen.getByText('View shell'))
    expect(screen.getByTestId('terminal').textContent).toBe('visible')
  })

  it('adds the Port Forward sub-tab for remote sessions', async () => {
    const ref = makeRef({ phase: WebAppPhase.Forwarding, port: 31000 })
    const { ssh } = makeSsh()
    render(<WebAppPane workspace={makeWorkspace(ref, true)} tabId="tab-1" isVisible ssh={ssh} openExternal={vi.fn()} />)
    fireEvent.click(screen.getByText('Port Forward'))
    expect(await screen.findByText(/No port forward yet/)).toBeTruthy()
  })

  it('follows runtime transitions', () => {
    const ref = makeRef({ phase: WebAppPhase.AllocatingPort })
    render(<WebAppPane workspace={makeWorkspace(ref, false)} tabId="tab-1" isVisible ssh={makeSsh().ssh} openExternal={vi.fn()} />)
    act(() => { ref.runtime.setState({ phase: WebAppPhase.WaitingForServer, port: 1234 }, true) })
    expect(screen.getByText('Waiting for server on port 1234…')).toBeTruthy()
  })
})
