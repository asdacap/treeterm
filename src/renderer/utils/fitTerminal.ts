import type { Terminal } from '@xterm/xterm'

/**
 * Measure the character cell dimensions from the terminal's rendered DOM,
 * then resize the terminal to fill its container while preserving scroll position.
 */
export function fitTerminal(terminal: Terminal, onResize: (cols: number, rows: number) => void): void {
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

  const parentStyle = window.getComputedStyle(parentElement)
  const parentPaddingX = parseFloat(parentStyle.paddingLeft) + parseFloat(parentStyle.paddingRight)
  const parentPaddingY = parseFloat(parentStyle.paddingTop) + parseFloat(parentStyle.paddingBottom)

  const availableWidth = parentElement.clientWidth - parentPaddingX
  const availableHeight = parentElement.clientHeight - parentPaddingY

  const cols = Math.max(2, Math.floor(availableWidth / cellWidth))
  const rows = Math.max(1, Math.floor(availableHeight / cellHeight))

  if (terminal.cols !== cols || terminal.rows !== rows) {
    const prevViewportY = terminal.buffer.active.viewportY
    const prevBaseY = terminal.buffer.active.baseY
    const wasAtBottom = prevBaseY - prevViewportY <= 3
    const scrollRatio = prevBaseY > 0 ? prevViewportY / prevBaseY : 0

    terminal.resize(cols, rows)
    onResize(cols, rows)

    if (wasAtBottom) {
      terminal.scrollToBottom()
    } else {
      const newScrollLine = Math.round(terminal.buffer.active.baseY * scrollRatio)
      terminal.scrollToLine(newScrollLine)
    }
  }
}
