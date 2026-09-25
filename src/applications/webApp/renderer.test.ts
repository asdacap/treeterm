import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createStore } from 'zustand/vanilla'
import { createWebAppVariant, resolveCommand } from './renderer'
import type { WebAppDeps } from './renderer'
import { WebAppPhase, forwardIdForTab } from './runtime'
import type { WebAppRef } from './runtime'
import type { HttpProbe } from './ports'
import type { Tab, Workspace, WebAppInstance, WebAppState, SSHApi, PortForwardInfo, PortForwardConfig } from '../../renderer/types'
import { WebAppPortStatus } from '../../renderer/types'
import { PortForwardStatus } from '../../shared/types'
import type { WorkspaceStore, WorkspaceStoreState } from '../../renderer/store/createWorkspaceStore'

vi.mock('react', () => ({
  createElement: vi.fn((component: unknown, props: unknown) => ({ component, props }))
}))

vi.mock('../../renderer/components/WebAppPane', () => ({
  default: vi.fn(() => null)
}))

const mockRemoveTabState = vi.fn<(tabId: string) => void>()
vi.mock('../../renderer/store/activityState', () => ({
  useActivityStateStore: { getState: vi.fn(() => ({ removeTabState: mockRemoveTabState })) }
}))

const probe = vi.fn<HttpProbe>()
vi.mock('./ports', async (importOriginal) => ({
  ...await importOriginal<typeof import('./ports')>(),
  createHttpProbe: () => probe,
}))

const instance: WebAppInstance = {
  id: 'next', name: 'Next', icon: '🌐', command: 'npm run dev -- -p $PORT', isDefault: false, keepOnExit: true,
}

const TAB_ID = 'tab-1'

/** Minimal in-memory ssh forward registry mirroring main's ConnectionManager. */
function makeSsh(statusOnAdd: (config: PortForwardConfig) => PortForwardStatus) {
  const forwards = new Map<string, PortForwardInfo>()
  const ssh = {
    addPortForward: vi.fn((config: PortForwardConfig) => {
      const info = { ...config, status: statusOnAdd(config), error: 'bind failed' } as PortForwardInfo
      forwards.set(config.id, info)
      return Promise.resolve(info)
    }),
    removePortForward: vi.fn((id: string) => { forwards.delete(id); return Promise.resolve() }),
    listPortForwards: vi.fn(() => Promise.resolve(Array.from(forwards.values()))),
  }
  return { ssh, forwards }
}

function makeDeps(ssh: Partial<SSHApi>): WebAppDeps {
  return {
    terminal: { kill: vi.fn() },
    ssh: ssh as SSHApi,
    openExternal: vi.fn(),
    sleep: vi.fn(() => Promise.resolve()),
    random: vi.fn(() => 0),
  }
}

function makeWorkspaceStore(state: WebAppState, isRemote: boolean, ensureTty = vi.fn().mockResolvedValue('pty-1')): WorkspaceStore {
  const store = createStore<WorkspaceStoreState>()((set, get) => ({
    workspace: { id: 'ws-1', path: '/proj', appStates: { [TAB_ID]: { applicationId: 'webapp-next', title: 'Next', state } } } as unknown as Workspace,
    connectionId: 'conn-1',
    isRemote,
    ensureTty,
    execApi: {},
    updateTabState: <T,>(tabId: string, updater: (s: T) => T) => {
      const ws = get().workspace
      const entry = ws.appStates[tabId]
      if (!entry) return
      set({ workspace: { ...ws, appStates: { ...ws.appStates, [tabId]: { ...entry, state: updater(entry.state as T) } } } })
    },
  }) as unknown as WorkspaceStoreState)
  return store
}

function tabState(store: WorkspaceStore): WebAppState {
  return store.getState().workspace.appStates[TAB_ID]!.state as WebAppState
}

