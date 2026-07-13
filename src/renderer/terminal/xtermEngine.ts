import { Terminal as XTerm } from '@xterm/xterm'
import { loadWebglRenderer, WebglFallbackReason, type WebglFallback } from '../utils/loadWebglRenderer'
import { TERMINAL_THEME_COLORS } from '../components/terminalTheme'
import { log } from '../utils/logger'
import { ScrollPosition } from '../types'
import { proposeDimensions } from './proposeDimensions'
import { xtermCellSize } from './xtermCellSize'
import type {
  TerminalBufferHost,
  TerminalDimensions,
  TerminalDisplayOptions,
  TerminalDisposable,
  TerminalEngine,
  TerminalEngineOptions,
} from './engine'
import '@xterm/xterm/css/xterm.css'

const MAX_OSC52_PAYLOAD_BYTES = 1024 * 1024

/** Decodes the `c;base64` payload supplied by an OSC 52 clipboard-write sequence. */
export function decodeOsc52Clipboard(data: string): string | undefined {
  const separator = data.indexOf(';')
  // OSC 52's only supported selection target is `c` (the clipboard).
  if (separator !== 1 || data.charCodeAt(0) !== 99) return undefined

  const encoded = data.slice(separator + 1)
  // `?` is a clipboard read request, which TreeTerm intentionally never serves.
  if ((encoded.length === 1 && encoded.charCodeAt(0) === 63) || encoded.length === 0 || encoded.length > Math.ceil(MAX_OSC52_PAYLOAD_BYTES / 3) * 4) return undefined
  if (!/^[A-Za-z0-9+/]*={0,2}$/.test(encoded)) return undefined

  try {
    const bytes = Uint8Array.from(atob(encoded), (character: string) => character.charCodeAt(0))
    if (bytes.byteLength > MAX_OSC52_PAYLOAD_BYTES) return undefined
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes)
  } catch {
    return undefined
  }
}

/**
 * `BaseTerminal`'s original engine: xterm.js with the WebGL renderer, falling back to the
 * DOM renderer when there is no GL context to be had.
 */
class XtermEngine implements TerminalEngine {
  private opened = false
  private allowOsc52Clipboard: boolean

  constructor(
    private readonly terminal: XTerm,
    private readonly label: string,
    allowOsc52Clipboard: boolean,
    writeClipboardText: (text: string) => void,
  ) {
    this.allowOsc52Clipboard = allowOsc52Clipboard
    this.terminal.parser.registerOscHandler(52, (data: string): boolean => {
      if (!this.allowOsc52Clipboard) return true
      const text = decodeOsc52Clipboard(data)
      if (text !== undefined) writeClipboardText(text)
      return true
    })
  }

  get raw(): TerminalBufferHost {
    return this.terminal
  }

  get cols(): number {
    return this.terminal.cols
  }

  get rows(): number {
    return this.terminal.rows
  }

  attach(container: HTMLElement): void {
    if (!this.opened) {
      this.terminal.open(container)
      this.opened = true
      loadWebglRenderer(this.terminal, (fallback: WebglFallback) => {
        if (fallback.reason === WebglFallbackReason.Unavailable) {
          log.warn(`[${this.label}] WebGL unavailable, using DOM renderer:`, fallback.error.message)
        } else {
          log.warn(`[${this.label}] WebGL context lost, reverted to DOM renderer`)
        }
      })
      return
    }
    // Reparent. The WebGL canvas moves with the element — a GL context survives being
    // detached from the DOM and re-attached elsewhere.
    container.appendChild(this.element())
    this.terminal.refresh(0, this.terminal.rows - 1)
  }

  applyDisplayOptions(options: TerminalDisplayOptions): void {
    this.terminal.options.fontSize = options.fontSize
    this.terminal.options.fontFamily = options.fontFamily
    this.terminal.options.cursorBlink = options.cursorBlink
    this.terminal.options.cursorStyle = options.cursorStyle
    this.allowOsc52Clipboard = options.allowOsc52Clipboard
    this.terminal.options.theme = {
      ...TERMINAL_THEME_COLORS,
      background: options.themeBackground,
      cursorAccent: options.themeBackground,
    }
  }

