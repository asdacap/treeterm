import type { ScrollPosition } from '../types'

/** The disposable shape both xterm.js and ghostty-web return from their event registrations. */
export interface TerminalDisposable {
  dispose(): void
}

export interface TerminalDimensions {
  cols: number
  rows: number
}

/** Character cell size in CSS pixels, as measured by the engine's own renderer. */
export interface TerminalCellSize {
  width: number
  height: number
}

/**
 * The slice of the underlying terminal that e2e reads back off the container element.
 * Both engines satisfy it structurally — see `e2e/helpers.ts#getTerminalText`.
 */
export interface TerminalBufferLine {
  translateToString(trimRight?: boolean): string
}

export interface TerminalBufferHost {
  readonly buffer: {
    readonly active: {
      readonly length: number
      getLine(y: number): TerminalBufferLine | undefined
    }
  }
}

/** Options that can change while a terminal is alive, e.g. when the user edits settings. */
export interface TerminalDisplayOptions {
  fontSize: number
  fontFamily: string
  cursorStyle: 'block' | 'underline' | 'bar'
  cursorBlink: boolean
  themeBackground: string
  /** Whether OSC 52 output may write to the local system clipboard. */
  allowOsc52Clipboard: boolean
}

export interface TerminalEngineOptions extends TerminalDisplayOptions {
  scrollback: number
  /** Where an activated hyperlink goes. Left to the engines, both would open a BrowserWindow. */
  openExternal: (uri: string) => void
  /** Writes terminal-requested clipboard text to the local system clipboard. */
  writeClipboardText: (text: string) => void
  /** Identifies this terminal in the engine's own renderer diagnostics. */
  label: string
}

/**
 * A terminal frontend, reduced to what `BaseTerminal` needs of it.
 *
 * Two implementations exist — `xtermEngine` (xterm.js + WebGL) and `ghosttyEngine`
 * (libghostty-vt in WASM + Canvas2D). Everything above this interface — the terminal cache,
 * scroll badge, pin-to-bottom, activity detection, exit handling — is engine-agnostic and
 * lives once, in `BaseTerminal`.
 *
 * Scroll orientation is normalised here: the engines disagree (xterm counts lines from the
 * top of the scrollback, ghostty counts them from the bottom), so neither raw coordinate
 * system escapes into `BaseTerminal`.
 */
export interface TerminalEngine {
  /** The engine's own terminal object, published for e2e to read the buffer through. */
  readonly raw: TerminalBufferHost
  readonly cols: number
  readonly rows: number

  /**
   * Put the engine's element inside `container`. Called on first mount and again on every
   * remount: the later calls reparent rather than rebuild, which is what lets a cached
   * terminal survive a tab switch with its scrollback — and, for xterm, its GL context — intact.
   */
  attach(container: HTMLElement): void

  applyDisplayOptions(options: TerminalDisplayOptions): void
  write(data: string | Uint8Array, onWritten?: () => void): void
  resize(cols: number, rows: number): void
  focus(): void
  getSelection(): string
  dispose(): void

  onData(handler: (data: string) => void): TerminalDisposable
  /** Fires on every viewport movement, whether the user or the program caused it. */
  onScroll(handler: () => void): TerminalDisposable
  /** Fires on wheel over the terminal. `deltaY < 0` means scrolling back toward older output. */
  onWheel(handler: (deltaY: number) => void): TerminalDisposable

  isAlternateScreen(): boolean
  getScrollPosition(): ScrollPosition
  /** Where the viewport sits in the scrollback: 0 = the oldest line, 1 = the newest. */
  getScrollRatio(): number
  scrollToRatio(ratio: number): void
  scrollToTop(): void
  scrollToBottom(): void

  /**
   * The cell grid that fills the container, or undefined when it has no layout yet.
   * A proposal only — the daemon owns the size and echoes back the one it applied.
   */
  proposeDimensions(computeStyle: (element: Element) => CSSStyleDeclaration): TerminalDimensions | undefined
}

export type TerminalEngineFactory = (options: TerminalEngineOptions) => Promise<TerminalEngine>
