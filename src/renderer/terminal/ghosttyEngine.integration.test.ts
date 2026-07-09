// @vitest-environment jsdom
import { describe, it, expect, beforeAll, vi } from 'vitest'
import { ScrollPosition } from '../types'
import { createGhosttyEngine } from './ghosttyEngine'
import type { TerminalEngine, TerminalEngineOptions } from './engine'

/**
 * The unit tests drive a fake ghostty-web. This one drives the real one — WASM VT engine and
 * all — so the assumptions the engine is built on stay pinned to the library:
 *
 *  - `Terminal.open()` takes over the element it is handed (hence the engine's own host)
 *  - `viewportY` counts back from the bottom, opposite to xterm
 *  - `write()` yanks the viewport to the bottom, which `GhosttyEngine.write` compensates for
 *
 * If ghostty-web ever fixes the third one, this test fails and the workaround can go.
 */

/** jsdom has no 2D context; ghostty's CanvasRenderer needs measurement plus draw no-ops. */
function stubCanvas2D(): void {
  HTMLCanvasElement.prototype.getContext = vi.fn(() => ({
    measureText: (text: string) => ({ width: text.length * 8, actualBoundingBoxAscent: 12, actualBoundingBoxDescent: 4 }),
    fillRect() {}, clearRect() {}, fillText() {}, save() {}, restore() {}, beginPath() {},
    rect() {}, clip() {}, translate() {}, scale() {}, setTransform() {}, drawImage() {},
    moveTo() {}, lineTo() {}, stroke() {}, closePath() {},
    createLinearGradient: () => ({ addColorStop() {} }),
  })) as unknown as typeof HTMLCanvasElement.prototype.getContext
}

function options(): TerminalEngineOptions {
  return {
    fontSize: 14,
    fontFamily: 'monospace',
    cursorStyle: 'block',
    cursorBlink: true,
    themeBackground: '#1e1e1e',
    scrollback: 50000,
    openExternal: vi.fn(),
    label: 'Ghostty integration',
  }
}

async function mountEngine(): Promise<{ engine: TerminalEngine; container: HTMLDivElement }> {
  const engine = await createGhosttyEngine(options())
  const container = document.createElement('div')
  document.body.appendChild(container)
  engine.attach(container)
  return { engine, container }
}

/** Fill the scrollback so there is somewhere to scroll to. */
function writeLines(engine: TerminalEngine, count: number): void {
  for (let i = 0; i < count; i++) engine.write(`line ${String(i)}\r\n`)
}

beforeAll(() => { stubCanvas2D() })

describe('GhosttyEngine against real ghostty-web', () => {
  it('opens onto its own host, leaving the caller container alone', async () => {
    const { container } = await mountEngine()

    const host = container.querySelector('.ghostty-terminal-host')
    expect(host).not.toBeNull()
    // ghostty-web sets contenteditable/tabindex on whatever it opens onto. Not our container.
    expect(container.getAttribute('contenteditable')).toBeNull()
    expect(host?.getAttribute('contenteditable')).toBe('true')
  })

  it('holds the reader in place while output keeps arriving', async () => {
    const { engine } = await mountEngine()
    writeLines(engine, 200)
    expect(engine.getScrollPosition()).toBe(ScrollPosition.Bottom)

    engine.scrollToRatio(0.5)
    const parked = engine.getScrollRatio()
    expect(engine.getScrollPosition()).toBe(ScrollPosition.Middle)

    // Without the compensation in GhosttyEngine.write, ghostty-web snaps straight to the bottom.
    writeLines(engine, 10)

    expect(engine.getScrollPosition()).toBe(ScrollPosition.Middle)
    expect(engine.getScrollRatio()).toBeCloseTo(parked, 1)
  })

  it('leaves a reader at the bottom following the newest output', async () => {
    const { engine } = await mountEngine()
    writeLines(engine, 200)

    writeLines(engine, 10)

    expect(engine.getScrollPosition()).toBe(ScrollPosition.Bottom)
  })

  it('maps the top and bottom of the real scrollback onto the shared orientation', async () => {
    const { engine } = await mountEngine()
    writeLines(engine, 200)

    engine.scrollToTop()
    expect(engine.getScrollPosition()).toBe(ScrollPosition.Top)
    expect(engine.getScrollRatio()).toBe(0)

    engine.scrollToBottom()
    expect(engine.getScrollPosition()).toBe(ScrollPosition.Bottom)
    expect(engine.getScrollRatio()).toBe(1)
  })

  it('tracks the real alternate screen across enter and exit', async () => {
    const { engine } = await mountEngine()
    expect(engine.isAlternateScreen()).toBe(false)

    engine.write('\x1b[?1049h')
    expect(engine.isAlternateScreen()).toBe(true)

    engine.write('\x1b[?1049l')
    expect(engine.isAlternateScreen()).toBe(false)
  })

  it('exposes a buffer e2e can read text back from', async () => {
    const { engine } = await mountEngine()
    writeLines(engine, 3)

    // Mirrors e2e/helpers.ts#getTerminalText.
    expect(engine.raw.buffer.active.getLine(0)?.translateToString(true)).toBe('line 0')
  })

  it('measures a real cell size from the renderer', async () => {
    const { engine, container } = await mountEngine()
    Object.defineProperty(container, 'clientWidth', { value: 800 })
    Object.defineProperty(container, 'clientHeight', { value: 360 })
    container.getBoundingClientRect = () => ({ width: 800, height: 360 }) as DOMRect

    const dimensions = engine.proposeDimensions(
      () => ({ paddingLeft: '0', paddingRight: '0', paddingTop: '0', paddingBottom: '0' }) as CSSStyleDeclaration,
    )

    expect(dimensions?.cols).toBe(100) // 800 / 8px stub cell width
    expect(dimensions?.rows).toBeGreaterThan(0)
  })
})
