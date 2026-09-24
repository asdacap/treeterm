// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { StrictMode } from 'react'
import { render, act, cleanup } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { SessionStoreContext } from '../contexts/SessionStoreContext'
import BaseTerminal, { type BaseTerminalConfig, type TerminalContainerElement } from './BaseTerminal'
import { ActivityState, ScrollPosition } from '../types'
import { ActivityTransitionKind } from '../store/activityState'
import { useSettingsStore } from '../store/settings'
import { PtyEventType } from '../../shared/ipc-types'
import type { CachedTerminal, PtyEvent } from '../types'
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

  cell: { width: number; height: number } | undefined = undefined
  cellSize(): { width: number; height: number } | undefined { return this.cell }
}

const engines: FakeEngine[] = []
const createEngine = vi.fn(async (): Promise<TerminalEngine> => {
  await Promise.resolve()
  const engine = new FakeEngine()
  engines.push(engine)
  return engine
})

const { processedData, setTabState } = vi.hoisted(() => ({
  processedData: [] as string[],
  setTabState: vi.fn(),
}))
vi.mock('../utils/idleDetector', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../utils/idleDetector')>()
  return {
    ...actual,
    createIdleDetector: (...args: Parameters<typeof actual.createIdleDetector>) => {
      const detector = actual.createIdleDetector(...args)
      return {
        ...detector,
        processSnapshot: (data: string): void => {
          processedData.push(data)
          detector.processSnapshot(data)
        },
      }
    },
  }
})
vi.mock('./ContextMenu', () => ({ default: () => null }))

// Each mock exposes a STABLE state object so BaseTerminal's effect deps (settings,
// openExternal, setTabState) don't change identity between renders — otherwise the
// effect re-runs on every render and the StrictMode double-mount can't be observed.
vi.mock('../store/settings', () => {
  const settings = {
    terminal: { fontSize: 14, fontFamily: 'monospace', cursorBlink: true, cursorStyle: 'block', showRawChars: false, allowOsc52Clipboard: false, maxCols: 80 },
    debug: { showBadge: false },
    terminalAnalyzer: { idleDebounceMs: 1000, idleDebounceUnreadMs: 15000 },
  }
  const useSettingsStore = <T,>(selector: (s: { settings: unknown }) => T): T => selector({ settings })
  useSettingsStore.getState = (): { settings: unknown } => ({ settings })
  return { useSettingsStore }
})
vi.mock('../store/app', () => {
  const state = { clipboard: { writeText: () => {}, readText: () => {} }, openExternal: () => {} }
  return { useAppStore: <T,>(selector: (s: typeof state) => T): T => selector(state) }
})
vi.mock('../store/activityState', async (importOriginal) => {
  const { ActivityTransitionKind } = await importOriginal<typeof import('../store/activityState')>()
  const state = { setTabState }
  return { ActivityTransitionKind, useActivityStateStore: <T,>(selector: (s: typeof state) => T): T => selector(state) }
})
vi.mock('../store/contextMenu', () => {
  const state = { open: () => {}, close: () => {}, activeMenuId: null, position: { x: 0, y: 0 } }
  return { useContextMenuStore: <T,>(selector: (s: typeof state) => T): T => selector(state) }
})

// jsdom lacks ResizeObserver, which BaseTerminal instantiates on mount.
beforeEach(() => {
  engines.length = 0
  processedData.length = 0
  setTabState.mockClear()
  createEngine.mockClear()
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  }
})

