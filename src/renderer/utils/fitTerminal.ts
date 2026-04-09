import type { Terminal } from '@xterm/xterm'

/**
 * Measure the character cell dimensions from the terminal's rendered DOM,
 * then send the computed size to the daemon. The daemon echoes the resize
 * back and the caller applies it to xterm.js — this function does NOT
 * resize the terminal locally.
 */
export function fitTerminal(terminal: Terminal, onResize: (cols: number, rows: number) => void, computeStyle: (el: Element) => CSSStyleDeclaration): void {
  const core = (terminal as unknown as { _core: { _renderService: { dimensions: { css: { cell: { width: number; height: number } } } } } })._core
  const cellWidth = core._renderService.dimensions.css.cell.width
  const cellHeight = core._renderService.dimensions.css.cell.height
  if (!cellWidth || !cellHeight) return

  const element = terminal.element
  if (!element) return
  const parentElement = element.parentElement
  if (!parentElement) return

  // Skip if container is not laid out (display:none gives zero dimensions)
  const rect = parentElement.getBoundingClientRect()
  if (rect.width === 0 || rect.height === 0) return

  const parentStyle = computeStyle(parentElement)
  const parentPaddingX = parseFloat(parentStyle.paddingLeft) + parseFloat(parentStyle.paddingRight)
  const parentPaddingY = parseFloat(parentStyle.paddingTop) + parseFloat(parentStyle.paddingBottom)

  const availableWidth = parentElement.clientWidth - parentPaddingX
  const availableHeight = parentElement.clientHeight - parentPaddingY

  const cols = Math.max(2, Math.floor(availableWidth / cellWidth))
  const rows = Math.max(1, Math.floor(availableHeight / cellHeight))

  if (terminal.cols !== cols || terminal.rows !== rows) {
    onResize(cols, rows)
  }
}
