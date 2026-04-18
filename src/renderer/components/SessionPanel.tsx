/* eslint-disable custom/no-string-literal-comparison -- TODO: migrate existing string-literal comparisons to enums */
import React, { useState, useCallback } from 'react'
import type { ReactNode } from 'react'
import { useContextMenuStore } from '../store/contextMenu'
import ContextMenu from './ContextMenu'
import { GitFork, ChevronDown, ChevronRight, Loader2, AlertCircle, LockOpen } from 'lucide-react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { SessionState, WorkspaceEntry } from '../store/createSessionStore'
import { WorkspaceEntryStatus } from '../store/createSessionStore'
import { useAppStore } from '../store/app'
import { useNavigationStore } from '../store/navigation'
import { useKeybindingStore, PrefixModeState } from '../store/keybinding'
import { useSessionNamesStore } from '../store/sessionNames'
import CreateChildDialog, { TabMode } from './CreateChildDialog'
import OpenWorkspaceDialog from './OpenWorkspaceDialog'
import UpstreamWarningDialog from './UpstreamWarningDialog'
import type { ReviewState, WorktreeSettings, Workspace } from '../types'
import { ConnectionStatus, ConnectionTargetType } from '../../shared/types'

// Import WorkspaceIcon from TreePane
import { WorkspaceIcon } from './TreePane'

interface SessionPanelProps {
  sessionId: string
  sessionStore: StoreApi<SessionState>
  selectFolder: () => Promise<string | null>
}

