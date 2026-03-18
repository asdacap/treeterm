import { useState, useMemo, useEffect } from 'react'
import { Loader2, GitFork, GitBranch, Folder, ChevronDown, ChevronRight, Circle } from 'lucide-react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { WorkspaceState } from '../store/createWorkspaceStore'
import { useActivityStateStore } from '../store/activityState'
import { usePrefixModeStore } from '../store/prefixMode'
import { useAppStore } from '../store/app'
import { useGitApi } from '../contexts/GitApiContext'
import { humanId } from 'human-id'
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
      {activityState === 'working' ? <Loader2 size={10} /> : <Circle size={8} fill="currentColor" />}
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
    setActiveWorkspace
  } = useStore(workspaceStore)

  const git = useGitApi()
  const { activeSessionId, workspaceStores, switchSession } = useAppStore()
  const sessionIds = Object.keys(workspaceStores)
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

  // Expand any workspace that gains children after mount (e.g., async session restore)
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
    setContextMenu({ x: e.clientX, y: e.clientY, workspaceId: id })
  }

  const closeContextMenu = () => {
    setContextMenu(null)
  }

  const handleCreateChild = (parentId: string) => {
    closeContextMenu()
    setCreateChildDialogParentId(parentId)
  }

  const handleQuickFork = async (wsId: string) => {
    const ws = workspaces[wsId]
    if (!ws?.gitRootPath) return

    const existingBranches = await git.listLocalBranches(ws.gitRootPath)
    const parentBranch = ws.gitBranch || ''

    let name: string | null = null
    for (let i = 0; i < 3; i++) {
      const candidate = humanId({ separator: '-', capitalize: false })
      const fullBranch = parentBranch ? `${parentBranch}/${candidate}` : candidate
      if (!existingBranches.includes(fullBranch)) {
        name = candidate
        break
      }
    }

    if (!name) throw new Error('Failed to generate unique branch name')

    const result = await addChildWorkspace(wsId, name, false)
    if (result.success) {
      setExpanded((prev) => new Set([...Array.from(prev), wsId]))
    }
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
      {createChildDialogParent && (
        <CreateChildDialog
          parentWorkspace={createChildDialogParent}
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
          getRecentDirectories={getRecentDirectories}
        />
      )}

    </div>
  )
}
