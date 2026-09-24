import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createStore } from 'zustand/vanilla'
import { createBrowserVariant } from './renderer'
import { forwardedUrl, parseLocalUrl, targetPort } from './url'
import type { BrowserDeps } from './renderer'
import { WebAppPhase, forwardIdForTab } from '../webApp/runtime'
import type { PageRef } from '../webApp/runtime'
import type { BrowserInstance, BrowserState, SSHApi, Tab, Workspace } from '../../renderer/types'
import { WebAppPortStatus } from '../../renderer/types'
import type { WorkspaceStore, WorkspaceStoreState } from '../../renderer/store/createWorkspaceStore'

vi.mock('react', () => ({
  createElement: vi.fn((component: unknown, props: unknown) => ({ component, props }))
}))

vi.mock('../../renderer/components/BrowserPane', () => ({
  default: vi.fn(() => null)
}))

const mockEnsureForward = vi.fn<(opts: { preferredLocalPort: unknown; remotePort: number; forwardId: string }) => Promise<number>>()
vi.mock('../webApp/forward', () => ({
  ensureForward: (opts: { preferredLocalPort: unknown; remotePort: number; forwardId: string }) => mockEnsureForward(opts),
}))

const TAB_ID = 'tab-1'
const instance: BrowserInstance = { id: 'graf', name: 'Grafana', icon: '📈', url: 'http://localhost:3000/d/x?from=now', isDefault: false }

function makeDeps(): BrowserDeps {
  return {
    ssh: { removePortForward: vi.fn().mockResolvedValue(undefined) } as unknown as SSHApi,
    openExternal: vi.fn(),
    sleep: vi.fn(() => Promise.resolve()),
    random: () => 0,
  }
}

function makeStore(isRemote: boolean, state: BrowserState = { localPort: { status: WebAppPortStatus.Unassigned } }): WorkspaceStore {
  return createStore<WorkspaceStoreState>()((set, get) => ({
    workspace: { id: 'ws', path: '/p', appStates: { [TAB_ID]: { applicationId: 'browser-graf', title: 'Grafana', state } } } as unknown as Workspace,
    connectionId: 'conn-1',
    isRemote,
    updateTabState: <T,>(tabId: string, updater: (s: T) => T) => {
      const ws = get().workspace
      const entry = ws.appStates[tabId]!
      set({ workspace: { ...ws, appStates: { ...ws.appStates, [tabId]: { ...entry, state: updater(entry.state as T) } } } })
    },
  }) as unknown as WorkspaceStoreState)
}

function load(store: WorkspaceStore, deps = makeDeps(), inst = instance, tabId = TAB_ID): PageRef {
  const tab = { id: tabId, applicationId: 'browser-graf', title: 'Grafana', state: {} } as Tab
  return createBrowserVariant(inst, deps).onWorkspaceLoad(tab, store)
}

beforeEach(() => { vi.clearAllMocks() })

describe('url helpers', () => {
  it('accepts only http(s) localhost URLs', () => {
    expect(parseLocalUrl('https://127.0.0.1:8443/').port).toBe('8443')
    expect(() => parseLocalUrl('http://example.com/')).toThrow('URL must be http(s)://localhost or 127.0.0.1')
    expect(() => parseLocalUrl('ftp://localhost/')).toThrow()
    expect(() => parseLocalUrl('nonsense')).toThrow()
  })

  it('derives the port, defaulting by scheme', () => {
    expect(targetPort(new URL('http://localhost:8080/'))).toBe(8080)
    expect(targetPort(new URL('http://localhost/'))).toBe(80)
    expect(targetPort(new URL('https://localhost/'))).toBe(443)
  })

  it('rewrites host and port but keeps path and query', () => {
    expect(forwardedUrl(new URL('http://127.0.0.1:3000/a/b?c=1#h'), 21000)).toBe('http://localhost:21000/a/b?c=1#h')
  })
})

