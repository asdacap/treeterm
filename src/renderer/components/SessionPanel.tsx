import { useState, useCallback } from 'react'
import type { ReactNode } from 'react'
import { useContextMenuStore } from '../store/contextMenu'
import ContextMenu from './ContextMenu'
import { GitFork, ChevronDown, ChevronRight, Loader2, AlertCircle } from 'lucide-react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { SessionState, WorkspaceEntry } from '../store/createSessionStore'
import { useAppStore } from '../store/app'
import { useNavigationStore } from '../store/navigation'
import { useKeybindingStore } from '../store/keybinding'
import { useSessionNamesStore } from '../store/sessionNames'
import CreateChildDialog from './CreateChildDialog'
import OpenWorkspaceDialog from './OpenWorkspaceDialog'
import UpstreamWarningDialog from './UpstreamWarningDialog'
import type { ReviewState, WorktreeSettings, Workspace } from '../types'

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
}: SessionPanelProps): JSX.Element {
  const connection = useStore(sessionStore, s => s.connection)
  const {
    workspaces,
    activeWorkspaceId,
    addWorkspace,
    addChildWorkspace,
    adoptExistingWorktree,
    createWorktreeFromBranch,
    createWorktreeFromRemote,
    quickForkWorkspace,
    setActiveWorkspace,
  } = useStore(sessionStore)
  const { activeView, setActiveView } = useNavigationStore()
  const {
    prefixState,
    focusedWorkspaceIndex,
    workspaceIds: focusedWorkspaceIds
  } = useKeybindingStore()

  const disconnectSession = useAppStore(s => s.disconnectSession)
  const filesystem = useAppStore(s => s.filesystem)

  const openContextMenu = useContextMenuStore((s) => s.open)
  const closeContextMenu = useContextMenuStore((s) => s.close)
  const activeMenuId = useContextMenuStore((s) => s.activeMenuId)
  const menuPosition = useContextMenuStore((s) => s.position)
  const getChildren = (parentId: string) =>
    Object.entries(workspaces)
      .filter(([, e]) => (e.status === 'loaded' || e.status === 'operation-error') && e.data.parentId === parentId)
      .map(([, e]) => (e as Extract<typeof e, { status: 'loaded' | 'operation-error' }>).data)

  const [expanded, setExpanded] = useState<Set<string>>(() => {
    return new Set(
      Object.keys(workspaces)
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

  // Session name editing
  const displayName = useSessionNamesStore(s => s.names[sessionId]?.name)
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
    const parentIds = Object.keys(workspaces)
      .filter((id) =>
        Object.entries(workspaces)
          .filter(([, e]) => (e.status === 'loaded' || e.status === 'operation-error') && e.data.parentId === id)
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
  const openWorktreePaths = Object.values(workspaces)
    .filter((e): e is Extract<typeof e, { status: 'loaded' | 'operation-error' }> =>
      e.status === 'loaded' || e.status === 'operation-error')
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
    openContextMenu('session-context', e.clientX, e.clientY)
  }

  const handleDisconnectSession = () => {
    closeContextMenu()
    disconnectSession(sessionId)
  }

  const handleCreateChild = (parentId: string) => {
    closeContextMenu()
    const entry = workspaces[parentId]
    if (entry && (entry.status === 'loaded' || entry.status === 'operation-error')) {
      const behindCount = entry.store.getState().gitController.getState().behindCount
      if (behindCount > 0) {
        setUpstreamWarning({
          workspaceId: parentId,
          behindCount,
          workspaceName: entry.data.metadata?.displayName || entry.data.name,
          action: 'createChild'
        })
        return
      }
    }
    setCreateChildDialogParentId(parentId)
  }


  const handleQuickFork = async (wsId: string) => {
    const entry = workspaces[wsId]
    if (entry && (entry.status === 'loaded' || entry.status === 'operation-error')) {
      const behindCount = entry.store.getState().gitController.getState().behindCount
      if (behindCount > 0) {
        setUpstreamWarning({
          workspaceId: wsId,
          behindCount,
          workspaceName: entry.data.metadata?.displayName || entry.data.name,
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
    settings?: WorktreeSettings
  ) => {
    if (!createChildDialogParentId) return { success: false, error: 'No parent selected' }

    const result = await adoptExistingWorktree(
      createChildDialogParentId,
      worktreePath,
      branch,
      name,
      settings
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
    const entry = workspaces[id]
    if (!entry || (entry.status !== 'loaded' && entry.status !== 'operation-error')) return
    const ws = entry.data

    // For worktree workspaces with a parent, open the Review tab
    if (ws.isWorktree && ws.parentId) {
      setActiveWorkspace(id)
      entry.store.getState().addTab<ReviewState>('review', {
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
    const entry = workspaces[workspaceId]
    if (entry && (entry.status === 'loaded' || entry.status === 'operation-error')) {
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
  const rootWorkspaceIds = Object.entries(workspaces)
    .filter(([, e]) => {
      if (e.status === 'loaded' || e.status === 'operation-error') return !e.data.parentId
      return true // loading/error entries are always top-level
    })
    .map(([id]) => id)

  // Get create child dialog parent handle
  const createChildDialogParentEntry = createChildDialogParentId ? workspaces[createChildDialogParentId] : undefined
  const createChildDialogParentHandle = createChildDialogParentEntry &&
    (createChildDialogParentEntry.status === 'loaded' || createChildDialogParentEntry.status === 'operation-error')
    ? createChildDialogParentEntry.store
    : null

  const handleWorkspaceClick = (id: string) => {
    setActiveWorkspace(id)
    setActiveView({ type: 'workspace', workspaceId: id, sessionId })
  }

  const renderWorkspace = (id: string, depth: number = 0): ReactNode => {
    const entry = workspaces[id]
    if (!entry) return null

    const children = getChildren(id)
    const isFocused = prefixState === 'workspace_focus' && focusedWorkspaceIds[focusedWorkspaceIndex] === id

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
        onQuickFork={handleQuickFork}
        onCreateChild={handleCreateChild}
        onRemove={handleRemove}
        onOpenSettings={handleOpenSettings}
        children={children}
        renderChild={renderWorkspace}
      />
    )
  }

  return (
    <div className="session-panel">
      <div className="session-panel-header" onClick={() => setActiveView({ type: 'session', sessionId })} onContextMenu={handleSessionContextMenu}>
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
            onChange={(e) => setEditName(e.target.value)}
            onFocus={(e) => e.target.select()}
            onKeyDown={(e) => {
              if (e.key === 'Enter') handleSaveName()
              if (e.key === 'Escape') setIsEditingName(false)
            }}
            onBlur={handleSaveName}
            onClick={(e) => e.stopPropagation()}
          />
        ) : (
          <span
            className="tree-title"
            onDoubleClick={(e) => { e.stopPropagation(); handleStartEditName() }}
            style={{ display: 'flex', alignItems: 'center', gap: 6 }}
          >
            {connection?.status === 'connecting' && <Loader2 size={14} className="spinning" />}
            {connection?.status === 'error' && <AlertCircle size={14} style={{ color: '#f44336' }} />}
            {displayName || sessionId}
          </span>
        )}
        {(!connection || connection.status === 'connected') && (
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
          {connection?.status === 'connecting' ? (
            <div className="tree-empty" style={{ fontSize: 12, padding: '4px 8px' }}>Connecting...</div>
          ) : connection?.status === 'error' ? (
            <div className="tree-empty" style={{ fontSize: 12, padding: '4px 8px' }}>{connection.error}</div>
          ) : rootWorkspaceIds.length === 0 ? (
            <div className="tree-empty">No workspaces. Click + to add one.</div>
          ) : (
            rootWorkspaceIds.map((id) => renderWorkspace(id))
          )}
        </div>
      )}

      {/* Session Context Menu */}
      <ContextMenu menuId="session-context" activeMenuId={activeMenuId} position={menuPosition}>
        <div className="context-menu-item danger" onClick={handleDisconnectSession}>
          Disconnect
        </div>
      </ContextMenu>

      {/* Upstream Warning Dialog */}
      {upstreamWarning && (
        <UpstreamWarningDialog
          behindCount={upstreamWarning.behindCount}
          workspaceName={upstreamWarning.workspaceName}
          onConfirm={handleUpstreamWarningConfirm}
          onCancel={() => setUpstreamWarning(null)}
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
          onCancel={() => setCreateChildDialogParentId(null)}
          openWorktreePaths={openWorktreePaths}
          initialMode="branch"
        />
      )}

      {/* Open Workspace Dialog */}
      {isOpenWorkspaceDialogOpen && (
        <OpenWorkspaceDialog
          onOpen={handleOpenWorkspaceSubmit}
          onCancel={() => setIsOpenWorkspaceDialogOpen(false)}
          selectFolder={selectFolder}
          connectionKey={connection?.target.type === 'remote'
            ? `${connection.target.config.user}@${connection.target.config.host}:${connection.target.config.port}`
            : 'local'}
          isRemote={connection?.target.type === 'remote'}
          readDirectory={connection?.target.type === 'remote'
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
  onOpenSettings: (id: string) => void
  children: Workspace[]
  renderChild: (id: string, depth: number) => ReactNode
}

function WorkspaceTreeItem({
  id, depth, entry, isActive, isFocused, isExpanded,
  onToggleExpand, onClick, onQuickFork, onCreateChild, onRemove, onOpenSettings,
  children, renderChild,
}: WorkspaceTreeItemProps): JSX.Element {
  const openContextMenu = useContextMenuStore((s) => s.open)
  const activeMenuId = useContextMenuStore((s) => s.activeMenuId)
  const menuPosition = useContextMenuStore((s) => s.position)
  const menuId = `ws-context-${id}`

  const ws = (entry.status === 'loaded' || entry.status === 'operation-error') ? entry.data : undefined
  const displayName = ws ? (ws.metadata?.displayName || ws.name) : (entry as { name: string }).name
  const hasChildren = children.length > 0
  const tabIds = ws ? Object.keys(ws.appStates) : []

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault()
    e.stopPropagation()
    openContextMenu(menuId, e.clientX, e.clientY)
  }

  return (
    <div>
      <div
        className={`tree-item ${depth === 0 ? 'tree-item-root' : ''} ${isActive ? 'active' : ''} ${isFocused ? 'focused' : ''}`}
        style={{ paddingLeft: 4 + depth * 4 }}
        onClick={() => onClick(id)}
        onContextMenu={handleContextMenu}
        title={ws?.metadata?.description ? `${ws.path}\n\n${ws.metadata.description}` : ws?.path}
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
          <WorkspaceIcon tabIds={tabIds} loadStatus={entry.status === 'loading' || entry.status === 'error' ? entry.status : undefined} isWorktree={ws?.isWorktree ?? false} />
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
          <div className="context-menu-item" onClick={() => onOpenSettings(id)}>
            Settings
          </div>
        )}
        {ws?.isGitRepo && (
          <div className="context-menu-item" onClick={() => onCreateChild(id)}>
            Open Existing Branch
          </div>
        )}
        {ws?.isWorktree && ws?.parentId && (
          <div className="context-menu-item" onClick={() => onRemove(id)}>
            Review & Merge
          </div>
        )}
        {ws && !ws.isWorktree && (
          <div className="context-menu-item danger" onClick={() => onRemove(id)}>
            Remove
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

export function CollapsedSessionPanel({ sessionId, sessionStore }: CollapsedSessionPanelProps): JSX.Element {
  const { workspaces, activeWorkspaceId, setActiveWorkspace } = useStore(sessionStore)
  const { activeView, setActiveView } = useNavigationStore()
  const isActiveSession = activeView?.type === 'workspace' && activeView.sessionId === sessionId

  const getChildren = (parentId: string): Workspace[] =>
    Object.entries(workspaces)
      .filter(([, e]) => (e.status === 'loaded' || e.status === 'operation-error') && e.data.parentId === parentId)
      .map(([, e]) => (e as Extract<typeof e, { status: 'loaded' | 'operation-error' }>).data)

  const rootWorkspaceIds = Object.entries(workspaces)
    .filter(([, e]) => {
      if (e.status === 'loaded' || e.status === 'operation-error') return !e.data.parentId
      return true
    })
    .map(([id]) => id)

  const handleClick = (id: string) => {
    setActiveWorkspace(id)
    setActiveView({ type: 'workspace', workspaceId: id, sessionId })
  }

  const renderIcon = (id: string): ReactNode => {
    const entry = workspaces[id]
    if (!entry) return null

    const ws = (entry.status === 'loaded' || entry.status === 'operation-error') ? entry.data : undefined
    const displayName = ws ? (ws.metadata?.displayName || ws.name) : (entry as { name: string }).name
    const tabIds = ws ? Object.keys(ws.appStates) : []
    const isActive = isActiveSession && activeWorkspaceId === id
    const children = getChildren(id)

    return (
      <div key={id}>
        <div
          className={`collapsed-workspace-icon ${isActive ? 'active' : ''}`}
          title={displayName}
          onClick={() => handleClick(id)}
        >
          <WorkspaceIcon
            tabIds={tabIds}
            loadStatus={entry.status === 'loading' || entry.status === 'error' ? entry.status : undefined}
            isWorktree={ws?.isWorktree ?? false}
          />
        </div>
        {children.map((child) => renderIcon(child.id))}
      </div>
    )
  }

  return <>{rootWorkspaceIds.map((id) => renderIcon(id))}</>
}