export default function SessionPanel({
  sessionId,
  sessionStore,
  selectFolder,
}: SessionPanelProps): React.JSX.Element {
  const connection = useStore(sessionStore, s => s.connection)
  const sessionLock = useStore(sessionStore, s => s.sessionLock)
  const workspaces = useStore(sessionStore, s => s.workspaces)
  const activeWorkspaceId = useStore(sessionStore, s => s.activeWorkspaceId)
  const addWorkspace = useStore(sessionStore, s => s.addWorkspace)
  const addChildWorkspace = useStore(sessionStore, s => s.addChildWorkspace)
  const adoptExistingWorktree = useStore(sessionStore, s => s.adoptExistingWorktree)
  const createWorktreeFromBranch = useStore(sessionStore, s => s.createWorktreeFromBranch)
  const createWorktreeFromRemote = useStore(sessionStore, s => s.createWorktreeFromRemote)
  const quickForkWorkspace = useStore(sessionStore, s => s.quickForkWorkspace)
  const setActiveWorkspace = useStore(sessionStore, s => s.setActiveWorkspace)
  const moveWorkspace = useStore(sessionStore, s => s.moveWorkspace)
  const forceUnlock = useStore(sessionStore, s => s.forceUnlock)
  const dismissWorkspace = useStore(sessionStore, s => s.dismissWorkspace)
  const { activeView, setActiveView } = useNavigationStore()
  const {
    prefixState,
    focusedWorkspaceIndex,
    workspaceIds: focusedWorkspaceIds
  } = useKeybindingStore()

  const disconnectSession = useAppStore(s => s.disconnectSession)
  const ssh = useAppStore(s => s.ssh)
  const filesystem = useAppStore(s => s.filesystem)

  const openContextMenu = useContextMenuStore((s) => s.open)
  const closeContextMenu = useContextMenuStore((s) => s.close)
  const activeMenuId = useContextMenuStore((s) => s.activeMenuId)
  const menuPosition = useContextMenuStore((s) => s.position)
  const getChildren = (parentId: string) =>
    Array.from(workspaces.values())
      .filter((e): e is Extract<typeof e, { status: WorkspaceEntryStatus.Loaded | WorkspaceEntryStatus.OperationError }> => (e.status === WorkspaceEntryStatus.Loaded || e.status === WorkspaceEntryStatus.OperationError) && e.data.parentId === parentId)
      .map(e => e.data)
      .sort((a, b) => parseInt(a.metadata.sortOrder || '0') - parseInt(b.metadata.sortOrder || '0'))

  const [expanded, setExpanded] = useState<Set<string>>(() => {
    return new Set(
      Array.from(workspaces.keys())
        .filter((id) => getChildren(id).length > 0)
    )
  })
  const [createChildDialogParentId, setCreateChildDialogParentId] = useState<string | null>(null)
  const [isOpenWorkspaceDialogOpen, setIsOpenWorkspaceDialogOpen] = useState(false)
  const [upstreamWarning, setUpstreamWarning] = useState<{
    workspaceId: string
    behindCount: number
    workspaceName: string
    action: 'quickFork' | 'createChild'
  } | null>(null)

  // Drag-and-drop state for workspace reordering and reparenting
  const [dragState, setDragState] = useState<{
    dragId: string
    overId: string
    position: 'before' | 'after' | 'onto'
  } | null>(null)

  // Session name editing
  const displayName = useSessionNamesStore(s => s.names.get(sessionId)?.name)
  const setSessionName = useSessionNamesStore(s => s.setName)
  const removeSessionName = useSessionNamesStore(s => s.removeName)
  const [isEditingName, setIsEditingName] = useState(false)
  const [editName, setEditName] = useState('')

  const handleStartEditName = useCallback(() => {
    setEditName(displayName || sessionId)
    setIsEditingName(true)
  }, [displayName, sessionId])

  const handleSaveName = useCallback(() => {
    const trimmed = editName.trim()
    if (trimmed && trimmed !== sessionId) {
      setSessionName(sessionId, trimmed)
    } else {
      removeSessionName(sessionId)
    }
    setIsEditingName(false)
  }, [editName, sessionId, setSessionName, removeSessionName])

  const [isSessionCollapsed, setIsSessionCollapsed] = useState(false)

  const isActiveSession = activeView?.type === 'workspace' && activeView.sessionId === sessionId

  // Expand any workspace that gains children
  const [prevWorkspaces, setPrevWorkspaces] = useState(workspaces)
  if (workspaces !== prevWorkspaces) {
    setPrevWorkspaces(workspaces)
    const parentIds = Array.from(workspaces.keys())
      .filter((id) =>
        Array.from(workspaces.values())
          .filter(e => (e.status === WorkspaceEntryStatus.Loaded || e.status === WorkspaceEntryStatus.OperationError) && e.data.parentId === id)
          .length > 0
      )
    if (parentIds.length > 0) {
      setExpanded((prev) => {
        const next = new Set(prev)
        for (const id of parentIds) next.add(id)
        return next
      })
    }
  }

  // Compute paths of already-open worktrees
  const openWorktreePaths = Array.from(workspaces.values())
    .filter((e): e is Extract<typeof e, { status: WorkspaceEntryStatus.Loaded | WorkspaceEntryStatus.OperationError }> =>
      e.status === WorkspaceEntryStatus.Loaded || e.status === WorkspaceEntryStatus.OperationError)
    .filter(e => e.data.isWorktree)
    .map(e => e.data.path)

  const handleAddWorkspace = () => {
    setIsOpenWorkspaceDialogOpen(true)
  }

  const handleOpenWorkspaceSubmit = (path: string, settings?: WorktreeSettings) => {
    addWorkspace(path, { settings })
    setIsOpenWorkspaceDialogOpen(false)
  }

  const handleSessionContextMenu = (e: React.MouseEvent) => {
    e.preventDefault()
    e.stopPropagation()
    openContextMenu(`session-context-${sessionId}`, e.clientX, e.clientY)
  }

  const handleDisconnectSession = () => {
    closeContextMenu()
    disconnectSession(sessionId)
  }

  const handleCreateChild = (parentId: string) => {
    closeContextMenu()
    const entry = workspaces.get(parentId)
    if (entry && (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError)) {
      const behindCount = entry.store.getState().gitController.getState().behindCount
      if (behindCount > 0) {
        setUpstreamWarning({
          workspaceId: parentId,
          behindCount,
          workspaceName: entry.data.metadata.displayName || entry.data.name,
          action: 'createChild'
        })
        return
      }
    }
    setCreateChildDialogParentId(parentId)
  }


  const handleQuickFork = async (wsId: string) => {
    const entry = workspaces.get(wsId)
    if (entry && (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError)) {
      const behindCount = entry.store.getState().gitController.getState().behindCount
      if (behindCount > 0) {
        setUpstreamWarning({
          workspaceId: wsId,
          behindCount,
          workspaceName: entry.data.metadata.displayName || entry.data.name,
          action: 'quickFork'
        })
        return
      }
    }
    const result = await quickForkWorkspace(wsId)
    if (result.success) {
      setExpanded((prev) => new Set([...Array.from(prev), wsId]))
    }
  }

  const handleUpstreamWarningConfirm = async () => {
    if (!upstreamWarning) return
    const { workspaceId, action } = upstreamWarning
    setUpstreamWarning(null)

    if (action === 'quickFork') {
      const result = await quickForkWorkspace(workspaceId)
      if (result.success) {
        setExpanded((prev) => new Set([...Array.from(prev), workspaceId]))
      }
    } else {
      setCreateChildDialogParentId(workspaceId)
    }
  }

  const handleCreateChildSubmit = (name: string, isDetached: boolean, settings?: WorktreeSettings) => {
    if (!createChildDialogParentId) return { success: false, error: 'No parent selected' }

    const result = addChildWorkspace(createChildDialogParentId, name, isDetached, settings)
    if (result.success) {
      setExpanded((prev) => new Set([...Array.from(prev), createChildDialogParentId]))
      setCreateChildDialogParentId(null)
    }
    return result
  }

  const handleAdoptWorktreeSubmit = async (
    worktreePath: string,
    branch: string,
    name: string,
    settings?: WorktreeSettings,
    description?: string,
    displayName?: string
  ) => {
    if (!createChildDialogParentId) return { success: false, error: 'No parent selected' }

    const result = await adoptExistingWorktree(
      createChildDialogParentId,
      worktreePath,
      branch,
      name,
      settings,
      description,
      displayName
    )
    if (result.success) {
      setExpanded((prev) => new Set([...Array.from(prev), createChildDialogParentId]))
      setCreateChildDialogParentId(null)
    }
    return result
  }

  const handleCreateFromBranchSubmit = (branch: string, isDetached: boolean, settings?: WorktreeSettings) => {
    if (!createChildDialogParentId) return { success: false, error: 'No parent selected' }

    const result = createWorktreeFromBranch(createChildDialogParentId, branch, isDetached, settings)
    if (result.success) {
      setExpanded((prev) => new Set([...Array.from(prev), createChildDialogParentId]))
      setCreateChildDialogParentId(null)
    }
    return result
  }

  const handleCreateFromRemoteSubmit = (remoteBranch: string, isDetached: boolean, settings?: WorktreeSettings) => {
    if (!createChildDialogParentId) return { success: false, error: 'No parent selected' }

    const result = createWorktreeFromRemote(createChildDialogParentId, remoteBranch, isDetached, settings)
    if (result.success) {
      setExpanded((prev) => new Set([...Array.from(prev), createChildDialogParentId]))
      setCreateChildDialogParentId(null)
    }
    return result
  }

  const handleRemove = async (id: string) => {
    closeContextMenu()
    const entry = workspaces.get(id)
    if (!entry || (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError)) return
    const ws = entry.data

    // For worktree workspaces with a parent, open the Review tab
    if (ws.isWorktree && ws.parentId) {
      setActiveWorkspace(id)
      entry.store.getState().openOrFocusTab<ReviewState>('review', {
        parentWorkspaceId: ws.parentId
      })
      return
    }

    // For regular workspaces, just confirm and remove
    const message = `Remove workspace "${ws.name}"?`
    if (confirm(message)) {
      await entry.store.getState().remove()
    }
  }

  const handleOpenSettings = (workspaceId: string) => {
    closeContextMenu()
    const entry = workspaces.get(workspaceId)
    if (entry && (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError)) {
      entry.store.getState().addTab('workspace-settings')
    }
  }

  const toggleExpand = (id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }

  // Get root workspaces (those without parents) — includes loading/error entries (no parentId)
  // Orphans (parentId references a workspace not in the store) are also treated as top-level.
  const rootWorkspaceIds = Array.from(workspaces.entries())
    .filter(([, e]) => {
      if (e.status === WorkspaceEntryStatus.Loaded || e.status === WorkspaceEntryStatus.OperationError) {
        return !e.data.parentId || !workspaces.has(e.data.parentId)
      }
      return true // loading/error entries are always top-level
    })
    .sort(([, a], [, b]) => {
      const aOrder = (a.status === WorkspaceEntryStatus.Loaded || a.status === WorkspaceEntryStatus.OperationError) ? parseInt(a.data.metadata.sortOrder || '0') : Infinity
      const bOrder = (b.status === WorkspaceEntryStatus.Loaded || b.status === WorkspaceEntryStatus.OperationError) ? parseInt(b.data.metadata.sortOrder || '0') : Infinity
      return aOrder - bOrder
    })
    .map(([id]) => id)

  // Get create child dialog parent handle
  const createChildDialogParentEntry = createChildDialogParentId ? workspaces.get(createChildDialogParentId) : undefined
  const createChildDialogParentHandle = createChildDialogParentEntry &&
    (createChildDialogParentEntry.status === WorkspaceEntryStatus.Loaded || createChildDialogParentEntry.status === WorkspaceEntryStatus.OperationError)
    ? createChildDialogParentEntry.store
    : null

  const handleWorkspaceClick = (id: string) => {
    setActiveWorkspace(id)
    setActiveView({ type: 'workspace', workspaceId: id, sessionId })
  }

  const handleDragStart = (id: string) => {
    setDragState({ dragId: id, overId: '', position: 'before' })
  }

  const handleDragOver = (e: React.DragEvent, id: string) => {
    e.preventDefault()
    if (!dragState || id === dragState.dragId) return
    const dragEntry = workspaces.get(dragState.dragId)
    const overEntry = workspaces.get(id)
    if (!dragEntry || !overEntry) return
    if (dragEntry.status !== WorkspaceEntryStatus.Loaded && dragEntry.status !== WorkspaceEntryStatus.OperationError) return
    if (overEntry.status !== WorkspaceEntryStatus.Loaded && overEntry.status !== WorkspaceEntryStatus.OperationError) return

    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
    const y = e.clientY - rect.top
    let position: 'before' | 'after' | 'onto'
    if (y < rect.height * 0.25) {
      position = 'before'
    } else if (y > rect.height * 0.75) {
      position = 'after'
    } else {
      position = 'onto'
      // Cycle check: cannot drop onto own descendant
      let current: string | null = id
      while (current) {
        if (current === dragState.dragId) return
        const entry = workspaces.get(current)
        if (!entry || (entry.status !== WorkspaceEntryStatus.Loaded && entry.status !== WorkspaceEntryStatus.OperationError)) break
        current = entry.data.parentId
      }
    }

    if (dragState.overId !== id || dragState.position !== position) {
      setDragState({ ...dragState, overId: id, position })
    }
  }

  const handleDrop = () => {
    if (!dragState || !dragState.overId || dragState.dragId === dragState.overId) {
      setDragState(null)
      return
    }
    moveWorkspace(dragState.dragId, dragState.overId, dragState.position)
    if (dragState.position === 'onto') {
      setExpanded(prev => new Set([...Array.from(prev), dragState.overId]))
    }
    setDragState(null)
  }

  const handleDragEnd = () => {
    setDragState(null)
  }

  const renderWorkspace = (id: string, depth: number = 0): ReactNode => {
    const entry = workspaces.get(id)
    if (!entry) return null

    const children = getChildren(id)
    const isFocused = prefixState === PrefixModeState.WorkspaceFocus && focusedWorkspaceIds[focusedWorkspaceIndex] === id

    return (
      <WorkspaceTreeItem
        key={id}
        id={id}
        depth={depth}
        entry={entry}
        isActive={isActiveSession && activeWorkspaceId === id}
        isFocused={isFocused}
        isExpanded={expanded.has(id)}
        onToggleExpand={toggleExpand}
        onClick={handleWorkspaceClick}
        onQuickFork={(wsId) => { void handleQuickFork(wsId); }}
        onCreateChild={handleCreateChild}
        onRemove={(wsId) => { void handleRemove(wsId); }}
        onDismiss={dismissWorkspace}
        onOpenSettings={handleOpenSettings}
        children={children}
        renderChild={renderWorkspace}
        isDragging={dragState?.dragId === id}
        dragOverPosition={dragState?.overId === id ? dragState.position : null}
        onDragStart={handleDragStart}
        onDragOver={handleDragOver}
        onDrop={handleDrop}
        onDragEnd={handleDragEnd}
      />
    )
  }

  const renderStatusIcon = (status: ConnectionStatus): ReactNode => {
    switch (status) {
      case ConnectionStatus.Connecting: return <Loader2 size={14} className="spinning" />
      case ConnectionStatus.Connected: return <span style={{ display: 'inline-block', width: 8, height: 8, borderRadius: '50%', backgroundColor: '#4caf50' }} />
      case ConnectionStatus.Reconnecting: return <Loader2 size={14} className="spinning" style={{ color: '#ff9800' }} />
      case ConnectionStatus.Disconnected: return <AlertCircle size={14} style={{ color: '#f44336' }} />
      case ConnectionStatus.Error: return <AlertCircle size={14} style={{ color: '#f44336' }} />
    }
  }

  const renderStatusContent = (status: ConnectionStatus): ReactNode => {
    const phaseLabel = connection.status === ConnectionStatus.Connecting && 'connectPhase' in connection
      ? { bootstrap: 'Bootstrapping...', tunnel: 'Establishing tunnel...', daemon: 'Connecting to daemon...' }[connection.connectPhase ?? 'bootstrap']
      : 'Connecting...'
    switch (status) {
      case ConnectionStatus.Connecting: return <div className="tree-empty" style={{ fontSize: 12, padding: '4px 8px' }}>{phaseLabel}</div>
      case ConnectionStatus.Connected: return null
      case ConnectionStatus.Reconnecting: return null
      case ConnectionStatus.Disconnected: return null
      case ConnectionStatus.Error: return null
    }
  }

  const isDegraded = connection.status !== ConnectionStatus.Connected && connection.status !== ConnectionStatus.Connecting

  const renderConnectionBanner = (): ReactNode => {
    const errorMsg = 'error' in connection ? connection.error : undefined
    switch (connection.status) {
      case ConnectionStatus.Reconnecting:
        return (
          <div className="connection-banner reconnecting">
            <Loader2 size={12} className="spinning" />
            <span>Reconnecting (attempt {String(connection.attempt)})...{errorMsg ? ` ${errorMsg}` : ''}</span>
            <button onClick={() => { void ssh.reconnectNow(connection.id) }}>Retry now</button>
            <button onClick={() => { void ssh.cancelReconnect(connection.id) }}>Stop</button>
          </div>
        )
      case ConnectionStatus.Error:
        return (
          <div className="connection-banner error">
            <AlertCircle size={12} />
            <span>{errorMsg}</span>
            <button onClick={() => { void ssh.reconnect(connection.id) }}>Reconnect</button>
            <button onClick={() => { disconnectSession(sessionId) }}>Disconnect</button>
          </div>
        )
      case ConnectionStatus.Disconnected:
        return (
          <div className="connection-banner disconnected">
            <AlertCircle size={12} />
            <span>{errorMsg ?? 'Disconnected'}</span>
            <button onClick={() => { void ssh.reconnect(connection.id) }}>Reconnect</button>
            <button onClick={() => { disconnectSession(sessionId) }}>Disconnect</button>
          </div>
        )
      default:
        return null
    }
  }

  return (
    <div className="session-panel">
      <div className="session-panel-header" onClick={() => { setActiveView({ type: 'session', sessionId }); }} onContextMenu={handleSessionContextMenu}>
        <span
          className="session-panel-expand"
          onClick={(e) => { e.stopPropagation(); setIsSessionCollapsed(!isSessionCollapsed) }}
        >
          {isSessionCollapsed ? <ChevronRight size={16} /> : <ChevronDown size={16} />}
        </span>
        {isEditingName ? (
          <input
            autoFocus
            className="tree-title-input"
            value={editName}
            onChange={(e) => { setEditName(e.target.value); }}
            onFocus={(e) => { e.target.select(); }}
            onKeyDown={(e) => {
              if (e.key === 'Enter') handleSaveName()
              if (e.key === 'Escape') setIsEditingName(false)
            }}
            onBlur={handleSaveName}
            onClick={(e) => { e.stopPropagation(); }}
          />
        ) : (
          <span
            className="tree-title"
            onDoubleClick={(e) => { e.stopPropagation(); handleStartEditName() }}
            style={{ display: 'flex', alignItems: 'center', gap: 6 }}
          >
            {renderStatusIcon(connection.status)}
            {displayName || sessionId}
            {sessionLock && (
              <button
                className="force-unlock-button"
                title="Force unlock session"
                onClick={(e) => { e.stopPropagation(); void forceUnlock() }}
              >
                <LockOpen size={14} />
              </button>
            )}
          </span>
        )}
        {connection.status === ConnectionStatus.Connected && (
          <button
            className="add-button"
            onClick={(e) => { e.stopPropagation(); handleAddWorkspace() }}
            title="Add workspace"
          >
            +
          </button>
        )}
      </div>

      {!isSessionCollapsed && (
        <div className="tree-list">
          {renderConnectionBanner()}
          {renderStatusContent(connection.status) ? (
            renderStatusContent(connection.status)
          ) : rootWorkspaceIds.length === 0 ? (
            <div className="tree-empty">No workspaces. Click + to add one.</div>
          ) : (
            <div style={isDegraded ? { opacity: 0.6, pointerEvents: 'none' } : undefined}>
              {rootWorkspaceIds.map((id) => renderWorkspace(id))}
            </div>
          )}
        </div>
      )}

      {/* Session Context Menu */}
      <ContextMenu menuId={`session-context-${sessionId}`} activeMenuId={activeMenuId} position={menuPosition}>
        <div className="context-menu-item danger" onClick={handleDisconnectSession}>
          Disconnect
        </div>
      </ContextMenu>

      {/* Upstream Warning Dialog */}
      {upstreamWarning && (
        <UpstreamWarningDialog
          behindCount={upstreamWarning.behindCount}
          workspaceName={upstreamWarning.workspaceName}
          onConfirm={() => { void handleUpstreamWarningConfirm(); }}
          onCancel={() => { setUpstreamWarning(null); }}
        />
      )}

      {/* Create Child Dialog */}
      {createChildDialogParentHandle && (
        <CreateChildDialog
          parentWorkspace={createChildDialogParentHandle}
          onCreate={handleCreateChildSubmit}
          onAdopt={handleAdoptWorktreeSubmit}
          onCreateFromBranch={handleCreateFromBranchSubmit}
          onCreateFromRemote={handleCreateFromRemoteSubmit}
          onCancel={() => { setCreateChildDialogParentId(null); }}
          openWorktreePaths={openWorktreePaths}
          initialMode={TabMode.Branch}
        />
      )}

      {/* Open Workspace Dialog */}
      {isOpenWorkspaceDialogOpen && (
        <OpenWorkspaceDialog
          onOpen={handleOpenWorkspaceSubmit}
          onCancel={() => { setIsOpenWorkspaceDialogOpen(false); }}
          selectFolder={selectFolder}
          connectionKey={connection.target.type === ConnectionTargetType.Remote
            ? `${connection.target.config.user}@${connection.target.config.host}:${String(connection.target.config.port)}`
            : 'local'}
          isRemote={connection.target.type === ConnectionTargetType.Remote}
          readDirectory={connection.target.type === ConnectionTargetType.Remote
            ? (dirPath: string) => filesystem.readDirectory(connection.id, '/', dirPath)
            : undefined}
        />
      )}
    </div>
  )
}

