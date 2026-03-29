import { useState, useMemo, useEffect, useRef, useCallback } from 'react'
import { useContextMenuStore } from '../store/contextMenu'
import ContextMenu from './ContextMenu'
import { GitFork, ChevronDown, ChevronRight, Loader2, AlertCircle } from 'lucide-react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { SessionState, SessionEntry } from '../store/createSessionStore'
import { useAppStore } from '../store/app'
import { useNavigationStore } from '../store/navigation'
import { useKeybindingStore } from '../store/keybinding'
import { useSessionNamesStore } from '../store/sessionNames'
import CreateChildDialog from './CreateChildDialog'
import OpenWorkspaceDialog from './OpenWorkspaceDialog'
import type { ReviewState, WorktreeSettings } from '../types'

// Import WorkspaceIcon from TreePane
import { WorkspaceIcon } from './TreePane'

interface SessionPanelProps {
  sessionId: string
  sessionEntry: SessionEntry
  selectFolder: () => Promise<string | null>
}

export default function SessionPanel({
  sessionId,
  sessionEntry,
  selectFolder,
}: SessionPanelProps): JSX.Element {
  if (sessionEntry.status === 'connected') {
    return (
      <ConnectedSessionPanel
        sessionId={sessionId}
        sessionStore={sessionEntry.store}
        selectFolder={selectFolder}
      />
    )
  }

  // Connecting or error state — render minimal panel
  return (
    <PendingSessionPanel
      sessionId={sessionId}
      sessionEntry={sessionEntry}
    />
  )
}

// Minimal panel for connecting/error sessions
function PendingSessionPanel({
  sessionId,
  sessionEntry,
}: {
  sessionId: string
  sessionEntry: Extract<SessionEntry, { status: 'connecting' | 'error' }>
}) {
  const { setActiveView } = useNavigationStore()

  const label = sessionEntry.config.label || `${sessionEntry.config.user}@${sessionEntry.config.host}`

  return (
    <div className="session-panel">
      <div
        className="session-panel-header"
        onClick={() => setActiveView({ type: 'session', sessionId })}
      >
        <span className="tree-title" style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          {sessionEntry.status === 'connecting' && <Loader2 size={14} className="spinning" />}
          {sessionEntry.status === 'error' && <AlertCircle size={14} style={{ color: '#f44336' }} />}
          {label}
        </span>
      </div>
      <div className="tree-list">
        <div className="tree-empty" style={{ fontSize: 12, padding: '4px 8px' }}>
          {sessionEntry.status === 'connecting' ? 'Connecting...' : sessionEntry.error}
        </div>
      </div>
    </div>
  )
}

