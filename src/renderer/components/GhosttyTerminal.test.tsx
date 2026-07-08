// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { StrictMode } from 'react'
import { render, act } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { SessionStoreContext } from '../contexts/SessionStoreContext'
import GhosttyTerminal, { routeLinksExternally } from './GhosttyTerminal'
import { PtyEventType } from '../../shared/ipc-types'
import type { PtyEvent } from '../types'
import type { ILink, ILinkProvider } from 'ghostty-web'

// --- Fake ghostty-web: the real module instantiates a WASM VT engine and a Canvas2D renderer,
// --- neither of which exists under jsdom.
const { terminals, fitAddons, initCalls, FakeTerminal } = vi.hoisted(() => {
  const terminals: FakeTerminal[] = []
  const fitAddons: { proposeDimensions: () => { cols: number; rows: number } | undefined }[] = []
  const initCalls = { count: 0 }

  class FakeTerminal {
    disposed = false
    opened = false
    focused = false
    cols = 80
    rows = 24
    writes: (string | Uint8Array)[] = []
    resizes: { cols: number; rows: number }[] = []
    linkProviders: unknown[] = []
    dataListener: ((data: string) => void) | null = null
    selection = ''

    constructor() { terminals.push(this) }

    open(container: HTMLElement): void {
      this.opened = true
      container.appendChild(document.createElement('canvas'))
    }
    registerLinkProvider(provider: unknown): void {
      if (!this.opened) throw new Error('Terminal must be opened before registering link providers')
      this.linkProviders.push(provider)
    }
    loadAddon(): void {}
    onData(listener: (data: string) => void): { dispose(): void } {
      this.dataListener = listener
      return { dispose: () => { this.dataListener = null } }
    }
    write(data: string | Uint8Array): void {
      if (this.disposed) throw new Error('Terminal has been disposed')
      this.writes.push(data)
    }
    resize(cols: number, rows: number): void {
      this.cols = cols
      this.rows = rows
      this.resizes.push({ cols, rows })
    }
    focus(): void { this.focused = true }
    getSelection(): string { return this.selection }
    dispose(): void { this.disposed = true }
  }

  return { terminals, fitAddons, initCalls, FakeTerminal }
})

vi.mock('ghostty-web', () => ({
  init: vi.fn(async () => { initCalls.count++; await Promise.resolve() }),
  Terminal: FakeTerminal,
  FitAddon: class {
    proposeDimensions(): { cols: number; rows: number } | undefined { return undefined }
    constructor() { fitAddons.push(this) }
  },
  UrlRegexProvider: class { provideLinks(): void {} dispose(): void {} },
  OSC8LinkProvider: class { provideLinks(): void {} dispose(): void {} },
}))

vi.mock('./ContextMenu', () => ({ default: () => null }))

// Stable state objects: these feed the effect's dep array, so a fresh identity per render would
// re-run the effect and hide the mount/unmount behaviour under test.
const { openExternal } = vi.hoisted(() => ({ openExternal: vi.fn<(uri: string) => void>() }))

vi.mock('../store/settings', () => {
  const settings = {
    terminal: { fontSize: 14, fontFamily: 'monospace', cursorBlink: true, cursorStyle: 'block', showRawChars: false },
    debug: { showBadge: false },
  }
  return { useSettingsStore: <T,>(selector: (s: { settings: unknown }) => T): T => selector({ settings }) }
})
vi.mock('../store/app', () => {
  const state = { clipboard: { writeText: vi.fn(), readText: vi.fn() }, openExternal }
  return { useAppStore: <T,>(selector: (s: typeof state) => T): T => selector(state) }
})
vi.mock('../store/contextMenu', () => {
  const state = { open: vi.fn(), close: vi.fn(), activeMenuId: null, position: { x: 0, y: 0 } }
  return { useContextMenuStore: <T,>(selector: (s: typeof state) => T): T => selector(state) }
})

beforeEach(() => {
  terminals.length = 0
  fitAddons.length = 0
  initCalls.count = 0
  openExternal.mockClear()
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  }
})

afterEach(() => { vi.useRealTimers() })

const ttyWrite = vi.fn<(d: string) => Promise<void>>()
const ttyResize = vi.fn<(cols: number, rows: number) => void>()

function makeFakeTty() {
  ttyWrite.mockReset().mockResolvedValue(undefined)
  ttyResize.mockReset()
  const state = { ptyId: 'pty1', write: ttyWrite, resize: ttyResize, kill: vi.fn() }
  return { getState: () => state }
}

interface WorkspaceOptions {
  ptyId?: string | null
  activeTabId?: string
  keepOnExit?: boolean
}

