// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { ILink, ILinkProvider } from 'ghostty-web'
import { ScrollPosition } from '../types'
import { createGhosttyEngine, routeLinksExternally } from './ghosttyEngine'
import type { TerminalEngine, TerminalEngineOptions } from './engine'

// The real module instantiates a WASM VT engine and a Canvas2D renderer, neither of which
// exists under jsdom.
const { terminals, initCalls, FakeTerminal } = vi.hoisted(() => {
  const terminals: FakeTerminal[] = []
  const initCalls = { count: 0 }

  class FakeTerminal {
    disposed = false
    opened = false
    focused = false
    cols = 80
    rows = 24
    /** ghostty-web counts viewport lines back from the bottom: 0 is the newest output. */
    viewportY = 0
    scrollbackLength = 0
    /** Grows the scrollback by this many lines on the next write, like real output would. */
    growBy = 0
    alternate = false
    element?: HTMLElement
    renderer: { charWidth: number; charHeight: number } | undefined = { charWidth: 8, charHeight: 16 }
    options: Record<string, unknown> = {}
    writes: (string | Uint8Array)[] = []
    resizes: { cols: number; rows: number }[] = []
    linkProviders: unknown[] = []
    dataListener: ((data: string) => void) | null = null
    scrollListener: ((y: number) => void) | null = null
    selection = ''

    constructor() { terminals.push(this) }

    get buffer() {
      return { active: { type: this.alternate ? 'alternate' : 'normal' } }
    }

    open(host: HTMLElement): void {
      if (this.opened) throw new Error('Terminal is already open')
      this.opened = true
      this.element = host
      host.appendChild(document.createElement('canvas'))
    }
    registerLinkProvider(provider: unknown): void {
      if (!this.opened) throw new Error('Terminal must be opened before registering link providers')
      this.linkProviders.push(provider)
    }
    onData(listener: (data: string) => void): { dispose(): void } {
      this.dataListener = listener
      return { dispose: () => { this.dataListener = null } }
    }
    onScroll(listener: (y: number) => void): { dispose(): void } {
      this.scrollListener = listener
      return { dispose: () => { this.scrollListener = null } }
    }
    write(data: string | Uint8Array, callback?: () => void): void {
      if (this.disposed) throw new Error('Terminal has been disposed')
      this.writes.push(data)
      this.scrollbackLength += this.growBy
      // The behaviour this engine exists to paper over: any write yanks you to the bottom.
      this.viewportY = 0
      callback?.()
    }
    resize(cols: number, rows: number): void {
      this.cols = cols
      this.rows = rows
      this.resizes.push({ cols, rows })
    }
    getScrollbackLength(): number { return this.scrollbackLength }
    scrollToLine(line: number): void { this.viewportY = Math.max(0, Math.min(this.scrollbackLength, line)) }
    scrollToTop(): void { this.viewportY = this.scrollbackLength }
    scrollToBottom(): void { this.viewportY = 0 }
    focus(): void { this.focused = true }
    getSelection(): string { return this.selection }
    dispose(): void { this.disposed = true }
  }

  return { terminals, initCalls, FakeTerminal }
})

vi.mock('ghostty-web', () => ({
  init: vi.fn(async () => { initCalls.count++; await Promise.resolve() }),
  Terminal: FakeTerminal,
  UrlRegexProvider: class { provideLinks(): void {} dispose(): void {} },
  OSC8LinkProvider: class { provideLinks(): void {} dispose(): void {} },
}))

const openExternal = vi.fn<(uri: string) => void>()

function options(): TerminalEngineOptions {
  return {
    fontSize: 14,
    fontFamily: 'monospace',
    cursorStyle: 'block',
    cursorBlink: true,
    themeBackground: '#1e1e1e',
    scrollback: 50000,
    openExternal,
    label: 'Ghostty tab1',
  }
}

type FakeTerm = InstanceType<typeof FakeTerminal>

async function makeEngine(): Promise<{ engine: TerminalEngine; terminal: FakeTerm; container: HTMLDivElement }> {
  const engine = await createGhosttyEngine(options())
  const container = document.createElement('div')
  document.body.appendChild(container)
  engine.attach(container)
  return { engine, terminal: terminals[terminals.length - 1]!, container }
}

beforeEach(() => {
  terminals.length = 0
  initCalls.count = 0
  openExternal.mockClear()
  document.body.innerHTML = ''
})