interface WorkspaceTreeItemProps {
  id: string
  depth: number
  entry: WorkspaceEntry
  isActive: boolean
  isFocused: boolean
  isExpanded: boolean
  onToggleExpand: (id: string) => void
  onClick: (id: string) => void
  onQuickFork: (id: string) => void
  onCreateChild: (id: string) => void
  onRemove: (id: string) => void
  onDismiss: (id: string) => void
  onOpenSettings: (id: string) => void
  children: Workspace[]
  renderChild: (id: string, depth: number) => ReactNode
  isDragging: boolean
  dragOverPosition: 'before' | 'after' | 'onto' | null
  onDragStart: (id: string) => void
  onDragOver: (e: React.DragEvent, id: string) => void
  onDrop: () => void
  onDragEnd: () => void
}

function WorkspaceTreeItem({
  id, depth, entry, isActive, isFocused, isExpanded,
  onToggleExpand, onClick, onQuickFork, onCreateChild, onRemove, onDismiss, onOpenSettings,
  children, renderChild,
  isDragging, dragOverPosition, onDragStart, onDragOver, onDrop, onDragEnd,
}: WorkspaceTreeItemProps): React.JSX.Element {
  const openContextMenu = useContextMenuStore((s) => s.open)
  const activeMenuId = useContextMenuStore((s) => s.activeMenuId)
  const menuPosition = useContextMenuStore((s) => s.position)
  const menuId = `ws-context-${id}`

  const ws = (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError) ? entry.data : undefined
  const displayName = ws ? (ws.metadata.displayName || ws.name) : (entry as { name: string }).name
  const hasChildren = children.length > 0
  const tabIds = ws ? Object.keys(ws.appStates) : []

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault()
    e.stopPropagation()
    openContextMenu(menuId, e.clientX, e.clientY)
  }

  const dragClasses = [
    isDragging ? 'dragging' : '',
    dragOverPosition === 'before' ? 'drag-before' : '',
    dragOverPosition === 'after' ? 'drag-after' : '',
    dragOverPosition === 'onto' ? 'drag-onto' : '',
  ].filter(Boolean).join(' ')

  return (
    <div>
      <div
        className={`tree-item ${depth === 0 ? 'tree-item-root' : ''} ${isActive ? 'active' : ''} ${isFocused ? 'focused' : ''} ${dragClasses}`}
        style={{ paddingLeft: 4 + depth * 8 }}
        onClick={() => { onClick(id); }}
        onContextMenu={handleContextMenu}
        title={ws?.metadata.description ? `${ws.path}\n\n${ws.metadata.description}` : ws?.path}
        draggable={ws !== undefined}
        onDragStart={(e) => { e.stopPropagation(); onDragStart(id) }}
        onDragOver={(e) => { e.stopPropagation(); onDragOver(e, id) }}
        onDrop={(e) => { e.preventDefault(); e.stopPropagation(); onDrop() }}
        onDragEnd={onDragEnd}
      >
        {hasChildren ? (
          <span
            className="tree-item-expand"
            onClick={(e) => {
              e.stopPropagation()
              onToggleExpand(id)
            }}
          >
            {isExpanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
          </span>
        ) : (
          <span className="tree-item-expand-placeholder" />
        )}
        <span className="tree-item-icon">
          <WorkspaceIcon tabIds={tabIds} loadStatus={entry.status === WorkspaceEntryStatus.Loading || entry.status === WorkspaceEntryStatus.Error ? entry.status : undefined} isWorktree={ws?.isWorktree ?? false} />
        </span>
        <span className="tree-item-name">
          {displayName}
        </span>
        <span className="tree-item-actions">
          {ws?.isGitRepo && (
            <button
              className="tree-item-action"
              title="Fork"
              onClick={(e) => { e.stopPropagation(); onQuickFork(id) }}
            >
              <GitFork size={16} />
            </button>
          )}
        </span>
      </div>

      <ContextMenu menuId={menuId} activeMenuId={activeMenuId} position={menuPosition}>
        {ws && (
          <div className="context-menu-item" onClick={() => { onOpenSettings(id); }}>
            Settings
          </div>
        )}
        {ws?.isGitRepo && (
          <div className="context-menu-item" onClick={() => { onCreateChild(id); }}>
            Open Existing Branch
          </div>
        )}
        {ws?.isWorktree && ws.parentId && (
          <div className="context-menu-item" onClick={() => { onRemove(id); }}>
            Review & Merge
          </div>
        )}
        {ws && !ws.isWorktree && (
          <div className="context-menu-item danger" onClick={() => { onRemove(id); }}>
            Remove
          </div>
        )}
        {!ws && (
          <div className="context-menu-item danger" onClick={() => { onDismiss(id); }}>
            Dismiss
          </div>
        )}
      </ContextMenu>

      {isExpanded && children.map((child) => renderChild(child.id, depth + 1))}
    </div>
  )
}

