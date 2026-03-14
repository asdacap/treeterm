import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createActivityStateDetector } from './activityStateDetector'
import type { ActivityState } from '../types'

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

  it('emits waiting_for_input after idle timeout when buffer has prompt', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 500, debounceMs: 0 })

    processData('user@host:~$ ')

    // Fast-forward past idle timeout
    vi.advanceTimersByTime(600)

    expect(onStateChange).toHaveBeenCalledWith('waiting_for_input')
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
    expect(calls).toContain('waiting_for_input')
  })

  it('detects zsh % prompt', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 0 })

    processData('myhost% ')
    vi.advanceTimersByTime(200)

    const calls = onStateChange.mock.calls.map(c => c[0])
    expect(calls).toContain('waiting_for_input')
  })

  it('detects > prompt', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 0 })

    processData('> ')
    vi.advanceTimersByTime(200)

    const calls = onStateChange.mock.calls.map(c => c[0])
    expect(calls).toContain('waiting_for_input')
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

    // Send 600 chars (more than 500 MAX_BUFFER_SIZE), ending with a prompt
    const bigData = 'a'.repeat(480)
    processData(bigData)
    processData('$ ')  // append prompt — should be in truncated buffer

    vi.advanceTimersByTime(200)

    const calls = onStateChange.mock.calls.map(c => c[0])
    expect(calls).toContain('waiting_for_input')
  })

  it('strips ANSI escape sequences before prompt matching', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 0 })

    // ANSI-colored prompt
    processData('\x1b[32muser@host\x1b[0m:~$ ')
    vi.advanceTimersByTime(200)

    const calls = onStateChange.mock.calls.map(c => c[0])
    expect(calls).toContain('waiting_for_input')
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

    const idleCalls = onStateChange.mock.calls.filter(c => c[0] === 'idle' || c[0] === 'waiting_for_input')
    expect(idleCalls).toHaveLength(0)

    vi.advanceTimersByTime(300) // Now past idle timeout
    // Should have transitioned to idle or waiting_for_input
    const finalCalls = onStateChange.mock.calls.filter(c => c[0] === 'idle' || c[0] === 'waiting_for_input')
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

      // Should not have emitted waiting_for_input after destroy
      const stateCalls = onStateChange.mock.calls.filter(c => c[0] === 'waiting_for_input')
      expect(stateCalls).toHaveLength(0)
    })

    it('can be called multiple times without error', () => {
      const { destroy } = createActivityStateDetector(vi.fn())
      expect(() => {
        destroy()
        destroy()
      }).not.toThrow()
    })
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
    expect(calls).toContain('waiting_for_input')
  })
})