// Full panel for connected sessions (original implementation)
function ConnectedSessionPanel({
  sessionId,
  sessionStore,
  selectFolder,
}: {
  sessionId: string
  sessionStore: StoreApi<SessionState>
  selectFolder: () => Promise<string | null>
}): JSX.Element {
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

  const openContextMenu = useContextMenuStore((s) => s.open)
  const closeContextMenu = useContextMenuStore((s) => s.close)
  const [contextMenuWorkspaceId, setContextMenuWorkspaceId] = useState('')
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

  // Session name editing
  const displayName = useSessionNamesStore(s => s.names[sessionId]?.name)
  const setSessionName = useSessionNamesStore(s => s.setName)
  const removeSessionName = useSessionNamesStore(s => s.removeName)
  const [isEditingName, setIsEditingName] = useState(false)
  const [editName, setEditName] = useState('')
  const nameInputRef = useRef<HTMLInputElement>(null)

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

  useEffect(() => {
    if (isEditingName && nameInputRef.current) {
      nameInputRef.current.focus()
      nameInputRef.current.select()
    }
  }, [isEditingName])

  const isActiveSession = activeView?.type === 'workspace' && activeView.sessionId === sessionId

  // Expand any workspace that gains children after mount
  useEffect(() => {
    const parentIds = Object.keys(workspaces)
      .filter((id) => getChildren(id).length > 0)
    if (parentIds.length > 0) {
      setExpanded((prev) => {
        const next = new Set(prev)
        for (const id of parentIds) next.add(id)
        return next
      })
    }
  }, [workspaces])

  // Compute paths of already-open worktrees
  const openWorktreePaths = useMemo(() => {
    return Object.values(workspaces)
      .filter((e): e is Extract<typeof e, { status: 'loaded' | 'operation-error' }> =>
        e.status === 'loaded' || e.status === 'operation-error')
      .filter(e => e.data.isWorktree)
      .map(e => e.data.path)
  }, [workspaces])

  const handleAddWorkspace = () => {
    setIsOpenWorkspaceDialogOpen(true)
  }

  const handleOpenWorkspaceSubmit = (path: string, settings?: WorktreeSettings) => {
    addWorkspace(path, { settings })
    setIsOpenWorkspaceDialogOpen(false)
  }

  const handleContextMenu = (e: React.MouseEvent, id: string) => {
    e.preventDefault()
    e.stopPropagation()
    setContextMenuWorkspaceId(id)
    openContextMenu('session-ws-context', e.clientX, e.clientY)
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
    setCreateChildDialogParentId(parentId)
  }


  const handleQuickFork = async (wsId: string) => {
    const result = await quickForkWorkspace(wsId)
    if (result.success) {
      setExpanded((prev) => new Set([...Array.from(prev), wsId]))
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

  const renderWorkspace = (id: string, depth: number = 0) => {
    const entry = workspaces[id]
    if (!entry) return null

    // For loaded/operation-error entries, use full data
    const ws = (entry.status === 'loaded' || entry.status === 'operation-error') ? entry.data : undefined
    const displayName = ws ? (ws.metadata?.displayName || ws.name) : (entry as { name: string }).name
    const children = getChildren(id)
    const hasChildren = children.length > 0
    const isExpanded = expanded.has(id)
    const tabIds = ws ? Object.keys(ws.appStates) : []

    // Check if this workspace is focused in workspace_focus mode
    const isFocused =
      prefixState === 'workspace_focus' &&
      focusedWorkspaceIds[focusedWorkspaceIndex] === id

    return (
      <div key={id}>
        <div
          className={`tree-item ${depth === 0 ? 'tree-item-root' : ''} ${isActiveSession && activeWorkspaceId === id ? 'active' : ''} ${isFocused ? 'focused' : ''}`}
          style={{ paddingLeft: 4 + depth * 4 }}
          onClick={() => handleWorkspaceClick(id)}
          onContextMenu={(e) => handleContextMenu(e, id)}
          title={ws?.metadata?.description ? `${ws.path}\n\n${ws.metadata.description}` : ws?.path}
        >
          {hasChildren ? (
            <span
              className="tree-item-expand"
              onClick={(e) => {
                e.stopPropagation()
                toggleExpand(id)
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
                onClick={(e) => { e.stopPropagation(); handleQuickFork(id) }}
              >
                <GitFork size={16} />
              </button>
            )}

          </span>
        </div>

        {/* Children */}
        {isExpanded && children.map((child) => renderWorkspace(child.id, depth + 1))}
      </div>
    )
  }

  return (
    <div className="session-panel">
      <div className="session-panel-header" onClick={() => setActiveView({ type: 'session', sessionId })} onContextMenu={handleSessionContextMenu}>
        {isEditingName ? (
          <input
            ref={nameInputRef}
            className="tree-title-input"
            value={editName}
            onChange={(e) => setEditName(e.target.value)}
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
          >
            {displayName || sessionId}
          </span>
        )}
        <button
          className="add-button"
          onClick={(e) => { e.stopPropagation(); handleAddWorkspace() }}
          title="Add workspace"
        >
          +
        </button>
      </div>

      <div className="tree-list">
        {rootWorkspaceIds.length === 0 ? (
          <div className="tree-empty">No workspaces. Click + to add one.</div>
        ) : (
          rootWorkspaceIds.map((id) => renderWorkspace(id))
        )}
      </div>

      {/* Context Menu */}
      <ContextMenu menuId="session-ws-context">
        {(() => {
          const ctxEntry = workspaces[contextMenuWorkspaceId]
          const ctxWs = ctxEntry && (ctxEntry.status === 'loaded' || ctxEntry.status === 'operation-error') ? ctxEntry.data : undefined
          return (
            <>
              {ctxWs && (
                <div className="context-menu-item" onClick={() => handleOpenSettings(contextMenuWorkspaceId)}>
                  Settings
                </div>
              )}
              {ctxWs?.isGitRepo && (
                <div className="context-menu-item" onClick={() => handleCreateChild(contextMenuWorkspaceId)}>
                  Open Existing Branch
                </div>
              )}
              {ctxWs?.isWorktree && ctxWs?.parentId && (
                <div className="context-menu-item" onClick={() => handleRemove(contextMenuWorkspaceId)}>
                  Review & Merge
                </div>
              )}
              {ctxWs && !ctxWs.isWorktree && (
                <div className="context-menu-item danger" onClick={() => handleRemove(contextMenuWorkspaceId)}>
                  Remove
                </div>
              )}
            </>
          )
        })()}
      </ContextMenu>

      {/* Session Context Menu */}
      <ContextMenu menuId="session-context">
        <div className="context-menu-item danger" onClick={handleDisconnectSession}>
          Disconnect
        </div>
      </ContextMenu>

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
        />
      )}
    </div>
  )
}
