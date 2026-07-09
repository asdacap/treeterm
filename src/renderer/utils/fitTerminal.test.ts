import { describe, it, expect, vi } from 'vitest'
import type { Terminal } from '@xterm/xterm'

import { fitTerminal } from './fitTerminal'

// The cell-grid maths itself is covered by terminal/proposeDimensions.test.ts. What is left
// here is the xterm-specific wiring: read the private cell size, only resize on a change.
const mockComputeStyle = vi.fn<(elt: Element) => CSSStyleDeclaration>().mockReturnValue({
  paddingLeft: '0',
  paddingRight: '0',
  paddingTop: '0',
  paddingBottom: '0',
} as unknown as CSSStyleDeclaration)

function makeTerminal(overrides: { hasElement?: boolean; cols?: number; rows?: number } = {}) {
  const { hasElement = true, cols = 80, rows = 24 } = overrides

  const parentElement = {
    getBoundingClientRect: () => ({ width: 800, height: 400 }),
    clientWidth: 800,
    clientHeight: 400,
  }

  return {
    _core: {
      _renderService: { dimensions: { css: { cell: { width: 8, height: 16 } } } },
    },
    element: hasElement ? { parentElement } : null,
    cols,
    rows,
  } as unknown as Terminal
}

describe('fitTerminal', () => {
  it('proposes the computed size to the daemon when it differs from the current one', () => {
    const onResize = vi.fn<(cols: number, rows: number) => void>()
    // 800/8 = 100 cols, 400/16 = 25 rows — different from the terminal's 80x24
    fitTerminal(makeTerminal(), onResize, mockComputeStyle)
    expect(onResize).toHaveBeenCalledWith(100, 25)
  })

  it('stays quiet when the terminal already has the computed size', () => {
    const onResize = vi.fn<(cols: number, rows: number) => void>()
    fitTerminal(makeTerminal({ cols: 100, rows: 25 }), onResize, mockComputeStyle)
    expect(onResize).not.toHaveBeenCalled()
  })

  it('stays quiet before the terminal has been opened', () => {
    const onResize = vi.fn<(cols: number, rows: number) => void>()
    fitTerminal(makeTerminal({ hasElement: false }), onResize, mockComputeStyle)
    expect(onResize).not.toHaveBeenCalled()
  })
})
