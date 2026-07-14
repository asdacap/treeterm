// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { StrictMode } from 'react'
import { render, act } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { SessionStoreContext } from '../contexts/SessionStoreContext'
import BaseTerminal, { type BaseTerminalConfig, type TerminalContainerElement } from './BaseTerminal'
import { ScrollPosition } from '../types'
import { PtyEventType } from '../../shared/ipc-types'
import type { PtyEvent } from '../types'
import type { TerminalDisposable, TerminalEngine } from '../terminal/engine'

// --- Fake engine: records what BaseTerminal drives it with, plus a minimal DOM presence ---
class FakeEngine implements TerminalEngine {
  disposed = false
  attachedTo: HTMLElement | null = null
  focused = false
  cols = 80
  rows = 24
  alternate = false
  scrollPosition = ScrollPosition.Bottom
  scrollRatio = 1
  scrolledToBottom = 0
  scrolledToRatio: number | null = null
  writes: (string | Uint8Array)[] = []
  resizes: { cols: number; rows: number }[] = []
  displayOptions: unknown = null
  selection = ''
  readonly element = document.createElement('div')
  // A minimal line buffer that `write` feeds, so `snapshotViewport` (activity detection) reads
  // real rendered content instead of an empty stub.
  readonly bufferLines: string[] = []
  readonly raw = {
    buffer: {
      active: {
        length: 0, // kept in sync with bufferLines by write()
        getLine: (y: number): { translateToString: () => string } | undefined =>
          y >= 0 && y < this.bufferLines.length
            ? { translateToString: (): string => this.bufferLines[y] ?? '' }
            : undefined,
      },
    },
  }

  dataListener: ((data: string) => void) | null = null
  scrollListener: (() => void) | null = null
  wheelListener: ((deltaY: number) => void) | null = null

  attach(container: HTMLElement): void {
    this.attachedTo = container
    container.appendChild(this.element)
  }
  applyDisplayOptions(options: unknown): void { this.displayOptions = options }
  write(data: string | Uint8Array, onWritten?: () => void): void {
    this.writes.push(data)
    const text = typeof data === 'string' ? data : new TextDecoder().decode(data)
    for (const line of text.split('\n')) this.bufferLines.push(line)
    this.raw.buffer.active.length = this.bufferLines.length
    onWritten?.()
  }
  resize(cols: number, rows: number): void { this.cols = cols; this.rows = rows; this.resizes.push({ cols, rows }) }
  focus(): void { this.focused = true }
  getSelection(): string { return this.selection }
  dispose(): void { this.disposed = true; this.element.remove() }
  onData(handler: (data: string) => void): TerminalDisposable {
    this.dataListener = handler
    return { dispose: () => { this.dataListener = null } }
  }
  onScroll(handler: () => void): TerminalDisposable {
    this.scrollListener = handler
    return { dispose: () => { this.scrollListener = null } }
  }
  onWheel(handler: (deltaY: number) => void): TerminalDisposable {
    this.wheelListener = handler
    return { dispose: () => { this.wheelListener = null } }
  }
  isAlternateScreen(): boolean { return this.alternate }
  getScrollPosition(): ScrollPosition { return this.scrollPosition }
  getScrollRatio(): number { return this.scrollRatio }
  scrollToRatio(ratio: number): void { this.scrolledToRatio = ratio }
  scrollToTop(): void {}
  scrollToBottom(): void { this.scrolledToBottom++ }

  /** What the container would fit. undefined means "not laid out yet", as in jsdom. */
  proposal: { cols: number; rows: number } | undefined = undefined
  proposeDimensions(): { cols: number; rows: number } | undefined { return this.proposal }
}

const engines: FakeEngine[] = []
const createEngine = vi.fn(async (): Promise<TerminalEngine> => {
  await Promise.resolve()
  const engine = new FakeEngine()
  engines.push(engine)
  return engine
})

const { processedData } = vi.hoisted(() => ({ processedData: [] as string[] }))
vi.mock('../utils/activityStateDetector', () => ({
  createActivityStateDetector: () => ({
    processData: (data: string) => processedData.push(data),
    destroy: () => {},
  }),
}))
vi.mock('./ContextMenu', () => ({ default: () => null }))

