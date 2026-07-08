// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { StrictMode } from 'react'
import { render, act } from '@testing-library/react'
import { createStore } from 'zustand/vanilla'
import { SessionStoreContext } from '../contexts/SessionStoreContext'
import BaseTerminal from './BaseTerminal'

// --- Fake xterm terminal: records instances + dispose calls, minimal DOM surface ---
const { createdTerminals, FakeTerminal } = vi.hoisted(() => {
  const createdTerminals: { disposed: boolean; element: HTMLDivElement }[] = []
  class FakeTerminal {
    disposed = false
    element = document.createElement('div')
    cols = 80
    rows = 24
    options: Record<string, unknown> = {}
    buffer = {
      active: { type: 'normal', baseY: 0, viewportY: 0 },
      onBufferChange: () => ({ dispose() {} }),
    }

    constructor() {
      createdTerminals.push(this)
      const viewport = document.createElement('div')
      viewport.className = 'xterm-viewport'
      this.element.appendChild(viewport)
    }

    open(container: HTMLElement): void { container.appendChild(this.element) }
    loadAddon(): void {}
    onData(): { dispose(): void } { return { dispose() {} } }
    write(_data: unknown, cb?: () => void): void { if (cb) cb() }
    resize(): void {}
    focus(): void {}
    scrollToBottom(): void {}
    scrollToTop(): void {}
    scrollToLine(): void {}
    refresh(): void {}
    getSelection(): string { return '' }
    dispose(): void { this.disposed = true; this.element.remove() }
  }
  return { createdTerminals, FakeTerminal }
})

vi.mock('@xterm/xterm', () => ({ Terminal: FakeTerminal }))
// jsdom has no WebGL2 context, so the real addon would throw on activate and BaseTerminal would
// silently take its DOM-renderer fallback — masking whether the addon is wired up at all.
vi.mock('@xterm/addon-webgl', () => ({
  WebglAddon: class {
    onContextLoss(): void {}
    dispose(): void {}
  },
}))
vi.mock('../utils/fitTerminal', () => ({ fitTerminal: () => {} }))
vi.mock('../utils/activityStateDetector', () => ({
  createActivityStateDetector: () => ({ processData: () => {}, destroy: () => {} }),
}))
vi.mock('./ContextMenu', () => ({ default: () => null }))

// Each mock exposes a STABLE state object so BaseTerminal's effect deps (settings,
// openExternal, setTabState) don't change identity between renders — otherwise the
// effect re-runs on every render and the StrictMode double-mount can't be observed.
vi.mock('../store/settings', () => {
  const settings = {
    terminal: { fontSize: 14, fontFamily: 'monospace', cursorBlink: true, cursorStyle: 'block', showRawChars: false },
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
  createdTerminals.length = 0
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe(): void {}
    unobserve(): void {}
    disconnect(): void {}
  }
})

interface Deferred {
  resolve: () => void
  unsubscribe: ReturnType<typeof vi.fn>
}

function makeFakeTty() {
  return {
    getState: () => ({
      ptyId: 'pty1',
      write: vi.fn<(d: string) => Promise<void>>().mockResolvedValue(undefined),
      resize: vi.fn(),
      kill: vi.fn(),
    }),
  }
}

function makeWorkspaceStore(tabId: string) {
  const appRef = {
    cachedTerminal: null as unknown,
    disposeCachedTerminal: vi.fn(),
    close: vi.fn(),
    dispose: vi.fn(),
  }
  const store = createStore<Record<string, unknown>>()(() => ({
    workspace: {
      id: 'ws1',
      activeTabId: tabId,
      appStates: { [tabId]: { applicationId: 'terminal', title: 'Terminal 1', state: { ptyId: 'pty1' } } },
    },
    removeTab: vi.fn(),
    getTabRef: () => appRef,
  }))
  return { store, appRef }
}

function makeSessionStore(deferreds: Deferred[]) {
  const openTtyStream = vi.fn(
    () =>
      new Promise((resolve) => {
        const unsubscribe = vi.fn()
        deferreds.push({
          resolve: () => { resolve({ tty: makeFakeTty(), unsubscribe }) },
          unsubscribe,
        })
      }),
  )
  const store = createStore<Record<string, unknown>>()(() => ({ openTtyStream }))
  return { store: store as never, openTtyStream }
}

describe('BaseTerminal — StrictMode double-mount cleanup', () => {
  it('disposes the orphaned terminal + stream from the cancelled first mount', async () => {
    const tabId = 'tab1'
    const deferreds: Deferred[] = []
    const { store: workspace } = makeWorkspaceStore(tabId)
    const { store: session, openTtyStream } = makeSessionStore(deferreds)
    // Stable config ref — mirrors Terminal.tsx's useState-stabilized config.
    const config = { themeBackground: '#000', logPrefix: 'Terminal' }

    render(
      <StrictMode>
        <SessionStoreContext.Provider value={session}>
          <BaseTerminal workspace={workspace as never} tabId={tabId} config={config} />
        </SessionStoreContext.Provider>
      </StrictMode>,
    )

    // StrictMode double-invoked the effect: mount → cleanup (cancelled) → mount.
    // Both mounts opened a terminal and started an attach before either resolved.
    expect(createdTerminals.length).toBe(2)
    expect(openTtyStream).toHaveBeenCalledTimes(2)
    expect(deferreds.length).toBe(2)

    // Resolve both attaches now that the double-mount has settled, then flush the
    // async init continuations (openTtyStream await → cancelled cleanup / attach).
    await act(async () => {
      deferreds.forEach((d) => { d.resolve() })
      await Promise.resolve()
    })

    // Exactly one live terminal must remain — the first (cancelled) mount's terminal
    // and its stream subscription must be torn down, not left orphaned in the DOM.
    const [first, second] = createdTerminals
    const liveTerminals = createdTerminals.filter((t) => !t.disposed)
    expect(liveTerminals.length).toBe(1)
    expect(first?.disposed).toBe(true)
    expect(second?.disposed).toBe(false)

    // The cancelled mount's stream is unsubscribed; the surviving mount's is not.
    expect(deferreds[0]?.unsubscribe).toHaveBeenCalledTimes(1)
    expect(deferreds[1]?.unsubscribe).not.toHaveBeenCalled()
  })
})
