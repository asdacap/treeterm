import type { TerminalCellSize, TerminalDimensions } from './engine'

/**
 * The cell grid that fills `element`'s container, measured from the container rather than the
 * terminal so that shrinking works: the terminal element is sized by its own grid and would
 * never report less space than it currently occupies.
 *
 * Returns undefined when the answer would be meaningless — the renderer has not measured a
 * font yet, or the container is unlaid-out (`display: none` gives it zero dimensions).
 */
export function proposeDimensions(
  element: HTMLElement,
  cell: TerminalCellSize,
  computeStyle: (element: Element) => CSSStyleDeclaration,
): TerminalDimensions | undefined {
  if (!cell.width || !cell.height) return undefined

  const parent = element.parentElement
  if (!parent) return undefined

  const rect = parent.getBoundingClientRect()
  if (rect.width === 0 || rect.height === 0) return undefined

  const style = computeStyle(parent)
  const paddingX = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight)
  const paddingY = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom)

  return {
    cols: Math.max(2, Math.floor((parent.clientWidth - paddingX) / cell.width)),
    rows: Math.max(1, Math.floor((parent.clientHeight - paddingY) / cell.height)),
  }
}