const cachedRefs: { cachedTerminal: CachedTerminal | null }[] = []
afterEach(() => {
  cleanup()
  for (const ref of cachedRefs) ref.cachedTerminal?.owner.dispose()
  cachedRefs.length = 0
  vi.useRealTimers()
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

function makeWorkspaceStore(tabId: string, options: { keepOnExit?: boolean; idleDetectorDisabled?: boolean; widthLimitDisabled?: boolean; activeTabId?: string; metadata?: Record<string, string> } = {}) {
  const { keepOnExit = false, idleDetectorDisabled = false, widthLimitDisabled = false, activeTabId = tabId, metadata = {} } = options
  const appRef = {
    cachedTerminal: null as CachedTerminal | null,
    disposeCachedTerminal: vi.fn(),
    close: vi.fn(),
    dispose: vi.fn(),
  }
  cachedRefs.push(appRef)
  const removeTab = vi.fn()
  const store = createStore<Record<string, unknown>>()(() => ({
    workspace: {
      id: 'ws1',
      activeTabId,
      appStates: { [tabId]: { applicationId: 'terminal', title: 'Terminal 1', state: { ptyId: 'pty1', keepOnExit, idleDetectorDisabled, widthLimitDisabled } } },
    },
    metadata,
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

describe('BaseTerminal — cached activity detector lifecycle', () => {
  async function mount(disableActivityDetector = false, metadata: Record<string, string> = {}, idleDetectorDisabled = false) {
    vi.useFakeTimers()
    const { store: workspace, appRef } = makeWorkspaceStore('tab1', { metadata, idleDetectorDisabled })
    const session = makeLiveSessionStore()
    const stableConfig = { ...config, disableActivityDetector }
    const view = (): React.ReactNode => (
      <SessionStoreContext.Provider value={session.store}>
        <BaseTerminal workspace={workspace as never} tabId="tab1" config={stableConfig} />
      </SessionStoreContext.Provider>
    )
    const result = render(view())
    await flush()
    const emit = (text: string): void => {
      act(() => { session.events[0]?.({ type: PtyEventType.Data, data: new TextEncoder().encode(text) }) })
    }
    const setIdleDetectorDisabled = (value: boolean): void => {
      workspace.setState((state) => {
        const ws = state.workspace as { appStates: Record<string, { state: Record<string, unknown> }> }
        const tab = ws.appStates.tab1!
        return { workspace: { ...ws, appStates: { tab1: { ...tab, state: { ...tab.state, idleDetectorDisabled: value } } } } }
      })
    }
    // The first burst after attach is the scrollback replay: silent, then Idle. Run it
    // through so the detector knows a resting state and later bursts publish Working.
    const settle = (): void => {
      emit('replay')
      expect(setTabState).not.toHaveBeenCalled()
      act(() => { vi.advanceTimersByTime(2000) })
      expect(setTabState).toHaveBeenCalledExactlyOnceWith('tab1', ActivityState.Idle, expect.objectContaining({ kind: ActivityTransitionKind.ViewportIdle }))
      setTabState.mockClear()
      processedData.length = 0
    }
    return { ...result, appRef, session, view, emit, engine: engines[0]!, setIdleDetectorDisabled, settle }
  }

  it('does not report the attach replay as Working, only the Idle that ends it', async () => {
    const { emit } = await mount()
    emit('replayed scrollback')
    emit('more scrollback')
    expect(setTabState).not.toHaveBeenCalled()
    act(() => { vi.advanceTimersByTime(2000) })
    expect(setTabState).toHaveBeenCalledExactlyOnceWith('tab1', ActivityState.Idle, {
      kind: ActivityTransitionKind.ViewportIdle, snapshot: processedData.at(-1), idleTimeoutMs: 1000,
    })
    emit('real work')
    act(() => { vi.advanceTimersByTime(1000) })
    expect(setTabState).toHaveBeenCalledWith('tab1', ActivityState.Working, expect.objectContaining({ kind: ActivityTransitionKind.ViewportChanged }))
  })

  it('records both edges with the screen that confirmed them and the armed timeout', async () => {
    const { emit, settle } = await mount()
    settle()
    emit('working')
    emit(' more')
    expect(setTabState).not.toHaveBeenCalled()
    act(() => { vi.advanceTimersByTime(1000) })
    expect(setTabState).toHaveBeenNthCalledWith(1, 'tab1', ActivityState.Working, {
      kind: ActivityTransitionKind.ViewportChanged, snapshot: processedData.at(-1),
    })
    expect(setTabState).toHaveBeenLastCalledWith('tab1', ActivityState.Idle, {
      kind: ActivityTransitionKind.ViewportIdle, snapshot: processedData.at(-1), idleTimeoutMs: 1000,
    })
    expect(processedData.at(-1)).toContain('more')
  })

  it('reports Working when the attach replay keeps changing past the idle timeout', async () => {
    const { emit } = await mount()
    emit('build output')
    act(() => { vi.advanceTimersByTime(999) })
    emit('still building')
    expect(setTabState).not.toHaveBeenCalled()
    act(() => { vi.advanceTimersByTime(1) })
    emit('and building')
    expect(setTabState).toHaveBeenCalledExactlyOnceWith('tab1', ActivityState.Working, {
      kind: ActivityTransitionKind.ViewportChanged, snapshot: processedData.at(-1),
    })
    act(() => { vi.advanceTimersByTime(1000) })
    expect(setTabState).toHaveBeenLastCalledWith('tab1', ActivityState.Idle, expect.objectContaining({ kind: ActivityTransitionKind.ViewportIdle }))
  })

  it('finishes pending Idle after unmount and stays idle on remount without new data', async () => {
    const { emit, unmount, view, settle } = await mount()
    settle()
    emit('working')
    // Unmount just before the idle timeout elapses.
    act(() => { vi.advanceTimersByTime(999) })
    unmount()
    act(() => { vi.advanceTimersByTime(1) })
    expect(setTabState).toHaveBeenNthCalledWith(1, 'tab1', ActivityState.Working, expect.objectContaining({ kind: ActivityTransitionKind.ViewportChanged }))
    expect(setTabState).toHaveBeenLastCalledWith('tab1', ActivityState.Idle, expect.objectContaining({ kind: ActivityTransitionKind.ViewportIdle }))

    render(view())
    await flush()
    act(() => { vi.advanceTimersByTime(2000) })
    expect(setTabState).toHaveBeenCalledTimes(2)
  })

  it('tracks changed background output and ignores unchanged repaints across remount', async () => {
    const { emit, unmount, view, engine, settle } = await mount()
    settle()
    emit('initial')
    unmount()
    act(() => { vi.advanceTimersByTime(1100) })
    emit('background change')
    const changed = processedData.at(-1)
    expect(changed).toContain('background change')

    // Simulate a TUI repaint that leaves the parsed viewport unchanged.
    vi.spyOn(engine, 'write').mockImplementation((_data, afterWrite) => { afterWrite?.() })
    act(() => { vi.advanceTimersByTime(500) })
    emit('same screen')
    render(view())
    await flush()
    emit('same screen')
    act(() => { vi.advanceTimersByTime(600) })
    expect(setTabState).toHaveBeenNthCalledWith(3, 'tab1', ActivityState.Working, { kind: ActivityTransitionKind.ViewportChanged, snapshot: changed })
    expect(setTabState).toHaveBeenLastCalledWith('tab1', ActivityState.Idle, expect.objectContaining({ kind: ActivityTransitionKind.ViewportIdle }))
    expect(setTabState).toHaveBeenCalledTimes(4)
  })

  it('cancels pending Idle and ignores late write callbacks on actual cache disposal', async () => {
    const { emit, unmount, appRef, engine, session, settle } = await mount()
    settle()
    emit('working')
    unmount()
    let finishWrite = (): void => { throw new Error('No pending write') }
    vi.spyOn(engine, 'write').mockImplementation((_data, afterWrite) => {
      if (afterWrite) finishWrite = afterWrite
    })
    emit('pending background write')
    appRef.cachedTerminal!.owner.dispose()
    expect(engine.disposed).toBe(true)
    expect(session.dispose).toHaveBeenCalledTimes(1)
    finishWrite()
    act(() => { vi.advanceTimersByTime(2000) })
    expect(setTabState).not.toHaveBeenCalled()
    expect(processedData).toHaveLength(1)
  })

  it('waits for the unread idle debounce while the workspace carries an unread marker', async () => {
    const { emit } = await mount(false, { workspaceAttention: JSON.stringify({ revision: 'r1', acknowledgedRevision: '' }) })
    emit('replay')
    act(() => { vi.advanceTimersByTime(14999) })
    expect(setTabState).not.toHaveBeenCalled()
    act(() => { vi.advanceTimersByTime(1) })
    expect(setTabState).toHaveBeenCalledExactlyOnceWith('tab1', ActivityState.Idle, expect.objectContaining({ kind: ActivityTransitionKind.ViewportIdle, idleTimeoutMs: 15000 }))
  })

  it('skips the Working edge while the tab has the idle detector disabled', async () => {
    const { emit, settle } = await mount(false, {}, true)
    settle()
    emit('spam')
    expect(setTabState).not.toHaveBeenCalled()
    act(() => { vi.advanceTimersByTime(2000) })
    expect(setTabState).toHaveBeenCalledTimes(1)
    expect(setTabState).toHaveBeenLastCalledWith('tab1', ActivityState.Idle, expect.objectContaining({ kind: ActivityTransitionKind.ViewportIdle }))
  })

  it('still settles to Idle when the detector is disabled mid-burst, and resumes when re-enabled', async () => {
    const { emit, setIdleDetectorDisabled, settle } = await mount()
    settle()
    // Keep changing past one timeout so the burst is confirmed as Working mid-burst.
    emit('working')
    act(() => { vi.advanceTimersByTime(600) })
    emit(' still')
    act(() => { vi.advanceTimersByTime(600) })
    emit(' working')
    expect(setTabState).toHaveBeenLastCalledWith('tab1', ActivityState.Working, expect.objectContaining({ kind: ActivityTransitionKind.ViewportChanged }))
    setIdleDetectorDisabled(true)
    act(() => { vi.advanceTimersByTime(2000) })
    expect(setTabState).toHaveBeenLastCalledWith('tab1', ActivityState.Idle, expect.objectContaining({ kind: ActivityTransitionKind.ViewportIdle }))
    emit('more spam')
    act(() => { vi.advanceTimersByTime(2000) })
    expect(setTabState).not.toHaveBeenCalledWith('tab1', ActivityState.Working, expect.objectContaining({ snapshot: processedData.at(-1) }))

    setIdleDetectorDisabled(false)
    emit('real work')
    act(() => { vi.advanceTimersByTime(1000) })
    expect(setTabState).toHaveBeenCalledWith('tab1', ActivityState.Working, { kind: ActivityTransitionKind.ViewportChanged, snapshot: processedData.at(-1) })
  })

  it('does not detect activity for terminals configured to use an external analyzer', async () => {
    const { emit, unmount } = await mount(true)
    emit('mounted')
    unmount()
    emit('background')
    act(() => { vi.advanceTimersByTime(2000) })
    expect(processedData).toHaveLength(0)
    expect(setTabState).not.toHaveBeenCalled()
  })
})

describe('BaseTerminal — mounted UI', () => {
  async function mount(options: { keepOnExit?: boolean; widthLimitDisabled?: boolean; activeTabId?: string } = {}) {
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
    engine.proposal = { cols: 60, rows: 30 }

    await act(async () => { await new Promise((r) => setTimeout(r, 150)) })

    expect(tty.resize).toHaveBeenCalledWith(60, 30)
    // The daemon owns the size — only its echoed Resize event moves the terminal.
    expect(engine.resizes).toHaveLength(0)
  })

  it('caps the proposed width at the terminal.maxCols setting by default', async () => {
    const { engine, tty } = await mount()
    engine.proposal = { cols: 200, rows: 30 }

    await act(async () => { await new Promise((r) => setTimeout(r, 150)) })

    expect(tty.resize).toHaveBeenCalledWith(80, 30)
  })

  it('caps at the configured column count', async () => {
    const settings = useSettingsStore.getState().settings as { terminal: { maxCols: number } }
    const previous = settings.terminal.maxCols
    settings.terminal.maxCols = 160
    try {
      const { engine, tty } = await mount()
      engine.proposal = { cols: 200, rows: 30 }

      await act(async () => { await new Promise((r) => setTimeout(r, 150)) })

      expect(tty.resize).toHaveBeenCalledWith(160, 30)
    } finally {
      settings.terminal.maxCols = previous
    }
  })

  it('skips the fit when the capped size already matches the engine', async () => {
    const { engine, tty } = await mount()
    // Engine is 80x24; a wider pane still caps to 80, so nothing changes.
    engine.proposal = { cols: 200, rows: 24 }

    await act(async () => { await new Promise((r) => setTimeout(r, 150)) })

    expect(tty.resize).not.toHaveBeenCalled()
  })

  it('fits to the full width when the tab has the limit switched off', async () => {
    const { engine, tty } = await mount({ widthLimitDisabled: true })
    engine.proposal = { cols: 200, rows: 30 }

    await act(async () => { await new Promise((r) => setTimeout(r, 150)) })

    expect(tty.resize).toHaveBeenCalledWith(200, 30)
  })

  it('re-fits when the width limit switch flips, and only then', async () => {
    const { engine, tty, workspace } = await mount()
    engine.proposal = { cols: 200, rows: 30 }
    await act(async () => { await new Promise((r) => setTimeout(r, 150)) })
    expect(tty.resize).toHaveBeenLastCalledWith(80, 30)

    const setTabState = (patch: Record<string, unknown>): void => {
      act(() => {
        workspace.setState((state) => {
          const ws = state.workspace as { appStates: Record<string, { state: Record<string, unknown> }> }
          const tab = ws.appStates.tab1!
          return { workspace: { ...ws, appStates: { tab1: { ...tab, state: { ...tab.state, ...patch } } } } }
        })
      })
    }

    // An unrelated tab-state change leaves the size alone.
    setTabState({ keepOnExit: true })
    expect(tty.resize).toHaveBeenCalledTimes(1)

    setTabState({ widthLimitDisabled: true })
    expect(tty.resize).toHaveBeenLastCalledWith(200, 30)

    setTabState({ widthLimitDisabled: false })
    expect(tty.resize).toHaveBeenLastCalledWith(80, 30)
    expect(tty.resize).toHaveBeenCalledTimes(3)
  })

  it('marks the width the column limit leaves unused, and only when a whole column is left', async () => {
    const { container, engine, emit, workspace } = await mount()
    const terminalContainer = container.querySelector('.terminal-container') as HTMLElement
    // jsdom has no layout: 1000px of container, 10px cells, inside a wrapper with 8px padding.
    Object.defineProperty(terminalContainer, 'clientWidth', { value: 1000, configurable: true })
    Object.defineProperty(terminalContainer, 'offsetLeft', { value: 8, configurable: true })
    engine.cell = { width: 10, height: 20 }

    // 80 columns use 800px of the 1000px, so the strip starts at the grid's right edge.
    emit({ type: PtyEventType.Resize, cols: 80, rows: 24 })
    const strip = (): HTMLElement | null => container.querySelector('.terminal-unused-width')
    expect(strip()?.style.left).toBe('808px')

    // A sub-cell leftover from rounding is not worth marking.
    emit({ type: PtyEventType.Resize, cols: 100, rows: 24 })
    expect(strip()).toBeNull()

    // Nor is anything while the tab has the limit switched off.
    emit({ type: PtyEventType.Resize, cols: 80, rows: 24 })
    expect(strip()).not.toBeNull()
    act(() => {
      workspace.setState((state) => {
        const ws = state.workspace as { appStates: Record<string, { state: Record<string, unknown> }> }
        const tab = ws.appStates.tab1!
        return { workspace: { ...ws, appStates: { tab1: { ...tab, state: { ...tab.state, widthLimitDisabled: true } } } } }
      })
    })
    expect(strip()).toBeNull()
  })

  it('badges the size the daemon actually applied when it differs from the request', async () => {
    const { container, engine, emit } = await mount()
    engine.proposal = { cols: 60, rows: 30 }
    await act(async () => { await new Promise((r) => setTimeout(r, 150)) })

    // The daemon clamped the 60x30 we asked for down to what the PTY would take.
    emit({ type: PtyEventType.Resize, cols: 80, rows: 24 })
    expect(container.querySelector('.size-mismatch-badge')?.textContent).toBe('80x24')

    // ...and it agrees on the next round trip.
    emit({ type: PtyEventType.Resize, cols: 60, rows: 30 })
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
