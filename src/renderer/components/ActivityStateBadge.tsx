import type { ActivityState } from '../types'

const STATE_COLORS: Record<ActivityState, string> = {
  idle: '#666',
  working: '#2472c8',
  user_input_required: '#e5e510',
  permission_request: '#cd6600',
  safe_permission_requested: '#0dbc79',
  completed: '#23d18b',
  error: '#f44747'
}

const STATE_LABELS: Record<ActivityState, string> = {
  idle: 'idle',
  working: 'working',
  user_input_required: 'input required',
  permission_request: 'permission request',
  safe_permission_requested: 'safe permission',
  completed: 'completed',
  error: 'error'
}

interface ActivityStateBadgeProps {
  state: ActivityState
  /** Shows a spinner while an analysis is in flight (AI harness). */
  analyzing?: boolean
  title?: string
  onContextMenu?: (e: React.MouseEvent) => void
}

/** The coloured pill in a terminal status bar that names the tab's activity state. */
export function ActivityStateBadge({ state, analyzing, title, onContextMenu }: ActivityStateBadgeProps) {
  return (
    <div
      className="activity-state-badge"
      style={{ background: STATE_COLORS[state] }}
      title={title}
      onContextMenu={onContextMenu}
    >
      {analyzing && <span className="activity-state-badge-spinner" />}
      {STATE_LABELS[state]}
    </div>
  )
}