describe('createBrowserVariant', () => {
  it('has instance-derived properties', () => {
    const app = createBrowserVariant({ ...instance, isDefault: true }, makeDeps())
    expect(app.id).toBe('browser-graf')
    expect(app.name).toBe('Grafana')
    expect(app.isDefault).toBe(true)
    expect(app.createInitialState()).toEqual({ localPort: { status: WebAppPortStatus.Unassigned } })
  })

  it('local: opens the URL directly', () => {
    const ref = load(makeStore(false))
    expect(ref.runtime.getState()).toEqual({ phase: WebAppPhase.Ready, url: 'http://localhost:3000/d/x?from=now' })
    expect(mockEnsureForward).not.toHaveBeenCalled()
  })

  it('reports an invalid URL', async () => {
    const ref = load(makeStore(false), makeDeps(), { ...instance, url: 'http://example.com' })
    await vi.waitFor(() => { expect(ref.runtime.getState().phase).toBe(WebAppPhase.Error) })
  })

  it('remote: forwards the port, persists the local end and opens the rewritten URL', async () => {
    mockEnsureForward.mockResolvedValue(21000)
    const store = makeStore(true, { localPort: { status: WebAppPortStatus.Assigned, port: 21000 } })
    const ref = load(store)
    expect(ref.runtime.getState()).toEqual({ phase: WebAppPhase.Forwarding, port: 3000 })
    await vi.waitFor(() => { expect(ref.runtime.getState().phase).toBe(WebAppPhase.Ready) })
    expect(ref.runtime.getState()).toEqual({ phase: WebAppPhase.Ready, url: 'http://localhost:21000/d/x?from=now' })
    expect(mockEnsureForward).toHaveBeenCalledWith(expect.objectContaining({
      remotePort: 3000, forwardId: forwardIdForTab(TAB_ID), preferredLocalPort: { status: WebAppPortStatus.Assigned, port: 21000 },
    }))
    expect(store.getState().workspace.appStates[TAB_ID]!.state).toEqual({ localPort: { status: WebAppPortStatus.Assigned, port: 21000 } })
  })

  it('remote: shows forward failures and retries', async () => {
    mockEnsureForward.mockRejectedValueOnce(new Error('ssh died')).mockResolvedValue(21000)
    const ref = load(makeStore(true))
    await vi.waitFor(() => { expect(ref.runtime.getState()).toEqual({ phase: WebAppPhase.Error, message: 'ssh died' }) })
    ref.retry()
    await vi.waitFor(() => { expect(ref.runtime.getState().phase).toBe(WebAppPhase.Ready) })
  })

  it('stringifies non-Error failures', async () => {
    mockEnsureForward.mockRejectedValueOnce('nope')
    const ref = load(makeStore(true))
    await vi.waitFor(() => { expect(ref.runtime.getState()).toEqual({ phase: WebAppPhase.Error, message: 'nope' }) })
  })

  it('remote: ignores results that land after dispose', async () => {
    let resolve: (port: number) => void = () => {}
    let reject: (err: Error) => void = () => {}
    mockEnsureForward.mockReturnValueOnce(new Promise((res) => { resolve = res }))
      .mockReturnValueOnce(new Promise((_res, rej) => { reject = rej }))
    const store = makeStore(true)
    const ref = load(store)
    ref.dispose()
    resolve(21000)
    const ref2 = load(store)
    ref2.dispose()
    reject(new Error('late'))
    await new Promise((r) => { setTimeout(r, 0) })
    expect(ref.runtime.getState().phase).toBe(WebAppPhase.Forwarding)
    expect(ref2.runtime.getState().phase).toBe(WebAppPhase.Forwarding)
    expect(store.getState().workspace.appStates[TAB_ID]!.state).toEqual({ localPort: { status: WebAppPortStatus.Unassigned } })
  })

  it('remote: does nothing when the tab is gone', () => {
    const ref = load(makeStore(true), makeDeps(), instance, 'missing')
    expect(mockEnsureForward).not.toHaveBeenCalled()
    expect(ref.runtime.getState().phase).toBe(WebAppPhase.Forwarding)
  })

  it('restartForward removes the forward and runs again', async () => {
    mockEnsureForward.mockResolvedValue(21000)
    const deps = makeDeps()
    const ref = load(makeStore(true), deps)
    await ref.restartForward()
    expect(deps.ssh.removePortForward).toHaveBeenCalledWith(forwardIdForTab(TAB_ID))
    expect(mockEnsureForward).toHaveBeenCalledTimes(2)
  })

  it('close removes the forward only on remote sessions', () => {
    mockEnsureForward.mockResolvedValue(21000)
    const remoteDeps = makeDeps()
    load(makeStore(true), remoteDeps).close()
    expect(remoteDeps.ssh.removePortForward).toHaveBeenCalledWith(forwardIdForTab(TAB_ID))
    const localDeps = makeDeps()
    load(makeStore(false), localDeps).close()
    expect(localDeps.ssh.removePortForward).not.toHaveBeenCalled()
  })

  it('renders the pane, or nothing for a foreign state', () => {
    const deps = makeDeps()
    const app = createBrowserVariant(instance, deps)
    const store = makeStore(false)
    const el = app.render({ tab: { id: TAB_ID, applicationId: 'x', title: 'x', state: { localPort: { status: WebAppPortStatus.Unassigned } } }, workspace: store, isVisible: true }) as unknown as { props: Record<string, unknown> }
    expect(el.props).toMatchObject({ tabId: TAB_ID, ssh: deps.ssh, openExternal: deps.openExternal })
    expect(app.render({ tab: { id: TAB_ID, applicationId: 'x', title: 'x', state: null }, workspace: store, isVisible: true })).toBeNull()
  })
})