function makeWorkspaceStore(tabId: string, options: WorkspaceOptions = {}) {
  const { ptyId = 'pty1', activeTabId = tabId, keepOnExit = false } = options
  const removeTab = vi.fn()
  const store = createStore<Record<string, unknown>>()(() => ({
    workspace: {
      id: 'ws1',
      activeTabId,
      appStates: { [tabId]: { applicationId: 'ghostty-terminal', title: 'Ghostty', state: { ptyId, keepOnExit } } },
    },
    removeTab,
  }))
  return { store, removeTab }
}

interface Attached { tty: ReturnType<typeof makeFakeTty>; unsubscribe: ReturnType<typeof vi.fn> }

/** Resolves openTtyStream immediately and hands back the captured PTY event sink. */
function makeSessionStore() {
  const events: ((event: PtyEvent) => void)[] = []
  const unsubscribe = vi.fn()
  const order: string[] = []
  const openTtyStream = vi.fn(async (_ptyId: string, onEvent: (event: PtyEvent) => void): Promise<Attached> => {
    events.push(onEvent)
    await Promise.resolve()
    return { tty: makeFakeTty(), unsubscribe: vi.fn(() => { order.push('unsubscribe') }) as ReturnType<typeof vi.fn> }
  })
  const store = createStore<Record<string, unknown>>()(() => ({ openTtyStream }))
  return { store: store as never, openTtyStream, events, unsubscribe, order }
}

async function renderTerminal(tabId: string, options: WorkspaceOptions = {}) {
  const { store: workspace, removeTab } = makeWorkspaceStore(tabId, options)
  const session = makeSessionStore()

  const utils = render(
    <SessionStoreContext.Provider value={session.store}>
      <GhosttyTerminal workspace={workspace as never} tabId={tabId} />
    </SessionStoreContext.Provider>,
  )
  // Flush `await init()` and `await openTtyStream(...)`.
  await act(async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve() })

  const emit = (event: PtyEvent) => { act(() => { session.events[0]?.(event) }) }
  return { ...utils, workspace, removeTab, session, emit, terminal: terminals[0] }
}

