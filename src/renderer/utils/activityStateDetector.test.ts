/* eslint-disable custom/no-string-literal-comparison -- test fixtures */
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

  it('emits idle after idle timeout', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 500, debounceMs: 0 })

    processData('some output')

    vi.advanceTimersByTime(600)

    expect(onStateChange).toHaveBeenCalledWith('idle')
  })

  it('does not emit same state twice in a row', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 0 })

    processData('output1')
    processData('output2')

    // Should only be called once with 'working' since state doesn't change
    const workingCalls = onStateChange.mock.calls.filter((c: unknown[]) => c[0] === 'working')
    expect(workingCalls).toHaveLength(1)
  })

  it('resets idle timer on each new data chunk', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 500, debounceMs: 0 })

    processData('output1')
    vi.advanceTimersByTime(300) // Not yet idle

    processData('output2') // Reset timer
    vi.advanceTimersByTime(300) // Still not idle (300 < 500)

    const idleCalls = onStateChange.mock.calls.filter((c: unknown[]) => c[0] === 'idle')
    expect(idleCalls).toHaveLength(0)

    vi.advanceTimersByTime(300) // Now past idle timeout
    const finalCalls = onStateChange.mock.calls.filter((c: unknown[]) => c[0] === 'idle')
    expect(finalCalls.length).toBeGreaterThan(0)
  })

  it('ignores identical snapshots: emits working once then idles despite repeated repaints', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 500, debounceMs: 0 })

    // A TUI repainting the exact same screen every 100ms.
    processData('same screen')
    for (let t = 0; t < 500; t += 100) {
      vi.advanceTimersByTime(100)
      processData('same screen') // identical — no visible change
    }
    vi.advanceTimersByTime(50) // flush the (debounced) idle emit past the timeout boundary

    // Working emitted exactly once (the first, changed frame); the repeats never reset the timer.
    const workingCalls = onStateChange.mock.calls.filter((c: unknown[]) => c[0] === 'working')
    expect(workingCalls).toHaveLength(1)

    // Timer was never reset by the identical repaints, so it has fired.
    const idleCalls = onStateChange.mock.calls.filter((c: unknown[]) => c[0] === 'idle')
    expect(idleCalls.length).toBeGreaterThan(0)
  })

  it('a changed snapshot after identical repaints re-emits working and resets the timer', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 500, debounceMs: 0 })

    processData('frame a')
    vi.advanceTimersByTime(600) // idle
    expect(onStateChange.mock.calls.filter((c: unknown[]) => c[0] === 'idle')).toHaveLength(1)

    processData('frame b') // different content — real activity
    const workingCalls = onStateChange.mock.calls.filter((c: unknown[]) => c[0] === 'working')
    expect(workingCalls).toHaveLength(2)
  })

  it('stays idle when identical repaints keep arriving after it has settled', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 0 })

    processData('static')
    vi.advanceTimersByTime(150) // settle to idle
    onStateChange.mockClear()

    // Identical repaints must not resurrect Working.
    processData('static')
    vi.advanceTimersByTime(200)
    processData('static')
    vi.advanceTimersByTime(200)

    expect(onStateChange).not.toHaveBeenCalled()
  })

  describe('destroy', () => {
    it('clears timers without throwing', () => {
      const onStateChange = vi.fn()
      const { processData, destroy } = createActivityStateDetector(onStateChange, { idleTimeout: 1000 })

      processData('some data')
      expect(() => { destroy(); }).not.toThrow()
    })

    it('stops idle transitions after destroy', () => {
      const onStateChange = vi.fn()
      const { processData, destroy } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 0 })

      processData('some data')
      destroy()

      vi.advanceTimersByTime(500)

      // Should not have emitted idle after destroy
      const idleCalls = onStateChange.mock.calls.filter((c: unknown[]) => c[0] === 'idle')
      expect(idleCalls).toHaveLength(0)
    })

    it('clears debounce timer on destroy', () => {
      const onStateChange = vi.fn()
      const { processData, destroy } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 500 })

      processData('some data')
      vi.advanceTimersByTime(150) // idle timer fires, starts debounce for idle
      destroy() // should clear debounce timer

      vi.advanceTimersByTime(600) // past debounce period
      const idleCalls = onStateChange.mock.calls.filter((c: unknown[]) => c[0] === 'idle')
      expect(idleCalls).toHaveLength(0)
    })

    it('can be called multiple times without error', () => {
      const { destroy } = createActivityStateDetector(vi.fn())
      expect(() => {
        destroy()
        destroy()
      }).not.toThrow()
    })
  })

  it('rapid data during debounce cancels pending idle transition', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 50 })

    processData('some data')
    vi.advanceTimersByTime(150) // past idle timeout, debounce starts

    // New data arrives during debounce period — should cancel it
    processData('more output')
    vi.advanceTimersByTime(10)

    // Should have gone back to 'working', not 'idle'
    const lastCall = onStateChange.mock.calls[onStateChange.mock.calls.length - 1]![0] as string
    expect(lastCall).toBe('working')
  })

  it('working state clears pending debounce', () => {
    const onStateChange = vi.fn()
    const { processData } = createActivityStateDetector(onStateChange, { idleTimeout: 100, debounceMs: 200 })

    processData('some data')
    vi.advanceTimersByTime(150) // idle timer fires, debounce for idle starts

    processData('new data') // working should cancel debounce

    // Advance past the original debounce period
    vi.advanceTimersByTime(250)

    // The idle should not have been emitted since working cleared the debounce
    // (a new idle timer was started, but we haven't waited long enough for it)
    const idleCalls = onStateChange.mock.calls.filter((c: unknown[]) => c[0] === 'idle')
    expect(idleCalls).toHaveLength(0)
  })
})
