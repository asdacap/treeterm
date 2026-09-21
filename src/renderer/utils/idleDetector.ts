import type { Settings } from '../../shared/types'

export interface IdleEdge {
  /** The last viewport seen before the timer ran out. */
  snapshot: string
  /** The timeout that was armed for this quiet period. */
  idleTimeoutMs: number
}

export interface IdleDetectorDeps {
  /** What is on screen at creation; the first differing snapshot is the first activity. */
  initialSnapshot: string
  /** Read every time the idle timer is (re)armed so settings and unread state apply live. */
  idleTimeoutMs: () => number
  /**
   * Quiet -> active transition; fires once per burst of viewport changes with the first changed
   * frame, and never for the first burst after creation (no resting state is known yet).
   */
  onActivity: (snapshot: string) => void
  /** No viewport change for the armed timeout. */
  onIdle: (edge: IdleEdge) => void
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
  // No resting state is known until the first idle. The first burst after creation is the
  // attach replay or initial paint, not work starting, so it is not reported as activity;
  // only the idle that ends it is. Otherwise every tab of a freshly opened session would
  // go Working -> Idle and ring.
  let settled = false

  const processSnapshot = (snapshot: string): void => {
    if (snapshot === lastSnapshot) return
    lastSnapshot = snapshot
    if (idleTimerId) {
      clearTimeout(idleTimerId)
    } else if (settled) {
      deps.onActivity(snapshot)
    }
    // Captured at arming so the edge reports the timeout that actually elapsed.
    const timeout = deps.idleTimeoutMs()
    idleTimerId = setTimeout(() => {
      idleTimerId = null
      settled = true
      deps.onIdle({ snapshot: lastSnapshot, idleTimeoutMs: timeout })
    }, timeout)
  }

  const destroy = (): void => {
    if (idleTimerId) {
      clearTimeout(idleTimerId)
      idleTimerId = null
    }
  }

  return { processSnapshot, destroy }
}
