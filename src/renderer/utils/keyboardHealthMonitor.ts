/* eslint-disable custom/no-string-literal-comparison -- TODO: migrate existing string-literal comparisons to enums */
/**
 * Keyboard health monitor — detects when Shift/Enter events stop arriving.
 * Toggle via DevTools: window.__enableKeyDiag = true
 */

import type { KeyEventTarget } from '../store/keybinding'

export function initKeyboardHealthMonitor(target: KeyEventTarget, isEnabled: () => boolean): () => void {
  let lastShiftOrEnter = Date.now()
  let lastAnyKey = Date.now()
  let warned = false

  const onKeyDown = (e: KeyboardEvent): void => {
    lastAnyKey = Date.now()
    if (e.key === 'Shift' || e.key === 'Enter') {
      lastShiftOrEnter = Date.now()
      warned = false
    }
  }

  const checkHealth = (): void => {
    if (!isEnabled()) return
    const now = Date.now()
    // If other keys active but Shift/Enter not seen for 30s
    if (now - lastShiftOrEnter > 30_000 && now - lastAnyKey < 5_000 && !warned) {
      warned = true
      console.warn('[KeyDiag] Shift/Enter not seen for 30s while other keys active', {
        activeElement: document.activeElement?.tagName,
        hasFocus: document.hasFocus(),
      })
    }
  }

  target.addEventListener('keydown', onKeyDown)
  const interval = setInterval(checkHealth, 5_000)
  return () => {
    target.removeEventListener('keydown', onKeyDown)
    clearInterval(interval)
  }
}
