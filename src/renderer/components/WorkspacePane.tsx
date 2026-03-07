import { useEffect, useCallback, useState } from 'react'
import { useWorkspaceStore } from '../store/workspace'
import { useSettingsStore } from '../store/settings'
import { applicationRegistry } from '../registry/applicationRegistry'
import TabBar from './TabBar'
import MergeDialog from './MergeDialog'
import CreateChildDialog from './CreateChildDialog'
// Import applications to ensure they are registered
import '../applications'

function matchesKeybinding(e: KeyboardEvent, keybinding: string): boolean {
  const parts = keybinding.split('+')
  const key = parts[parts.length - 1]
  const hasCmd = parts.includes('CommandOrControl')
  const hasShift = parts.includes('Shift')
  const hasAlt = parts.includes('Alt')

  const cmdMatch = hasCmd ? (e.metaKey || e.ctrlKey) : (!e.metaKey && !e.ctrlKey)
  const shiftMatch = hasShift ? e.shiftKey : !e.shiftKey
  const altMatch = hasAlt ? e.altKey : !e.altKey

  const pressedKey = e.key.length === 1 ? e.key.toUpperCase() : e.key
  const targetKey = key.length === 1 ? key.toUpperCase() : key
  const keyMatch = pressedKey === targetKey || e.key === key

  return cmdMatch && shiftMatch && altMatch && keyMatch
}

export default function WorkspacePane() {
  const {
    workspaces,
    activeWorkspaceId,
    addTab,
    removeTab,
    setActiveTab,
    addChildWorkspace,
    removeWorkspace,
    mergeAndRemoveWorkspace
  } = useWorkspaceStore()

  const { settings } = useSettingsStore()
  const keybindings = settings.keybindings

  const activeWorkspace = activeWorkspaceId ? workspaces[activeWorkspaceId] : null

  // Dialog state
  const [showCreateChildDialog, setShowCreateChildDialog] = useState(false)
  const [showMergeDialog, setShowMergeDialog] = useState(false)

  const handleNewTab = useCallback(
    (instanceId: string) => {
      if (activeWorkspaceId) {
        addTab(activeWorkspaceId, instanceId)
      }
    },
    [activeWorkspaceId, addTab]
  )

  // Create new tab using the first available application instance
  const handleNewDefaultTab = useCallback(() => {
    // Find the first instance where the app allows new tabs
    const defaultInstance = settings.applications.find((inst) => {
      const app = applicationRegistry.get(inst.applicationId)
      return app?.showInNewTabMenu && app?.canHaveMultiple
    })
    if (activeWorkspaceId && defaultInstance) {
      addTab(activeWorkspaceId, defaultInstance.id)
    }
  }, [activeWorkspaceId, addTab, settings.applications])

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

  // Fork handler
  const handleCreateChildSubmit = async (name: string) => {
    if (!activeWorkspaceId) return { success: false, error: 'No workspace selected' }

    const result = await addChildWorkspace(activeWorkspaceId, name)
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

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (!activeWorkspace) return

      // New tab
      if (matchesKeybinding(e, keybindings.newTab)) {
        e.preventDefault()
        handleNewDefaultTab()
        return
      }

      // Close tab
      if (matchesKeybinding(e, keybindings.closeTab)) {
        e.preventDefault()
        if (activeWorkspace.activeTabId && activeWorkspace.tabs.length > 1) {
          handleCloseTab(activeWorkspace.activeTabId)
        }
        return
      }

      // Cmd+1-9: Switch to tab by number (keep hardcoded as these are standard)
      if ((e.metaKey || e.ctrlKey) && e.key >= '1' && e.key <= '9') {
        e.preventDefault()
        const index = parseInt(e.key) - 1
        if (index < activeWorkspace.tabs.length) {
          handleSelectTab(activeWorkspace.tabs[index].id)
        }
        return
      }

      // Previous tab
      if (matchesKeybinding(e, keybindings.prevTab)) {
        e.preventDefault()
        const currentIndex = activeWorkspace.tabs.findIndex(
          (t) => t.id === activeWorkspace.activeTabId
        )
        const newIndex = currentIndex > 0 ? currentIndex - 1 : activeWorkspace.tabs.length - 1
        handleSelectTab(activeWorkspace.tabs[newIndex].id)
        return
      }

      // Next tab
      if (matchesKeybinding(e, keybindings.nextTab)) {
        e.preventDefault()
        const currentIndex = activeWorkspace.tabs.findIndex(
          (t) => t.id === activeWorkspace.activeTabId
        )
        const newIndex = currentIndex < activeWorkspace.tabs.length - 1 ? currentIndex + 1 : 0
        handleSelectTab(activeWorkspace.tabs[newIndex].id)
        return
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [activeWorkspace, keybindings, handleNewDefaultTab, handleCloseTab, handleSelectTab])

  if (!activeWorkspace) {
    return (
      <div className="workspace-empty">
        <div className="workspace-empty-content">
          <h2>No workspace selected</h2>
          <p>Select a workspace from the sidebar or add a new one to get started.</p>
        </div>
      </div>
    )
  }

  // Handle legacy workspaces - migrate terminals to tabs format
  const tabs = activeWorkspace.tabs || []
  const activeTabId = activeWorkspace.activeTabId || tabs[0]?.id

  return (
    <div className="workspace-content">
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
            <button
              className="workspace-action-btn workspace-action-btn-merge"
              onClick={() => setShowMergeDialog(true)}
              title="Merge: Close and merge this workspace"
            >
              Merge
            </button>
          )}
        </div>
      </div>
      <TabBar
        tabs={tabs}
        activeTabId={activeTabId}
        onSelectTab={handleSelectTab}
        onCloseTab={handleCloseTab}
        instances={settings.applications}
        onNewTab={handleNewTab}
      />
      <div className="workspace-terminal">
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
                className={`${tab.applicationId}-wrapper`}
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
          onCancel={() => setShowCreateChildDialog(false)}
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
    </div>
  )
}
