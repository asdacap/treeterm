import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { withTimeout } from './withTimeout'

describe('withTimeout', () => {
  beforeEach(() => { vi.useFakeTimers() })
  afterEach(() => { vi.useRealTimers() })

  it('passes through a resolved value', async () => {
    await expect(withTimeout(Promise.resolve('ok'), 1000, 'op')).resolves.toBe('ok')
  })

  it('passes through a rejection without timing out', async () => {
    await expect(withTimeout(Promise.reject(new Error('boom')), 1000, 'op')).rejects.toThrow('boom')
  })

  it('rejects with a labelled message when the inner promise never settles', async () => {
    const pending = new Promise<string>(() => undefined)
    const result = withTimeout(pending, 5000, 'resolveHomedir')
    const assertion = expect(result).rejects.toThrow('resolveHomedir timed out after 5000ms')
    await vi.advanceTimersByTimeAsync(5000)
    await assertion
  })

  it('invokes onTimeout cleanup when it times out', async () => {
    const onTimeout = vi.fn()
    const pending = new Promise<string>(() => undefined)
    const result = withTimeout(pending, 1000, 'op', onTimeout)
    const assertion = expect(result).rejects.toThrow('timed out')
    await vi.advanceTimersByTimeAsync(1000)
    await assertion
    expect(onTimeout).toHaveBeenCalledTimes(1)
  })

  it('does not invoke onTimeout when the promise resolves first', async () => {
    const onTimeout = vi.fn()
    await expect(withTimeout(Promise.resolve(42), 1000, 'op', onTimeout)).resolves.toBe(42)
    await vi.advanceTimersByTimeAsync(2000)
    expect(onTimeout).not.toHaveBeenCalled()
  })

  it('wraps a non-Error rejection into an Error', async () => {
    // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors -- exercising the non-Error path
    await expect(withTimeout(Promise.reject('string-error'), 1000, 'op')).rejects.toThrow('string-error')
  })
})
