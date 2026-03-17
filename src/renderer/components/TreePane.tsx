import { useState, useMemo } from 'react'
import { Loader2 } from 'lucide-react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { WorkspaceState } from '../store/createWorkspaceStore'
import { useActivityStateStore } from '../store/activityState'
import { usePrefixModeStore } from '../store/prefixMode'
import { useAppStore } from '../store/app'
import CreateChildDialog from './CreateChildDialog'
import OpenWorkspaceDialog from './OpenWorkspaceDialog'
import type { Workspace, ActivityState, ReviewState, WorktreeSettings } from '../types'

interface ContextMenu {
  x: number
  y: number
  workspaceId: string
}

// Small component to subscribe to activity state changes for a workspace
function WorkspaceActivityIndicator({ tabIds }: { tabIds: string[] }) {
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
      {activityState === 'working' ? <Loader2 size={10} /> : '●'}
    </span>
  )
}

interface TreePaneProps {
  workspaceStore: StoreApi<WorkspaceState>
  selectFolder: () => Promise<string | null>
  getRecentDirectories: () => Promise<string[]>
}

export default function TreePane({ workspaceStore, selectFolder, getRecentDirectories }: TreePaneProps): JSX.Element {
  const {
    workspaces,
    activeWorkspaceId,
    addWorkspace,
    addChildWorkspace,
    addTabWithState,
    adoptExistingWorktree,
    createWorktreeFromBranch,
    createWorktreeFromRemote,
    removeWorkspace,
    mergeAndRemoveWorkspace,
    setActiveWorkspace,
    updateWorkspaceMetadata
  } = useStore(workspaceStore)

  const { activeSessionId, workspaceStores, switchSession } = useAppStore()
  const sessionIds = Object.keys(workspaceStores)
  const {
    state: prefixState,
    focusedWorkspaceIndex,
    workspaceIds: focusedWorkspaceIds
  } = usePrefixModeStore()
  const [contextMenu, setContextMenu] = useState<ContextMenu | null>(null)
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [createChildDialogParentId, setCreateChildDialogParentId] = useState<string | null>(null)
  const [isOpenWorkspaceDialogOpen, setIsOpenWorkspaceDialogOpen] = useState(false)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editValue, setEditValue] = useState('')
  const [editDescriptionId, setEditDescriptionId] = useState<string | null>(null)
  const [editDescriptionValue, setEditDescriptionValue] = useState('')

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
    setContextMenu({ x: e.clientX, y: e.clientY, workspaceId: id })
  }

  const closeContextMenu = () => {
    setContextMenu(null)
  }

  const handleCreateChild = (parentId: string) => {
    closeContextMenu()
    setCreateChildDialogParentId(parentId)
  }

  const handleCreateChildSubmit = async (name: string, isDetached: boolean, settings?: WorktreeSettings) => {
    if (!createChildDialogParentId) return { success: false, error: 'No parent selected' }

    const result = await addChildWorkspace(createChildDialogParentId, name, isDetached, settings)
    if (result.success) {
      // Expand the parent to show the new child
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
    const workspace = workspaces[id]

    // For worktree workspaces with a parent, open the Review tab
    if (workspace.isWorktree && workspace.parentId) {
      setActiveWorkspace(id)
      addTabWithState<ReviewState>(id, 'review', {
        parentWorkspaceId: workspace.parentId
      })
      return
    }

    // For regular workspaces, just confirm and remove
    const message = `Remove workspace "${workspace.name}"?`
    if (confirm(message)) {
      await removeWorkspace(id)
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

  // Get create child dialog parent
  const createChildDialogParent = createChildDialogParentId
    ? workspaces[createChildDialogParentId]
    : null

  const renderWorkspace = (ws: Workspace, depth: number = 0) => {
    const hasChildren = ws.children.length > 0
    const isExpanded = expanded.has(ws.id)
    const children = ws.children.map((id) => workspaces[id]).filter(Boolean)
    const tabIds = ws.tabs.map((t) => t.id)

    // Check if this workspace is focused in workspace_focus mode
    const isFocused =
      prefixState === 'workspace_focus' &&
      focusedWorkspaceIds[focusedWorkspaceIndex] === ws.id

    return (
      <div key={ws.id}>
        <div
          className={`tree-item ${activeWorkspaceId === ws.id ? 'active' : ''} ${isFocused ? 'focused' : ''}`}
          style={{ paddingLeft: 16 + depth * 16 }}
          onClick={() => setActiveWorkspace(ws.id)}
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
              {isExpanded ? '▼' : '▶'}
            </span>
          ) : (
            <span className="tree-item-expand-placeholder" />
          )}
          <span className="tree-item-icon">{ws.isWorktree ? '🌿' : '📁'}</span>
          {editingId === ws.id ? (
            <input
              className="tree-item-name-input"
              value={editValue}
              onChange={(e) => setEditValue(e.target.value)}
              onBlur={() => {
                if (editValue.trim()) {
                  updateWorkspaceMetadata(ws.id, 'displayName', editValue.trim())
                }
                setEditingId(null)
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  if (editValue.trim()) {
                    updateWorkspaceMetadata(ws.id, 'displayName', editValue.trim())
                  }
                  setEditingId(null)
                } else if (e.key === 'Escape') {
                  setEditingId(null)
                }
              }}
              onClick={(e) => e.stopPropagation()}
              autoFocus
            />
          ) : (
            <span
              className="tree-item-name"
              onDoubleClick={(e) => {
                e.stopPropagation()
                setEditingId(ws.id)
                setEditValue(ws.metadata?.displayName || ws.name)
              }}
            >
              {ws.metadata?.displayName || ws.name}
            </span>
          )}
          <WorkspaceActivityIndicator tabIds={tabIds} />
        </div>

        {/* Children */}
        {isExpanded && children.map((child) => renderWorkspace(child, depth + 1))}
      </div>
    )
  }

  return (
    <div className="tree-pane-content" onClick={closeContextMenu}>
      {sessionIds.length > 1 && (
        <div className="session-selector">
          <select
            value={activeSessionId || ''}
            onChange={(e) => switchSession(e.target.value)}
          >
            {sessionIds.map((id) => (
              <option key={id} value={id}>{id}</option>
            ))}
          </select>
        </div>
      )}
      <div className="tree-header">
        <span className="tree-title">Workspaces</span>
        <button className="add-button" onClick={handleAddWorkspace} title="Add workspace">
          +
        </button>
      </div>
      <div className="tree-list">
        {rootWorkspaces.length === 0 ? (
          <div className="tree-empty">No workspaces yet. Click + to add one.</div>
        ) : (
          rootWorkspaces.map((ws) => renderWorkspace(ws))
        )}
      </div>

      {/* Context Menu */}
      {contextMenu && (
        <div
          className="context-menu"
          style={{ top: contextMenu.y, left: contextMenu.x }}
          onClick={(e) => e.stopPropagation()}
        >
          <div className="context-menu-item" onClick={() => {
            const ws = workspaces[contextMenu.workspaceId]
            if (ws) {
              setEditingId(ws.id)
              setEditValue(ws.metadata?.displayName || ws.name)
            }
            closeContextMenu()
          }}>
            Rename
          </div>
          <div className="context-menu-item" onClick={() => {
            const ws = workspaces[contextMenu.workspaceId]
            if (ws) {
              setEditDescriptionId(ws.id)
              setEditDescriptionValue(ws.metadata?.description || '')
            }
            closeContextMenu()
          }}>
            Edit Description
          </div>
          {workspaces[contextMenu.workspaceId]?.isGitRepo && (
            <div className="context-menu-item" onClick={() => handleCreateChild(contextMenu.workspaceId)}>
              New Child Workspace
            </div>
          )}
          <div className="context-menu-item danger" onClick={() => handleRemove(contextMenu.workspaceId)}>
            {workspaces[contextMenu.workspaceId]?.isWorktree ? 'Close & Merge...' : 'Remove'}
          </div>
        </div>
      )}

      {/* Create Child Dialog */}
      {createChildDialogParent && (
        <CreateChildDialog
          parentWorkspace={createChildDialogParent}
          onCreate={handleCreateChildSubmit}
          onAdopt={handleAdoptWorktreeSubmit}
          onCreateFromBranch={handleCreateFromBranchSubmit}
          onCreateFromRemote={handleCreateFromRemoteSubmit}
          onCancel={() => setCreateChildDialogParentId(null)}
          openWorktreePaths={openWorktreePaths}
        />
      )}

      {/* Open Workspace Dialog */}
      {isOpenWorkspaceDialogOpen && (
        <OpenWorkspaceDialog
          onOpen={handleOpenWorkspaceSubmit}
          onCancel={() => setIsOpenWorkspaceDialogOpen(false)}
          selectFolder={selectFolder}
          getRecentDirectories={getRecentDirectories}
        />
      )}

      {/* Edit Description Dialog */}
      {editDescriptionId && (
        <div className="dialog-overlay" onClick={() => setEditDescriptionId(null)}>
          <div className="dialog" onClick={(e) => e.stopPropagation()}>
            <h3>Edit Description</h3>
            <textarea
              className="tree-item-description-input"
              value={editDescriptionValue}
              onChange={(e) => setEditDescriptionValue(e.target.value)}
              placeholder="Enter a description..."
              rows={3}
              autoFocus
            />
            <div className="dialog-actions">
              <button onClick={() => setEditDescriptionId(null)}>Cancel</button>
              <button onClick={() => {
                updateWorkspaceMetadata(editDescriptionId, 'description', editDescriptionValue.trim())
                setEditDescriptionId(null)
              }}>
                Save
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
