import type { ActivityState, WorkspaceStore } from '../types'
import { useContextMenuStore } from '../store/contextMenu'
import ContextMenu from './ContextMenu'

const STATE_COLORS: Record<ActivityState, string> = {
  idle: '#666',
  working: '#2472c8',
  user_input_required: '#e5e510',
  permission_request: '#cd6600',
  safe_permission_requested: '#0dbc79',
  completed: '#23d18b',
  application_error: '#d16969',
  error: '#f44747'
}

const STATE_LABELS: Record<ActivityState, string> = {
  idle: 'idle',
  working: 'working',
  user_input_required: 'input required',
  permission_request: 'permission request',
  safe_permission_requested: 'safe permission',
  completed: 'completed',
  application_error: 'application error',
  error: 'error'
}

interface ActivityStateBadgeProps {
  workspace: WorkspaceStore
  tabId: string
  state: ActivityState
  /** Shows a spinner while an analysis is in flight (AI harness). */
  analyzing?: boolean
  title?: string
  /** What the debugger is seeded with: the tab's current viewport. */
  getBufferText: () => string
}

/**
 * The coloured pill in a terminal status bar that names the tab's activity state.
 * Right-click opens the debug menu: seed the system prompt debugger, or view the tab's history.
 */
export function ActivityStateBadge({ workspace, tabId, state, analyzing, title, getBufferText }: ActivityStateBadgeProps) {
  const openContextMenu = useContextMenuStore((s) => s.open)
  const closeContextMenu = useContextMenuStore((s) => s.close)
  const activeMenuId = useContextMenuStore((s) => s.activeMenuId)
  const menuPosition = useContextMenuStore((s) => s.position)
  const menuId = `activity-badge-${tabId}`

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault()
    e.stopPropagation()
    openContextMenu(menuId, e.clientX, e.clientY)
  }

  const handleDebug = () => {
    closeContextMenu()
    workspace.getState().addTab('system-prompt-debugger', { bufferText: getBufferText() })
  }

  const handleViewHistory = () => {
    closeContextMenu()
    workspace.getState().addTab('analyzer-history', { sourceTabId: tabId })
  }

  return (
    <>
      <div
        className="activity-state-badge"
        style={{ background: STATE_COLORS[state] }}
        title={title}
        onContextMenu={handleContextMenu}
      >
        {analyzing && <span className="activity-state-badge-spinner" />}
        {STATE_LABELS[state]}
      </div>
      <ContextMenu menuId={menuId} activeMenuId={activeMenuId} position={menuPosition}>
        <div className="context-menu-item" onClick={handleDebug}>
          Debug System Prompt
        </div>
        <div className="context-menu-item" onClick={handleViewHistory}>
          History
        </div>
      </ContextMenu>
    </>
  )
}
