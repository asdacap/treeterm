import { ActivityState } from '../types'

interface DetectorConfig {
  idleTimeout?: number // ms of no activity before switching to idle (default: 1000)
  debounceMs?: number // ms to debounce state changes (default: 100)
}

export function createActivityStateDetector(
  onStateChange: (state: ActivityState) => void,
  config?: DetectorConfig
): {
  processData: (data: string) => void
  destroy: () => void
} {
  const idleTimeout = config?.idleTimeout ?? 1000
  const debounceMs = config?.debounceMs ?? 100

  let currentState: ActivityState = ActivityState.Idle
  let idleTimerId: ReturnType<typeof setTimeout> | null = null
  let debounceTimerId: ReturnType<typeof setTimeout> | null = null
  // The last rendered viewport. An incoming frame identical to this repainted nothing visible, so
  // it is not treated as activity (see processData).
  let lastSnapshot: string | null = null

  const emitState = (state: ActivityState) => {
    // Always clear pending debounce when trying to emit 'working'
    // This prevents a race where data arrives during a debounced transition
    // to 'idle', which would incorrectly fire after the early return
    if (state === ActivityState.Working && debounceTimerId) {
      clearTimeout(debounceTimerId)
      debounceTimerId = null
    }

    if (state === currentState) {
      return
    }

    // Clear any pending debounce for other state transitions
    if (debounceTimerId) {
      clearTimeout(debounceTimerId)
      debounceTimerId = null
    }

    // Emit 'working' immediately - we want instant feedback when output starts
    // Debounce other state changes to prevent flickering
    if (state === ActivityState.Working) {
      currentState = state
      onStateChange(state)
    } else {
      debounceTimerId = setTimeout(() => {
        if (state !== currentState) {
          currentState = state
          onStateChange(state)
        }
        debounceTimerId = null
      }, debounceMs)
    }
  }

  const scheduleIdleCheck = () => {
    if (idleTimerId) {
      clearTimeout(idleTimerId)
    }

    idleTimerId = setTimeout(() => {
      emitState(ActivityState.Idle)
      idleTimerId = null
    }, idleTimeout)
  }

  const processData = (snapshot: string) => {
    // A frame that leaves the rendered viewport identical repainted nothing visible — ignore it
    // entirely so the idle timer keeps running toward Idle. Only a real screen change counts as
    // activity.
    if (snapshot === lastSnapshot) {
      return
    }
    lastSnapshot = snapshot
    emitState(ActivityState.Working)
    scheduleIdleCheck()
  }

  const destroy = () => {
    if (idleTimerId) {
      clearTimeout(idleTimerId)
      idleTimerId = null
    }
    if (debounceTimerId) {
      clearTimeout(debounceTimerId)
      debounceTimerId = null
    }
  }

  return {
    processData,
    destroy
  }
}
