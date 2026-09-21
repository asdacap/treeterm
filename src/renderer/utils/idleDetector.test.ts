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

describe('createIdleDetector', () => {
  beforeEach(() => { vi.useFakeTimers() })
  afterEach(() => { vi.useRealTimers() })

  it('fires activity once per burst and idle after the timeout', () => {
    const h = setup()
    h.detector.processSnapshot('one')
    h.detector.processSnapshot('two')
    expect(h.onActivity).toHaveBeenCalledTimes(1)
    expect(h.onIdle).not.toHaveBeenCalled()
    vi.advanceTimersByTime(500)
    expect(h.onIdle).toHaveBeenCalledTimes(1)
    h.detector.processSnapshot('three')
    expect(h.onActivity).toHaveBeenCalledTimes(2)
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
