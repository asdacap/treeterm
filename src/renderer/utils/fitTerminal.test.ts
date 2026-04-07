import { describe, it, expect, vi } from 'vitest'

vi.stubGlobal('window', {
  getComputedStyle: vi.fn<(elt: Element) => CSSStyleDeclaration>().mockReturnValue({
    paddingLeft: '0',
    paddingRight: '0',
    paddingTop: '0',
    paddingBottom: '0',
  } as unknown as CSSStyleDeclaration),
})

import { fitTerminal } from './fitTerminal'

function makeTerminal(overrides: {
  cellWidth?: number
  cellHeight?: number
  hasElement?: boolean
  hasParent?: boolean
  rectWidth?: number
  rectHeight?: number
  clientWidth?: number
  clientHeight?: number
  cols?: number
  rows?: number
} = {}) {
  const {
    cellWidth = 8,
    cellHeight = 16,
    hasElement = true,
    hasParent = true,
    rectWidth = 800,
    rectHeight = 400,
    clientWidth = 800,
    clientHeight = 400,
    cols = 80,
    rows = 24,
  } = overrides

  const parentElement = hasParent
    ? {
        getBoundingClientRect: () => ({ width: rectWidth, height: rectHeight }),
        clientWidth,
        clientHeight,
      }
    : null

  return {
    _core: {
      _renderService: {
        dimensions: { css: { cell: { width: cellWidth, height: cellHeight } } },
      },
    },
    element: hasElement ? { parentElement } : null,
    cols,
    rows,
  } as any
}

describe('fitTerminal', () => {
  it('computes cols/rows and calls onResize when dimensions change', () => {
    const onResize = vi.fn<(cols: number, rows: number) => void>()
    // 800/8 = 100 cols, 400/16 = 25 rows — different from default 80x24
    fitTerminal(makeTerminal(), onResize)
    expect(onResize).toHaveBeenCalledWith(100, 25)
  })

  it('returns early when cellWidth is 0', () => {
    const onResize = vi.fn<(cols: number, rows: number) => void>()
    fitTerminal(makeTerminal({ cellWidth: 0 }), onResize)
    expect(onResize).not.toHaveBeenCalled()
  })

  it('returns early when cellHeight is 0', () => {
    const onResize = vi.fn<(cols: number, rows: number) => void>()
    fitTerminal(makeTerminal({ cellHeight: 0 }), onResize)
    expect(onResize).not.toHaveBeenCalled()
  })

  it('returns early when element is null', () => {
    const onResize = vi.fn<(cols: number, rows: number) => void>()
    fitTerminal(makeTerminal({ hasElement: false }), onResize)
    expect(onResize).not.toHaveBeenCalled()
  })

  it('returns early when parentElement is null', () => {
    const onResize = vi.fn<(cols: number, rows: number) => void>()
    fitTerminal(makeTerminal({ hasParent: false }), onResize)
    expect(onResize).not.toHaveBeenCalled()
  })

  it('returns early when container has zero width', () => {
    const onResize = vi.fn<(cols: number, rows: number) => void>()
    fitTerminal(makeTerminal({ rectWidth: 0 }), onResize)
    expect(onResize).not.toHaveBeenCalled()
  })

  it('returns early when container has zero height', () => {
    const onResize = vi.fn<(cols: number, rows: number) => void>()
    fitTerminal(makeTerminal({ rectHeight: 0 }), onResize)
    expect(onResize).not.toHaveBeenCalled()
  })

  it('does not call onResize when cols/rows are unchanged', () => {
    const onResize = vi.fn<(cols: number, rows: number) => void>()
    // 800/8=100, 400/16=25 — set terminal to same values
    fitTerminal(makeTerminal({ cols: 100, rows: 25 }), onResize)
    expect(onResize).not.toHaveBeenCalled()
  })

  it('subtracts padding from available space', () => {
    vi.mocked(window.getComputedStyle).mockReturnValueOnce({
      paddingLeft: '10',
      paddingRight: '10',
      paddingTop: '8',
      paddingBottom: '8',
    } as unknown as CSSStyleDeclaration)
    const onResize = vi.fn<(cols: number, rows: number) => void>()
    // available: (800-20)/8 = 97.5 → 97 cols, (400-16)/16 = 24 rows
    fitTerminal(makeTerminal(), onResize)
    expect(onResize).toHaveBeenCalledWith(97, 24)
  })

  it('enforces minimum of 2 cols and 1 row', () => {
    const onResize = vi.fn<(cols: number, rows: number) => void>()
    // Very small container: 10/8=1.25→1 (clamped to 2), 10/16=0.625→0 (clamped to 1)
    fitTerminal(
      makeTerminal({ clientWidth: 10, clientHeight: 10, rectWidth: 10, rectHeight: 10 }),
      onResize
    )
    expect(onResize).toHaveBeenCalledWith(2, 1)
  })
})
