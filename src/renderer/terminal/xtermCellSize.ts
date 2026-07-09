import type { Terminal as XTerm } from '@xterm/xterm'
import type { TerminalCellSize } from './engine'

/** xterm exposes its measured cell size only through the private render service. */
interface XTermCore {
  _core: { _renderService: { dimensions: { css: { cell: TerminalCellSize } } } }
}

export function xtermCellSize(terminal: XTerm): TerminalCellSize {
  return (terminal as unknown as XTermCore)._core._renderService.dimensions.css.cell
}
