import { WebglAddon } from '@xterm/addon-webgl'
import type { Terminal } from '@xterm/xterm'

export enum WebglFallbackReason {
  /** No WebGL2 context could be created (software rendering, headless CI, driver blocklist). */
  Unavailable = 'unavailable',
  /** The GPU dropped the context after it was running. */
  ContextLost = 'context-lost',
}

export type WebglFallback =
  | { reason: WebglFallbackReason.Unavailable; error: Error }
  | { reason: WebglFallbackReason.ContextLost }

/**
 * Replace xterm's default DOM renderer with the GPU one. Must run *after* `terminal.open()`,
 * because the addon needs the screen element to attach its canvas layers to.
 *
 * Note this deletes xterm's `.xterm-rows` DOM subtree — DomRenderer removes its row container
 * when the render service swaps it out. Anything that scraped row elements for text must read
 * `terminal.buffer` instead (see `e2e/helpers.ts`).
 *
 * Falling back to the DOM renderer is normal operation, not an error, so this never throws:
 *
 *  - `Unavailable`: activation throws when there is no WebGL2 context. xterm keeps the DOM renderer.
 *  - `ContextLost`: Chromium caps live WebGL contexts (~16) and evicts the oldest. A workspace with
 *    many cached terminals will hit that ceiling, so every terminal must survive losing its context.
 *    Disposing the addon restores the DOM renderer for that terminal.
 *
 * The caller is handed the reason and decides whether to log or surface it — this function
 * neither logs nor swallows.
 */
export function loadWebglRenderer(terminal: Terminal, onFallback: (fallback: WebglFallback) => void): void {
  const addon = new WebglAddon()

  addon.onContextLoss(() => {
    addon.dispose()
    onFallback({ reason: WebglFallbackReason.ContextLost })
  })

  try {
    terminal.loadAddon(addon)
  } catch (error) {
    // activate() failed, so the addon owns no GPU resources and xterm never swapped
    // the renderer. Drop the half-built addon and report why.
    addon.dispose()
    onFallback({
      reason: WebglFallbackReason.Unavailable,
      error: error instanceof Error ? error : new Error(String(error)),
    })
  }
}