describe('GhosttyTerminal', () => {
  it('renders a placeholder and never boots the engine while the PTY is being created', () => {
    const { store: workspace } = makeWorkspaceStore('tab1', { ptyId: null })
    const session = makeSessionStore()

    const { container } = render(
      <SessionStoreContext.Provider value={session.store}>
        <GhosttyTerminal workspace={workspace as never} tabId="tab1" />
      </SessionStoreContext.Provider>,
    )

    expect(container.textContent).toContain('Creating terminal...')
    expect(initCalls.count).toBe(0)
    expect(terminals).toHaveLength(0)
  })

  it('boots the WASM engine, attaches, and clears the loading overlay', async () => {
    const { container, terminal, session } = await renderTerminal('tab1')

    expect(initCalls.count).toBe(1)
    expect(terminal?.opened).toBe(true)
    expect(session.openTtyStream).toHaveBeenCalledWith('pty1', expect.any(Function))
    expect(container.textContent).not.toContain('Loading terminal...')
  })

  it('registers link providers only after open(), since registerLinkProvider throws otherwise', async () => {
    const { terminal } = await renderTerminal('tab1')
    // FakeTerminal throws from registerLinkProvider when not open, mirroring ghostty-web.
    expect(terminal?.linkProviders).toHaveLength(2)
  })

  it('focuses the terminal when its tab is the active one', async () => {
    const { terminal } = await renderTerminal('tab1')
    expect(terminal?.focused).toBe(true)
  })

  it('does not focus a terminal whose tab is inactive', async () => {
    const { terminal } = await renderTerminal('tab1', { activeTabId: 'other-tab' })
    expect(terminal?.focused).toBe(false)
  })

  it('writes PTY data straight through as bytes', async () => {
    const { terminal, emit } = await renderTerminal('tab1')
    const data = new TextEncoder().encode('hello')

    emit({ type: PtyEventType.Data, data })

    expect(terminal?.writes).toEqual([data])
  })

  it('resizes only from the daemon-echoed Resize event', async () => {
    const { terminal, emit } = await renderTerminal('tab1')

    emit({ type: PtyEventType.Resize, cols: 100, rows: 30 })

    expect(terminal?.resizes).toEqual([{ cols: 100, rows: 30 }])
  })

  it('closes the tab when the PTY exits and keepOnExit is false', async () => {
    const { removeTab, emit, terminal } = await renderTerminal('tab1', { keepOnExit: false })

    emit({ type: PtyEventType.Exit, exitCode: 0, signal: 0 })

    expect(removeTab).toHaveBeenCalledWith('tab1')
    expect(terminal?.writes).toHaveLength(0)
  })

  it('keeps the tab and prints the exit code when keepOnExit is true', async () => {
    const { removeTab, emit, terminal } = await renderTerminal('tab1', { keepOnExit: true })

    emit({ type: PtyEventType.Exit, exitCode: 3, signal: 0 })

    expect(removeTab).not.toHaveBeenCalled()
    expect(String(terminal?.writes[0])).toContain('exit code 3')
  })

  it('ignores a PTY exit that lands after the tab is already gone', async () => {
    const { workspace, removeTab, emit } = await renderTerminal('tab1')
    act(() => { workspace.setState({ workspace: { id: 'ws1', activeTabId: 'tab1', appStates: {} } }) })

    expect(() => { emit({ type: PtyEventType.Exit, exitCode: 0, signal: 0 }) }).not.toThrow()
    expect(removeTab).not.toHaveBeenCalled()
  })

  it('surfaces a stream error in the overlay', async () => {
    const { container, emit } = await renderTerminal('tab1')

    emit({ type: PtyEventType.Error, message: 'daemon exploded' })

    expect(container.textContent).toContain('daemon exploded')
  })

  it('surfaces stream end as a disconnect', async () => {
    const { container, emit } = await renderTerminal('tab1')

    emit({ type: PtyEventType.End })

    expect(container.textContent).toContain('Terminal disconnected')
  })

  it('forwards keystrokes to the PTY when the tab is active', async () => {
    const { terminal } = await renderTerminal('tab1')

    act(() => { terminal?.dataListener?.('ls\r') })

    expect(ttyWrite).toHaveBeenCalledWith('ls\r')
  })

  it('drops onData from an inactive tab — it can only be a replay auto-response', async () => {
    const { terminal } = await renderTerminal('tab1', { activeTabId: 'other-tab' })

    act(() => { terminal?.dataListener?.('\x1b[>0;10;1c') })

    expect(ttyWrite).not.toHaveBeenCalled()
  })

  it('proposes a fit to the daemon but never resizes locally', async () => {
    vi.useFakeTimers()
    const { store: workspace } = makeWorkspaceStore('tab1')
    const session = makeSessionStore()

    render(
      <SessionStoreContext.Provider value={session.store}>
        <GhosttyTerminal workspace={workspace as never} tabId="tab1" />
      </SessionStoreContext.Provider>,
    )
    await act(async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve() })

    // Terminal is 80x24; the container fits 100x30.
    fitAddons[0]!.proposeDimensions = () => ({ cols: 100, rows: 30 })
    act(() => { vi.advanceTimersByTime(100) })

    expect(ttyResize).toHaveBeenCalledWith(100, 30)
    // The daemon owns the size — nothing resized the terminal locally.
    expect(terminals[0]?.resizes).toHaveLength(0)
  })

  it('skips the resize round-trip when the proposal matches the current size', async () => {
    vi.useFakeTimers()
    const { store: workspace } = makeWorkspaceStore('tab1')
    const session = makeSessionStore()

    render(
      <SessionStoreContext.Provider value={session.store}>
        <GhosttyTerminal workspace={workspace as never} tabId="tab1" />
      </SessionStoreContext.Provider>,
    )
    await act(async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve() })

    fitAddons[0]!.proposeDimensions = () => ({ cols: 80, rows: 24 })
    act(() => { vi.advanceTimersByTime(100) })

    expect(ttyResize).not.toHaveBeenCalled()
  })

  it('unsubscribes before disposing, so late PTY data cannot hit a disposed terminal', async () => {
    const order: string[] = []
    const { store: workspace } = makeWorkspaceStore('tab1')
    const events: ((event: PtyEvent) => void)[] = []
    const openTtyStream = vi.fn(async (_p: string, onEvent: (event: PtyEvent) => void) => {
      events.push(onEvent)
      await Promise.resolve()
      return { tty: makeFakeTty(), unsubscribe: () => order.push('unsubscribe') }
    })
    const session = createStore<Record<string, unknown>>()(() => ({ openTtyStream }))

    const { unmount } = render(
      <SessionStoreContext.Provider value={session as never}>
        <GhosttyTerminal workspace={workspace as never} tabId="tab1" />
      </SessionStoreContext.Provider>,
    )
    await act(async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve() })

    const terminal = terminals[0]!
    const originalDispose = terminal.dispose.bind(terminal)
    terminal.dispose = () => { order.push('dispose'); originalDispose() }

    unmount()

    expect(order).toEqual(['unsubscribe', 'dispose'])
    expect(terminal.disposed).toBe(true)
  })

  it('shows the attach failure and disposes the orphaned terminal', async () => {
    const { store: workspace } = makeWorkspaceStore('tab1')
    const openTtyStream = vi.fn(() => Promise.reject(new Error('pty is gone')))
    const session = createStore<Record<string, unknown>>()(() => ({ openTtyStream }))

    const { container } = render(
      <SessionStoreContext.Provider value={session as never}>
        <GhosttyTerminal workspace={workspace as never} tabId="tab1" />
      </SessionStoreContext.Provider>,
    )
    await act(async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve() })

    expect(container.textContent).toContain('Failed to attach terminal: pty is gone')
    expect(terminals[0]?.disposed).toBe(true)
  })

  it('creates nothing on the cancelled half of a StrictMode double-mount', async () => {
    const { store: workspace } = makeWorkspaceStore('tab1')
    const openTtyStream = vi.fn(async () => {
      await Promise.resolve()
      return { tty: makeFakeTty(), unsubscribe: vi.fn() }
    })
    const session = createStore<Record<string, unknown>>()(() => ({ openTtyStream }))

    render(
      <StrictMode>
        <SessionStoreContext.Provider value={session as never}>
          <GhosttyTerminal workspace={workspace as never} tabId="tab1" />
        </SessionStoreContext.Provider>
      </StrictMode>,
    )
    await act(async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve() })

    // The cancelled mount is already unwound by the `cancelled` guard after `await init()`,
    // before any terminal is constructed or any stream opened. Only the surviving mount attaches.
    expect(terminals).toHaveLength(1)
    expect(openTtyStream).toHaveBeenCalledTimes(1)
    expect(terminals[0]?.disposed).toBe(false)
  })

  it('unwinds the terminal and the stream when unmounted mid-attach', async () => {
    const { store: workspace } = makeWorkspaceStore('tab1')
    const unsubscribe = vi.fn()
    let resolveAttach: (() => void) | null = null
    const openTtyStream = vi.fn(
      () => new Promise<Attached>((resolve) => {
        resolveAttach = () => { resolve({ tty: makeFakeTty(), unsubscribe } as unknown as Attached) }
      }),
    )
    const session = createStore<Record<string, unknown>>()(() => ({ openTtyStream }))

    const { unmount } = render(
      <SessionStoreContext.Provider value={session as never}>
        <GhosttyTerminal workspace={workspace as never} tabId="tab1" />
      </SessionStoreContext.Provider>,
    )
    // init() has resolved, so the terminal exists and the attach is in flight.
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    expect(terminals).toHaveLength(1)
    expect(unsubscribe).not.toHaveBeenCalled()

    // A fast tab switch unmounts before the daemon answers.
    unmount()
    await act(async () => { resolveAttach?.(); await Promise.resolve() })

    // The cleanup never saw this stream — the resolved start() must unsubscribe it itself,
    // otherwise the PTY subscription leaks for the life of the window.
    expect(unsubscribe).toHaveBeenCalledTimes(1)
    expect(terminals[0]?.disposed).toBe(true)
  })
})

