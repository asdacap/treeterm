import type { ActivityState } from '../types'

interface DetectorConfig {
  promptPatterns?: RegExp[] // Patterns indicating "waiting for input"
  workingPatterns?: RegExp[] // Patterns indicating "working" (e.g., spinners)
  idleTimeout?: number // ms of no activity before confirming state (default: 300)
  debounceMs?: number // ms to debounce state changes (default: 100)
}

const DEFAULT_PROMPT_PATTERNS = [
  /\$\s*$/, // bash $ prompt
  /%\s*$/, // zsh % prompt
  />\s*$/, // generic > prompt (used by Claude)
  /#\s*$/, // root # prompt
  /❯\s*$/ // fancy prompt
]

const DEFAULT_WORKING_PATTERNS = [
  /[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]/ // Braille spinners
]

export function createActivityStateDetector(
  onStateChange: (state: ActivityState) => void,
  config?: DetectorConfig
): {
  processData: (data: string) => void
  destroy: () => void
} {
  const promptPatterns = config?.promptPatterns ?? DEFAULT_PROMPT_PATTERNS
  const workingPatterns = config?.workingPatterns ?? DEFAULT_WORKING_PATTERNS
  const idleTimeout = config?.idleTimeout ?? 300
  const debounceMs = config?.debounceMs ?? 100

  let buffer = ''
  const MAX_BUFFER_SIZE = 500

  let currentState: ActivityState = 'idle'
  let lastActivityTime = Date.now()
  let idleTimerId: ReturnType<typeof setTimeout> | null = null
  let debounceTimerId: ReturnType<typeof setTimeout> | null = null

  const emitState = (state: ActivityState) => {
    if (state === currentState) return

    // Clear any pending debounce
    if (debounceTimerId) {
      clearTimeout(debounceTimerId)
      debounceTimerId = null
    }

    // Debounce state changes to prevent flickering
    debounceTimerId = setTimeout(() => {
      if (state !== currentState) {
        currentState = state
        onStateChange(state)
      }
      debounceTimerId = null
    }, debounceMs)
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

  const checkForWorking = (data: string): boolean => {
    for (const pattern of workingPatterns) {
      if (pattern.test(data)) {
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
      // After idle timeout, check if we're at a prompt
      if (checkForPrompt()) {
        emitState('waiting_for_input')
      }
      idleTimerId = null
    }, idleTimeout)
  }

  const processData = (data: string) => {
    lastActivityTime = Date.now()

    // Update buffer
    buffer += data
    if (buffer.length > MAX_BUFFER_SIZE) {
      buffer = buffer.slice(-MAX_BUFFER_SIZE)
    }

    // Check for working indicators (spinners)
    if (checkForWorking(data)) {
      emitState('working')
      scheduleIdleCheck()
      return
    }

    // Check for prompt at end of buffer
    if (checkForPrompt()) {
      // Schedule idle check - if no more output, we're waiting for input
      scheduleIdleCheck()
    } else {
      // Activity but no prompt - likely working
      emitState('working')
      scheduleIdleCheck()
    }
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
