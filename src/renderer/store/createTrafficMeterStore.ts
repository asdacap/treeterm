import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'

/** Rolling window the download rate is averaged over. */
export const TRAFFIC_WINDOW_MS = 10_000
/** How often totals are sampled and published. Recording a chunk never sets state. */
export const TRAFFIC_TICK_MS = 1_000

export interface TrafficRate {
  totalBytes: number
  bytesPerSec: number
}

/** Stable zero value for a workspace that has not received anything yet. */
export const NO_TRAFFIC: TrafficRate = { totalBytes: 0, bytesPerSec: 0 }

export enum TrafficStatus {
  Loading = 'loading',
  Ready = 'ready',
  Error = 'error',
}

export type ConnectionTraffic =
  | { status: TrafficStatus.Loading }
  | { status: TrafficStatus.Ready; rate: TrafficRate }
  | { status: TrafficStatus.Error; message: string }

export const CONNECTION_TRAFFIC_LOADING: ConnectionTraffic = { status: TrafficStatus.Loading }

/** The part of the meter a session store needs: PTY bytes attributed to a workspace. */
export interface WorkspaceTrafficRecorder {
  recordWorkspace: (workspaceId: string, bytes: number) => void
  removeWorkspace: (workspaceId: string) => void
}

export interface TrafficMeterState extends WorkspaceTrafficRecorder {
  /** PTY bytes downloaded per workspace, across every attach stream it opened. */
  workspaces: Record<string, TrafficRate>
  /** Every gRPC byte received per connection, as counted by Main. */
  connections: Record<string, ConnectionTraffic>
  dispose: () => void
}

export interface TrafficMeterDeps {
  /** Running total of bytes received over the connection's gRPC traffic. */
  getReceivedBytes: (connectionId: string) => Promise<number>
  /** Connections to meter; one that drops out of this list is dropped from the meter. */
  listConnectionIds: () => string[]
  now: () => number
  setInterval: (callback: () => void, ms: number) => ReturnType<typeof setInterval>
  clearInterval: (handle: ReturnType<typeof setInterval>) => void
}

interface Sample {
  time: number
  total: number
}

/** Add `latest` to the samples still inside the window, and the rate across them. */
function addSample(samples: Sample[], latest: Sample): { samples: Sample[]; rate: TrafficRate } {
  const windowStart = latest.time - TRAFFIC_WINDOW_MS
  const kept = samples.filter(sample => sample.time >= windowStart)
  const oldest = kept[0] ?? latest
  const elapsedMs = latest.time - oldest.time
  return {
    samples: [...kept, latest],
    rate: {
      totalBytes: latest.total,
      bytesPerSec: elapsedMs > 0 ? ((latest.total - oldest.total) * 1000) / elapsedMs : 0,
    },
  }
}

export function createTrafficMeterStore(deps: TrafficMeterDeps): StoreApi<TrafficMeterState> {
  // Recorded bytes land here, not in state, so a busy terminal doesn't set state per chunk.
  const workspaceTotals = new Map<string, number>()
  const workspaceSamples = new Map<string, Sample[]>()
  const connectionSamples = new Map<string, Sample[]>()
  // A slow poll must not overlap the next one, or samples could land out of order.
  const pollsInFlight = new Set<string>()

  function publishWorkspaces(): void {
    const time = deps.now()
    const workspaces: Record<string, TrafficRate> = {}
    for (const [id, total] of Array.from(workspaceTotals.entries())) {
      const { samples, rate } = addSample(workspaceSamples.get(id) ?? [], { time, total })
      workspaceSamples.set(id, samples)
      workspaces[id] = rate
    }
    store.setState({ workspaces })
  }

  function setConnection(connectionId: string, traffic: ConnectionTraffic): void {
    store.setState((s) => ({ connections: { ...s.connections, [connectionId]: traffic } }))
  }

  async function pollConnection(connectionId: string): Promise<void> {
    pollsInFlight.add(connectionId)
    try {
      const total = await deps.getReceivedBytes(connectionId)
      // Dropped while the poll was in flight: don't resurrect it.
      if (!deps.listConnectionIds().includes(connectionId)) return
      const { samples, rate } = addSample(connectionSamples.get(connectionId) ?? [], { time: deps.now(), total })
      connectionSamples.set(connectionId, samples)
      setConnection(connectionId, { status: TrafficStatus.Ready, rate })
    } catch (error) {
      if (!deps.listConnectionIds().includes(connectionId)) return
      setConnection(connectionId, { status: TrafficStatus.Error, message: error instanceof Error ? error.message : String(error) })
    } finally {
      pollsInFlight.delete(connectionId)
    }
  }

  function pollConnections(): void {
    const live = deps.listConnectionIds()
    const connections: Record<string, ConnectionTraffic> = {}
    for (const id of live) {
      connections[id] = store.getState().connections[id] ?? CONNECTION_TRAFFIC_LOADING
    }
    for (const id of Array.from(connectionSamples.keys())) {
      if (!live.includes(id)) connectionSamples.delete(id)
    }
    store.setState({ connections })
    for (const id of live) {
      if (!pollsInFlight.has(id)) void pollConnection(id)
    }
  }

  function tick(): void {
    publishWorkspaces()
    pollConnections()
  }

  const store = createStore<TrafficMeterState>()((set) => ({
    workspaces: {},
    connections: {},

    recordWorkspace: (workspaceId, bytes) => {
      workspaceTotals.set(workspaceId, (workspaceTotals.get(workspaceId) ?? 0) + bytes)
    },

    removeWorkspace: (workspaceId) => {
      workspaceTotals.delete(workspaceId)
      workspaceSamples.delete(workspaceId)
      set((s) => {
        // eslint-disable-next-line @typescript-eslint/no-unused-vars
        const { [workspaceId]: _, ...rest } = s.workspaces
        return { workspaces: rest }
      })
    },

    dispose: () => {
      deps.clearInterval(interval)
    },
  }))

  const interval = deps.setInterval(tick, TRAFFIC_TICK_MS)
  return store
}
