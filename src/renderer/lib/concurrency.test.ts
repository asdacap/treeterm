import { describe, it, expect } from 'vitest'
import { mapWithConcurrency, coalesceByKey } from './concurrency'

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void; reject: (error: unknown) => void } {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej })
  return { promise, resolve, reject }
}

const flush = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0))

describe('mapWithConcurrency', () => {
  it('never exceeds the limit and preserves order', async () => {
    let active = 0
    let peak = 0
    const result = await mapWithConcurrency([1, 2, 3, 4, 5, 6, 7], 3, async (n) => {
      active++
      peak = Math.max(peak, active)
      await flush()
      active--
      return n * 10
    })
    expect(result).toEqual([10, 20, 30, 40, 50, 60, 70])
    expect(peak).toBe(3)
  })

  it('handles an empty list', async () => {
    expect(await mapWithConcurrency([], 4, () => Promise.resolve(1))).toEqual([])
  })

  it('rejects with the error and stops starting new items', async () => {
    const started: number[] = []
    await expect(mapWithConcurrency([1, 2, 3, 4, 5], 1, (n) => {
      started.push(n)
      return n === 2 ? Promise.reject(new Error('boom')) : Promise.resolve(n)
    })).rejects.toThrow('boom')
    expect(started).toEqual([1, 2])
  })

  it('rejects an invalid limit', async () => {
    await expect(mapWithConcurrency([1], 0, () => Promise.resolve(1))).rejects.toThrow('positive integer')
  })
})

describe('coalesceByKey', () => {
  it('runs once per key while idle', async () => {
    let calls = 0
    const fn = coalesceByKey((key: string) => { calls++; return Promise.resolve(key) })
    expect(await fn('a')).toBe('a')
    expect(await fn('a')).toBe('a')
    expect(calls).toBe(2)
  })

  it('queues a single follow-up run for callers that arrive mid-flight', async () => {
    const pending: Array<ReturnType<typeof deferred<number>>> = []
    const fn = coalesceByKey(() => { const d = deferred<number>(); pending.push(d); return d.promise })

    const first = fn('repo')
    const second = fn('repo')
    const third = fn('repo')
    expect(pending).toHaveLength(1)
    expect(second).toBe(third)

    pending[0]!.resolve(1)
    expect(await first).toBe(1)
    await flush()
    expect(pending).toHaveLength(2)

    pending[1]!.resolve(2)
    expect(await second).toBe(2)
    expect(await third).toBe(2)
  })

  it('keeps keys independent', () => {
    const keys: string[] = []
    const fn = coalesceByKey((key: string) => { keys.push(key); return new Promise<string>(() => undefined) })
    void fn('a')
    void fn('b')
    expect(keys).toEqual(['a', 'b'])
  })

  it('still runs the queued call after the running one rejects', async () => {
    const pending: Array<ReturnType<typeof deferred<number>>> = []
    const fn = coalesceByKey(() => { const d = deferred<number>(); pending.push(d); return d.promise })
    const first = fn('repo')
    const second = fn('repo')
    pending[0]!.reject(new Error('fail'))
    await expect(first).rejects.toThrow('fail')
    await flush()
    pending[1]!.resolve(5)
    expect(await second).toBe(5)
  })

  it('starts fresh after everything settles', async () => {
    let calls = 0
    const fn = coalesceByKey(() => { calls++; return Promise.resolve(calls) })
    await fn('repo')
    await flush()
    expect(await fn('repo')).toBe(2)
  })
})
