// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { ScrollPosition } from '../types'
import { log } from '../utils/logger'
import { createXtermEngine, decodeOsc52Clipboard } from './xtermEngine'
import type { TerminalEngine, TerminalEngineOptions } from './engine'

const { terminals, FakeTerminal } = vi.hoisted(() => {
  const terminals: FakeTerminal[] = []

  class FakeTerminal {
    disposed = false
    opened = false
    focused = false
    refreshed: { start: number; end: number }[] = []
    cols = 80
    rows = 24
    /** xterm's absolute line indices: viewportY is the top visible line, baseY it at the bottom. */
    baseY = 0
    viewportY = 0
    alternate = false
    scrolledToLine: number | null = null
    element: HTMLDivElement | undefined
    viewport = document.createElement('div')
    options: Record<string, unknown> = {}
    writes: (string | Uint8Array)[] = []
    resizes: { cols: number; rows: number }[] = []
    dataListener: ((data: string) => void) | null = null
    selection = ''
    osc52Handler: ((data: string) => boolean) | null = null
    parser = {
      registerOscHandler: (identifier: number, handler: (data: string) => boolean): { dispose(): void } => {
        if (identifier === 52) this.osc52Handler = handler
        return { dispose: () => { this.osc52Handler = null } }
      },
    }

    constructor(public readonly ctorOptions: { linkHandler: { activate: (event: MouseEvent, uri: string) => void } }) {
      terminals.push(this)
      this.viewport.className = 'xterm-viewport'
    }

    /** The private surface `xtermCellSize` reads through. */
    _core = { _renderService: { dimensions: { css: { cell: { width: 8, height: 16 } } } } }

    get buffer() {
      return {
        active: {
          type: this.alternate ? 'alternate' : 'normal',
          baseY: this.baseY,
          viewportY: this.viewportY,
        },
      }
    }

    /** Set to simulate a machine with no WebGL2 context — the real addon throws on activate. */
    webglUnavailable = false

    open(container: HTMLElement): void {
      this.opened = true
      this.element = document.createElement('div')
      this.element.appendChild(this.viewport)
      container.appendChild(this.element)
    }
    loadAddon(): void {
      if (this.webglUnavailable) throw new Error('WebGL2 is not supported')
    }
    refresh(start: number, end: number): void { this.refreshed.push({ start, end }) }
    onData(listener: (data: string) => void): { dispose(): void } {
      this.dataListener = listener
      return { dispose: () => { this.dataListener = null } }
    }
    write(data: string | Uint8Array, callback?: () => void): void { this.writes.push(data); callback?.() }
    resize(cols: number, rows: number): void { this.cols = cols; this.rows = rows; this.resizes.push({ cols, rows }) }
    focus(): void { this.focused = true }
    scrollToLine(line: number): void { this.scrolledToLine = line }
    scrollToTop(): void { this.scrolledToLine = 0 }
    scrollToBottom(): void { this.scrolledToLine = this.baseY }
    getSelection(): string { return this.selection }
    dispose(): void { this.disposed = true }
  }

  return { terminals, FakeTerminal }
})

vi.mock('@xterm/xterm', () => ({ Terminal: FakeTerminal }))
// jsdom has no WebGL2 context, so the real addon would throw on activate and the engine would
// silently take its DOM-renderer fallback — masking whether the addon is wired up at all.
const { webglAddons, contextLossHandlers } = vi.hoisted(() => ({
  webglAddons: [] as unknown[],
  contextLossHandlers: [] as (() => void)[],
}))
vi.mock('@xterm/addon-webgl', () => ({
  WebglAddon: class {
    constructor() { webglAddons.push(this) }
    onContextLoss(handler: () => void): void { contextLossHandlers.push(handler) }
    dispose(): void {}
  },
}))

const openExternal = vi.fn<(uri: string) => void>()

function options(): TerminalEngineOptions {
  return {
    fontSize: 14,
    fontFamily: 'monospace',
    cursorStyle: 'block',
    cursorBlink: true,
    themeBackground: '#1e1e1e',
    allowOsc52Clipboard: false,
    scrollback: 50000,
    openExternal,
    writeClipboardText: vi.fn(),
    label: 'Terminal tab1',
  }
}

type FakeTerm = InstanceType<typeof FakeTerminal>

async function makeEngine(): Promise<{ engine: TerminalEngine; terminal: FakeTerm; container: HTMLDivElement }> {
  const engine = await createXtermEngine(options())
  const container = document.createElement('div')
  document.body.appendChild(container)
  engine.attach(container)
  return { engine, terminal: terminals[terminals.length - 1]!, container }
}

