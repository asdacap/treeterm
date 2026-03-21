import { Loader2, Circle, Monitor } from 'lucide-react'
import { useActivityStateStore } from '../store/activityState'
import { useAppStore } from '../store/app'
import SessionPanel from './SessionPanel'

// Exported so SessionPanel can use it
export function WorkspaceActivityIndicator({ tabIds }: { tabIds: string[] }) {
  const activityState = useActivityStateStore((state) => {
    // Priority: working > waiting_for_input > idle
    if (tabIds.some((id) => state.states[id] === 'working')) return 'working'
    if (tabIds.some((id) => state.states[id] === 'waiting_for_input')) return 'waiting_for_input'
    return 'idle'
  })

  if (activityState === 'idle') return null

  return (
    <span
      className={`tree-item-activity tree-item-activity-${activityState}`}
      title={activityState === 'working' ? 'Working...' : 'Waiting for input'}
    >
      {activityState === 'working' ? <Loader2 size={10} /> : <Circle size={8} fill="currentColor" />}
    </span>
  )
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
      const result = await sessionApi.list()
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
      <div className="tree-sessions-scroll">
        {sessionIds.map((sessionId) => (
          <SessionPanel
            key={sessionId}
            sessionId={sessionId}
            sessionStore={sessionStores[sessionId]}
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
