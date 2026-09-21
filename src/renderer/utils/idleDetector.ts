import type { Settings } from '../../shared/types'

export interface IdleDetectorDeps {
  /** What is on screen at creation; the first differing snapshot is the first activity. */
  initialSnapshot: string
  /** Read every time the idle timer is (re)armed so settings and unread state apply live. */
  idleTimeoutMs: () => number
  /** Quiet -> active transition; fires once per burst of viewport changes. */
  onActivity: () => void
  /** No viewport change for idleTimeoutMs(). */
  onIdle: () => void
}

export interface IdleDetector {
  processSnapshot: (snapshot: string) => void
  destroy: () => void
}

/** Idle debounce for a tab: longer when the workspace already carries an unread marker. */
export function idleTimeoutMs(settings: Settings, unread: boolean): number {
  return unread ? settings.terminalAnalyzer.idleDebounceUnreadMs : settings.terminalAnalyzer.idleDebounceMs
}

/**
 * Shared quiet/active state machine behind terminal activity. Consumers feed rendered viewport
 * snapshots; a frame identical to the last one repainted nothing visible and is not activity.
 */
export function createIdleDetector(deps: IdleDetectorDeps): IdleDetector {
  let idleTimerId: ReturnType<typeof setTimeout> | null = null
  let lastSnapshot = deps.initialSnapshot

  const processSnapshot = (snapshot: string): void => {
    if (snapshot === lastSnapshot) return
    lastSnapshot = snapshot
    if (idleTimerId) {
      clearTimeout(idleTimerId)
    } else {
      deps.onActivity()
    }
    idleTimerId = setTimeout(() => {
      idleTimerId = null
      deps.onIdle()
    }, deps.idleTimeoutMs())
  }

  const destroy = (): void => {
    if (idleTimerId) {
      clearTimeout(idleTimerId)
      idleTimerId = null
    }
  }

  return { processSnapshot, destroy }
}
