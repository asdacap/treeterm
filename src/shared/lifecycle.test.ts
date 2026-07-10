import { describe, it, expect, vi, afterEach } from 'vitest'
import {
  DisposableMap,
  DisposableStore,
  toDisposable,
  thenRegisterOrDispose,
  setDisposableTracker,
  clearDisposableTracker,
  createLeakTrackingDisposableTracker,
} from './lifecycle'
import type { IDisposable } from './lifecycle'

afterEach(() => { clearDisposableTracker() })

describe('toDisposable', () => {
  it('runs the cleanup on dispose', () => {
    const fn = vi.fn()
    toDisposable(fn).dispose()
    expect(fn).toHaveBeenCalledTimes(1)
  })

  it('runs the cleanup at most once, so two owners cannot double-free', () => {
    const fn = vi.fn()
    const disposable = toDisposable(fn)
    disposable.dispose()
    disposable.dispose()
    expect(fn).toHaveBeenCalledTimes(1)
  })
})

describe('DisposableStore', () => {
  it('disposes everything it owns', () => {
    const store = new DisposableStore()
    const a = vi.fn()
    const b = vi.fn()
    store.add(toDisposable(a))
    store.add(toDisposable(b))

    store.dispose()

    expect(a).toHaveBeenCalledTimes(1)
    expect(b).toHaveBeenCalledTimes(1)
  })

  it('is idempotent', () => {
    const store = new DisposableStore()
    const fn = vi.fn()
    store.add(toDisposable(fn))

    store.dispose()
    store.dispose()

    expect(fn).toHaveBeenCalledTimes(1)
  })

  // The reason ownership inversion fixes the acquire-vs-dispose race: a resource that
  // lands after its owner is gone has exactly one place to go.
  it('immediately disposes anything added after it was disposed', () => {
    const store = new DisposableStore()
    store.dispose()

    const late = vi.fn()
    store.add(toDisposable(late))

    expect(late).toHaveBeenCalledTimes(1)
    expect(store.isDisposed).toBe(true)
  })

  it('reports isDisposed so late acquirers can skip publishing a dead handle', () => {
    const store = new DisposableStore()
    expect(store.isDisposed).toBe(false)
    store.dispose()
    expect(store.isDisposed).toBe(true)
  })

  it('rejects adding itself', () => {
    const store = new DisposableStore()
    expect(() => store.add(store)).toThrow(/itself/)
  })

  it('returns the disposable it was handed, so callers can keep using it', () => {
    const store = new DisposableStore()
    const disposable = toDisposable(() => {})
    expect(store.add(disposable)).toBe(disposable)
  })

  // "Fail Loudly" (AGENTS.md) — a throwing disposable must not strand its siblings,
  // and must not be swallowed either.
  it('disposes every sibling even when one throws, then rethrows', () => {
    const store = new DisposableStore()
    const after = vi.fn()
    store.add(toDisposable(() => { throw new Error('teardown blew up') }))
    store.add(toDisposable(after))

    expect(() => { store.dispose() }).toThrow('teardown blew up')
    expect(after).toHaveBeenCalledTimes(1)
  })
})

describe('DisposableMap', () => {
  it('disposes values it evicts, so a stale resource cannot survive replacement', () => {
    const map = new DisposableMap<string, IDisposable>()
    const first = vi.fn()
    map.set('k', toDisposable(first))

    map.set('k', toDisposable(() => {}))

    expect(first).toHaveBeenCalledTimes(1)
  })

  it('deleteAndDispose releases the value', () => {
    const map = new DisposableMap<string, IDisposable>()
    const fn = vi.fn()
    map.set('k', toDisposable(fn))

    map.deleteAndDispose('k')

    expect(fn).toHaveBeenCalledTimes(1)
    expect(map.get('k')).toBeUndefined()
  })

  it('disposes everything it still holds', () => {
    const map = new DisposableMap<string, IDisposable>()
    const a = vi.fn()
    const b = vi.fn()
    map.set('a', toDisposable(a))
    map.set('b', toDisposable(b))

    map.dispose()

    expect(a).toHaveBeenCalledTimes(1)
    expect(b).toHaveBeenCalledTimes(1)
  })

  // Same acquire-vs-dispose race as DisposableStore, for the keyed case.
  it('immediately disposes a value set after it was disposed', () => {
    const map = new DisposableMap<string, IDisposable>()
    map.dispose()

    const late = vi.fn()
    map.set('k', toDisposable(late))

    expect(late).toHaveBeenCalledTimes(1)
  })
})

describe('thenRegisterOrDispose', () => {
  it('registers a resource that lands while its owner is alive', async () => {
    const owner = new DisposableStore()
    const fn = vi.fn()

    await thenRegisterOrDispose(Promise.resolve(toDisposable(fn)), owner)
    expect(fn).not.toHaveBeenCalled()

    owner.dispose()
    expect(fn).toHaveBeenCalledTimes(1)
  })

  it('disposes a resource that lands after its owner died', async () => {
    const owner = new DisposableStore()
    const fn = vi.fn()
    owner.dispose()

    await thenRegisterOrDispose(Promise.resolve(toDisposable(fn)), owner)

    expect(fn).toHaveBeenCalledTimes(1)
  })
})

describe('leak tracking', () => {
  it('reports a disposable that was created but never disposed', () => {
    const tracker = createLeakTrackingDisposableTracker()
    setDisposableTracker(tracker)

    toDisposable(() => {})

    expect(tracker.getLiveStacks()).toHaveLength(1)
  })

  it('reports nothing once everything is disposed', () => {
    const tracker = createLeakTrackingDisposableTracker()
    setDisposableTracker(tracker)

    const store = new DisposableStore()
    store.add(toDisposable(() => {}))
    store.dispose()

    expect(tracker.getLiveStacks()).toEqual([])
  })

  it('counts a store owned by nobody as leaked', () => {
    const tracker = createLeakTrackingDisposableTracker()
    setDisposableTracker(tracker)

    new DisposableStore()

    expect(tracker.getLiveStacks()).toHaveLength(1)
  })

  it('tracks nothing once uninstalled', () => {
    const tracker = createLeakTrackingDisposableTracker()
    setDisposableTracker(tracker)
    clearDisposableTracker()

    toDisposable(() => {})

    expect(tracker.getLiveStacks()).toEqual([])
  })
})