beforeEach(() => {
  terminals.length = 0
  webglAddons.length = 0
  contextLossHandlers.length = 0
  openExternal.mockClear()
  document.body.innerHTML = ''
})

describe('decodeOsc52Clipboard', () => {
  it('decodes clipboard text only for the clipboard target', () => {
    expect(decodeOsc52Clipboard('c;aGVsbG8g4pyT')).toBe('hello ✓')
    expect(decodeOsc52Clipboard('p;aGVsbG8=')).toBeUndefined()
    expect(decodeOsc52Clipboard('c;?')).toBeUndefined()
    expect(decodeOsc52Clipboard('c;not base64')).toBeUndefined()
  })
})

describe('createXtermEngine', () => {
  it('writes an OSC 52 clipboard request only when enabled', async () => {
    const writeClipboardText = vi.fn<(text: string) => void>()
    await createXtermEngine({ ...options(), allowOsc52Clipboard: true, writeClipboardText })

    terminals[0]!.osc52Handler!('c;aGVsbG8=')

    expect(writeClipboardText).toHaveBeenCalledWith('hello')
  })

  it('routes activated links to the OS browser rather than a BrowserWindow', async () => {
    await createXtermEngine(options())
    terminals[0]!.ctorOptions.linkHandler.activate(new MouseEvent('click'), 'https://example.com')
    expect(openExternal).toHaveBeenCalledWith('https://example.com')
  })

  it('loads the GPU renderer when it first attaches', async () => {
    const { terminal, container } = await makeEngine()

    expect(terminal.opened).toBe(true)
    expect(webglAddons).toHaveLength(1)
    expect(container.contains(terminal.element ?? null)).toBe(true)
  })

  it('reparents on a second attach instead of reopening, and repaints the moved canvas', async () => {
    const { engine, terminal } = await makeEngine()

    const remounted = document.createElement('div')
    engine.attach(remounted)

    expect(remounted.contains(terminal.element ?? null)).toBe(true)
    expect(webglAddons).toHaveLength(1) // the GL context moved with the element
    expect(terminal.refreshed).toEqual([{ start: 0, end: 23 }])
  })

  it('falls back to the DOM renderer rather than failing when there is no WebGL context', async () => {
    const warn = vi.spyOn(log, 'warn').mockImplementation(() => {})
    const engine = await createXtermEngine(options())
    terminals[0]!.webglUnavailable = true

    // Headless CI and software rendering have no WebGL2 — the terminal must still come up.
    expect(() => { engine.attach(document.createElement('div')) }).not.toThrow()
    expect(warn.mock.calls[0]?.[0]).toContain('WebGL unavailable')
    warn.mockRestore()
  })

  it('survives Chromium evicting its GL context once too many terminals are live', async () => {
    const warn = vi.spyOn(log, 'warn').mockImplementation(() => {})
    await makeEngine()

    contextLossHandlers[0]?.()

    expect(warn.mock.calls[0]?.[0]).toContain('WebGL context lost')
    warn.mockRestore()
  })
})

describe('XtermEngine — scroll orientation', () => {
  it('reports Bottom when the buffer has never scrolled', async () => {
    const { engine } = await makeEngine()
    expect(engine.getScrollPosition()).toBe(ScrollPosition.Bottom)
  })

  it.each([
    { viewportY: 0, expected: ScrollPosition.Top, why: 'oldest line' },
    { viewportY: 50, expected: ScrollPosition.Middle, why: 'mid scrollback' },
    { viewportY: 99, expected: ScrollPosition.Bottom, why: 'one line off the bottom still counts as bottom' },
    { viewportY: 100, expected: ScrollPosition.Bottom, why: 'newest output' },
  ])('reports $expected at viewportY $viewportY ($why)', async ({ viewportY, expected }) => {
    const { engine, terminal } = await makeEngine()
    terminal.baseY = 100
    terminal.viewportY = viewportY

    expect(engine.getScrollPosition()).toBe(expected)
  })

  it('reads viewportY as a ratio where 1 is the newest output', async () => {
    const { engine, terminal } = await makeEngine()
    terminal.baseY = 100

    terminal.viewportY = 100
    expect(engine.getScrollRatio()).toBe(1)
    terminal.viewportY = 0
    expect(engine.getScrollRatio()).toBe(0)
    terminal.viewportY = 75
    expect(engine.getScrollRatio()).toBe(0.75)
  })

  it('reports a ratio of 1 when there is no scrollback to be anywhere in', async () => {
    const { engine } = await makeEngine()
    expect(engine.getScrollRatio()).toBe(1)
  })

  it('restores a ratio back to the line it came from', async () => {
    const { engine, terminal } = await makeEngine()
    terminal.baseY = 100

    engine.scrollToRatio(0.75)

    expect(terminal.scrolledToLine).toBe(75)
  })

  it('scrolls to the oldest and newest lines', async () => {
    const { engine, terminal } = await makeEngine()
    terminal.baseY = 100

    engine.scrollToTop()
    expect(terminal.scrolledToLine).toBe(0)
    engine.scrollToBottom()
    expect(terminal.scrolledToLine).toBe(100)
  })
})

