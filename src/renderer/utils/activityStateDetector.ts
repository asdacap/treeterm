import type { ActivityState } from '../types'

interface DetectorConfig {
  promptPatterns?: RegExp[] // Patterns indicating "waiting for input"
  workingPatterns?: RegExp[] // Patterns indicating "working" (e.g., spinners)
  idleTimeout?: number // ms of no activity before confirming state (default: 1000)
  debounceMs?: number // ms to debounce state changes (default: 100)
}

const DEFAULT_PROMPT_PATTERNS = [
  /\$\s*$/, // bash $ prompt
  /%\s*$/, // zsh % prompt
  />\s*$/, // generic > prompt (used by Claude)
  /#\s*$/, // root # prompt
  /❯\s*$/ // fancy prompt
]

export function createActivityStateDetector(
  onStateChange: (state: ActivityState) => void,
  config?: DetectorConfig
): {
  processData: (data: string) => void
  destroy: () => void
} {
  const promptPatterns = config?.promptPatterns ?? DEFAULT_PROMPT_PATTERNS
  const idleTimeout = config?.idleTimeout ?? 1000
  const debounceMs = config?.debounceMs ?? 100

  let buffer = ''
  const MAX_BUFFER_SIZE = 500

  let currentState: ActivityState = 'idle'
  let idleTimerId: ReturnType<typeof setTimeout> | null = null
  let debounceTimerId: ReturnType<typeof setTimeout> | null = null

  const emitState = (state: ActivityState) => {
    // Always clear pending debounce when trying to emit 'working'
    // This prevents a race where data arrives during a debounced transition
    // to 'user_input_required' or 'idle', which would incorrectly fire after the early return
    if (state === 'working' && debounceTimerId) {
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
    if (state === 'working') {
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

  const checkForPrompt = (): boolean => {
    // Strip ANSI escape sequences for cleaner matching
    const cleanBuffer = buffer.replace(/\x1b\[[0-9;]*[a-zA-Z]/g, '')

    for (const pattern of promptPatterns) {
      if (pattern.test(cleanBuffer)) {
        return true
      }
    }
    return false
  }

  const scheduleIdleCheck = () => {
    if (idleTimerId) {
      clearTimeout(idleTimerId)
    }

    idleTimerId = setTimeout(() => {
      // After 1 second of no activity, determine final state
      const hasPrompt = checkForPrompt()
      if (hasPrompt) {
        emitState('user_input_required')
      } else {
        emitState('idle')
      }
      idleTimerId = null
    }, idleTimeout)
  }

  const processData = (data: string) => {
    // Update buffer
    buffer += data
    if (buffer.length > MAX_BUFFER_SIZE) {
      buffer = buffer.slice(-MAX_BUFFER_SIZE)
    }

    // Any stream activity = working
    emitState('working')
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
