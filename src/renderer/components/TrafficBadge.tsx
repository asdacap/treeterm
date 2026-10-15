import type { ReactNode } from 'react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import { AlertTriangle } from 'lucide-react'
import {
  CONNECTION_TRAFFIC_LOADING, NO_TRAFFIC, TrafficStatus,
  type ConnectionTraffic, type TrafficMeterState, type TrafficRate,
} from '../store/createTrafficMeterStore'
import { formatBytes, formatRate } from '../utils/formatBytes'

function RateText({ rate, testId }: { rate: TrafficRate; testId: string }): ReactNode {
  return (
    <span className="traffic-badge" title={formatBytes(rate.totalBytes)} data-testid={testId}>
      ↓ {formatRate(rate.bytesPerSec)}
    </span>
  )
}

export function WorkspaceTrafficBadge({ meter, workspaceId }: { meter: StoreApi<TrafficMeterState>; workspaceId: string }): ReactNode {
  const rate = useStore(meter, s => s.workspaces[workspaceId] ?? NO_TRAFFIC)
  return <RateText rate={rate} testId="workspace-traffic" />
}

export function ConnectionTrafficBadge({ meter, connectionId }: { meter: StoreApi<TrafficMeterState>; connectionId: string }): ReactNode {
  const traffic: ConnectionTraffic = useStore(meter, s => s.connections[connectionId] ?? CONNECTION_TRAFFIC_LOADING)
  const views: Record<TrafficStatus, () => ReactNode> = {
    [TrafficStatus.Loading]: () => <span className="traffic-badge" data-testid="connection-traffic">↓ …</span>,
    [TrafficStatus.Ready]: () => traffic.status === TrafficStatus.Ready && (
      <RateText rate={traffic.rate} testId="connection-traffic" />
    ),
    [TrafficStatus.Error]: () => traffic.status === TrafficStatus.Error && (
      <span className="traffic-badge traffic-badge-error" title={`Download meter unavailable: ${traffic.message}`} data-testid="connection-traffic">
        <AlertTriangle size={12} /> ↓ ?
      </span>
    ),
  }
  return views[traffic.status]()
}
