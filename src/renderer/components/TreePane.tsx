import { Monitor, Loader2, AlertCircle, GitBranch, Folder } from 'lucide-react'
import { useActivityStateStore } from '../store/activityState'
import { useAppStore } from '../store/app'
import SessionPanel from './SessionPanel'
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
}

export default function TreePane({ selectFolder }: TreePaneProps): JSX.Element {
  const sessionStores = useAppStore(s => s.sessionStores)
  const sessionIds = Object.keys(sessionStores)

  const handleShowSessions = async () => {
    const { sessionApi } = useAppStore.getState()
    try {
      const result = await sessionApi.list('local')
      if (result.success && result.sessions) {
        useAppStore.setState({ daemonSessions: result.sessions, showWorkspacePicker: true })
      }
    } catch (error) {
      console.error('Failed to list daemon sessions:', error)
      alert(`Failed to list sessions: ${error instanceof Error ? error.message : 'Unknown error'}`)
    }
  }

  return (
    <div className="tree-pane-content">
      <div className="tree-header">
        <span className="tree-title">Sessions</span>
        <div className="tree-header-actions">
          <button
            className="add-button"
            onClick={() => useAppStore.setState({ showConnectionPicker: true })}
            title="Connect to SSH"
          >
            <Monitor size={14} />
          </button>
          <button className="add-button" onClick={handleShowSessions} title="Browse sessions">
            +
          </button>
        </div>
      </div>
      <div className="tree-sessions-scroll">
        {sessionIds.map((sessionId) => (
          <SessionPanel
            key={sessionId}
            sessionId={sessionId}
            sessionEntry={sessionStores[sessionId]}
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
