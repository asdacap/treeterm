/** The window's focus events — injected, since only main.tsx may touch `window`. */
export interface WindowFocusTarget {
  addEventListener(type: 'focus', listener: () => void): void
  removeEventListener(type: 'focus', listener: () => void): void
}

/** Whether `element` holds keyboard focus in a window that itself has OS focus. */
export function hasKeyboardFocus(element: Element | null): boolean {
  return document.hasFocus() && element !== null && element.contains(document.activeElement)
}

/**
 * Call `claim` whenever the user arrives at or acts in the terminal in `container`: focus moving
 * into it, `windowTarget` regaining OS focus with the terminal still focused inside it, or a key
 * press or pointer press inside it — and once now, if it already has focus. Terminals use it to
 * claim their PTY's input: the daemon accepts input only from the most recently claimed terminal
 * attachment, so a terminal open on another machine cannot answer the program's queries a second
 * time. The key and pointer claims cover walking up to a machine whose window and terminal were
 * already focused — no focus event fires there. Capture phase, so the claim goes out before the
 * terminal turns the key press into a write.
 */
export function onKeyboardFocus(container: HTMLElement, windowTarget: WindowFocusTarget, claim: () => void): { dispose(): void } {
  const onFocus = (): void => { if (hasKeyboardFocus(container)) claim() }
  container.addEventListener('focusin', onFocus)
  container.addEventListener('keydown', onFocus, true)
  container.addEventListener('pointerdown', claim, true)
  windowTarget.addEventListener('focus', onFocus)
  onFocus()
  return {
    dispose: () => {
      container.removeEventListener('focusin', onFocus)
      container.removeEventListener('keydown', onFocus, true)
      container.removeEventListener('pointerdown', claim, true)
      windowTarget.removeEventListener('focus', onFocus)
    },
  }
}
