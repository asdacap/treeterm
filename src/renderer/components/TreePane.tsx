import React, { useState } from 'react'
import { Monitor, Loader2, AlertCircle, GitBranch, Folder, PanelLeftClose, PanelLeftOpen } from 'lucide-react'
import { useActivityStateStore } from '../store/activityState'
import { useAppStore } from '../store/app'
import { useSessionNamesStore } from '../store/sessionNames'
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
  const rawSessionIds = Array.from(sessionStores.keys())
  const getSortedIds = useSessionNamesStore(s => s.getSortedIds)
  const reorderSession = useSessionNamesStore(s => s.reorderSession)
  const sessionIds = getSortedIds(rawSessionIds)

  // Session drag-and-drop state
  const [dragState, setDragState] = useState<{
    dragId: string
    overId: string
    position: 'before' | 'after'
  } | null>(null)

  const handleSessionDragStart = (id: string) => {
    setDragState({ dragId: id, overId: '', position: 'before' })
  }

  const handleSessionDragOver = (e: React.DragEvent, id: string) => {
    e.preventDefault()
    if (!dragState) return
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
    const position: 'before' | 'after' = e.clientY < rect.top + rect.height / 2 ? 'before' : 'after'
    if (dragState.overId !== id || dragState.position !== position) {
      setDragState({ ...dragState, overId: id, position })
    }
  }

  const handleSessionDrop = () => {
    if (!dragState || !dragState.overId || dragState.dragId === dragState.overId) {
      setDragState(null)
      return
    }
    reorderSession(dragState.dragId, dragState.overId, dragState.position)
    setDragState(null)
  }

  const handleSessionDragEnd = () => {
    setDragState(null)
  }

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
          {sessionIds.map((sessionId) => {
            const isDragging = dragState?.dragId === sessionId
            const dragOverPosition = dragState?.overId === sessionId ? dragState.position : null
            const dragClasses = [
              isDragging ? 'dragging' : '',
              dragOverPosition === 'before' ? 'drag-before' : '',
              dragOverPosition === 'after' ? 'drag-after' : '',
            ].filter(Boolean).join(' ')

            return (
              <div
                key={sessionId}
                className={`session-drag-handle ${dragClasses}`}
                draggable
                onDragStart={() => { handleSessionDragStart(sessionId) }}
                onDragOver={(e) => { handleSessionDragOver(e, sessionId) }}
                onDrop={(e) => { e.preventDefault(); handleSessionDrop() }}
                onDragEnd={handleSessionDragEnd}
              >
                <CollapsedSessionPanel
                  sessionId={sessionId}
                  // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- sessionId from sessionStores.keys()
                  sessionStore={sessionStores.get(sessionId)!.store}
                />
              </div>
            )
          })}
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
        {sessionIds.map((sessionId) => {
          const isDragging = dragState?.dragId === sessionId
          const dragOverPosition = dragState?.overId === sessionId ? dragState.position : null
          const dragClasses = [
            isDragging ? 'dragging' : '',
            dragOverPosition === 'before' ? 'drag-before' : '',
            dragOverPosition === 'after' ? 'drag-after' : '',
          ].filter(Boolean).join(' ')

          return (
            <div
              key={sessionId}
              className={`session-drag-handle ${dragClasses}`}
              draggable
              onDragStart={() => { handleSessionDragStart(sessionId) }}
              onDragOver={(e) => { handleSessionDragOver(e, sessionId) }}
              onDrop={(e) => { e.preventDefault(); handleSessionDrop() }}
              onDragEnd={handleSessionDragEnd}
            >
              <SessionPanel
                sessionId={sessionId}
                // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- sessionId from sessionStores.keys()
                sessionStore={sessionStores.get(sessionId)!.store}
                selectFolder={selectFolder}
              />
            </div>
          )
        })}
        {sessionIds.length === 0 && (
          <div className="tree-empty" style={{ padding: '16px' }}>No sessions. Click + to browse.</div>
        )}
      </div>
    </div>
  )
}