function load(deps: WebAppDeps, store: WorkspaceStore): WebAppRef {
  const app = createWebAppVariant(instance, deps)
  const tab = { id: TAB_ID, ...store.getState().workspace.appStates[TAB_ID]! } as Tab
  return app.onWorkspaceLoad(tab, store)
}

function freshState(): WebAppState {
  return createWebAppVariant(instance, makeDeps({})).createInitialState()
}

async function waitForPhase(ref: WebAppRef, phase: WebAppPhase): Promise<void> {
  await vi.waitFor(() => { expect(ref.runtime.getState().phase).toBe(phase) })
}

beforeEach(() => {
  vi.clearAllMocks()
  probe.mockReset()
})

describe('resolveCommand', () => {
  it('replaces every $PORT', () => {
    expect(resolveCommand('a $PORT b $PORT', 42)).toBe('a 42 b 42')
  })
})

describe('createWebAppVariant', () => {
  it('has instance-derived properties and unassigned ports', () => {
    const app = createWebAppVariant({ ...instance, isDefault: true }, makeDeps({}))
    expect(app.id).toBe('webapp-next')
    expect(app.name).toBe('Next')
    expect(app.isDefault).toBe(true)
    const state = app.createInitialState()
    expect(state.ptyId).toBeNull()
    expect(state.keepOnExit).toBe(true)
    expect(state.port).toEqual({ status: WebAppPortStatus.Unassigned })
    expect(state.localPort).toEqual({ status: WebAppPortStatus.Unassigned })
    expect(state.path).toBe('')
  })

  it('remembers the page the tab was on', () => {
    const store = makeWorkspaceStore(freshState(), false)
    const ref = load(makeDeps({}), store)
    ref.rememberPage('http://localhost:3000/', 'http://localhost:3000/about')
    expect(tabState(store).path).toBe('/about')
    expect(ref.pageUrl('http://localhost:3000/')).toBe('http://localhost:3000/about')
    ref.dispose()
  })

  it('local: allocates a port, starts the command with it, and becomes ready', async () => {
    probe.mockResolvedValueOnce(7) // port free
      .mockResolvedValueOnce(7) // server not up yet
      .mockResolvedValue(0)
    const store = makeWorkspaceStore(freshState(), false)
    const ref = load(makeDeps({}), store)

    await waitForPhase(ref, WebAppPhase.Ready)
    expect(ref.runtime.getState()).toEqual({ phase: WebAppPhase.Ready, url: 'http://localhost:20000/' })
    expect(store.getState().ensureTty).toHaveBeenCalledWith(expect.any(String), '/proj', undefined, 'npm run dev -- -p 20000')
    expect(tabState(store)).toMatchObject({ ptyId: 'pty-1', connectionId: 'conn-1', port: { status: WebAppPortStatus.Assigned, port: 20000 } })
  })

  it('restore: reuses the persisted port and PTY', async () => {
    probe.mockResolvedValue(0)
    const store = makeWorkspaceStore({ ...freshState(), ptyId: 'pty-9', port: { status: WebAppPortStatus.Assigned, port: 31000 } }, false)
    const ref = load(makeDeps({}), store)
    await waitForPhase(ref, WebAppPhase.Ready)
    expect(store.getState().ensureTty).not.toHaveBeenCalled()
    expect(ref.runtime.getState()).toEqual({ phase: WebAppPhase.Ready, url: 'http://localhost:31000/' })
  })

  it('reports errors and can retry', async () => {
    probe.mockRejectedValueOnce(new Error('curl missing')).mockResolvedValue(0)
    const store = makeWorkspaceStore({ ...freshState(), ptyId: 'pty-9', port: { status: WebAppPortStatus.Assigned, port: 31000 } }, false)
    const ref = load(makeDeps({}), store)
    await waitForPhase(ref, WebAppPhase.Error)
    expect(ref.runtime.getState()).toEqual({ phase: WebAppPhase.Error, message: 'curl missing' })
    ref.retry()
    await waitForPhase(ref, WebAppPhase.Ready)
  })

  it('stringifies non-Error failures', async () => {
    probe.mockRejectedValueOnce('boom')
    const store = makeWorkspaceStore({ ...freshState(), ptyId: 'pty-9', port: { status: WebAppPortStatus.Assigned, port: 1 } }, false)
    const ref = load(makeDeps({}), store)
    await waitForPhase(ref, WebAppPhase.Error)
    expect(ref.runtime.getState()).toEqual({ phase: WebAppPhase.Error, message: 'boom' })
  })

  it('does not start the PTY or report errors once disposed', async () => {
    let releasePort: (code: number) => void = () => {}
    probe.mockReturnValueOnce(new Promise((resolve) => { releasePort = resolve }))
    const store = makeWorkspaceStore(freshState(), false)
    const ref = load(makeDeps({}), store)
    ref.dispose()
    releasePort(7)
    await vi.waitFor(() => { expect(tabState(store).port.status).toBe(WebAppPortStatus.Assigned) })
    expect(store.getState().ensureTty).not.toHaveBeenCalled()
    expect(ref.runtime.getState().phase).toBe(WebAppPhase.AllocatingPort)
    expect(mockRemoveTabState).toHaveBeenCalledWith(TAB_ID)
  })

  it('swallows a failure that lands after dispose', async () => {
    let fail: (err: Error) => void = () => {}
    probe.mockReturnValueOnce(new Promise((_resolve, reject) => { fail = reject }))
    const store = makeWorkspaceStore(freshState(), false)
    const ref = load(makeDeps({}), store)
    ref.dispose()
    fail(new Error('late'))
    await Promise.resolve()
    await Promise.resolve()
    expect(ref.runtime.getState().phase).toBe(WebAppPhase.AllocatingPort)
  })

  it('does nothing when the tab is already gone', () => {
    const store = makeWorkspaceStore(freshState(), false)
    const app = createWebAppVariant(instance, makeDeps({}))
    const ref = app.onWorkspaceLoad({ id: 'missing', applicationId: 'webapp-next', title: 'x', state: freshState() }, store)
    expect(ref.runtime.getState().phase).toBe(WebAppPhase.AllocatingPort)
    expect(probe).not.toHaveBeenCalled()
  })

  describe('remote', () => {
    const readyState = (): WebAppState => ({ ...freshState(), ptyId: 'pty-9', port: { status: WebAppPortStatus.Assigned, port: 31000 } })

    it('forwards the port and points the browser at the local end', async () => {
      probe.mockResolvedValue(0)
      const { ssh } = makeSsh(() => PortForwardStatus.Active)
      const store = makeWorkspaceStore(readyState(), true)
      const ref = load(makeDeps(ssh), store)
      await waitForPhase(ref, WebAppPhase.Ready)
      expect(ssh.addPortForward).toHaveBeenCalledWith({
        id: forwardIdForTab(TAB_ID), connectionId: 'conn-1', localPort: 20000, remoteHost: 'localhost', remotePort: 31000, persist: false,
      })
      expect(ref.runtime.getState()).toEqual({ phase: WebAppPhase.Ready, url: 'http://localhost:20000/' })
      expect(tabState(store).localPort).toEqual({ status: WebAppPortStatus.Assigned, port: 20000 })
    })

    it('restartForward removes the forward and runs again', async () => {
      probe.mockResolvedValue(0)
      const { ssh } = makeSsh(() => PortForwardStatus.Active)
      const ref = load(makeDeps(ssh), makeWorkspaceStore(readyState(), true))
      await waitForPhase(ref, WebAppPhase.Ready)
      await ref.restartForward()
      await vi.waitFor(() => { expect(ssh.addPortForward).toHaveBeenCalledTimes(2) })
      expect(ssh.removePortForward).toHaveBeenCalledWith(forwardIdForTab(TAB_ID))
    })

    it('stops before forwarding when disposed while waiting for the server', async () => {
      let answer: (code: number) => void = () => {}
      probe.mockReturnValueOnce(new Promise((resolve) => { answer = resolve }))
      const { ssh } = makeSsh(() => PortForwardStatus.Active)
      const ref = load(makeDeps(ssh), makeWorkspaceStore(readyState(), true))
      await waitForPhase(ref, WebAppPhase.WaitingForServer)
      ref.dispose()
      answer(7)
      await vi.waitFor(() => { expect(probe).toHaveBeenCalledTimes(1) })
      await Promise.resolve()
      expect(ssh.listPortForwards).not.toHaveBeenCalled()
    })

    it('close removes the forward and kills the PTY', () => {
      probe.mockReturnValue(new Promise(() => {}))
      const { ssh } = makeSsh(() => PortForwardStatus.Active)
      const deps = makeDeps(ssh)
      const ref = load(deps, makeWorkspaceStore(readyState(), true))
      ref.close()
      expect(ssh.removePortForward).toHaveBeenCalledWith(forwardIdForTab(TAB_ID))
      expect(deps.terminal.kill).toHaveBeenCalledWith('conn-1', 'pty-9')
    })
  })

  describe('close (local)', () => {
    it('kills a PTY whose creation is still in flight', async () => {
      probe.mockResolvedValueOnce(7).mockReturnValue(new Promise(() => {}))
      let resolvePty: (id: string) => void = () => {}
      const ensureTty = vi.fn(() => new Promise<string>((resolve) => { resolvePty = resolve }))
      const deps = makeDeps({})
      const ref = load(deps, makeWorkspaceStore(freshState(), false, ensureTty))
      await vi.waitFor(() => { expect(ensureTty).toHaveBeenCalled() })
      ref.close()
      expect(deps.terminal.kill).not.toHaveBeenCalled()
      resolvePty('pty-late')
      await vi.waitFor(() => { expect(deps.terminal.kill).toHaveBeenCalledWith('conn-1', 'pty-late') })
    })

    it('does nothing when no PTY was ever requested', () => {
      probe.mockReturnValue(new Promise(() => {}))
      const deps = makeDeps({ removePortForward: vi.fn() })
      const ref = load(deps, makeWorkspaceStore(freshState(), false))
      ref.close()
      expect(deps.terminal.kill).not.toHaveBeenCalled()
      expect(deps.ssh.removePortForward).not.toHaveBeenCalled()
    })
  })

  it('disposeCachedTerminal tears down the cached engine', () => {
    probe.mockReturnValue(new Promise(() => {}))
    const ref = load(makeDeps({}), makeWorkspaceStore(freshState(), false))
    const owner = { dispose: vi.fn() }
    ref.cachedTerminal = { mountedHandler: vi.fn(), owner } as unknown as WebAppRef['cachedTerminal']
    ref.dispose()
    expect(owner.dispose).toHaveBeenCalled()
    expect(ref.cachedTerminal).toBeNull()
  })

  describe('render', () => {
    it('renders the pane with injected deps', () => {
      const deps = makeDeps({})
      const app = createWebAppVariant(instance, deps)
      const store = makeWorkspaceStore(freshState(), false)
      const tab = { id: TAB_ID, ...store.getState().workspace.appStates[TAB_ID]! } as Tab
      const el = app.render({ tab, workspace: store, isVisible: true }) as unknown as { props: Record<string, unknown> }
      expect(el.props).toMatchObject({ tabId: TAB_ID, isVisible: true, ssh: deps.ssh, openExternal: deps.openExternal })
    })

    it('renders nothing for a foreign state shape', () => {
      const app = createWebAppVariant(instance, makeDeps({}))
      const store = makeWorkspaceStore(freshState(), false)
      expect(app.render({ tab: { id: 't', applicationId: 'x', title: 'x', state: {} }, workspace: store, isVisible: true })).toBeNull()
    })
  })
})
