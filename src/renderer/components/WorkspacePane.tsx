import { useEffect, useCallback, useState, useMemo } from 'react'
import { useWorkspaceStore } from '../store/workspace'
import { applicationRegistry } from '../registry/applicationRegistry'
import { usePrefixKeybindings } from '../hooks/usePrefixKeybindings'
import TabBar from './TabBar'
import MergeDialog from './MergeDialog'
import CreateChildDialog from './CreateChildDialog'
import PrefixModeIndicator from './PrefixModeIndicator'
// Import applications to ensure they are registered
import '../applications'

export default function WorkspacePane() {
  const {
    workspaces,
    activeWorkspaceId,
    addTab,
    removeTab,
    setActiveTab,
    addChildWorkspace,
    adoptExistingWorktree,
    removeWorkspace,
    mergeAndRemoveWorkspace
  } = useWorkspaceStore()

  const activeWorkspace = activeWorkspaceId ? workspaces[activeWorkspaceId] : null

  // Dialog state
  const [showCreateChildDialog, setShowCreateChildDialog] = useState(false)
  const [showMergeDialog, setShowMergeDialog] = useState(false)

  const handleNewTab = useCallback(
    (applicationId: string) => {
      if (activeWorkspaceId) {
        addTab(activeWorkspaceId, applicationId)
      }
    },
    [activeWorkspaceId, addTab]
  )

  // Create new tab using the first available application
  const handleNewDefaultTab = useCallback(() => {
    // Find the first app that allows new tabs
    const menuApps = applicationRegistry.getMenuItems()
    const defaultApp = menuApps.find((app) => app.canHaveMultiple)
    if (activeWorkspaceId && defaultApp) {
      addTab(activeWorkspaceId, defaultApp.id)
    }
  }, [activeWorkspaceId, addTab])

  const handleCloseTab = useCallback(
    (tabId: string) => {
      if (activeWorkspaceId) {
        removeTab(activeWorkspaceId, tabId)
      }
    },
    [activeWorkspaceId, removeTab]
  )

  const handleSelectTab = useCallback(
    (tabId: string) => {
      if (activeWorkspaceId) {
        setActiveTab(activeWorkspaceId, tabId)
      }
    },
    [activeWorkspaceId, setActiveTab]
  )

  // Compute paths of already-open worktrees
  const openWorktreePaths = useMemo(() => {
    return Object.values(workspaces)
      .filter(ws => ws.isWorktree)
      .map(ws => ws.path)
  }, [workspaces])

  // Fork handler - create new worktree
  const handleCreateChildSubmit = async (name: string) => {
    if (!activeWorkspaceId) return { success: false, error: 'No workspace selected' }

    const result = await addChildWorkspace(activeWorkspaceId, name)
    if (result.success) {
      setShowCreateChildDialog(false)
    }
    return result
  }

  // Adopt existing worktree handler
  const handleAdoptWorktreeSubmit = async (worktreePath: string, branch: string, name: string) => {
    if (!activeWorkspaceId) return { success: false, error: 'No workspace selected' }

    const result = await adoptExistingWorktree(activeWorkspaceId, worktreePath, branch, name)
    if (result.success) {
      setShowCreateChildDialog(false)
    }
    return result
  }

  // Merge handlers
  const handleMerge = async (squash: boolean) => {
    if (!activeWorkspaceId) return
    const result = await mergeAndRemoveWorkspace(activeWorkspaceId, squash)
    if (!result.success) {
      throw new Error(result.error)
    }
    setShowMergeDialog(false)
  }

  const handleAbandon = async () => {
    if (!activeWorkspaceId) return
    await removeWorkspace(activeWorkspaceId)
    setShowMergeDialog(false)
  }

  // Keybinding handlers for prefix mode hook
  const keybindingHandlers = useMemo(
    () => ({
      newTab: handleNewDefaultTab,
      closeTab: () => {
        if (activeWorkspace?.activeTabId && activeWorkspace.tabs.length > 1) {
          handleCloseTab(activeWorkspace.activeTabId)
        }
      },
      nextTab: () => {
        if (!activeWorkspace) return
        const currentIndex = activeWorkspace.tabs.findIndex(
          (t) => t.id === activeWorkspace.activeTabId
        )
        const newIndex = currentIndex < activeWorkspace.tabs.length - 1 ? currentIndex + 1 : 0
        handleSelectTab(activeWorkspace.tabs[newIndex].id)
      },
      prevTab: () => {
        if (!activeWorkspace) return
        const currentIndex = activeWorkspace.tabs.findIndex(
          (t) => t.id === activeWorkspace.activeTabId
        )
        const newIndex = currentIndex > 0 ? currentIndex - 1 : activeWorkspace.tabs.length - 1
        handleSelectTab(activeWorkspace.tabs[newIndex].id)
      }
    }),
    [activeWorkspace, handleNewDefaultTab, handleCloseTab, handleSelectTab]
  )

  // Use the prefix keybindings hook
  usePrefixKeybindings(keybindingHandlers)

  // Cmd+1-9: Switch to tab by number (keep hardcoded as these are standard)
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (!activeWorkspace) return

      if ((e.metaKey || e.ctrlKey) && e.key >= '1' && e.key <= '9') {
        e.preventDefault()
        const index = parseInt(e.key) - 1
        if (index < activeWorkspace.tabs.length) {
          handleSelectTab(activeWorkspace.tabs[index].id)
        }
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [activeWorkspace, handleSelectTab])

  // Handle legacy workspaces - migrate terminals to tabs format
  const tabs = activeWorkspace?.tabs || []
  const activeTabId = activeWorkspace?.activeTabId || tabs[0]?.id

  return (
    <div className="workspace-content">
      {/* Show empty state when no workspace is active, but keep terminals mounted below */}
      {!activeWorkspace ? (
        <div className="workspace-empty">
          <div className="workspace-empty-content">
            <h2>No workspace selected</h2>
            <p>Select a workspace from the sidebar or add a new one to get started.</p>
          </div>
        </div>
      ) : (
        <>
          <div className="workspace-header">
            <span className="workspace-title">{activeWorkspace.name}</span>
            <span className="workspace-path">{activeWorkspace.path}</span>
            {activeWorkspace.gitBranch && (
              <span className="workspace-branch">{activeWorkspace.gitBranch}</span>
            )}
            <div className="workspace-actions">
              {activeWorkspace.isGitRepo && (
                <button
                  className="workspace-action-btn"
                  onClick={() => setShowCreateChildDialog(true)}
                  title="Fork: Create new child workspace"
                >
                  Fork
                </button>
              )}
              {activeWorkspace.isWorktree && activeWorkspace.parentId && (
                <>
                  <button
                    className="workspace-action-btn workspace-action-btn-merge"
                    onClick={() => setShowMergeDialog(true)}
                    title="Merge: Close and merge this workspace"
                  >
                    Merge
                  </button>
                  <button
                    className="workspace-action-btn workspace-action-btn-abandon"
                    onClick={handleAbandon}
                    title="Abandon: Discard changes and remove this workspace"
                  >
                    Abandon
                  </button>
                </>
              )}
            </div>
          </div>
          <TabBar
            tabs={tabs}
            activeTabId={activeTabId}
            onSelectTab={handleSelectTab}
            onCloseTab={handleCloseTab}
            onNewTab={handleNewTab}
          />
        </>
      )}
      <div className="workspace-terminal" style={{ display: activeWorkspace ? 'flex' : 'none' }}>
        {/* Render tabs for ALL workspaces to keep PTYs alive when switching */}
        {Object.values(workspaces).map((workspace) => {
          const wsTabs = workspace.tabs || []
          const wsActiveTabId = workspace.activeTabId || wsTabs[0]?.id
          const isActiveWorkspace = workspace.id === activeWorkspaceId

          return wsTabs.map((tab) => {
            const isVisible = isActiveWorkspace && tab.id === wsActiveTabId
            const app = applicationRegistry.get(tab.applicationId)

            if (!app) return null

            // Skip rendering if app doesn't need to stay alive and workspace is inactive
            if (!app.keepAlive && !isActiveWorkspace) return null

            return (
              <div
                key={`${workspace.id}-${tab.id}`}
                className={`app-wrapper ${tab.applicationId}-wrapper`}
                style={{ display: isVisible ? app.displayStyle : 'none' }}
              >
                {app.render({
                  tab,
                  workspaceId: workspace.id,
                  workspacePath: workspace.path,
                  isVisible
                })}
              </div>
            )
          })
        })}
      </div>

      {/* Create Child Dialog (Fork) */}
      {showCreateChildDialog && activeWorkspace && (
        <CreateChildDialog
          parentWorkspace={activeWorkspace}
          onCreate={handleCreateChildSubmit}
          onAdopt={handleAdoptWorktreeSubmit}
          onCancel={() => setShowCreateChildDialog(false)}
          openWorktreePaths={openWorktreePaths}
        />
      )}

      {/* Merge Dialog */}
      {showMergeDialog && activeWorkspace && activeWorkspace.parentId && workspaces[activeWorkspace.parentId] && (
        <MergeDialog
          workspace={activeWorkspace}
          parentWorkspace={workspaces[activeWorkspace.parentId]}
          onMerge={handleMerge}
          onAbandon={handleAbandon}
          onCancel={() => setShowMergeDialog(false)}
        />
      )}

      {/* Prefix Mode Indicator */}
      <PrefixModeIndicator />
    </div>
  )
}