// Each mock exposes a STABLE state object so BaseTerminal's effect deps (settings,
// openExternal, setTabState) don't change identity between renders — otherwise the
// effect re-runs on every render and the StrictMode double-mount can't be observed.
vi.mock('../store/settings', () => {
  const settings = {
    terminal: { fontSize: 14, fontFamily: 'monospace', cursorBlink: true, cursorStyle: 'block', showRawChars: false, allowOsc52Clipboard: false },
    debug: { showBadge: false },
  }
  return { useSettingsStore: <T,>(selector: (s: { settings: unknown }) => T): T => selector({ settings }) }
})
vi.mock('../store/app', () => {
  const state = { clipboard: { writeText: () => {}, readText: () => {} }, openExternal: () => {} }
  return { useAppStore: <T,>(selector: (s: typeof state) => T): T => selector(state) }
})
vi.mock('../store/activityState', () => {
  const state = { setTabState: () => {} }
  return { useActivityStateStore: <T,>(selector: (s: typeof state) => T): T => selector(state) }
})
vi.mock('../store/contextMenu', () => {
  const state = { open: () => {}, close: () => {}, activeMenuId: null, position: { x: 0, y: 0 } }
  return { useContextMenuStore: <T,>(selector: (s: typeof state) => T): T => selector(state) }
})

// jsdom lacks ResizeObserver, which BaseTerminal instantiates on mount.
beforeEach(() => {
  engines.length = 0
  processedData.length = 0
  createEngine.mockClear()
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  }
})

/** Stable config ref — mirrors Terminal.tsx's useState-stabilized config. */
const config: BaseTerminalConfig = { createEngine, themeBackground: '#000', logPrefix: 'Terminal' }

interface Deferred {
  resolve: () => void
  dispose: ReturnType<typeof vi.fn>
}

/** One state object per tty, so `getState().write` is the same mock every call.
 *  The Tty owns its subscription, so `dispose()` is what releases it. */
function makeFakeTty() {
  const state = {
    ptyId: 'pty1',
    write: vi.fn<(d: string) => Promise<void>>().mockResolvedValue(undefined),
    resize: vi.fn<(cols: number, rows: number) => void>(),
    kill: vi.fn(),
  }
  return { getState: () => state, state, dispose: vi.fn() }
}

function makeWorkspaceStore(tabId: string, options: { keepOnExit?: boolean; activeTabId?: string } = {}) {
  const { keepOnExit = false, activeTabId = tabId } = options
  const appRef = {
    cachedTerminal: null as unknown,
    disposeCachedTerminal: vi.fn(),
    close: vi.fn(),
    dispose: vi.fn(),
  }
  const removeTab = vi.fn()
  const store = createStore<Record<string, unknown>>()(() => ({
    workspace: {
      id: 'ws1',
      activeTabId,
      appStates: { [tabId]: { applicationId: 'terminal', title: 'Terminal 1', state: { ptyId: 'pty1', keepOnExit } } },
    },
    removeTab,
    getTabRef: () => appRef,
  }))
  return { store, appRef, removeTab }
}

function makeSessionStore(deferreds: Deferred[]) {
  const openTtyStream = vi.fn(
    () =>
      new Promise((resolve) => {
        const tty = makeFakeTty()
        deferreds.push({
          resolve: () => { resolve(tty) },
          dispose: tty.dispose,
        })
      }),
  )
  const store = createStore<Record<string, unknown>>()(() => ({ openTtyStream }))
  return { store: store as never, openTtyStream }
}

/** Resolves openTtyStream immediately and hands back the captured PTY event sink. */
function makeLiveSessionStore() {
  const events: ((event: PtyEvent) => void)[] = []
  const tty = makeFakeTty()
  const openTtyStream = vi.fn(async (_ptyId: string, onEvent: (event: PtyEvent) => void) => {
    events.push(onEvent)
    await Promise.resolve()
    return tty
  })
  const store = createStore<Record<string, unknown>>()(() => ({ openTtyStream }))
  return { store: store as never, openTtyStream, events, dispose: tty.dispose, tty: tty.state }
}

/** Flushes `await createEngine()` and `await openTtyStream()`. */
async function flush() {
  await act(async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve() })
}

