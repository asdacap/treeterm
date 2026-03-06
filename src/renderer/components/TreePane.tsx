import { useState } from 'react'
import { useWorkspaceStore } from '../store/workspace'
import MergeDialog from './MergeDialog'
import type { Workspace } from '../types'

interface ContextMenu {
  x: number
  y: number
  workspaceId: string
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
  const [newChildName, setNewChildName] = useState('')
  const [showNewChildInput, setShowNewChildInput] = useState<string | null>(null)
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [mergeDialogWorkspaceId, setMergeDialogWorkspaceId] = useState<string | null>(null)

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
    setShowNewChildInput(parentId)
    setNewChildName('')
    // Expand the parent
    setExpanded((prev) => new Set([...prev, parentId]))
  }

  const handleSubmitNewChild = async (parentId: string) => {
    if (!newChildName.trim()) {
      setShowNewChildInput(null)
      return
    }

    const result = await addChildWorkspace(parentId, newChildName.trim())
    if (!result.success) {
      alert(result.error)
    }
    setShowNewChildInput(null)
    setNewChildName('')
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

  const renderWorkspace = (ws: Workspace, depth: number = 0) => {
    const hasChildren = ws.children.length > 0
    const isExpanded = expanded.has(ws.id)
    const children = ws.children.map((id) => workspaces[id]).filter(Boolean)

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
          {ws.isGitRepo && ws.gitBranch && (
            <span className="tree-item-branch">{ws.gitBranch}</span>
          )}
        </div>

        {/* New child input */}
        {showNewChildInput === ws.id && (
          <div className="tree-new-child" style={{ paddingLeft: 32 + depth * 16 }}>
            <input
              type="text"
              value={newChildName}
              onChange={(e) => setNewChildName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') handleSubmitNewChild(ws.id)
                if (e.key === 'Escape') setShowNewChildInput(null)
              }}
              onBlur={() => handleSubmitNewChild(ws.id)}
              placeholder="Workspace name..."
              autoFocus
            />
          </div>
        )}

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
    </div>
  )
}