describe('routeLinksExternally', () => {
  function fakeLink(text: string): ILink {
    return { text, range: { start: { x: 0, y: 0 }, end: { x: 0, y: 0 } }, activate: () => {} }
  }

  it('replaces activate() so links open in the OS browser, not a BrowserWindow', () => {
    const inner: ILinkProvider = {
      provideLinks: (_y, callback) => { callback([fakeLink('https://example.com')]) },
    }
    const open = vi.fn<(uri: string) => void>()

    let links: ILink[] | undefined
    routeLinksExternally(inner, open).provideLinks(0, (result) => { links = result })
    links?.[0]?.activate(new MouseEvent('click'))

    expect(open).toHaveBeenCalledWith('https://example.com')
  })

  it('passes through when the inner provider finds nothing', () => {
    const inner: ILinkProvider = { provideLinks: (_y, callback) => { callback(undefined) } }

    let called = false
    let links: ILink[] | undefined = []
    routeLinksExternally(inner, vi.fn()).provideLinks(0, (result) => { called = true; links = result })

    expect(called).toBe(true)
    expect(links).toBeUndefined()
  })

  it('delegates dispose to the inner provider when it has one', () => {
    const dispose = vi.fn()
    const inner: ILinkProvider = { provideLinks: (_y, cb) => { cb(undefined) }, dispose }

    routeLinksExternally(inner, vi.fn()).dispose?.()

    expect(dispose).toHaveBeenCalledTimes(1)
  })

  it('tolerates an inner provider with no dispose', () => {
    const inner: ILinkProvider = { provideLinks: (_y, cb) => { cb(undefined) } }
    expect(() => routeLinksExternally(inner, vi.fn()).dispose?.()).not.toThrow()
  })
})
