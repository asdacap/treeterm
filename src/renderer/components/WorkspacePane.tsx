import { useEffect, useCallback, useState, useRef, useMemo } from 'react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { WorkspaceState } from '../store/createWorkspaceStore'
import { usePrefixModeStore } from '../store/prefixMode'
import { applicationRegistry } from '../registry/applicationRegistry'
import { usePrefixKeybindings } from '../hooks/usePrefixKeybindings'
import TabBar from './TabBar'
import CreateChildDialog from './CreateChildDialog'
import KeybindingOverlay from './KeybindingOverlay'
import { ErrorBoundary } from './ErrorBoundary'
import WorkspaceErrorFallback from './WorkspaceErrorFallback'
import TabErrorFallback from './TabErrorFallback'
import type { ReviewState, Platform } from '../types'

interface WorkspacePaneProps {
  workspaceStore: StoreApi<WorkspaceState>
  platform: Platform
}

export default function WorkspacePane({ workspaceStore, platform }: WorkspacePaneProps) {
  const {
    workspaces,
    activeWorkspaceId,
    addTab,
    addTabWithState,
    removeTab,
    setActiveTab,
    addChildWorkspace,
    adoptExistingWorktree,
    createWorktreeFromBranch,
    createWorktreeFromRemote,
    removeWorkspace,
    mergeAndRemoveWorkspace,
    closeAndCleanWorkspace,
    setActiveWorkspace,
    updateWorkspaceMetadata
  } = useStore(workspaceStore)
  const { enterWorkspaceFocus } = usePrefixModeStore()

  const activeWorkspace = activeWorkspaceId ? workspaces[activeWorkspaceId] : null

  // Dialog state
  const [showCreateChildDialog, setShowCreateChildDialog] = useState(false)

  // Inline header edit state — independent for name and description
  const [isEditingName, setIsEditingName] = useState(false)
  const [isEditingDescription, setIsEditingDescription] = useState(false)
  const [editName, setEditName] = useState('')
  const [editDescription, setEditDescription] = useState('')
  const nameInputRef = useRef<HTMLInputElement>(null)
  const descriptionRef = useRef<HTMLTextAreaElement>(null)

  const handleStartEditName = useCallback(() => {
    if (!activeWorkspace) return
    setEditName(activeWorkspace.metadata?.displayName || activeWorkspace.name)
    setIsEditingName(true)
  }, [activeWorkspace])

  const handleSaveName = useCallback(() => {
    if (!activeWorkspaceId) return
    const trimmedName = editName.trim()
    if (trimmedName) {
      updateWorkspaceMetadata(activeWorkspaceId, 'displayName', trimmedName)
    }
    setIsEditingName(false)
  }, [activeWorkspaceId, editName, updateWorkspaceMetadata])

  const handleStartEditDescription = useCallback(() => {
    if (!activeWorkspace) return
    setEditDescription(activeWorkspace.metadata?.description || '')
    setIsEditingDescription(true)
  }, [activeWorkspace])

  const handleSaveDescription = useCallback(() => {
    if (!activeWorkspaceId) return
    updateWorkspaceMetadata(activeWorkspaceId, 'description', editDescription.trim())
    setIsEditingDescription(false)
  }, [activeWorkspaceId, editDescription, updateWorkspaceMetadata])

  // Focus name input when entering edit mode
  useEffect(() => {
    if (isEditingName && nameInputRef.current) {
      nameInputRef.current.focus()
      nameInputRef.current.select()
    }
  }, [isEditingName])

  // Focus description textarea when entering edit mode
  useEffect(() => {
    if (isEditingDescription && descriptionRef.current) {
      descriptionRef.current.focus()
    }
  }, [isEditingDescription])

  // Cancel edit mode when switching workspaces
  useEffect(() => {
    setIsEditingName(false)
    setIsEditingDescription(false)
  }, [activeWorkspaceId])

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
  const handleCreateChildSubmit = async (name: string, isDetached: boolean, settings?: import('../types').WorktreeSettings, description?: string) => {
    if (!activeWorkspaceId) return { success: false, error: 'No workspace selected' }

    const result = await addChildWorkspace(activeWorkspaceId, name, isDetached, settings, description)
    if (result.success) {
      setShowCreateChildDialog(false)
    }
    return result
  }

  // Adopt existing worktree handler
  const handleAdoptWorktreeSubmit = async (worktreePath: string, branch: string, name: string, settings?: import('../types').WorktreeSettings, description?: string) => {
    if (!activeWorkspaceId) return { success: false, error: 'No workspace selected' }

    const result = await adoptExistingWorktree(activeWorkspaceId, worktreePath, branch, name, settings, description)
    if (result.success) {
      setShowCreateChildDialog(false)
    }
    return result
  }

  // Create worktree from existing branch handler
  const handleCreateFromBranchSubmit = async (branch: string, isDetached: boolean, settings?: import('../types').WorktreeSettings, description?: string) => {
    console.log('[WorkspacePane] handleCreateFromBranchSubmit called:', { branch, isDetached, activeWorkspaceId })
    if (!activeWorkspaceId) return { success: false, error: 'No workspace selected' }

    const result = await createWorktreeFromBranch(activeWorkspaceId, branch, isDetached, settings, description)
    console.log('[WorkspacePane] handleCreateFromBranchSubmit result:', result)
    if (result.success) {
      setShowCreateChildDialog(false)
    }
    return result
  }

  // Create worktree from remote branch handler
  const handleCreateFromRemoteSubmit = async (remoteBranch: string, isDetached: boolean, settings?: import('../types').WorktreeSettings, description?: string) => {
    console.log('[WorkspacePane] handleCreateFromRemoteSubmit called:', { remoteBranch, isDetached, activeWorkspaceId })
    if (!activeWorkspaceId) return { success: false, error: 'No workspace selected' }

    const result = await createWorktreeFromRemote(activeWorkspaceId, remoteBranch, isDetached, settings, description)
    console.log('[WorkspacePane] handleCreateFromRemoteSubmit result:', result)
    if (result.success) {
      setShowCreateChildDialog(false)
    }
    return result
  }

  // Review handler
  const handleOpenReview = () => {
    if (!activeWorkspaceId || !activeWorkspace?.parentId) return
    addTabWithState<ReviewState>(activeWorkspaceId, 'review', {
      parentWorkspaceId: activeWorkspace.parentId
    })
  }

  // Abandon handler (direct)
  const handleAbandon = async () => {
    if (!activeWorkspaceId) return
    if (!confirm('Are you sure you want to abandon this workspace? All changes will be discarded.')) {
      return
    }
    await removeWorkspace(activeWorkspaceId)
  }

  // Compute flattened workspace list for navigation
  const flattenedWorkspaceIds = useMemo(() => {
    const result: string[] = []
    const traverse = (wsId: string) => {
      result.push(wsId)
      const ws = workspaces[wsId]
      if (ws) {
        ws.children.forEach(traverse)
      }
    }
    // Get root workspaces (those without parents)
    Object.values(workspaces)
      .filter((ws) => !ws.parentId)
      .forEach((ws) => traverse(ws.id))
    return result
  }, [workspaces])

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
      },
      workspaceFocus: () => {
        const currentIndex = activeWorkspaceId
          ? flattenedWorkspaceIds.indexOf(activeWorkspaceId)
          : 0
        enterWorkspaceFocus(flattenedWorkspaceIds, currentIndex >= 0 ? currentIndex : 0)
      },
      setActiveWorkspace
    }),
    [
      activeWorkspace,
      activeWorkspaceId,
      flattenedWorkspaceIds,
      handleNewDefaultTab,
      handleCloseTab,
      handleSelectTab,
      enterWorkspaceFocus,
      setActiveWorkspace
    ]
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
    <ErrorBoundary fallback={(error, reset) => <WorkspaceErrorFallback error={error} onReset={reset} />}>
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
              <div className="workspace-header-top">
                {isEditingName ? (
                  <input
                    ref={nameInputRef}
                    className="workspace-edit-input"
                    value={editName}
                    onChange={(e) => setEditName(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') handleSaveName()
                      else if (e.key === 'Escape') setIsEditingName(false)
                    }}
                    onBlur={handleSaveName}
                  />
                ) : (
                  <>
                    <span className="workspace-title">{activeWorkspace.metadata?.displayName || activeWorkspace.name}</span>
                    <button
                      className="workspace-edit-btn"
                      onClick={handleStartEditName}
                      title="Edit name"
                    >
                      ✎
                    </button>
                  </>
                )}
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
                      onClick={handleOpenReview}
                      title="Review & Merge: Review changes and merge this workspace"
                    >
                      Review & Merge
                    </button>
                  )}
                </div>
              </div>
              <div className="workspace-header-description-row">
                {isEditingDescription ? (
                  <textarea
                    ref={descriptionRef}
                    className="workspace-edit-textarea"
                    value={editDescription}
                    onChange={(e) => setEditDescription(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' && !e.shiftKey) {
                        e.preventDefault()
                        handleSaveDescription()
                      } else if (e.key === 'Escape') {
                        setIsEditingDescription(false)
                      }
                    }}
                    onBlur={handleSaveDescription}
                    placeholder="Add a description..."
                    rows={1}
                  />
                ) : activeWorkspace.metadata?.description ? (
                  <>
                    <span className="workspace-description">{activeWorkspace.metadata.description}</span>
                    <button
                      className="workspace-edit-btn"
                      onClick={handleStartEditDescription}
                      title="Edit description"
                    >
                      ✎
                    </button>
                  </>
                ) : null}
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
                  <ErrorBoundary
                    key={`error-${workspace.id}-${tab.id}`}
                    fallback={(error, reset) => (
                      <TabErrorFallback
                        error={error}
                        tabTitle={tab.title}
                        onReset={reset}
                        onClose={() => handleCloseTab(tab.id)}
                      />
                    )}
                  >
                    {app.render({
                      tab,
                      workspaceId: workspace.id,
                      workspacePath: workspace.path,
                      isVisible,
                      workspaceStore
                    })}
                  </ErrorBoundary>
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
            onCreateFromBranch={handleCreateFromBranchSubmit}
            onCreateFromRemote={handleCreateFromRemoteSubmit}
            onCancel={() => setShowCreateChildDialog(false)}
            openWorktreePaths={openWorktreePaths}
          />
        )}

        {/* Keybinding Overlay */}
        <KeybindingOverlay platform={platform} />
      </div>
    </ErrorBoundary>
  )
}
