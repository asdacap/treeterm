import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createTrafficMeterStore, TrafficStatus, TRAFFIC_TICK_MS, type TrafficMeterDeps } from './createTrafficMeterStore'

interface Harness {
  deps: TrafficMeterDeps
  tick: () => Promise<void>
  received: Map<string, number>
  connectionIds: string[]
}

function makeHarness(): Harness {
  let time = 0
  let callback: (() => void) | undefined
  const received = new Map<string, number>()
  const connectionIds: string[] = []
  const deps: TrafficMeterDeps = {
    getReceivedBytes: vi.fn((id: string) => {
      const total = received.get(id)
      return total === undefined ? Promise.reject(new Error(`Connection not found: ${id}`)) : Promise.resolve(total)
    }),
    listConnectionIds: () => connectionIds,
    now: () => time,
    setInterval: vi.fn((cb: () => void) => { callback = cb; return 1 as unknown as ReturnType<typeof setInterval> }),
    clearInterval: vi.fn(),
  }
  return {
    deps,
    received,
    connectionIds,
    tick: async () => {
      time += TRAFFIC_TICK_MS
      callback!()
      // Let the connection polls settle.
      await Promise.resolve()
      await Promise.resolve()
    },
  }
}

describe('createTrafficMeterStore', () => {
  let h: Harness

  beforeEach(() => {
    h = makeHarness()
  })

  it('ticks on the configured interval', () => {
    createTrafficMeterStore(h.deps)
    expect(h.deps.setInterval).toHaveBeenCalledWith(expect.any(Function), TRAFFIC_TICK_MS)
  })

  describe('workspaces', () => {
    it('publishes recorded bytes only on tick', async () => {
      const store = createTrafficMeterStore(h.deps)
      store.getState().recordWorkspace('ws-1', 100)
      store.getState().recordWorkspace('ws-1', 50)
      expect(store.getState().workspaces).toEqual({})

      await h.tick()
      expect(store.getState().workspaces['ws-1']).toEqual({ totalBytes: 150, bytesPerSec: 0 })
    })

    it('averages the rate over the last ten seconds and decays to zero', async () => {
      const store = createTrafficMeterStore(h.deps)
      store.getState().recordWorkspace('ws-1', 0)
      await h.tick()
      store.getState().recordWorkspace('ws-1', 2000)
      await h.tick()
      await h.tick()
      expect(store.getState().workspaces['ws-1']).toEqual({ totalBytes: 2000, bytesPerSec: 1000 })

      for (let i = 0; i < 10; i++) await h.tick()
      expect(store.getState().workspaces['ws-1']).toEqual({ totalBytes: 2000, bytesPerSec: 0 })
    })

    it('keeps workspaces separate', async () => {
      const store = createTrafficMeterStore(h.deps)
      store.getState().recordWorkspace('ws-1', 10)
      store.getState().recordWorkspace('ws-2', 20)
      await h.tick()
      expect(store.getState().workspaces['ws-1']?.totalBytes).toBe(10)
      expect(store.getState().workspaces['ws-2']?.totalBytes).toBe(20)
    })

    it('removeWorkspace drops its numbers', async () => {
      const store = createTrafficMeterStore(h.deps)
      store.getState().recordWorkspace('ws-1', 10)
      await h.tick()
      store.getState().removeWorkspace('ws-1')
      expect(store.getState().workspaces['ws-1']).toBeUndefined()
      await h.tick()
      expect(store.getState().workspaces['ws-1']).toBeUndefined()
    })
  })

  describe('connections', () => {
    it('is loading until the first sample, then ready', async () => {
      const store = createTrafficMeterStore(h.deps)
      h.connectionIds.push('conn-1')
      h.received.set('conn-1', 500)
      let pending: ((n: number) => void) | undefined
      vi.mocked(h.deps.getReceivedBytes).mockImplementationOnce(() => new Promise((resolve) => { pending = resolve }))

      await h.tick()
      expect(store.getState().connections['conn-1']).toEqual({ status: TrafficStatus.Loading })

      pending!(500)
      await Promise.resolve()
      await Promise.resolve()
      expect(store.getState().connections['conn-1']).toEqual({ status: TrafficStatus.Ready, rate: { totalBytes: 500, bytesPerSec: 0 } })
    })

    it('works out the rate from the running total', async () => {
      const store = createTrafficMeterStore(h.deps)
      h.connectionIds.push('conn-1')
      h.received.set('conn-1', 1000)
      await h.tick()
      h.received.set('conn-1', 4000)
      await h.tick()
      expect(store.getState().connections['conn-1']).toEqual({ status: TrafficStatus.Ready, rate: { totalBytes: 4000, bytesPerSec: 3000 } })
    })

    it('does not start a poll while the previous one is in flight', async () => {
      createTrafficMeterStore(h.deps)
      h.connectionIds.push('conn-1')
      vi.mocked(h.deps.getReceivedBytes).mockImplementationOnce(() => new Promise(() => {}))
      await h.tick()
      await h.tick()
      expect(h.deps.getReceivedBytes).toHaveBeenCalledTimes(1)
    })

    it('shows the error when the poll fails', async () => {
      const store = createTrafficMeterStore(h.deps)
      h.connectionIds.push('conn-1')
      await h.tick()
      expect(store.getState().connections['conn-1']).toEqual({ status: TrafficStatus.Error, message: 'Connection not found: conn-1' })
    })

    it('stringifies a non-Error rejection', async () => {
      const store = createTrafficMeterStore(h.deps)
      h.connectionIds.push('conn-1')
      // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors -- the non-Error branch is what's under test
      vi.mocked(h.deps.getReceivedBytes).mockImplementationOnce(() => Promise.reject('boom'))
      await h.tick()
      expect(store.getState().connections['conn-1']).toEqual({ status: TrafficStatus.Error, message: 'boom' })
    })

    it('drops a connection that leaves the list, even mid-poll', async () => {
      const store = createTrafficMeterStore(h.deps)
      h.connectionIds.push('conn-1')
      h.received.set('conn-1', 10)
      await h.tick()
      expect(store.getState().connections['conn-1']?.status).toBe(TrafficStatus.Ready)

      let pending: ((n: number) => void) | undefined
      vi.mocked(h.deps.getReceivedBytes).mockImplementationOnce(() => new Promise((resolve) => { pending = resolve }))
      await h.tick()
      h.connectionIds.length = 0
      pending!(20)
      await Promise.resolve()
      await Promise.resolve()
      expect(store.getState().connections['conn-1']?.status).toBe(TrafficStatus.Ready)

      await h.tick()
      expect(store.getState().connections['conn-1']).toBeUndefined()
    })

    it('ignores a failed poll for a connection that left the list', async () => {
      const store = createTrafficMeterStore(h.deps)
      h.connectionIds.push('conn-1')
      let fail: ((e: Error) => void) | undefined
      vi.mocked(h.deps.getReceivedBytes).mockImplementationOnce(() => new Promise((_resolve, reject) => { fail = reject }))
      await h.tick()
      h.connectionIds.length = 0
      fail!(new Error('gone'))
      await Promise.resolve()
      await Promise.resolve()
      expect(store.getState().connections['conn-1']).toEqual({ status: TrafficStatus.Loading })
    })
  })

  it('dispose stops the ticks', () => {
    const store = createTrafficMeterStore(h.deps)
    store.getState().dispose()
    expect(h.deps.clearInterval).toHaveBeenCalledWith(1)
  })
})