describe('createGhosttyEngine', () => {
  it('boots the WASM module before constructing a terminal', async () => {
    await createGhosttyEngine(options())
    expect(initCalls.count).toBe(1)
    expect(terminals).toHaveLength(1)
  })

  it('opens onto a host of its own rather than the caller container', async () => {
    const { terminal, container } = await makeEngine()

    const host = container.querySelector('.ghostty-terminal-host')
    expect(host).not.toBeNull()
    // ghostty-web takes over whatever it is opened onto — that must not be the container.
    expect(terminal.element).toBe(host)
  })

  it('registers both link providers, and only after open()', async () => {
    const { terminal } = await makeEngine()
    // FakeTerminal throws from registerLinkProvider when not open, mirroring ghostty-web.
    expect(terminal.linkProviders).toHaveLength(2)
  })

  it('reparents its host on a second attach instead of reopening the terminal', async () => {
    const { engine, terminal, container } = await makeEngine()
    const host = container.querySelector('.ghostty-terminal-host')

    const remounted = document.createElement('div')
    engine.attach(remounted)

    // Reopening would throw; the same host — and so the same scrollback — moved across.
    expect(remounted.querySelector('.ghostty-terminal-host')).toBe(host)
    expect(container.querySelector('.ghostty-terminal-host')).toBeNull()
    expect(terminal.opened).toBe(true)
  })

  it('drops its host from the DOM on dispose', async () => {
    const { engine, terminal, container } = await makeEngine()

    engine.dispose()

    expect(terminal.disposed).toBe(true)
    expect(container.querySelector('.ghostty-terminal-host')).toBeNull()
  })
})

describe('GhosttyEngine.write — scroll preservation', () => {
  it('puts the reader back where they were, offset by the lines the write appended', async () => {
    const { engine, terminal } = await makeEngine()
    terminal.scrollbackLength = 100
    terminal.viewportY = 30 // 30 lines above the bottom
    terminal.growBy = 5

    engine.write('output\r\n')

    // Same content on screen: it is now 35 lines above the (new) bottom.
    expect(terminal.viewportY).toBe(35)
  })

  it('leaves a reader who is already at the bottom pinned to the bottom', async () => {
    const { engine, terminal } = await makeEngine()
    terminal.scrollbackLength = 100
    terminal.viewportY = 0
    terminal.growBy = 5

    engine.write('output\r\n')

    expect(terminal.viewportY).toBe(0)
  })

  it('holds the reader in place once the scrollback has hit its cap', async () => {
    const { engine, terminal } = await makeEngine()
    terminal.scrollbackLength = 40
    terminal.viewportY = 38
    terminal.growBy = 0 // capped: the oldest line is evicted for each new one

    engine.write('output\r\n')

    // Nothing better is possible — the content itself shifted up by a line.
    expect(terminal.viewportY).toBe(38)
  })

  it('forwards the write callback', async () => {
    const { engine, terminal } = await makeEngine()
    const onWritten = vi.fn()

    engine.write('hi', onWritten)

    expect(terminal.writes).toEqual(['hi'])
    expect(onWritten).toHaveBeenCalledTimes(1)
  })
})

describe('GhosttyEngine — scroll orientation', () => {
  it('reports Bottom when there is no scrollback at all', async () => {
    const { engine } = await makeEngine()
    expect(engine.getScrollPosition()).toBe(ScrollPosition.Bottom)
  })

  it.each([
    { viewportY: 0, expected: ScrollPosition.Bottom, why: 'newest output' },
    { viewportY: 1, expected: ScrollPosition.Bottom, why: 'one line off the bottom still counts as bottom' },
    { viewportY: 50, expected: ScrollPosition.Middle, why: 'mid scrollback' },
    { viewportY: 100, expected: ScrollPosition.Top, why: 'oldest line' },
  ])('reports $expected at viewportY $viewportY ($why)', async ({ viewportY, expected }) => {
    const { engine, terminal } = await makeEngine()
    terminal.scrollbackLength = 100
    terminal.viewportY = viewportY

    expect(engine.getScrollPosition()).toBe(expected)
  })

  it('inverts viewportY into a ratio where 1 is the newest output', async () => {
    const { engine, terminal } = await makeEngine()
    terminal.scrollbackLength = 100

    terminal.viewportY = 0
    expect(engine.getScrollRatio()).toBe(1)
    terminal.viewportY = 100
    expect(engine.getScrollRatio()).toBe(0)
    terminal.viewportY = 25
    expect(engine.getScrollRatio()).toBe(0.75)
  })

  it('reports a ratio of 1 when there is no scrollback to be anywhere in', async () => {
    const { engine } = await makeEngine()
    expect(engine.getScrollRatio()).toBe(1)
  })

  it('restores a ratio back to the viewportY it came from', async () => {
    const { engine, terminal } = await makeEngine()
    terminal.scrollbackLength = 100

    engine.scrollToRatio(0.75)

    expect(terminal.viewportY).toBe(25)
  })

  it('scrolls to the oldest and newest lines', async () => {
    const { engine, terminal } = await makeEngine()
    terminal.scrollbackLength = 100

    engine.scrollToTop()
    expect(terminal.viewportY).toBe(100)
    engine.scrollToBottom()
    expect(terminal.viewportY).toBe(0)
  })
})

