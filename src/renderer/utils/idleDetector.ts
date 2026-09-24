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
   * Quiet -> active transition; fires at most once per burst of viewport changes, with the frame
   * that confirmed the burst. A burst is confirmed when it is still changing one idle timeout
   * after it began, or when it goes quiet on a screen other than the one it started from. The
   * first burst after creation is assumed to be the attach replay and is only reported by the
   * first rule, which a replay never meets.
   */
  onActivity: (snapshot: string) => void
  /** No viewport change for the armed timeout, after a burst that changed the screen. */
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
 * A short burst that goes quiet on the screen it started from (a blip) reports nothing, so it
 * neither rings nor triggers a reclassification.
 */
export function createIdleDetector(deps: IdleDetectorDeps): IdleDetector {
  let idleTimerId: ReturnType<typeof setTimeout> | null = null
  let lastSnapshot = deps.initialSnapshot
  // No resting state is known until the first burst resolves. That burst is usually the
  // attach replay or initial paint, not work starting, so a short one is not reported as
  // activity; otherwise every tab of a freshly opened session would go Working -> Idle and ring.
  let settled = false
  let burstBaseline = deps.initialSnapshot
  let burstStartedAt = 0
  let reported = false

  const report = (snapshot: string): void => {
    reported = true
    deps.onActivity(snapshot)
  }

  const processSnapshot = (snapshot: string): void => {
    if (snapshot === lastSnapshot) return
    if (idleTimerId) {
      clearTimeout(idleTimerId)
      if (!reported && Date.now() - burstStartedAt >= deps.idleTimeoutMs()) report(snapshot)
    } else {
      burstBaseline = lastSnapshot
      burstStartedAt = Date.now()
      reported = false
    }
    lastSnapshot = snapshot
    // Captured at arming so the edge reports the timeout that actually elapsed.
    const timeout = deps.idleTimeoutMs()
    idleTimerId = setTimeout(() => {
      idleTimerId = null
      const wasSettled = settled
      settled = true
      if (!reported) {
        if (lastSnapshot === burstBaseline) return
        if (wasSettled) report(lastSnapshot)
      }
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
