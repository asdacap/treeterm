import { useState, useMemo, useEffect, useRef, useCallback } from 'react'
import { GitFork, GitBranch, Folder, ChevronDown, ChevronRight } from 'lucide-react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { SessionState } from '../store/createSessionStore'
import { useAppStore } from '../store/app'
import { useNavigationStore } from '../store/navigation'
import { usePrefixModeStore } from '../store/prefixMode'
import { useSessionNamesStore } from '../store/sessionNames'
import CreateChildDialog from './CreateChildDialog'
import OpenWorkspaceDialog from './OpenWorkspaceDialog'
import type { Workspace, ReviewState, WorktreeSettings } from '../types'

// Import WorkspaceActivityIndicator from TreePane
import { WorkspaceActivityIndicator } from './TreePane'
import { clampContextMenuPosition } from '../utils/contextMenuPosition'

interface ContextMenu {
  x: number
  y: number
  workspaceId: string
}

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
    workspaceStores,
    activeWorkspaceId,
    addWorkspace,
    addChildWorkspace,
    adoptExistingWorktree,
    createWorktreeFromBranch,
    createWorktreeFromRemote,
    quickForkWorkspace,
    setActiveWorkspace,
  } = useStore(sessionStore)
  const switchSession = useAppStore(s => s.switchSession)
  const { activeView, setActiveView } = useNavigationStore()
  const {
    state: prefixState,
    focusedWorkspaceIndex,
    workspaceIds: focusedWorkspaceIds
  } = usePrefixModeStore()

  const [contextMenu, setContextMenu] = useState<ContextMenu | null>(null)
  const [expanded, setExpanded] = useState<Set<string>>(() => {
    return new Set(
      Object.values(workspaces)
        .filter((ws) => ws.children.length > 0)
        .map((ws) => ws.id)
    )
  })
  const [createChildDialogParentId, setCreateChildDialogParentId] = useState<string | null>(null)
  const [isOpenWorkspaceDialogOpen, setIsOpenWorkspaceDialogOpen] = useState(false)
  const [isCollapsed, setIsCollapsed] = useState(false)

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
    const parentIds = Object.values(workspaces)
      .filter((ws) => ws.children.length > 0)
      .map((ws) => ws.id)
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
      .filter(ws => ws.isWorktree)
      .map(ws => ws.path)
  }, [workspaces])

  const handleAddWorkspace = () => {
    setIsOpenWorkspaceDialogOpen(true)
  }

  const handleOpenWorkspaceSubmit = async (path: string, settings?: WorktreeSettings) => {
    await addWorkspace(path, { settings })
    setIsOpenWorkspaceDialogOpen(false)
  }

  const handleContextMenu = (e: React.MouseEvent, id: string) => {
    e.preventDefault()
    e.stopPropagation()
    const pos = clampContextMenuPosition(e.clientX, e.clientY)
    setContextMenu({ ...pos, workspaceId: id })
  }

  const closeContextMenu = () => {
    setContextMenu(null)
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

  const handleCreateChildSubmit = async (name: string, isDetached: boolean, settings?: WorktreeSettings) => {
    if (!createChildDialogParentId) return { success: false, error: 'No parent selected' }

    const result = await addChildWorkspace(createChildDialogParentId, name, isDetached, settings)
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

  const handleCreateFromBranchSubmit = async (branch: string, isDetached: boolean, settings?: WorktreeSettings) => {
    if (!createChildDialogParentId) return { success: false, error: 'No parent selected' }

    const result = await createWorktreeFromBranch(createChildDialogParentId, branch, isDetached, settings)
    if (result.success) {
      setExpanded((prev) => new Set([...Array.from(prev), createChildDialogParentId]))
      setCreateChildDialogParentId(null)
    }
    return result
  }

  const handleCreateFromRemoteSubmit = async (remoteBranch: string, isDetached: boolean, settings?: WorktreeSettings) => {
    if (!createChildDialogParentId) return { success: false, error: 'No parent selected' }

    const result = await createWorktreeFromRemote(createChildDialogParentId, remoteBranch, isDetached, settings)
    if (result.success) {
      setExpanded((prev) => new Set([...Array.from(prev), createChildDialogParentId]))
      setCreateChildDialogParentId(null)
    }
    return result
  }

  const handleRemove = async (id: string) => {
    closeContextMenu()
    const ws = workspaces[id]

    // For worktree workspaces with a parent, open the Review tab
    if (ws.isWorktree && ws.parentId) {
      setActiveWorkspace(id)
      const handle = workspaceStores[id]
      if (handle) {
        handle.getState().addTab<ReviewState>('review', {
          parentWorkspaceId: ws.parentId
        })
      }
      return
    }

    // For regular workspaces, just confirm and remove
    const message = `Remove workspace "${ws.name}"?`
    if (confirm(message)) {
      await workspaceStores[id]!.getState().remove()
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

  // Get root workspaces (those without parents)
  const rootWorkspaces = Object.values(workspaces).filter((ws) => !ws.parentId)

  // Get create child dialog parent handle
  const createChildDialogParentHandle = createChildDialogParentId
    ? workspaceStores[createChildDialogParentId] ?? null
    : null

  const handleWorkspaceClick = (ws: Workspace) => {
    switchSession(sessionId)
    setActiveWorkspace(ws.id)
    setActiveView({ type: 'workspace', workspaceId: ws.id, sessionId })
  }

  const renderWorkspace = (ws: Workspace, depth: number = 0) => {
    const hasChildren = ws.children.length > 0
    const isExpanded = expanded.has(ws.id)
    const children = ws.children.map((id) => workspaces[id]).filter(Boolean)
    const tabIds = Object.keys(ws.appStates)

    // Check if this workspace is focused in workspace_focus mode
    const isFocused =
      prefixState === 'workspace_focus' &&
      focusedWorkspaceIds[focusedWorkspaceIndex] === ws.id

    return (
      <div key={ws.id}>
        <div
          className={`tree-item ${isActiveSession && activeWorkspaceId === ws.id ? 'active' : ''} ${isFocused ? 'focused' : ''}`}
          style={{ paddingLeft: 4 + depth * 4 }}
          onClick={() => handleWorkspaceClick(ws)}
          onContextMenu={(e) => handleContextMenu(e, ws.id)}
          title={ws.metadata?.description ? `${ws.path}\n\n${ws.metadata.description}` : ws.path}
        >
          {hasChildren ? (
            <span
              className="tree-item-expand"
              onClick={(e) => {
                e.stopPropagation()
                toggleExpand(ws.id)
              }}
            >
              {isExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            </span>
          ) : (
            <span className="tree-item-expand-placeholder" />
          )}
          <span className="tree-item-icon">{ws.isWorktree ? <GitBranch size={16} /> : <Folder size={16} />}</span>
          <span className="tree-item-name">
            {ws.metadata?.displayName || ws.name}
          </span>

          <WorkspaceActivityIndicator tabIds={tabIds} />
          <span className="tree-item-actions">
            {ws.isGitRepo && (
              <button
                className="tree-item-action"
                title="Fork"
                onClick={(e) => { e.stopPropagation(); handleQuickFork(ws.id) }}
              >
                <GitFork size={14} />
              </button>
            )}

          </span>
        </div>

        {/* Children */}
        {isExpanded && children.map((child) => renderWorkspace(child, depth + 1))}
      </div>
    )
  }

  return (
    <div className="session-panel" onClick={closeContextMenu}>
      <div className="session-panel-header" onClick={() => setIsCollapsed(!isCollapsed)}>
        <span className="session-panel-expand">
          {isCollapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
        </span>
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

      {!isCollapsed && (
        <>
          <div className="tree-list">
            {rootWorkspaces.length === 0 ? (
              <div className="tree-empty">No workspaces. Click + to add one.</div>
            ) : (
              rootWorkspaces.map((ws) => renderWorkspace(ws))
            )}
          </div>

          {/* SSH Connection for this session */}
          {connection && connection.target.type === 'remote' && (() => {
            const config = connection.target.config
            const label = `${config.user}@${config.host}`
            const isActive = activeView?.type === 'ssh' && activeView.connectionId === connection.id
            return (
              <div className="tree-list ssh-connections-list">
                <div
                  className={`tree-item ${isActive ? 'active' : ''}`}
                  onClick={() => setActiveView({ type: 'ssh', connectionId: connection.id })}
                  title={label}
                >
                  <span className="tree-item-expand-placeholder" />
                  <span className="tree-item-icon">
                    <span
                      className="ssh-status-dot"
                      style={{
                        backgroundColor:
                          connection.status === 'connected' ? '#4caf50' :
                          connection.status === 'connecting' ? '#ff9800' :
                          connection.status === 'error' ? '#f44336' : '#666'
                      }}
                    />
                  </span>
                  <span className="tree-item-name">{label}</span>
                  <span className="ssh-status-text">{connection.status}</span>
                </div>
              </div>
            )
          })()}
        </>
      )}

      {/* Context Menu */}
      {contextMenu && (
        <div
          className="context-menu"
          style={{ top: contextMenu.y, left: contextMenu.x }}
          onClick={(e) => e.stopPropagation()}
        >
          {workspaces[contextMenu.workspaceId]?.isGitRepo && (
            <div className="context-menu-item" onClick={() => handleCreateChild(contextMenu.workspaceId)}>
              Open Existing Branch
            </div>
          )}
          {workspaces[contextMenu.workspaceId]?.isWorktree && workspaces[contextMenu.workspaceId]?.parentId && (
            <div className="context-menu-item" onClick={() => handleRemove(contextMenu.workspaceId)}>
              Review & Merge
            </div>
          )}
          {!workspaces[contextMenu.workspaceId]?.isWorktree && (
            <div className="context-menu-item danger" onClick={() => handleRemove(contextMenu.workspaceId)}>
              Remove
            </div>
          )}
        </div>
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
        />
      )}
    </div>
  )
}