describe('XtermEngine — DOM wiring', () => {
  it('listens for scroll on the viewport, which xterm.js does not report through onScroll', async () => {
    const { engine, terminal } = await makeEngine()
    const handler = vi.fn()
    const disposable = engine.onScroll(handler)

    terminal.viewport.dispatchEvent(new Event('scroll'))
    expect(handler).toHaveBeenCalledTimes(1)

    disposable.dispose()
    terminal.viewport.dispatchEvent(new Event('scroll'))
    expect(handler).toHaveBeenCalledTimes(1)
  })

  it('reports wheel deltas, and stops on dispose', async () => {
    const { engine, terminal } = await makeEngine()
    const handler = vi.fn<(deltaY: number) => void>()
    const disposable = engine.onWheel(handler)

    terminal.viewport.dispatchEvent(new WheelEvent('wheel', { deltaY: -120 }))
    expect(handler).toHaveBeenCalledWith(-120)

    disposable.dispose()
    terminal.viewport.dispatchEvent(new WheelEvent('wheel', { deltaY: -120 }))
    expect(handler).toHaveBeenCalledTimes(1)
  })

  it('fails loudly when asked for DOM handlers before it has attached', async () => {
    const engine = await createXtermEngine(options())
    expect(() => engine.onScroll(() => {})).toThrow(/has no element/)
  })

  it('measures the container through the private render service', async () => {
    const { engine, container } = await makeEngine()
    Object.defineProperty(container, 'clientWidth', { value: 800 })
    Object.defineProperty(container, 'clientHeight', { value: 400 })
    container.getBoundingClientRect = () => ({ width: 800, height: 400 }) as DOMRect
    const computeStyle = () => ({ paddingLeft: '0', paddingRight: '0', paddingTop: '0', paddingBottom: '0' }) as CSSStyleDeclaration

    expect(engine.proposeDimensions(computeStyle)).toEqual({ cols: 100, rows: 25 })
  })

  it('reports the alternate screen', async () => {
    const { engine, terminal } = await makeEngine()
    expect(engine.isAlternateScreen()).toBe(false)
    terminal.alternate = true
    expect(engine.isAlternateScreen()).toBe(true)
  })

  it('pushes display options onto the live terminal', async () => {
    const { engine, terminal } = await makeEngine()

    engine.applyDisplayOptions({
      fontSize: 20,
      fontFamily: 'Fira Code',
      cursorBlink: false,
      cursorStyle: 'bar',
      themeBackground: '#000000',
      allowOsc52Clipboard: false,
    })

    expect(terminal.options.fontSize).toBe(20)
    expect(terminal.options.cursorStyle).toBe('bar')
    expect((terminal.options.theme as { background: string }).background).toBe('#000000')
  })

  it('exposes write, resize, focus, selection and dispose', async () => {
    const { engine, terminal } = await makeEngine()
    terminal.selection = 'selected'
    const onWritten = vi.fn()

    engine.write('hi', onWritten)
    expect(terminal.writes).toEqual(['hi'])
    expect(onWritten).toHaveBeenCalledTimes(1)

    engine.resize(100, 30)
    expect(terminal.resizes).toEqual([{ cols: 100, rows: 30 }])
    expect(engine.cols).toBe(100)
    expect(engine.rows).toBe(30)

    engine.focus()
    expect(terminal.focused).toBe(true)
    expect(engine.getSelection()).toBe('selected')

    engine.dispose()
    expect(terminal.disposed).toBe(true)
  })

  it('forwards input through onData', async () => {
    const { engine, terminal } = await makeEngine()
    const handler = vi.fn<(data: string) => void>()
    engine.onData(handler)

    terminal.dataListener?.('ls\r')

    expect(handler).toHaveBeenCalledWith('ls\r')
  })
})
