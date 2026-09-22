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
   * frame. The first burst after creation is assumed to be the attach replay and is not
   * reported unless it outlives one idle timeout, which a replay never does.
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
  // No resting state is known until the first burst resolves. That burst is usually the
  // attach replay or initial paint, not work starting, so it is not reported as activity up
  // front; otherwise every tab of a freshly opened session would go Working -> Idle and ring.
  // A replay lands well within one idle timeout, though, so a first burst still changing
  // the viewport that long after it began is a process that was already working when we
  // attached, and it is reported as Working at that change.
  let settled = false
  let firstBurstStartedAt = 0

  const processSnapshot = (snapshot: string): void => {
    if (snapshot === lastSnapshot) return
    lastSnapshot = snapshot
    if (idleTimerId) {
      clearTimeout(idleTimerId)
      if (!settled && Date.now() - firstBurstStartedAt >= deps.idleTimeoutMs()) {
        settled = true
        deps.onActivity(snapshot)
      }
    } else if (settled) {
      deps.onActivity(snapshot)
    } else {
      firstBurstStartedAt = Date.now()
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
