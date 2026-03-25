/**
 * Keyboard health monitor — detects when Shift/Enter events stop arriving.
 * Toggle via DevTools: window.__enableKeyDiag = true
 */

declare global {
  interface Window {
    __enableKeyDiag?: boolean
  }
}

export function initKeyboardHealthMonitor(): () => void {
  if (typeof window === 'undefined') return () => {}

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
    if (!window.__enableKeyDiag) return
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

  window.addEventListener('keydown', onKeyDown)
  const interval = setInterval(checkHealth, 5_000)
  return () => {
    window.removeEventListener('keydown', onKeyDown)
    clearInterval(interval)
  }
}