describe('BaseTerminal — StrictMode double-mount cleanup', () => {
  it('disposes the engine from the cancelled first mount before it opens a stream', async () => {
    const tabId = 'tab1'
    const deferreds: Deferred[] = []
    const { store: workspace } = makeWorkspaceStore(tabId)
    const { store: session, openTtyStream } = makeSessionStore(deferreds)

    render(
      <StrictMode>
        <SessionStoreContext.Provider value={session}>
          <BaseTerminal workspace={workspace as never} tabId={tabId} config={config} />
        </SessionStoreContext.Provider>
      </StrictMode>,
    )
    await flush()

    // StrictMode double-invoked the effect: mount → cleanup (cancelled) → mount. Both mounts
    // asked for an engine, but the cancelled one is torn down at the first await — so it never
    // attaches, and never opens a PTY stream that would immediately need unsubscribing.
    expect(engines.length).toBe(2)
    const [first, second] = engines
    expect(first?.disposed).toBe(true)
    expect(second?.disposed).toBe(false)
    expect(second?.attachedTo).not.toBeNull()
    expect(openTtyStream).toHaveBeenCalledTimes(1)
    expect(deferreds.length).toBe(1)
  })

  it('unwinds the engine and the stream when unmounted mid-attach', async () => {
    const deferreds: Deferred[] = []
    const { store: workspace } = makeWorkspaceStore('tab1')
    const { store: session } = makeSessionStore(deferreds)

    const { unmount } = render(
      <SessionStoreContext.Provider value={session}>
        <BaseTerminal workspace={workspace as never} tabId="tab1" config={config} />
      </SessionStoreContext.Provider>,
    )
    await flush()
    expect(engines).toHaveLength(1)

    // A fast tab switch unmounts before the daemon answers.
    unmount()
    await act(async () => { deferreds[0]?.resolve(); await Promise.resolve() })

    // The cleanup never saw this stream. `owner` was already disposed, so registering the
    // late-landing Tty disposes it on the spot instead of leaking it for the window's life.
    expect(deferreds[0]?.dispose).toHaveBeenCalledTimes(1)
    expect(engines[0]?.disposed).toBe(true)
  })

  it('disposes the engine and explains itself when the attach fails', async () => {
    const { store: workspace } = makeWorkspaceStore('tab1')
    const openTtyStream = vi.fn(() => Promise.reject(new Error('pty is gone')))
    const session = createStore<Record<string, unknown>>()(() => ({ openTtyStream }))

    const { container } = render(
      <SessionStoreContext.Provider value={session as never}>
        <BaseTerminal workspace={workspace as never} tabId="tab1" config={config} />
      </SessionStoreContext.Provider>,
    )
    await flush()

    expect(container.textContent).toContain('Failed to reattach terminal: pty is gone')
    expect(engines[0]?.disposed).toBe(true)
  })
})

describe('BaseTerminal — terminal cache across unmount', () => {
  it('reuses the cached engine on remount rather than rebuilding it', async () => {
    const { store: workspace, appRef } = makeWorkspaceStore('tab1')
    const session = makeLiveSessionStore()

    const first = render(
      <SessionStoreContext.Provider value={session.store}>
        <BaseTerminal workspace={workspace as never} tabId="tab1" config={config} />
      </SessionStoreContext.Provider>,
    )
    await flush()
    expect(appRef.cachedTerminal).not.toBeNull()

    first.unmount()
    // The engine and its PTY subscription outlive the component — that is the whole point.
    expect(engines[0]?.disposed).toBe(false)
    expect(session.dispose).not.toHaveBeenCalled()

    const second = render(
      <SessionStoreContext.Provider value={session.store}>
        <BaseTerminal workspace={workspace as never} tabId="tab1" config={config} />
      </SessionStoreContext.Provider>,
    )
    await flush()

    expect(engines).toHaveLength(1)
    expect(session.openTtyStream).toHaveBeenCalledTimes(1)
    // Reparented into the new container, with the current settings re-applied.
    expect(engines[0]?.attachedTo).toBe(second.container.querySelector('.terminal-container'))
    expect(engines[0]?.displayOptions).toMatchObject({ fontSize: 14, themeBackground: '#000' })
  })

  it('keeps the buffer fed while unmounted, so scrollback survives a tab switch', async () => {
    const { store: workspace } = makeWorkspaceStore('tab1')
    const session = makeLiveSessionStore()

    const { unmount } = render(
      <SessionStoreContext.Provider value={session.store}>
        <BaseTerminal workspace={workspace as never} tabId="tab1" config={config} />
      </SessionStoreContext.Provider>,
    )
    await flush()
    unmount()

    act(() => { session.events[0]?.({ type: PtyEventType.Data, data: new TextEncoder().encode('while away') }) })

    expect(engines[0]?.writes).toHaveLength(1)
  })

  it('publishes the engine buffer on the container for e2e to read', async () => {
    const { store: workspace } = makeWorkspaceStore('tab1')
    const session = makeLiveSessionStore()

    const { container } = render(
      <SessionStoreContext.Provider value={session.store}>
        <BaseTerminal workspace={workspace as never} tabId="tab1" config={config} />
      </SessionStoreContext.Provider>,
    )
    await flush()

    const host = container.querySelector('.terminal-container') as TerminalContainerElement
    expect(host.terminal).toBe(engines[0]?.raw)
  })
})

