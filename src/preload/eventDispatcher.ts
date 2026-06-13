// Generic event dispatcher that buffers events arriving before a listener subscribes.
//
// The renderer runs a command in two steps over separate, unordered IPC channels:
//   1. `start` (an invoke) returns a main-generated id
//   2. the caller then subscribes to that id for stream events
// A fast command can have its events broadcast before the `start` reply resolves — i.e. before
// the caller subscribes. Without buffering, those early events (including the terminal Exit/Error)
// are dropped and the awaiting promise never settles. Buffering them per-id and flushing on
// subscribe closes that race.

export interface EventDispatcher<E> {
  /** Deliver an event for `id`, or buffer it if no listener has subscribed yet. */
  dispatch: (id: string, event: E, isTerminal: (e: E) => boolean) => void
  /** Subscribe to events for `id`. Flushes any buffered events to the new callback. */
  subscribe: (id: string, cb: (e: E) => void) => () => void
}

interface BufferedEvents<E> {
  events: E[]
  /** True once a terminal (Exit/Error/End) event has been buffered for this id. */
  terminated: boolean
  /** Evicts the buffer if no listener ever subscribes — otherwise it would leak forever. */
  evictionTimer: ReturnType<typeof setTimeout>
}

/** How long a buffered id may wait for a subscriber before its events are evicted. */
export const BUFFER_EVICTION_MS = 60_000

export function createEventDispatcher<E>(): EventDispatcher<E> {
  const listeners = new Map<string, ((e: E) => void)[]>()
  const buffer = new Map<string, BufferedEvents<E>>()

  return {
    dispatch: (id, event, isTerminal) => {
      const subscribers = listeners.get(id)
      if (subscribers && subscribers.length > 0) {
        subscribers.forEach((cb) => { cb(event); })
        if (isTerminal(event)) listeners.delete(id)
        return
      }
      // No listener yet — buffer until one subscribes so the event isn't lost.
      const buffered = buffer.get(id)
      if (buffered) {
        buffered.events.push(event)
        if (isTerminal(event)) buffered.terminated = true
      } else {
        buffer.set(id, {
          events: [event],
          terminated: isTerminal(event),
          evictionTimer: setTimeout(() => { buffer.delete(id); }, BUFFER_EVICTION_MS),
        })
      }
    },

    subscribe: (id, cb) => {
      const existing = listeners.get(id)
      if (existing) existing.push(cb)
      else listeners.set(id, [cb])

      // Flush events that arrived before this listener registered.
      const buffered = buffer.get(id)
      if (buffered) {
        clearTimeout(buffered.evictionTimer)
        buffer.delete(id)
        for (const event of buffered.events) cb(event)
        // Mirror the live path: once the terminal event has been delivered, drop the listener set.
        // Don't rely on the caller unsubscribing — during a synchronous flush its unsub closure may
        // not be assigned yet.
        if (buffered.terminated) listeners.delete(id)
      }

      return () => {
        const subscribers = listeners.get(id)
        if (!subscribers) return
        const index = subscribers.indexOf(cb)
        if (index > -1) subscribers.splice(index, 1)
      }
    },
  }
}
