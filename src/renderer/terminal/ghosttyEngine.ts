import {
  init,
  Terminal as GhosttyTerm,
  OSC8LinkProvider,
  UrlRegexProvider,
  type ILink,
  type ILinkProvider,
} from 'ghostty-web'
import { TERMINAL_THEME_COLORS } from '../components/terminalTheme'
import { ScrollPosition } from '../types'
import { proposeDimensions } from './proposeDimensions'
import type {
  TerminalBufferHost,
  TerminalDimensions,
  TerminalDisplayOptions,
  TerminalDisposable,
  TerminalEngine,
  TerminalEngineOptions,
} from './engine'

/**
 * ghostty-web's bundled link providers activate with `window.open`, which would spawn a
 * BrowserWindow inside Electron. Keep their detection, replace the activation.
 */
export function routeLinksExternally(
  provider: ILinkProvider,
  openExternal: (uri: string) => void,
): ILinkProvider {
  return {
    provideLinks(y: number, callback: (links: ILink[] | undefined) => void): void {
      provider.provideLinks(y, (links) => {
        callback(links?.map((link) => ({ ...link, activate: () => { openExternal(link.text) } })))
      })
    },
    dispose(): void {
      provider.dispose?.()
    },
  }
}

/**
 * Ghostty's VT engine (libghostty-vt, compiled to WASM) drawn by a Canvas2D renderer.
 *
 * ghostty-web opens onto the element it is handed and takes it over — tabindex, contenteditable,
 * event listeners. So the engine opens onto a host element of its own and reparents *that*,
 * leaving `BaseTerminal`'s container untouched between mounts.
 *
 * Scroll coordinates are inverted relative to xterm: `viewportY` counts lines *back from the
 * bottom*, so 0 is the newest output and `getScrollbackLength()` is the oldest. The engine
 * interface hides this.
 */
class GhosttyEngine implements TerminalEngine {
  private readonly host = document.createElement('div')
  private opened = false

  constructor(
    private readonly terminal: GhosttyTerm,
    private readonly openExternal: (uri: string) => void,
  ) {
    this.host.className = 'ghostty-terminal-host'
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
    container.appendChild(this.host)
    if (this.opened) return

    this.terminal.open(this.host)
    this.opened = true

    // registerLinkProvider throws unless the terminal is already open.
    const route = (uri: string): void => { this.openExternal(uri) }
    this.terminal.registerLinkProvider(routeLinksExternally(new UrlRegexProvider(this.terminal), route))
    this.terminal.registerLinkProvider(routeLinksExternally(new OSC8LinkProvider(this.terminal), route))
  }

  applyDisplayOptions(options: TerminalDisplayOptions): void {
    // `options` is a Proxy — each assignment reconfigures the live terminal.
    this.terminal.options.fontSize = options.fontSize
    this.terminal.options.fontFamily = options.fontFamily
    this.terminal.options.cursorBlink = options.cursorBlink
    this.terminal.options.cursorStyle = options.cursorStyle
    this.terminal.options.theme = {
      ...TERMINAL_THEME_COLORS,
      background: options.themeBackground,
      cursorAccent: options.themeBackground,
    }
  }

  write(data: string | Uint8Array, onWritten?: () => void): void {
    // ghostty-web's write() yanks the viewport to the bottom on every call. Note where the
    // reader was and put them back, offset by however many lines the write pushed into the
    // scrollback — otherwise scrollback is unreadable while a command is still producing output.
    const viewportY = Math.round(this.terminal.viewportY)
    const scrollbackBefore = this.terminal.getScrollbackLength()

    this.terminal.write(data, onWritten)

    if (viewportY === 0) return
    const grown = this.terminal.getScrollbackLength() - scrollbackBefore
    this.terminal.scrollToLine(viewportY + grown)
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
    this.host.remove()
  }

  onData(handler: (data: string) => void): TerminalDisposable {
    return this.terminal.onData(handler)
  }

  onScroll(handler: () => void): TerminalDisposable {
    return this.terminal.onScroll(() => { handler() })
  }

  onWheel(handler: (deltaY: number) => void): TerminalDisposable {
    const onWheel = (event: Event): void => { handler((event as WheelEvent).deltaY) }
    this.host.addEventListener('wheel', onWheel)
    return { dispose: () => { this.host.removeEventListener('wheel', onWheel) } }
  }

  isAlternateScreen(): boolean {
    // eslint-disable-next-line custom/no-string-literal-comparison -- ghostty-web buffer type is external
    return this.terminal.buffer.active.type === 'alternate'
  }

  getScrollPosition(): ScrollPosition {
    const scrollback = this.terminal.getScrollbackLength()
    if (scrollback === 0) return ScrollPosition.Bottom
    const viewportY = Math.round(this.terminal.viewportY)
    if (viewportY >= scrollback) return ScrollPosition.Top
    if (viewportY <= 1) return ScrollPosition.Bottom
    return ScrollPosition.Middle
  }

  getScrollRatio(): number {
    const scrollback = this.terminal.getScrollbackLength()
    if (scrollback === 0) return 1
    return (scrollback - Math.round(this.terminal.viewportY)) / scrollback
  }

  scrollToRatio(ratio: number): void {
    const scrollback = this.terminal.getScrollbackLength()
    this.terminal.scrollToLine(Math.round(scrollback * (1 - ratio)))
  }

  scrollToTop(): void {
    this.terminal.scrollToTop()
  }

  scrollToBottom(): void {
    this.terminal.scrollToBottom()
  }

  proposeDimensions(computeStyle: (element: Element) => CSSStyleDeclaration): TerminalDimensions | undefined {
    const renderer = this.terminal.renderer
    if (!renderer) return undefined
    return proposeDimensions(this.host, { width: renderer.charWidth, height: renderer.charHeight }, computeStyle)
  }
}

export async function createGhosttyEngine(options: TerminalEngineOptions): Promise<TerminalEngine> {
  // Instantiates the ghostty-vt WASM module, which the bundle carries as an inline data: URI.
  // Idempotent — later calls resolve against the same instance.
  await init()

  const terminal = new GhosttyTerm({
    cursorBlink: options.cursorBlink,
    cursorStyle: options.cursorStyle,
    fontSize: options.fontSize,
    fontFamily: options.fontFamily,
    scrollback: options.scrollback,
    theme: {
      ...TERMINAL_THEME_COLORS,
      background: options.themeBackground,
      cursorAccent: options.themeBackground,
    },
  })
  return new GhosttyEngine(terminal, options.openExternal)
}
