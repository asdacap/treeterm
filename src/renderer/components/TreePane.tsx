import { useState, useMemo } from 'react'
import { Loader2 } from 'lucide-react'
import { useWorkspaceStore } from '../store/workspace'
import { useActivityStateStore } from '../store/activityState'
import MergeDialog from './MergeDialog'
import CreateChildDialog from './CreateChildDialog'
import type { Workspace, ActivityState } from '../types'

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

export default function TreePane() {
  const {
    workspaces,
    activeWorkspaceId,
    addWorkspace,
    addChildWorkspace,
    removeWorkspace,
    mergeAndRemoveWorkspace,
    setActiveWorkspace
  } = useWorkspaceStore()
  const [contextMenu, setContextMenu] = useState<ContextMenu | null>(null)
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [mergeDialogWorkspaceId, setMergeDialogWorkspaceId] = useState<string | null>(null)
  const [createChildDialogParentId, setCreateChildDialogParentId] = useState<string | null>(null)

  const handleAddWorkspace = async () => {
    const path = await window.electron.selectFolder()
    if (path) {
      await addWorkspace(path)
    }
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

  const handleCreateChildSubmit = async (name: string) => {
    if (!createChildDialogParentId) return { success: false, error: 'No parent selected' }

    const result = await addChildWorkspace(createChildDialogParentId, name)
    if (result.success) {
      // Expand the parent to show the new child
      setExpanded((prev) => new Set([...prev, createChildDialogParentId]))
      setCreateChildDialogParentId(null)
    }
    return result
  }

  const handleRemove = async (id: string) => {
    closeContextMenu()
    const workspace = workspaces[id]

    // For worktree workspaces with a parent, show the merge dialog
    if (workspace.isWorktree && workspace.parentId) {
      setMergeDialogWorkspaceId(id)
      return
    }

    // For regular workspaces, just confirm and remove
    const message = `Remove workspace "${workspace.name}"?`
    if (confirm(message)) {
      await removeWorkspace(id)
    }
  }

  const handleMerge = async (squash: boolean) => {
    if (!mergeDialogWorkspaceId) return
    const result = await mergeAndRemoveWorkspace(mergeDialogWorkspaceId, squash)
    if (!result.success) {
      throw new Error(result.error)
    }
    setMergeDialogWorkspaceId(null)
  }

  const handleAbandon = async () => {
    if (!mergeDialogWorkspaceId) return
    await removeWorkspace(mergeDialogWorkspaceId)
    setMergeDialogWorkspaceId(null)
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

  // Get merge dialog workspace and parent
  const mergeDialogWorkspace = mergeDialogWorkspaceId ? workspaces[mergeDialogWorkspaceId] : null
  const mergeDialogParent =
    mergeDialogWorkspace?.parentId ? workspaces[mergeDialogWorkspace.parentId] : null

  // Get create child dialog parent
  const createChildDialogParent = createChildDialogParentId
    ? workspaces[createChildDialogParentId]
    : null

  const renderWorkspace = (ws: Workspace, depth: number = 0) => {
    const hasChildren = ws.children.length > 0
    const isExpanded = expanded.has(ws.id)
    const children = ws.children.map((id) => workspaces[id]).filter(Boolean)
    const tabIds = ws.tabs.map((t) => t.id)

    return (
      <div key={ws.id}>
        <div
          className={`tree-item ${activeWorkspaceId === ws.id ? 'active' : ''}`}
          style={{ paddingLeft: 16 + depth * 16 }}
          onClick={() => setActiveWorkspace(ws.id)}
          onContextMenu={(e) => handleContextMenu(e, ws.id)}
          title={ws.path}
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
          <span className="tree-item-name">{ws.name}</span>
          <WorkspaceActivityIndicator tabIds={tabIds} />
          {ws.isGitRepo && ws.gitBranch && (
            <span className="tree-item-branch">{ws.gitBranch}</span>
          )}
        </div>

        {/* Children */}
        {isExpanded && children.map((child) => renderWorkspace(child, depth + 1))}
      </div>
    )
  }

  return (
    <div className="tree-pane-content" onClick={closeContextMenu}>
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

      {/* Merge Dialog */}
      {mergeDialogWorkspace && mergeDialogParent && (
        <MergeDialog
          workspace={mergeDialogWorkspace}
          parentWorkspace={mergeDialogParent}
          onMerge={handleMerge}
          onAbandon={handleAbandon}
          onCancel={() => setMergeDialogWorkspaceId(null)}
        />
      )}

      {/* Create Child Dialog */}
      {createChildDialogParent && (
        <CreateChildDialog
          parentWorkspace={createChildDialogParent}
          onCreate={handleCreateChildSubmit}
          onCancel={() => setCreateChildDialogParentId(null)}
        />
      )}
    </div>
  )
}