describe('GhosttyEngine — passthrough', () => {
  it('reports the alternate screen', async () => {
    const { engine, terminal } = await makeEngine()
    expect(engine.isAlternateScreen()).toBe(false)
    terminal.alternate = true
    expect(engine.isAlternateScreen()).toBe(true)
  })

  it('measures the container through the renderer cell size', async () => {
    const { engine, container } = await makeEngine()
    Object.defineProperty(container, 'clientWidth', { value: 800 })
    Object.defineProperty(container, 'clientHeight', { value: 400 })
    container.getBoundingClientRect = () => ({ width: 800, height: 400 }) as DOMRect
    const computeStyle = () => ({ paddingLeft: '0', paddingRight: '0', paddingTop: '0', paddingBottom: '0' }) as CSSStyleDeclaration

    expect(engine.proposeDimensions(computeStyle)).toEqual({ cols: 100, rows: 25 })
  })

  it('cannot propose dimensions before the renderer has measured a font', async () => {
    const { engine, terminal } = await makeEngine()
    terminal.renderer = undefined

    expect(engine.proposeDimensions(() => ({}) as CSSStyleDeclaration)).toBeUndefined()
  })

  it('forwards scroll events from the terminal', async () => {
    const { engine, terminal } = await makeEngine()
    const handler = vi.fn()
    const disposable = engine.onScroll(handler)

    terminal.scrollListener?.(5)
    expect(handler).toHaveBeenCalledTimes(1)

    disposable.dispose()
    expect(terminal.scrollListener).toBeNull()
  })

  it('reports wheel deltas from its host, and stops on dispose', async () => {
    const { engine, container } = await makeEngine()
    const handler = vi.fn<(deltaY: number) => void>()
    const disposable = engine.onWheel(handler)
    const host = container.querySelector('.ghostty-terminal-host')

    host?.dispatchEvent(new WheelEvent('wheel', { deltaY: -120 }))
    expect(handler).toHaveBeenCalledWith(-120)

    disposable.dispose()
    host?.dispatchEvent(new WheelEvent('wheel', { deltaY: -120 }))
    expect(handler).toHaveBeenCalledTimes(1)
  })

  it('pushes display options onto the live terminal', async () => {
    const { engine, terminal } = await makeEngine()

    engine.applyDisplayOptions({
      fontSize: 20,
      fontFamily: 'Fira Code',
      cursorBlink: false,
      cursorStyle: 'bar',
      themeBackground: '#000000',
    })

    expect(terminal.options.fontSize).toBe(20)
    expect(terminal.options.fontFamily).toBe('Fira Code')
    expect(terminal.options.cursorStyle).toBe('bar')
    expect((terminal.options.theme as { background: string }).background).toBe('#000000')
  })

  it('exposes cols, rows, selection, focus and resize', async () => {
    const { engine, terminal } = await makeEngine()
    terminal.selection = 'selected'

    expect(engine.cols).toBe(80)
    expect(engine.rows).toBe(24)
    expect(engine.getSelection()).toBe('selected')

    engine.focus()
    expect(terminal.focused).toBe(true)

    engine.resize(100, 30)
    expect(terminal.resizes).toEqual([{ cols: 100, rows: 30 }])
    expect(engine.cols).toBe(100)
  })

  it('forwards input through onData', async () => {
    const { engine, terminal } = await makeEngine()
    const handler = vi.fn<(data: string) => void>()
    engine.onData(handler)

    terminal.dataListener?.('ls\r')

    expect(handler).toHaveBeenCalledWith('ls\r')
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
    routeLinksExternally(inner, vi.fn()).provideLinks(0, (result: ILink[] | undefined) => { called = true; links = result })

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
