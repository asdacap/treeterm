import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createActivityStateDetector } from './activityStateDetector'

describe('createActivityStateDetector', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('emits working immediately when data arrives', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange)

    processData('some output')

    expect(onStateChange).toHaveBeenCalledWith('working')
  })

  it('starts in idle state (no event emitted until data)', () => {
    const onStateChange = vi.fn()
    createActivityStateDetector(onStateChange)
    expect(onStateChange).not.toHaveBeenCalled()
  })

  it('emits user_input_required after idle timeout when buffer has prompt', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 500, debounceMs: 0 })

    processData('user@host:~$ ')

    // Fast-forward past idle timeout
    vi.advanceTimersByTime(600)

    expect(onStateChange).toHaveBeenCalledWith('user_input_required')
  })

  it('emits idle after idle timeout when no prompt is detected', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 500, debounceMs: 0 })

    processData('some random output without prompt')

    vi.advanceTimersByTime(600)

    expect(onStateChange).toHaveBeenCalledWith('idle')
  })

  it('detects bash $ prompt', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 0 })

    processData('user@host:~$ ')
    vi.advanceTimersByTime(200)

    const calls = onStateChange.mock.calls.map(c => c[0])
    expect(calls).toContain('user_input_required')
  })

  it('detects zsh % prompt', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 0 })

    processData('myhost% ')
    vi.advanceTimersByTime(200)

    const calls = onStateChange.mock.calls.map(c => c[0])
    expect(calls).toContain('user_input_required')
  })

  it('detects > prompt', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 0 })

    processData('> ')
    vi.advanceTimersByTime(200)

    const calls = onStateChange.mock.calls.map(c => c[0])
    expect(calls).toContain('user_input_required')
  })

  it('detects braille spinner as working indicator', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, {
      workingPatterns: [/[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]/],
      idleTimeout: 1000,
      debounceMs: 0,
    })

    // First process: marks working
    processData('⠋ loading...')
    expect(onStateChange).toHaveBeenCalledWith('working')
  })

  it('truncates buffer when data exceeds MAX_BUFFER_SIZE', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 0 })

    // Send >500 chars to trigger truncation, ending with a prompt
    processData('a'.repeat(510))
    processData('$ ')  // append prompt — should be in truncated buffer

    vi.advanceTimersByTime(200)

    const calls = onStateChange.mock.calls.map(c => c[0])
    expect(calls).toContain('user_input_required')
  })

  it('strips ANSI escape sequences before prompt matching', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 0 })

    // ANSI-colored prompt
    processData('\x1b[32muser@host\x1b[0m:~$ ')
    vi.advanceTimersByTime(200)

    const calls = onStateChange.mock.calls.map(c => c[0])
    expect(calls).toContain('user_input_required')
  })

  it('does not emit same state twice in a row', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 0 })

    processData('output1')
    processData('output2')

    // Should only be called once with 'working' since state doesn't change
    const workingCalls = onStateChange.mock.calls.filter(c => c[0] === 'working')
    expect(workingCalls).toHaveLength(1)
  })

  it('resets idle timer on each new data chunk', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 500, debounceMs: 0 })

    processData('output1')
    vi.advanceTimersByTime(300) // Not yet idle

    processData('output2') // Reset timer
    vi.advanceTimersByTime(300) // Still not idle (300 < 500)

    const idleCalls = onStateChange.mock.calls.filter(c => c[0] === 'idle' || c[0] === 'user_input_required')
    expect(idleCalls).toHaveLength(0)

    vi.advanceTimersByTime(300) // Now past idle timeout
    // Should have transitioned to idle or user_input_required
    const finalCalls = onStateChange.mock.calls.filter(c => c[0] === 'idle' || c[0] === 'user_input_required')
    expect(finalCalls.length).toBeGreaterThan(0)
  })

  describe('destroy', () => {
    it('clears timers without throwing', () => {
      const onStateChange = vi.fn()
      const { processData, destroy } = createActivityStateDetector(onStateChange, { idleTimeout: 1000 })

      processData('some data')
      expect(() => destroy()).not.toThrow()
    })

    it('stops idle transitions after destroy', () => {
      const onStateChange = vi.fn()
      const { processData, destroy } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 0 })

      processData('user@host:~$ ')
      destroy()

      vi.advanceTimersByTime(500)

      // Should not have emitted user_input_required after destroy
      const stateCalls = onStateChange.mock.calls.filter(c => c[0] === 'user_input_required')
      expect(stateCalls).toHaveLength(0)
    })

    it('clears debounce timer on destroy', () => {
      const onStateChange = vi.fn()
      const { processData, destroy } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 500 })

      processData('user@host:~$ ')
      vi.advanceTimersByTime(150) // idle timer fires, starts debounce for user_input_required
      destroy() // should clear debounce timer

      vi.advanceTimersByTime(600) // past debounce period
      const waitingCalls = onStateChange.mock.calls.filter(c => c[0] === 'user_input_required')
      expect(waitingCalls).toHaveLength(0)
    })

    it('can be called multiple times without error', () => {
      const { destroy } = createActivityStateDetector(vi.fn())
      expect(() => {
        destroy()
        destroy()
      }).not.toThrow()
    })
  })

  it('detects root # prompt as user_input_required', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 0 })

    processData('root@host:~# ')
    vi.advanceTimersByTime(200)

    const calls = onStateChange.mock.calls.map(c => c[0])
    expect(calls).toContain('user_input_required')
  })

  it('detects fancy \u276f prompt as user_input_required', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 0 })

    processData('~/project \u276f ')
    vi.advanceTimersByTime(200)

    const calls = onStateChange.mock.calls.map(c => c[0])
    expect(calls).toContain('user_input_required')
  })

  it('rapid data during debounce cancels pending idle transition', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 50 })

    processData('user@host:~$ ')
    vi.advanceTimersByTime(150) // past idle timeout, debounce starts

    // New data arrives during debounce period — should cancel it
    processData('more output')
    vi.advanceTimersByTime(10)

    // Should have gone back to 'working', not 'user_input_required'
    const lastCall = onStateChange.mock.calls[onStateChange.mock.calls.length - 1][0]
    expect(lastCall).toBe('working')
  })

  it('working state clears pending debounce', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 200 })

    processData('user@host:~$ ')
    vi.advanceTimersByTime(150) // idle timer fires, debounce for user_input_required starts

    processData('new data') // working should cancel debounce

    // Advance past the original debounce period
    vi.advanceTimersByTime(250)

    // The last state should not be user_input_required since working cleared it
    const waitingCalls = onStateChange.mock.calls.filter(c => c[0] === 'user_input_required')
    expect(waitingCalls).toHaveLength(0)
  })

  it('detects all braille spinner chars as working', () => {
    const spinners = ['\u280b', '\u2819', '\u2839', '\u2838', '\u283c', '\u2834', '\u2826', '\u2827', '\u2807', '\u280f']
    for (const spinner of spinners) {
      const onStateChange = vi.fn()
      const { processData } = createActivityStateDetector(onStateChange, {
        workingPatterns: [/[\u280b\u2819\u2839\u2838\u283c\u2834\u2826\u2827\u2807\u280f]/],
        idleTimeout: 1000,
        debounceMs: 0,
      })
      processData(`${spinner} loading...`)
      expect(onStateChange).toHaveBeenCalledWith('working')
    }
  })

  it('uses custom prompt patterns when provided', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, {
      promptPatterns: [/READY>/],
      idleTimeout: 100,
      debounceMs: 0,
    })

    processData('READY>')
    vi.advanceTimersByTime(200)

    const calls = onStateChange.mock.calls.map(c => c[0])
    expect(calls).toContain('user_input_required')
  })
})
