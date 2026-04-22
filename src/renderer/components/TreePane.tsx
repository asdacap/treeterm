/* eslint-disable custom/no-string-literal-comparison -- TODO: migrate existing string-literal comparisons to enums */
import React, { useState } from 'react'
import { Monitor, Loader2, AlertCircle, GitBranch, Folder, PanelLeftClose, PanelLeftOpen, Star } from 'lucide-react'
import { useActivityStateStore } from '../store/activityState'
import { useAppStore } from '../store/app'
import { ActivityState } from '../types'
import type { ReviewState, WorkspaceStore } from '../types'
import { useSessionNamesStore } from '../store/sessionNames'
import SessionPanel, { CollapsedSessionPanel, LoadedWorkspaceTreeItem } from './SessionPanel'
import { ActivityIndicator } from './ActivityIndicator'
import { useNavigationStore } from '../store/navigation'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { SessionState } from '../store/createSessionStore'
import { WorkspaceEntryStatus } from '../store/createSessionStore'

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
  if (activityState !== ActivityState.Idle) return <ActivityIndicator activityState={activityState} className="tree-item-icon-activity" />
  return isWorktree ? <GitBranch size={16} /> : <Folder size={16} />
}

function FavouriteRow({
  sessionId,
  sessionStore,
  workspaceId,
  workspaceStore,
  data,
}: {
  sessionId: string
  sessionStore: StoreApi<SessionState>
  workspaceId: string
  workspaceStore: WorkspaceStore
  data: import('../types').Workspace
}): React.JSX.Element | null {
  const isFavourite = useStore(workspaceStore, s => s.metadata.isFavourite === 'true')
  const { activeView, setActiveView } = useNavigationStore()
  const setActiveWorkspace = useStore(sessionStore, s => s.setActiveWorkspace)
  const activeWorkspaceId = useStore(sessionStore, s => s.activeWorkspaceId)
  const isActiveSession = activeView?.type === 'workspace' && activeView.sessionId === sessionId
  const isActive = isActiveSession && activeWorkspaceId === workspaceId

  if (!isFavourite) return null

  const handleClick = (id: string) => {
    setActiveWorkspace(id)
    setActiveView({ type: 'workspace', workspaceId: id, sessionId })
  }

  const handleRemove = (id: string) => {
    const ws = workspaceStore.getState().workspace
    if (ws.isWorktree && ws.parentId) {
      setActiveWorkspace(id)
      workspaceStore.getState().openOrFocusTab<ReviewState>('review', { parentWorkspaceId: ws.parentId })
      return
    }
    const message = `Remove workspace "${ws.name}"?`
    if (confirm(message)) {
      void workspaceStore.getState().remove()
    }
  }

  return (
    <LoadedWorkspaceTreeItem
      key={workspaceId}
      id={workspaceId}
      store={workspaceStore}
      data={data}
      depth={0}
      isActive={isActive}
      isFocused={false}
      isExpanded={false}
      onToggleExpand={() => { /* no-op: favourites list is flat */ }}
      onClick={handleClick}
      onQuickFork={() => { void workspaceStore.getState().quickForkWorkspace() }}
      onCreateChild={() => { /* no-op: use session tree for branch operations */ }}
      onRemove={handleRemove}
      onDismiss={() => { /* no-op: unloaded workspaces don't appear as favourites */ }}
      onOpenSettings={() => { workspaceStore.getState().addTab('workspace-settings') }}
      onToggleFavourite={() => { workspaceStore.getState().toggleFavourite() }}
      children={[]}
      renderChild={() => null}
      isDragging={false}
      dragOverPosition={null}
      onDragStart={() => { /* no-op */ }}
      onDragOver={() => { /* no-op */ }}
      onDrop={() => { /* no-op */ }}
      onDragEnd={() => { /* no-op */ }}
    />
  )
}

function SessionFavouriteRows({
  sessionId,
  sessionStore,
}: {
  sessionId: string
  sessionStore: StoreApi<SessionState>
}): React.JSX.Element {
  const workspaces = useStore(sessionStore, s => s.workspaces)

  return (
    <>
      {Array.from(workspaces.entries()).map(([workspaceId, entry]) => {
        if (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError) {
          return null
        }
        return (
          <FavouriteRow
            key={workspaceId}
            sessionId={sessionId}
            sessionStore={sessionStore}
            workspaceId={workspaceId}
            workspaceStore={entry.store}
            data={entry.data}
          />
        )
      })}
    </>
  )
}

interface FavouritesPanelProps {
  sessionIds: string[]
  sessionStores: Map<string, { store: StoreApi<SessionState> }>
}

function FavouritesPanel({ sessionIds, sessionStores }: FavouritesPanelProps): React.JSX.Element {
  return (
    <div className="favourites-panel">
      <div className="favourites-header">
        <Star size={12} fill="currentColor" />
        <span>Favourites</span>
      </div>
      <div className="favourites-list">
        {sessionIds.map((sessionId) => {
          const entry = sessionStores.get(sessionId)
          if (!entry) return null
          return (
            <SessionFavouriteRows
              key={sessionId}
              sessionId={sessionId}
              sessionStore={entry.store}
            />
          )
        })}
      </div>
    </div>
  )
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
      <FavouritesPanel sessionIds={sessionIds} sessionStores={sessionStores} />
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
