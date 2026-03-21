import { useEffect, useCallback, useState, useRef, useMemo } from 'react'
import { ChevronDown } from 'lucide-react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { WorkspaceState } from '../store/createWorkspaceStore'
import { usePrefixModeStore } from '../store/prefixMode'
import { useAppStore } from '../store/app'
import { usePrefixKeybindings } from '../hooks/usePrefixKeybindings'
import FlexLayoutPane from './FlexLayoutPane'
import TabContentPortals from './TabContentPortals'
import CreateChildDialog from './CreateChildDialog'
import KeybindingOverlay from './KeybindingOverlay'
import { ErrorBoundary } from './ErrorBoundary'
import WorkspaceErrorFallback from './WorkspaceErrorFallback'
import type { ReviewState, Platform } from '../types'
import { getTabs } from '../types'
import { PromptDescriptionButton } from './PromptDescriptionButton'
import RunActionDropdown from './RunActionDropdown'

interface WorkspacePaneProps {
  workspaceStore: StoreApi<WorkspaceState>
  platform: Platform
}

export default function WorkspacePane({ workspaceStore, platform }: WorkspacePaneProps) {
  const {
    workspaces,
    activeWorkspaceId,
    addTab,
    removeTab,
    setActiveTab,
    addChildWorkspace,
    adoptExistingWorktree,
    createWorktreeFromBranch,
    createWorktreeFromRemote,
    removeWorkspace,
    removeWorkspaceKeepBranch,
    removeWorkspaceKeepWorktree,
    removeWorkspaceKeepBoth,
    mergeAndRemoveWorkspace,
    closeAndCleanWorkspace,
    setActiveWorkspace,
    updateWorkspaceMetadata,
    updateTabTitle
  } = useStore(workspaceStore)
  const { enterWorkspaceFocus } = usePrefixModeStore()
  const applications = useAppStore((s) => s.applications)
  const getApplication = useCallback((id: string) => applications[id], [applications])
  const menuApplications = useMemo(() => Object.values(applications).filter((app) => app.showInNewTabMenu), [applications])

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
      descriptionRef.current.style.height = 'auto'
      descriptionRef.current.style.height = descriptionRef.current.scrollHeight + 'px'
    }
  }, [isEditingDescription])

  // Cancel edit mode when switching workspaces
  useEffect(() => {
    setIsEditingName(false)
    setIsEditingDescription(false)
  }, [activeWorkspaceId])

  const handleNewTab = useCallback(
    (applicationId: string) => {
      if (!activeWorkspaceId) return
      if (applicationId === 'review') {
        addTab<ReviewState>(activeWorkspaceId, 'review', {
          parentWorkspaceId: activeWorkspace?.parentId ?? undefined
        })
      } else {
        addTab(activeWorkspaceId, applicationId)
      }
    },
    [activeWorkspaceId, activeWorkspace?.parentId, addTab]
  )

  // Create new tab using the first available application
  const handleNewDefaultTab = useCallback(() => {
    // Find the first app that allows new tabs
    const defaultApp = menuApplications.find((app) => app.canHaveMultiple)
    if (activeWorkspaceId && defaultApp) {
      addTab(activeWorkspaceId, defaultApp.id)
    }
  }, [activeWorkspaceId, addTab, menuApplications])

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
    addTab<ReviewState>(activeWorkspaceId, 'review', {
      parentWorkspaceId: activeWorkspace.parentId
    })
  }

  // Abandon dropdown state
  const [abandonMenuOpen, setAbandonMenuOpen] = useState(false)
  const abandonMenuRef = useRef<HTMLDivElement>(null)
  const abandonButtonRef = useRef<HTMLButtonElement>(null)

  // Close abandon dropdown when clicking outside
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (
        abandonMenuOpen &&
        abandonMenuRef.current &&
        !abandonMenuRef.current.contains(e.target as Node) &&
        abandonButtonRef.current &&
        !abandonButtonRef.current.contains(e.target as Node)
      ) {
        setAbandonMenuOpen(false)
      }
    }

    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [abandonMenuOpen])

  // Abandon handlers
  const handleAbandon = async () => {
    if (!activeWorkspaceId) return
    if (!confirm('Are you sure you want to abandon this workspace? All changes will be discarded.')) {
      return
    }
    setAbandonMenuOpen(false)
    await removeWorkspace(activeWorkspaceId)
  }

  const handleAbandonKeepBranch = async () => {
    if (!activeWorkspaceId) return
    if (!confirm('Abandon this workspace but keep the branch? The worktree will be removed but the branch will be kept.')) {
      return
    }
    setAbandonMenuOpen(false)
    await removeWorkspaceKeepBranch(activeWorkspaceId)
  }

  const handleAbandonKeepWorktree = async () => {
    if (!activeWorkspaceId) return
    if (!confirm('Abandon this workspace but keep the worktree on disk? The worktree will remain but the branch will be deleted.')) {
      return
    }
    setAbandonMenuOpen(false)
    await removeWorkspaceKeepWorktree(activeWorkspaceId)
  }

  const handleAbandonKeepBoth = async () => {
    if (!activeWorkspaceId) return
    if (!confirm('Abandon this workspace but keep both the worktree and branch? They will remain but will no longer be tracked in TreeTerm.')) {
      return
    }
    setAbandonMenuOpen(false)
    await removeWorkspaceKeepBoth(activeWorkspaceId)
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
        if (activeWorkspace?.activeTabId && tabs.length > 1) {
          handleCloseTab(activeWorkspace.activeTabId)
        }
      },
      nextTab: () => {
        if (!activeWorkspace) return
        const currentIndex = tabs.findIndex(
          (t) => t.id === activeWorkspace.activeTabId
        )
        const newIndex = currentIndex < tabs.length - 1 ? currentIndex + 1 : 0
        handleSelectTab(tabs[newIndex].id)
      },
      prevTab: () => {
        if (!activeWorkspace) return
        const currentIndex = tabs.findIndex(
          (t) => t.id === activeWorkspace.activeTabId
        )
        const newIndex = currentIndex > 0 ? currentIndex - 1 : tabs.length - 1
        handleSelectTab(tabs[newIndex].id)
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
        if (index < tabs.length) {
          handleSelectTab(tabs[index].id)
        }
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [activeWorkspace, handleSelectTab])

  // Handle legacy workspaces - migrate terminals to tabs format
  const tabs = activeWorkspace ? getTabs(activeWorkspace) : []
  const activeTabId = activeWorkspace?.activeTabId || tabs[0]?.id

  // Prompt description: show button next to description for AI harness tabs that haven't been prompted yet
  const activeTab = tabs.find((t) => t.id === activeTabId)
  const showPromptDescriptionButton = activeTab?.applicationId.startsWith('aiharness-')
    && activeWorkspace?.metadata?.description
    && !activeWorkspace?.metadata?.descriptionPrompted

  const handlePromptDescriptionDismiss = useCallback(() => {
    if (activeWorkspaceId) {
      updateWorkspaceMetadata(activeWorkspaceId, 'descriptionPrompted', 'true')
    }
  }, [activeWorkspaceId, updateWorkspaceMetadata])

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
                <div className="workspace-actions">
                  {activeWorkspace.gitBranch && (
                    <span className="workspace-branch">{activeWorkspace.gitBranch}</span>
                  )}
                  <RunActionDropdown
                    workspacePath={activeWorkspace.path}
                    onRun={async (ptyId, actionId) => {
                      if (activeWorkspaceId) {
                        const tabId = addTab(activeWorkspaceId, 'terminal', { ptyId, ptyHandle: null, keepOnExit: true })
                        updateTabTitle(activeWorkspaceId, tabId, actionId)
                      }
                    }}
                  />
                  {activeWorkspace.isWorktree && activeWorkspace.parentId && (
                    <div className="abandon-dropdown-container">
                      <button
                        className="workspace-action-btn workspace-action-btn-merge abandon-split-btn"
                        onClick={handleOpenReview}
                        title="Review & Merge: Review changes and merge this workspace"
                      >
                        Review & Merge
                      </button>
                      <button
                        ref={abandonButtonRef}
                        className="workspace-action-btn workspace-action-btn-merge abandon-dropdown-btn"
                        onClick={() => setAbandonMenuOpen(!abandonMenuOpen)}
                        title="More options"
                      >
                        <ChevronDown size={14} />
                      </button>
                      {abandonMenuOpen && (
                        <div className="abandon-menu" ref={abandonMenuRef}>
                          <div
                            className="abandon-menu-item"
                            onClick={handleAbandon}
                          >
                            Abandon
                            <span className="abandon-menu-hint">Delete worktree and branch</span>
                          </div>
                          <div
                            className="abandon-menu-item"
                            onClick={handleAbandonKeepBranch}
                          >
                            Abandon (Keep Branch)
                            <span className="abandon-menu-hint">Delete worktree, keep branch</span>
                          </div>
                          <div
                            className="abandon-menu-item"
                            onClick={handleAbandonKeepWorktree}
                          >
                            Abandon (Keep Worktree)
                            <span className="abandon-menu-hint">Keep worktree, delete branch</span>
                          </div>
                          <div
                            className="abandon-menu-item"
                            onClick={handleAbandonKeepBoth}
                          >
                            Abandon (Keep Both)
                            <span className="abandon-menu-hint">Keep worktree and branch</span>
                          </div>
                        </div>
                      )}
                    </div>
                  )}
                </div>
              </div>
              <div className="workspace-header-description-row">
                {isEditingDescription ? (
                  <textarea
                    ref={descriptionRef}
                    className="workspace-edit-textarea"
                    value={editDescription}
                    onChange={(e) => {
                      setEditDescription(e.target.value)
                      e.target.style.height = 'auto'
                      e.target.style.height = e.target.scrollHeight + 'px'
                    }}
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
                  <span className="workspace-description">
                    {activeWorkspace.metadata.description}
                    {showPromptDescriptionButton && (
                      <PromptDescriptionButton
                        description={activeWorkspace.metadata.description}
                        workspaceStore={workspaceStore}
                        workspaceId={activeWorkspaceId!}
                        onDismiss={handlePromptDescriptionDismiss}
                      />
                    )}
                    <button
                      className="workspace-edit-btn"
                      onClick={handleStartEditDescription}
                      title="Edit description"
                    >
                      ✎
                    </button>
                  </span>
                ) : (
                  <>
                    <span className="workspace-description workspace-description-placeholder">no description</span>
                    <button
                      className="workspace-edit-btn workspace-add-description-btn"
                      onClick={handleStartEditDescription}
                      title="Add description"
                    >
                      ✎
                    </button>
                  </>
                )}
              </div>
            </div>
          </>
        )}
        <div className="workspace-terminal" style={{ display: activeWorkspace ? 'flex' : 'none' }}>
          {activeWorkspaceId && (
            <FlexLayoutPane
              key={activeWorkspaceId}
              workspaceId={activeWorkspaceId}
              workspaceStore={workspaceStore}
              onNewTab={handleNewTab}
            />
          )}
          <TabContentPortals
            workspaceStore={workspaceStore}
            activeWorkspaceId={activeWorkspaceId}
          />
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
