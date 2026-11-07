/**
 * Map `items` through `fn` with at most `limit` calls in flight, preserving
 * input order in the result. Rejects with the first error, like Promise.all;
 * workers stop picking up new items once any call has failed.
 */
export async function mapWithConcurrency<T, R>(
  items: readonly T[],
  limit: number,
  fn: (item: T, index: number) => Promise<R>,
): Promise<R[]> {
  if (!Number.isInteger(limit) || limit < 1) throw new Error(`concurrency limit must be a positive integer, got ${String(limit)}`)
  const results = new Array<R>(items.length)
  let next = 0
  let failed = false
  const worker = async (): Promise<void> => {
    while (!failed && next < items.length) {
      const index = next++
      try {
        results[index] = await fn(items[index] as T, index)
      } catch (error) {
        failed = true
        throw error
      }
    }
  }
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker))
  return results
}

interface KeyedRun<R> {
  running: Promise<R>
  queued: Promise<R> | null
}

/**
 * Wrap `fn` so that, per key, at most one call runs at a time and at most one
 * more waits behind it. Callers arriving while a call is running all share the
 * single queued follow-up run, so every caller still gets a result computed
 * after it asked — never a stale in-flight one — without piling up work.
 */
export function coalesceByKey<K, R>(fn: (key: K) => Promise<R>): (key: K) => Promise<R> {
  const runs = new Map<K, KeyedRun<R>>()

  const track = (key: K, run: KeyedRun<R>, promise: Promise<R>): Promise<R> => {
    run.running = promise
    const settle = (): void => {
      if (run.running === promise && run.queued === null) runs.delete(key)
    }
    promise.then(settle, settle)
    return promise
  }

  return (key: K): Promise<R> => {
    const existing = runs.get(key)
    if (existing === undefined) {
      const promise = fn(key)
      const run: KeyedRun<R> = { running: promise, queued: null }
      runs.set(key, run)
      return track(key, run, promise)
    }
    if (existing.queued !== null) return existing.queued
    const ignore = (): void => undefined
    const queued = existing.running.then(ignore, ignore).then(() => {
      existing.queued = null
      return track(key, existing, fn(key))
    })
    existing.queued = queued
    return queued
  }
}