// Collapsed sidebar view — shows only workspace icons with tooltips
interface CollapsedSessionPanelProps {
  sessionId: string
  sessionStore: StoreApi<SessionState>
}

export function CollapsedSessionPanel({ sessionId, sessionStore }: CollapsedSessionPanelProps): React.JSX.Element {
  const workspaces = useStore(sessionStore, s => s.workspaces)
  const activeWorkspaceId = useStore(sessionStore, s => s.activeWorkspaceId)
  const setActiveWorkspace = useStore(sessionStore, s => s.setActiveWorkspace)
  const { activeView, setActiveView } = useNavigationStore()
  const isActiveSession = activeView?.type === 'workspace' && activeView.sessionId === sessionId

  const getChildren = (parentId: string): Workspace[] =>
    Array.from(workspaces.values())
      .filter((e): e is Extract<typeof e, { status: WorkspaceEntryStatus.Loaded | WorkspaceEntryStatus.OperationError }> => (e.status === WorkspaceEntryStatus.Loaded || e.status === WorkspaceEntryStatus.OperationError) && e.data.parentId === parentId)
      .map(e => e.data)
      .sort((a, b) => parseInt(a.metadata.sortOrder || '0') - parseInt(b.metadata.sortOrder || '0'))

  const rootWorkspaceIds = Array.from(workspaces.entries())
    .filter(([, e]) => {
      if (e.status === WorkspaceEntryStatus.Loaded || e.status === WorkspaceEntryStatus.OperationError) {
        return !e.data.parentId || !workspaces.has(e.data.parentId)
      }
      return true
    })
    .sort(([, a], [, b]) => {
      const aOrder = (a.status === WorkspaceEntryStatus.Loaded || a.status === WorkspaceEntryStatus.OperationError) ? parseInt(a.data.metadata.sortOrder || '0') : Infinity
      const bOrder = (b.status === WorkspaceEntryStatus.Loaded || b.status === WorkspaceEntryStatus.OperationError) ? parseInt(b.data.metadata.sortOrder || '0') : Infinity
      return aOrder - bOrder
    })
    .map(([id]) => id)

  const handleClick = (id: string) => {
    setActiveWorkspace(id)
    setActiveView({ type: 'workspace', workspaceId: id, sessionId })
  }

  const renderIcon = (id: string): ReactNode => {
    const entry = workspaces.get(id)
    if (!entry) return null

    const ws = (entry.status === WorkspaceEntryStatus.Loaded || entry.status === WorkspaceEntryStatus.OperationError) ? entry.data : undefined
    const displayName = ws ? (ws.metadata.displayName || ws.name) : (entry as { name: string }).name
    const tabIds = ws ? Object.keys(ws.appStates) : []
    const isActive = isActiveSession && activeWorkspaceId === id
    const children = getChildren(id)

    return (
      <div key={id}>
        <div
          className={`collapsed-workspace-icon ${isActive ? 'active' : ''}`}
          title={displayName}
          onClick={() => { handleClick(id); }}
        >
          <WorkspaceIcon
            tabIds={tabIds}
            loadStatus={entry.status === WorkspaceEntryStatus.Loading || entry.status === WorkspaceEntryStatus.Error ? entry.status : undefined}
            isWorktree={ws?.isWorktree ?? false}
          />
        </div>
        {children.map((child) => renderIcon(child.id))}
      </div>
    )
  }

  return <>{rootWorkspaceIds.map((id) => renderIcon(id))}</>
}