describe('BaseTerminal — mounted UI', () => {
  async function mount(options: { keepOnExit?: boolean; activeTabId?: string } = {}) {
    const { store: workspace, removeTab } = makeWorkspaceStore('tab1', options)
    const session = makeLiveSessionStore()
    const utils = render(
      <SessionStoreContext.Provider value={session.store}>
        <BaseTerminal workspace={workspace as never} tabId="tab1" config={config} />
      </SessionStoreContext.Provider>,
    )
    await flush()
    const emit = (event: PtyEvent) => { act(() => { session.events[0]?.(event) }) }
    return { ...utils, workspace, removeTab, emit, tty: session.tty, engine: engines[0]! }
  }

  it('raises the alt-screen badge when the engine switches screens', async () => {
    const { container, engine, emit } = await mount()
    expect(container.textContent).not.toContain('ALT SCREEN')

    engine.alternate = true
    emit({ type: PtyEventType.Data, data: new TextEncoder().encode('\x1b[?1049h') })

    expect(container.textContent).toContain('ALT SCREEN')
  })

  it('tracks the scroll position the engine reports', async () => {
    const { container, engine } = await mount()

    engine.scrollPosition = ScrollPosition.Middle
    act(() => { engine.scrollListener?.() })

    expect(container.textContent).toContain('MIDDLE')
  })

  it('unpins when the user wheels back toward older output', async () => {
    const { engine, workspace } = await mount()
    const cache = (workspace.getState().getTabRef as () => { cachedTerminal: { pinnedToBottom: boolean } })().cachedTerminal
    cache.pinnedToBottom = true

    act(() => { engine.wheelListener?.(-120) })

    expect(cache.pinnedToBottom).toBe(false)
  })

  it('stays pinned when the user wheels toward newer output', async () => {
    const { engine, workspace } = await mount()
    const cache = (workspace.getState().getTabRef as () => { cachedTerminal: { pinnedToBottom: boolean } })().cachedTerminal
    cache.pinnedToBottom = true

    act(() => { engine.wheelListener?.(120) })

    expect(cache.pinnedToBottom).toBe(true)
  })

  it('holds a pinned terminal at the bottom when it scrolls away', async () => {
    const { engine, workspace } = await mount()
    const cache = (workspace.getState().getTabRef as () => { cachedTerminal: { pinnedToBottom: boolean } })().cachedTerminal
    cache.pinnedToBottom = true
    const before = engine.scrolledToBottom

    engine.scrollPosition = ScrollPosition.Middle
    act(() => { engine.scrollListener?.() })

    expect(engine.scrolledToBottom).toBeGreaterThan(before)
  })

  it('follows new output while the reader sits at the bottom', async () => {
    const { engine, emit } = await mount()
    // Default scrollPosition is Bottom — the reader is watching the tail.
    const before = engine.scrolledToBottom

    emit({ type: PtyEventType.Data, data: new TextEncoder().encode('more output') })

    expect(engine.scrolledToBottom).toBeGreaterThan(before)
  })

  it('does not yank a scrolled-up reader down when data arrives before the scroll event fires', async () => {
    const { engine, emit } = await mount()
    // The reader wheeled up: xterm moved the viewport synchronously, so getScrollPosition()
    // already reports Middle — but the DOM 'scroll' event that would update any cached copy
    // has not fired yet (scrollListener deliberately left uncalled). A continuously-repainting
    // TUI streams a data frame in this window; it must honour the live position, not a stale one.
    engine.scrollPosition = ScrollPosition.Middle
    const before = engine.scrolledToBottom

    emit({ type: PtyEventType.Data, data: new TextEncoder().encode('repaint frame') })

    expect(engine.scrolledToBottom).toBe(before)
  })

  it('feeds the activity state detector the rendered viewport snapshot', async () => {
    const { emit } = await mount()

    emit({ type: PtyEventType.Data, data: new TextEncoder().encode('hello') })

    // The detector receives the post-write screen snapshot, not the raw bytes.
    expect(processedData.at(-1)).toContain('hello')
  })

  it('restores the scroll ratio across a resize when the reader is not at the bottom', async () => {
    const { engine, emit } = await mount()
    engine.scrollPosition = ScrollPosition.Middle
    act(() => { engine.scrollListener?.() })
    engine.scrollRatio = 0.4

    emit({ type: PtyEventType.Resize, cols: 100, rows: 30 })

    expect(engine.resizes).toEqual([{ cols: 100, rows: 30 }])
    expect(engine.scrolledToRatio).toBe(0.4)
  })

  it('proposes a fit to the daemon but never resizes the terminal locally', async () => {
    const { engine, tty } = await mount()
    engine.proposal = { cols: 100, rows: 30 }

    await act(async () => { await new Promise((r) => setTimeout(r, 150)) })

    expect(tty.resize).toHaveBeenCalledWith(100, 30)
    // The daemon owns the size — only its echoed Resize event moves the terminal.
    expect(engine.resizes).toHaveLength(0)
  })

  it('badges the size the daemon actually applied when it differs from the request', async () => {
    const { container, engine, emit } = await mount()
    engine.proposal = { cols: 100, rows: 30 }
    await act(async () => { await new Promise((r) => setTimeout(r, 150)) })

    // The daemon clamped the 100x30 we asked for down to what the PTY would take.
    emit({ type: PtyEventType.Resize, cols: 80, rows: 24 })
    expect(container.querySelector('.size-mismatch-badge')?.textContent).toBe('80x24')

    // ...and it agrees on the next round trip.
    emit({ type: PtyEventType.Resize, cols: 100, rows: 30 })
    expect(container.querySelector('.size-mismatch-badge')).toBeNull()
  })

  it('reports an immediate non-zero exit rather than silently closing the tab', async () => {
    const { container, removeTab, emit } = await mount()

    emit({ type: PtyEventType.Exit, exitCode: 127, signal: 0 })

    expect(container.textContent).toContain('Process exited immediately with code 127')
    expect(removeTab).not.toHaveBeenCalled()
  })

  it('prints the exit code and keeps the tab when keepOnExit is set', async () => {
    const { removeTab, emit, engine } = await mount({ keepOnExit: true })
    // Push past the immediate-failure window so keepOnExit is what decides.
    vi.spyOn(Date, 'now').mockReturnValue(Date.now() + 5000)

    emit({ type: PtyEventType.Exit, exitCode: 3, signal: 0 })

    expect(removeTab).not.toHaveBeenCalled()
    expect(String(engine.writes.at(-1))).toContain('exit code 3')
    vi.restoreAllMocks()
  })

  it('forwards keystrokes to the PTY when its tab is active', async () => {
    const { engine, tty } = await mount()

    act(() => { engine.dataListener?.('ls\r') })

    expect(tty.write).toHaveBeenCalledWith('ls\r')
  })

  it('drops onData from an inactive tab — it can only be a replay auto-response', async () => {
    const { engine, tty } = await mount({ activeTabId: 'other-tab' })

    act(() => { engine.dataListener?.('\x1b[>0;10;1c') })

    expect(tty.write).not.toHaveBeenCalled()
  })

  it('surfaces a stream error, then clears it on the next byte of output', async () => {
    const { container, emit } = await mount()

    emit({ type: PtyEventType.Error, message: 'daemon exploded' })
    expect(container.textContent).toContain('daemon exploded')

    emit({ type: PtyEventType.Data, data: new TextEncoder().encode('recovered') })
    expect(container.textContent).not.toContain('daemon exploded')
  })

  it('surfaces stream end as a disconnect', async () => {
    const { container, emit } = await mount()

    emit({ type: PtyEventType.End })

    expect(container.textContent).toContain('Terminal disconnected')
  })

  it('focuses the engine when its tab is the active one', async () => {
    const { engine } = await mount()
    expect(engine.focused).toBe(true)
  })
})
