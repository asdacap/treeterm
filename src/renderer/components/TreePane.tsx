import React from 'react'
import { Monitor, Loader2, AlertCircle, GitBranch, Folder, PanelLeftClose, PanelLeftOpen } from 'lucide-react'
import { useActivityStateStore } from '../store/activityState'
import { useAppStore } from '../store/app'
import SessionPanel, { CollapsedSessionPanel } from './SessionPanel'
import { ActivityIndicator } from './ActivityIndicator'

// Shows activity indicator in icon slot when active, otherwise shows workspace icon
export function WorkspaceIcon({ tabIds, loadStatus, isWorktree }: {
  tabIds: string[]
  loadStatus?: string
  isWorktree: boolean
}) {
  const activityState = useActivityStateStore((state) =>
    state.getWorkspaceState(tabIds)
  )

  if (loadStatus === 'loading') return <Loader2 size={16} className="spinning" />
  if (loadStatus === 'error') return <AlertCircle size={16} className="tree-item-error-icon" />
  if (activityState !== 'idle') return <ActivityIndicator activityState={activityState} className="tree-item-icon-activity" />
  return isWorktree ? <GitBranch size={16} /> : <Folder size={16} />
}

interface TreePaneProps {
  selectFolder: () => Promise<string | null>
  isCollapsed: boolean
  onToggleCollapse: () => void
}

export default function TreePane({ selectFolder, isCollapsed, onToggleCollapse }: TreePaneProps): React.JSX.Element {
  const sessionStores = useAppStore(s => s.sessionStores)
  const sessionIds = Array.from(sessionStores.keys())

  if (isCollapsed) {
    return (
      <div className="tree-pane-collapsed">
        <div className="tree-pane-collapsed-header">
          <button
            className="add-button"
            onClick={onToggleCollapse}
            title="Expand sidebar"
          >
            <PanelLeftOpen size={14} />
          </button>
        </div>
        <div className="tree-pane-collapsed-rail">
          {sessionIds.map((sessionId) => (
            <CollapsedSessionPanel
              key={sessionId}
              sessionId={sessionId}
              // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- sessionId from sessionStores.keys()
              sessionStore={sessionStores.get(sessionId)!.store}
            />
          ))}
        </div>
      </div>
    )
  }

  return (
    <div className="tree-pane-content">
      <div className="tree-header">
        <span className="tree-title">Sessions</span>
        <div className="tree-header-actions">
          <button
            className="add-button"
            onClick={() => { useAppStore.setState({ showConnectionPicker: true }); }}
            title="Connect to SSH"
          >
            <Monitor size={14} />
          </button>
          <button
            className="add-button"
            onClick={onToggleCollapse}
            title="Collapse sidebar"
          >
            <PanelLeftClose size={14} />
          </button>
        </div>
      </div>
      <div className="tree-sessions-scroll">
        {sessionIds.map((sessionId) => (
          <SessionPanel
            key={sessionId}
            sessionId={sessionId}
            // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- sessionId from sessionStores.keys()
            sessionStore={sessionStores.get(sessionId)!.store}
            selectFolder={selectFolder}
          />
        ))}
        {sessionIds.length === 0 && (
          <div className="tree-empty" style={{ padding: '16px' }}>No sessions. Click + to browse.</div>
        )}
      </div>
    </div>
  )
}
