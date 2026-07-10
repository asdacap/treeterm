/*---------------------------------------------------------------------------------------
 *  Derived from Microsoft VS Code, `src/vs/base/common/lifecycle.ts`.
 *  Copyright (c) Microsoft Corporation. Licensed under the MIT License.
 *  https://github.com/microsoft/vscode/blob/main/LICENSE.txt
 *
 *  Vendored rather than depended upon: `vs/base/common` is internal to the VS Code
 *  monorepo and is not published to npm. It is trimmed to what this codebase uses, and
 *  diverges deliberately — see the notes on the tracker and on `dispose()` vs `kill()`.
 *--------------------------------------------------------------------------------------*/

/**
 * Minimal disposable primitives, modelled on VS Code's `vs/base/common/lifecycle`.
 *
 * The value here is not the `dispose()` method — much of this codebase already had
 * one. It is that a cleanup becomes a *value with a name*, which gives tooling
 * something to grab:
 *
 *   - `custom/no-discarded-disposable` fails lint when one is dropped on the floor.
 *   - `setDisposableTracker` fails tests when one is never disposed.
 *
 * A bare `() => void` gives neither of those any purchase — it is shaped exactly like
 * every other callback in the codebase.
 *
 * IMPORTANT: `dispose()` releases a *local* handle. It is never the verb for
 * destroying a daemon-side resource — that is `AppRef.close()`. See "Unmount is not
 * close" in AGENTS.md. Collapsing the two verbs is how PTYs get orphaned.
 */

export interface IDisposable {
  dispose(): void
}

/**
 * Observes disposable creation and disposal. Off by default via a no-op impl rather
 * than a nullable global, so the hot path carries no branch.
 */
export interface IDisposableTracker {
  trackDisposable(disposable: IDisposable): void
  markAsDisposed(disposable: IDisposable): void
}

const NO_TRACKING: IDisposableTracker = {
  trackDisposable: () => {},
  markAsDisposed: () => {},
}

let tracker: IDisposableTracker = NO_TRACKING

export function setDisposableTracker(next: IDisposableTracker): void {
  tracker = next
}

export function clearDisposableTracker(): void {
  tracker = NO_TRACKING
}

/**
 * Wrap a cleanup function as an owned resource. Disposal is idempotent: the wrapped
 * function runs at most once, so a resource handed to two owners cannot double-free.
 */
export function toDisposable(fn: () => void): IDisposable {
  let disposed = false
  const disposable: IDisposable = {
    dispose: () => {
      if (disposed) return
      disposed = true
      tracker.markAsDisposed(disposable)
      fn()
    },
  }
  tracker.trackDisposable(disposable)
  return disposable
}

/**
 * Owns a set of disposables and releases them together.
 *
 * Pass a store *into* an async acquire rather than returning cleanup out of it. Then
 * there is nothing for the caller to drop, and nothing a narrowed dependency type can
 * silently erase — which is exactly how the analyzer's TTY subscription leaked.
 */
export class DisposableStore implements IDisposable {
  private readonly items = new Set<IDisposable>()
  private disposed = false

  constructor() {
    tracker.trackDisposable(this)
  }

  get isDisposed(): boolean {
    return this.disposed
  }

  /**
   * Take ownership of `disposable`, returning it for convenience.
   *
   * If this store is already disposed, `disposable` is disposed immediately instead of
   * retained. That is deliberate, and it is the whole reason the async acquire-vs-dispose
   * race disappears: a resource that finishes being acquired *after* its owner is gone
   * has exactly one place to go. This replaces the three hand-rolled `cancelled` flags
   * and in-flight `creating` promises that previously guarded this race.
   */
  add<T extends IDisposable>(disposable: T): T {
    if ((disposable as IDisposable) === this) {
      throw new Error('Cannot add a DisposableStore to itself')
    }
    if (this.disposed) {
      disposable.dispose()
      return disposable
    }
    this.items.add(disposable)
    return disposable
  }

  /**
   * Errors are collected so one bad disposable cannot strand the rest, then rethrown —
   * teardown failures are bugs and must not be swallowed ("Fail Loudly").
   */
  dispose(): void {
    if (this.disposed) return
    this.disposed = true
    tracker.markAsDisposed(this)
    const errors: unknown[] = []
    this.items.forEach((item) => {
      try {
        item.dispose()
      } catch (error) {
        errors.push(error)
      }
    })
    this.items.clear()
    if (errors.length > 0) throw errors[0]
  }
}

/**
 * Register an asynchronously-acquired resource with its owner, disposing it instead if
 * the owner died while the acquire was in flight.
 *
 * This is the whole acquire-vs-dispose race, in one place. It replaces the `cancelled`
 * flags and in-flight `creating` promises that previously guarded it by hand.
 */
export function thenRegisterOrDispose<T extends IDisposable>(
  promise: Promise<T>,
  owner: DisposableStore,
): Promise<T> {
  return promise.then((disposable) => owner.add(disposable))
}

/**
 * A map that owns its values. `set` over an existing key disposes the value it evicts,
 * so a stale resource cannot survive its own replacement.
 */
export class DisposableMap<K, V extends IDisposable> implements IDisposable {
  private readonly items = new Map<K, V>()
  private disposed = false

  constructor() {
    tracker.trackDisposable(this)
  }

  get(key: K): V | undefined {
    return this.items.get(key)
  }

  /** Takes ownership of `value`. Disposes it immediately if this map is already disposed. */
  set(key: K, value: V): V {
    if (this.disposed) {
      value.dispose()
      return value
    }
    this.items.get(key)?.dispose()
    this.items.set(key, value)
    return value
  }

  deleteAndDispose(key: K): void {
    this.items.get(key)?.dispose()
    this.items.delete(key)
  }

  dispose(): void {
    if (this.disposed) return
    this.disposed = true
    tracker.markAsDisposed(this)
    const errors: unknown[] = []
    this.items.forEach((item) => {
      try {
        item.dispose()
      } catch (error) {
        errors.push(error)
      }
    })
    this.items.clear()
    if (errors.length > 0) throw errors[0]
  }
}

export interface LeakTrackingDisposableTracker extends IDisposableTracker {
  /** Creation stacks of every disposable created but not yet disposed. */
  getLiveStacks(): string[]
}

/**
 * Records a creation stack per live disposable and drops it on disposal. Anything left
 * at the end of a test was leaked.
 *
 * VS Code's equivalent hangs off `FinalizationRegistry` and reports on GC, which is
 * non-deterministic and therefore useless as a CI gate. Tracking liveness explicitly
 * costs a Map and gives an assertion you can actually fail a build on.
 */
export function createLeakTrackingDisposableTracker(): LeakTrackingDisposableTracker {
  const live = new Map<IDisposable, string>()
  return {
    trackDisposable: (disposable) => {
      live.set(disposable, new Error('Disposable created here').stack ?? '<no stack>')
    },
    markAsDisposed: (disposable) => {
      live.delete(disposable)
    },
    getLiveStacks: () => Array.from(live.values()),
  }
}
