import type { Terminal } from '@xterm/xterm'
import { proposeDimensions } from '../terminal/proposeDimensions'
import { xtermCellSize } from '../terminal/xtermCellSize'

/**
 * Measure the character cell dimensions from the terminal's rendered DOM,
 * then send the computed size to the daemon. The daemon echoes the resize
 * back and the caller applies it to xterm.js — this function does NOT
 * resize the terminal locally.
 *
 * For terminals that go through `BaseTerminal`, use `TerminalEngine.proposeDimensions`
 * instead. This exists for `PtyViewer`, which drives a bare xterm of its own.
 */
export function fitTerminal(terminal: Terminal, onResize: (cols: number, rows: number) => void, computeStyle: (el: Element) => CSSStyleDeclaration): void {
  const element = terminal.element
  if (!element) return

  const dimensions = proposeDimensions(element, xtermCellSize(terminal), computeStyle)
  if (!dimensions) return

  if (terminal.cols !== dimensions.cols || terminal.rows !== dimensions.rows) {
    onResize(dimensions.cols, dimensions.rows)
  }
}