  write(data: string | Uint8Array, onWritten?: () => void): void {
    this.terminal.write(data, onWritten)
  }

  resize(cols: number, rows: number): void {
    this.terminal.resize(cols, rows)
  }

  focus(): void {
    this.terminal.focus()
  }

  getSelection(): string {
    return this.terminal.getSelection()
  }

  dispose(): void {
    this.terminal.dispose()
  }

  onData(handler: (data: string) => void): TerminalDisposable {
    return this.terminal.onData(handler)
  }

  onScroll(handler: () => void): TerminalDisposable {
    // Listen on the DOM viewport rather than `terminal.onScroll`, which xterm.js suppresses
    // for user-initiated scrolling (mouse wheel, touch).
    const viewport = this.viewport()
    viewport.addEventListener('scroll', handler)
    return { dispose: () => { viewport.removeEventListener('scroll', handler) } }
  }

  onWheel(handler: (deltaY: number) => void): TerminalDisposable {
    const viewport = this.viewport()
    const onWheel = (event: Event): void => { handler((event as WheelEvent).deltaY) }
    viewport.addEventListener('wheel', onWheel)
    return { dispose: () => { viewport.removeEventListener('wheel', onWheel) } }
  }

  isAlternateScreen(): boolean {
    // eslint-disable-next-line custom/no-string-literal-comparison -- xterm.js buffer type is external
    return this.terminal.buffer.active.type === 'alternate'
  }

  getScrollPosition(): ScrollPosition {
    const buffer = this.terminal.buffer.active
    if (buffer.baseY === 0) return ScrollPosition.Bottom
    if (buffer.viewportY === 0) return ScrollPosition.Top
    if (buffer.baseY - buffer.viewportY <= 1) return ScrollPosition.Bottom
    return ScrollPosition.Middle
  }

  getScrollRatio(): number {
    const buffer = this.terminal.buffer.active
    // viewportY is the absolute index of the top visible line, baseY that index at the bottom.
    return buffer.baseY > 0 ? buffer.viewportY / buffer.baseY : 1
  }

  scrollToRatio(ratio: number): void {
    this.terminal.scrollToLine(Math.round(this.terminal.buffer.active.baseY * ratio))
  }

  scrollToTop(): void {
    this.terminal.scrollToTop()
  }

  scrollToBottom(): void {
    this.terminal.scrollToBottom()
  }

  proposeDimensions(computeStyle: (element: Element) => CSSStyleDeclaration): TerminalDimensions | undefined {
    return proposeDimensions(this.element(), xtermCellSize(this.terminal), computeStyle)
  }

  private element(): HTMLElement {
    const element = this.terminal.element
    if (!element) throw new Error(`[${this.label}] xterm terminal has no element — attach() has not run`)
    return element
  }

  private viewport(): Element {
    const viewport = this.element().querySelector('.xterm-viewport')
    if (!viewport) throw new Error(`[${this.label}] xterm terminal has no .xterm-viewport element`)
    return viewport
  }
}

export function createXtermEngine(options: TerminalEngineOptions): Promise<TerminalEngine> {
  const terminal = new XTerm({
    cursorBlink: options.cursorBlink,
    cursorStyle: options.cursorStyle,
    fontSize: options.fontSize,
    fontFamily: options.fontFamily,
    scrollback: options.scrollback,
    linkHandler: {
      activate: (_event, uri) => { options.openExternal(uri) },
    },
    theme: {
      ...TERMINAL_THEME_COLORS,
      background: options.themeBackground,
      cursorAccent: options.themeBackground,
    },
  })
  return Promise.resolve(new XtermEngine(
    terminal,
    options.label,
    options.allowOsc52Clipboard,
    options.writeClipboardText,
  ))
}
