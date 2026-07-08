import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createGhosttyTerminalApplication } from './renderer'
import type { Tab, TerminalState, WorkspaceStore } from '../../renderer/types'

// Mock React so render() returns an inspectable descriptor rather than an element.
vi.mock('react', () => ({
  createElement: vi.fn((component: unknown, props: unknown) => ({ component, props }))
}))

// GhosttyTerminal pulls in ghostty-web, which needs a DOM and a WASM instance.
vi.mock('../../renderer/components/GhosttyTerminal', () => ({ default: vi.fn(() => null) }))

// Reaching makeTerminalOnWorkspaceLoad drags in Terminal → BaseTerminal → store/app → the whole
// application registry, monaco-editor included. Stub the component out of that chain.
vi.mock('../../renderer/components/Terminal', () => ({ default: vi.fn(() => null) }))

const mockRemoveTabState = vi.fn<(tabId: string) => void>()
vi.mock('../../renderer/store/activityState', () => ({
  useActivityStateStore: { getState: vi.fn(() => ({ removeTabState: mockRemoveTabState })) }
}))

const mockKill = vi.fn<(connectionId: string, ptyId: string) => void>()
const deps = { terminal: { kill: mockKill } }

const mockEnsureTty = vi.fn<(...args: unknown[]) => Promise<string>>()
const mockUpdateTabState = vi.fn<(tabId: string, updater: (s: TerminalState) => TerminalState) => void>()

/** Only the slice of WorkspaceStore that makeTerminalOnWorkspaceLoad touches. */
function makeWorkspaceStore(appStates: Record<string, { state: TerminalState }> = {}): WorkspaceStore {
  const state = {
    workspace: { id: 'ws1', path: '/repo', appStates },
    connectionId: 'local',
    ensureTty: mockEnsureTty,
    updateTabState: mockUpdateTabState,
  }
  return { getState: () => state } as unknown as WorkspaceStore
}

function makeTab(state: Partial<TerminalState> = {}): Tab {
  return {
    id: 'tab1',
    applicationId: 'ghostty-terminal',
    title: 'Ghostty',
    state: { ptyId: null, ptyHandle: 'handle-1', keepOnExit: false, ...state },
  } as unknown as Tab
}

describe('createGhosttyTerminalApplication', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockEnsureTty.mockResolvedValue('pty-1')
  })

  it('registers as a closable, non-default terminal offered in the new-tab menu', () => {
    const app = createGhosttyTerminalApplication(deps)

    expect(app.id).toBe('ghostty-terminal')
    expect(app.name).toBe('Terminal (Ghostty)')
    expect(app.canClose).toBe(true)
    expect(app.showInNewTabMenu).toBe(true)
    expect(app.displayStyle).toBe('flex')
    // The xterm terminal stays the default; this one is opt-in.
    expect(app.isDefault).toBe(false)
  })

  it('mints a fresh ptyHandle per tab and starts with no PTY', () => {
    const app = createGhosttyTerminalApplication(deps)

    const first = app.createInitialState()
    const second = app.createInitialState()

    expect(first.ptyId).toBeNull()
    expect(first.keepOnExit).toBe(false)
    expect(first.ptyHandle).toEqual(expect.any(String))
    expect(second.ptyHandle).not.toBe(first.ptyHandle)
  })

  it('creates the PTY through the shared terminal lifecycle and writes the id back', async () => {
    const app = createGhosttyTerminalApplication(deps)
    const tab = makeTab()

    app.onWorkspaceLoad(tab, makeWorkspaceStore())
    await vi.waitFor(() => { expect(mockUpdateTabState).toHaveBeenCalled() })

    expect(mockEnsureTty).toHaveBeenCalledWith('handle-1', '/repo', undefined, undefined)
    const updater = mockUpdateTabState.mock.calls[0]?.[1]
    expect(updater?.({ ptyId: null, ptyHandle: 'handle-1', keepOnExit: false })).toEqual({
      ptyId: 'pty-1',
      ptyHandle: 'handle-1',
      connectionId: 'local',
      keepOnExit: false,
    })
  })

  it('reuses an existing PTY instead of spawning a second one', () => {
    const app = createGhosttyTerminalApplication(deps)

    app.onWorkspaceLoad(makeTab({ ptyId: 'pty-existing' }), makeWorkspaceStore())

    expect(mockEnsureTty).not.toHaveBeenCalled()
  })

  it('kills the PTY on close', () => {
    const app = createGhosttyTerminalApplication(deps)
    const tab = makeTab({ ptyId: 'pty-existing' })
    const workspace = makeWorkspaceStore({
      tab1: { state: { ptyId: 'pty-existing', ptyHandle: 'handle-1', connectionId: 'local', keepOnExit: false } },
    })

    app.onWorkspaceLoad(tab, workspace).close()

    expect(mockKill).toHaveBeenCalledWith('local', 'pty-existing')
  })

  it('drops activity state on dispose', () => {
    const app = createGhosttyTerminalApplication(deps)

    app.onWorkspaceLoad(makeTab({ ptyId: 'pty-existing' }), makeWorkspaceStore()).dispose()

    expect(mockRemoveTabState).toHaveBeenCalledWith('tab1')
  })

  it('renders GhosttyTerminal keyed by tab id', () => {
    const app = createGhosttyTerminalApplication(deps)
    const workspace = makeWorkspaceStore()

    const rendered = app.render({ tab: makeTab(), workspace, isVisible: true }) as unknown as {
      props: { key: string; tabId: string; workspace: WorkspaceStore }
    }

    expect(rendered.props.key).toBe('tab1')
    expect(rendered.props.tabId).toBe('tab1')
    expect(rendered.props.workspace).toBe(workspace)
  })
})
