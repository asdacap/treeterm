import { describe, it, expect, vi } from 'vitest'

import { proposeDimensions } from './proposeDimensions'

const noPadding = vi.fn<(elt: Element) => CSSStyleDeclaration>().mockReturnValue({
  paddingLeft: '0',
  paddingRight: '0',
  paddingTop: '0',
  paddingBottom: '0',
} as unknown as CSSStyleDeclaration)

function makeElement(overrides: {
  hasParent?: boolean
  rectWidth?: number
  rectHeight?: number
  clientWidth?: number
  clientHeight?: number
} = {}) {
  const {
    hasParent = true,
    rectWidth = 800,
    rectHeight = 400,
    clientWidth = 800,
    clientHeight = 400,
  } = overrides

  const parentElement = hasParent
    ? {
        getBoundingClientRect: () => ({ width: rectWidth, height: rectHeight }),
        clientWidth,
        clientHeight,
      }
    : null

  return { parentElement } as unknown as HTMLElement
}

const cell = { width: 8, height: 16 }

describe('proposeDimensions', () => {
  it('divides the container by the cell size', () => {
    // 800/8 = 100 cols, 400/16 = 25 rows
    expect(proposeDimensions(makeElement(), cell, noPadding)).toEqual({ cols: 100, rows: 25 })
  })

  it('returns undefined when the renderer has not measured a cell width', () => {
    expect(proposeDimensions(makeElement(), { width: 0, height: 16 }, noPadding)).toBeUndefined()
  })

  it('returns undefined when the renderer has not measured a cell height', () => {
    expect(proposeDimensions(makeElement(), { width: 8, height: 0 }, noPadding)).toBeUndefined()
  })

  it('returns undefined when the element has no container', () => {
    expect(proposeDimensions(makeElement({ hasParent: false }), cell, noPadding)).toBeUndefined()
  })

  it('returns undefined when the container has zero width', () => {
    expect(proposeDimensions(makeElement({ rectWidth: 0 }), cell, noPadding)).toBeUndefined()
  })

  it('returns undefined when the container has zero height', () => {
    expect(proposeDimensions(makeElement({ rectHeight: 0 }), cell, noPadding)).toBeUndefined()
  })

  it('subtracts the container padding from the available space', () => {
    const padded = vi.fn<(elt: Element) => CSSStyleDeclaration>().mockReturnValue({
      paddingLeft: '10',
      paddingRight: '10',
      paddingTop: '8',
      paddingBottom: '8',
    } as unknown as CSSStyleDeclaration)

    // (800-20)/8 = 97.5 → 97 cols, (400-16)/16 = 24 rows
    expect(proposeDimensions(makeElement(), cell, padded)).toEqual({ cols: 97, rows: 24 })
  })

  it('enforces a minimum of 2 cols and 1 row', () => {
    // 10/8 = 1.25 → 1 (clamped to 2), 10/16 = 0.625 → 0 (clamped to 1)
    const tiny = makeElement({ clientWidth: 10, clientHeight: 10, rectWidth: 10, rectHeight: 10 })
    expect(proposeDimensions(tiny, cell, noPadding)).toEqual({ cols: 2, rows: 1 })
  })
})
