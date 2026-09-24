import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createIdleDetector, idleTimeoutMs } from './idleDetector'
import type { Settings } from '../../shared/types'

function setup(timeout: () => number = () => 500): {
  onActivity: ReturnType<typeof vi.fn>
  onIdle: ReturnType<typeof vi.fn>
  detector: ReturnType<typeof createIdleDetector>
} {
  const onActivity = vi.fn()
  const onIdle = vi.fn()
  const detector = createIdleDetector({ initialSnapshot: '', idleTimeoutMs: timeout, onActivity, onIdle })
  return { onActivity, onIdle, detector }
}

/** Runs the silent first burst through to its idle so a resting state is known. */
function settle(h: ReturnType<typeof setup>): void {
  h.detector.processSnapshot('boot')
  vi.advanceTimersByTime(500)
  expect(h.onActivity).not.toHaveBeenCalled()
  expect(h.onIdle).toHaveBeenCalledTimes(1)
  h.onIdle.mockClear()
}

describe('createIdleDetector', () => {
  beforeEach(() => { vi.useFakeTimers() })
  afterEach(() => { vi.useRealTimers() })

  it('does not report the first burst as activity because no resting state is known', () => {
    const h = setup()
    h.detector.processSnapshot('replay')
    h.detector.processSnapshot('more replay')
    expect(h.onActivity).not.toHaveBeenCalled()
    vi.advanceTimersByTime(500)
    expect(h.onIdle).toHaveBeenCalledTimes(1)
    expect(h.onActivity).not.toHaveBeenCalled()
    h.detector.processSnapshot('real work')
    vi.advanceTimersByTime(500)
    expect(h.onActivity).toHaveBeenCalledExactlyOnceWith('real work')
  })

  it('reports a first burst that keeps changing past one idle timeout as activity', () => {
    const h = setup()
    h.detector.processSnapshot('spinner 1')
    vi.advanceTimersByTime(499)
    h.detector.processSnapshot('spinner 2')
    expect(h.onActivity).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    h.detector.processSnapshot('spinner 3')
    expect(h.onActivity).toHaveBeenCalledExactlyOnceWith('spinner 3')
    h.detector.processSnapshot('spinner 4')
    expect(h.onActivity).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(500)
    expect(h.onIdle).toHaveBeenCalledExactlyOnceWith({ snapshot: 'spinner 4', idleTimeoutMs: 500 })
    h.detector.processSnapshot('next job')
    vi.advanceTimersByTime(500)
    expect(h.onActivity).toHaveBeenCalledTimes(2)
  })

  it('reports a short burst that changed the screen as activity then idle when it goes quiet', () => {
    const h = setup()
    settle(h)
    h.detector.processSnapshot('one')
    h.detector.processSnapshot('two')
    expect(h.onActivity).not.toHaveBeenCalled()
    vi.advanceTimersByTime(500)
    expect(h.onActivity).toHaveBeenCalledExactlyOnceWith('two')
    expect(h.onIdle).toHaveBeenCalledExactlyOnceWith({ snapshot: 'two', idleTimeoutMs: 500 })
    expect(h.onActivity.mock.invocationCallOrder[0]).toBeLessThan(h.onIdle.mock.invocationCallOrder[0] ?? 0)
  })

  it('reports a long burst while it is still changing, once', () => {
    const h = setup()
    settle(h)
    h.detector.processSnapshot('spinner 1')
    vi.advanceTimersByTime(499)
    h.detector.processSnapshot('spinner 2')
    expect(h.onActivity).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    h.detector.processSnapshot('spinner 3')
    expect(h.onActivity).toHaveBeenCalledExactlyOnceWith('spinner 3')
    h.detector.processSnapshot('spinner 4')
    vi.advanceTimersByTime(500)
    expect(h.onActivity).toHaveBeenCalledTimes(1)
    expect(h.onIdle).toHaveBeenCalledExactlyOnceWith({ snapshot: 'spinner 4', idleTimeoutMs: 500 })
  })

  it('ignores a blip that goes quiet on the screen it started from', () => {
    const h = setup()
    settle(h)
    h.detector.processSnapshot('flash')
    vi.advanceTimersByTime(100)
    h.detector.processSnapshot('boot')
    vi.advanceTimersByTime(5000)
    expect(h.onActivity).not.toHaveBeenCalled()
    expect(h.onIdle).not.toHaveBeenCalled()
    // The next burst is measured from the screen the blip left behind.
    h.detector.processSnapshot('work')
    vi.advanceTimersByTime(500)
    expect(h.onActivity).toHaveBeenCalledExactlyOnceWith('work')
    expect(h.onIdle).toHaveBeenCalledTimes(1)
  })

  it('still reports a long burst that ends back on the screen it started from', () => {
    const h = setup()
    settle(h)
    h.detector.processSnapshot('spinner 1')
    vi.advanceTimersByTime(300)
    h.detector.processSnapshot('spinner 2')
    vi.advanceTimersByTime(300)
    h.detector.processSnapshot('boot')
    expect(h.onActivity).toHaveBeenCalledExactlyOnceWith('boot')
    vi.advanceTimersByTime(500)
    expect(h.onIdle).toHaveBeenCalledExactlyOnceWith({ snapshot: 'boot', idleTimeoutMs: 500 })
  })

  it('ignores a first burst that returns to the initial screen, leaving the detector settled', () => {
    const h = setup()
    h.detector.processSnapshot('flash')
    h.detector.processSnapshot('')
    vi.advanceTimersByTime(500)
    expect(h.onIdle).not.toHaveBeenCalled()
    h.detector.processSnapshot('work')
    vi.advanceTimersByTime(500)
    expect(h.onActivity).toHaveBeenCalledExactlyOnceWith('work')
  })

  it('emits nothing until a snapshot differs from the initial one', () => {
    const h = setup()
    h.detector.processSnapshot('')
    vi.advanceTimersByTime(5000)
    expect(h.onActivity).not.toHaveBeenCalled()
    expect(h.onIdle).not.toHaveBeenCalled()
  })

  it('re-arms the idle timer on each visible change', () => {
    const h = setup()
    h.detector.processSnapshot('one')
    vi.advanceTimersByTime(300)
    h.detector.processSnapshot('two')
    vi.advanceTimersByTime(300)
    expect(h.onIdle).not.toHaveBeenCalled()
    vi.advanceTimersByTime(200)
    expect(h.onIdle).toHaveBeenCalledTimes(1)
  })

  it('ignores identical snapshots so repeated repaints still idle', () => {
    const h = setup()
    settle(h)
    h.detector.processSnapshot('same screen')
    for (let t = 0; t < 500; t += 100) {
      vi.advanceTimersByTime(100)
      h.detector.processSnapshot('same screen')
    }
    expect(h.onActivity).toHaveBeenCalledTimes(1)
    expect(h.onIdle).toHaveBeenCalledTimes(1)
  })

  it('reads the timeout when arming, so a change mid-burst is honoured', () => {
    let timeout = 500
    const h = setup(() => timeout)
    h.detector.processSnapshot('one')
    timeout = 2000
    h.detector.processSnapshot('two')
    vi.advanceTimersByTime(1999)
    expect(h.onIdle).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(h.onIdle).toHaveBeenCalledTimes(1)
    expect(h.onIdle).toHaveBeenCalledWith({ snapshot: 'two', idleTimeoutMs: 2000 })
  })

  it('destroy cancels a pending idle and is safe to repeat', () => {
    const h = setup()
    h.detector.processSnapshot('one')
    h.detector.destroy()
    h.detector.destroy()
    vi.advanceTimersByTime(5000)
    expect(h.onIdle).not.toHaveBeenCalled()
  })
})

describe('idleTimeoutMs', () => {
  const settings = { terminalAnalyzer: { idleDebounceMs: 2000, idleDebounceUnreadMs: 15000 } } as Settings
  it('uses the unread debounce only while the workspace is unread', () => {
    expect(idleTimeoutMs(settings, false)).toBe(2000)
    expect(idleTimeoutMs(settings, true)).toBe(15000)
  })
})
