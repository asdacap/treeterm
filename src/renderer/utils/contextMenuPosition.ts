export function clampContextMenuPosition(
  x: number,
  y: number,
  menuWidth = 160,
  menuHeight = 200
): { x: number; y: number } {
  const vw = window.innerWidth
  const vh = window.innerHeight
  return {
    x: Math.min(x, vw - menuWidth),
    y: Math.min(y, vh - menuHeight),
  }
}
