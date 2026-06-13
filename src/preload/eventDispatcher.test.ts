/* eslint-disable custom/no-string-literal-comparison -- synthetic test event type, not a domain enum */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createEventDispatcher, BUFFER_EVICTION_MS } from './eventDispatcher'

interface TestEvent {
  type: 'data' | 'end'
  value?: string
}

const isTerminal = (e: TestEvent): boolean => e.type === 'end'

describe('createEventDispatcher', () => {
  it('buffers events that arrive before a listener subscribes, then flushes them in order', () => {
    const dispatcher = createEventDispatcher<TestEvent>()

    // Events arrive before anyone subscribes (the race that caused the worktree-tab hang).
    dispatcher.dispatch('id1', { type: 'data', value: 'a' }, isTerminal)
    dispatcher.dispatch('id1', { type: 'data', value: 'b' }, isTerminal)
    dispatcher.dispatch('id1', { type: 'end' }, isTerminal)

    const received: TestEvent[] = []
    dispatcher.subscribe('id1', (e) => received.push(e))

    expect(received).toEqual([
      { type: 'data', value: 'a' },
      { type: 'data', value: 'b' },
      { type: 'end' },
    ])
  })

  it('delivers events live when subscribed before dispatch, with no double-delivery', () => {
    const dispatcher = createEventDispatcher<TestEvent>()
    const received: TestEvent[] = []

    dispatcher.subscribe('id1', (e) => received.push(e))
    dispatcher.dispatch('id1', { type: 'data', value: 'x' }, isTerminal)
    dispatcher.dispatch('id1', { type: 'end' }, isTerminal)

    expect(received).toEqual([{ type: 'data', value: 'x' }, { type: 'end' }])
  })

  it('cleans up after a terminal event so a later dispatch is buffered, not delivered to the old listener', () => {
    const dispatcher = createEventDispatcher<TestEvent>()
    const first: TestEvent[] = []

    dispatcher.subscribe('id1', (e) => first.push(e))
    dispatcher.dispatch('id1', { type: 'end' }, isTerminal)

    // After terminal, the original listener set is gone — a stray dispatch must not reach it.
    dispatcher.dispatch('id1', { type: 'data', value: 'late' }, isTerminal)
    expect(first).toEqual([{ type: 'end' }])

    // A fresh subscriber gets the buffered stray event.
    const second: TestEvent[] = []
    dispatcher.subscribe('id1', (e) => second.push(e))
    expect(second).toEqual([{ type: 'data', value: 'late' }])
  })

  it('flushes a buffered terminal event and lets the subscriber unsubscribe inline', () => {
    const dispatcher = createEventDispatcher<TestEvent>()
    dispatcher.dispatch('id1', { type: 'end' }, isTerminal)

    const received: TestEvent[] = []
    let unsub: (() => void) | null = null
    unsub = dispatcher.subscribe('id1', (e) => {
      received.push(e)
      if (isTerminal(e)) unsub?.()
    })

    expect(received).toEqual([{ type: 'end' }])

    // The empty listener set was cleaned up: a subsequent dispatch buffers for a new subscriber.
    dispatcher.dispatch('id1', { type: 'data', value: 'after' }, isTerminal)
    const next: TestEvent[] = []
    dispatcher.subscribe('id1', (e) => next.push(e))
    expect(next).toEqual([{ type: 'data', value: 'after' }])
  })

  it('delivers to multiple listeners and unsubscribe removes only that callback', () => {
    const dispatcher = createEventDispatcher<TestEvent>()
    const a = vi.fn()
    const b = vi.fn()

    const unsubA = dispatcher.subscribe('id1', a)
    dispatcher.subscribe('id1', b)

    dispatcher.dispatch('id1', { type: 'data', value: '1' }, isTerminal)
    expect(a).toHaveBeenCalledTimes(1)
    expect(b).toHaveBeenCalledTimes(1)

    unsubA()
    dispatcher.dispatch('id1', { type: 'data', value: '2' }, isTerminal)
    expect(a).toHaveBeenCalledTimes(1)
    expect(b).toHaveBeenCalledTimes(2)
  })

  it('keeps separate buffers per id', () => {
    const dispatcher = createEventDispatcher<TestEvent>()
    dispatcher.dispatch('id1', { type: 'data', value: 'one' }, isTerminal)
    dispatcher.dispatch('id2', { type: 'data', value: 'two' }, isTerminal)

    const r1: TestEvent[] = []
    const r2: TestEvent[] = []
    dispatcher.subscribe('id1', (e) => r1.push(e))
    dispatcher.subscribe('id2', (e) => r2.push(e))

    expect(r1).toEqual([{ type: 'data', value: 'one' }])
    expect(r2).toEqual([{ type: 'data', value: 'two' }])
  })

  it('unsubscribe is a no-op when the id has no listeners', () => {
    const dispatcher = createEventDispatcher<TestEvent>()
    const unsub = dispatcher.subscribe('id1', () => undefined)
    unsub()
    expect(() => { unsub(); }).not.toThrow()
  })

  describe('buffer eviction', () => {
    beforeEach(() => { vi.useFakeTimers() })
    afterEach(() => { vi.useRealTimers() })

    it('evicts a buffer that never gets a subscriber after the TTL', () => {
      const dispatcher = createEventDispatcher<TestEvent>()
      dispatcher.dispatch('id1', { type: 'data', value: 'orphan' }, isTerminal)

      vi.advanceTimersByTime(BUFFER_EVICTION_MS)

      // A late subscriber gets nothing — the buffer was evicted.
      const received: TestEvent[] = []
      dispatcher.subscribe('id1', (e) => received.push(e))
      expect(received).toEqual([])
    })

    it('subscribing before the TTL cancels eviction and flushes the buffer', () => {
      const dispatcher = createEventDispatcher<TestEvent>()
      dispatcher.dispatch('id1', { type: 'data', value: 'kept' }, isTerminal)

      vi.advanceTimersByTime(BUFFER_EVICTION_MS - 1)
      const received: TestEvent[] = []
      dispatcher.subscribe('id1', (e) => received.push(e))
      expect(received).toEqual([{ type: 'data', value: 'kept' }])

      // Advancing past the original deadline must not affect live delivery.
      vi.advanceTimersByTime(BUFFER_EVICTION_MS)
      dispatcher.dispatch('id1', { type: 'data', value: 'live' }, isTerminal)
      expect(received).toEqual([{ type: 'data', value: 'kept' }, { type: 'data', value: 'live' }])
    })
  })
})
