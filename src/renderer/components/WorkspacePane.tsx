import React, { useEffect, useCallback, useState } from 'react'
 
import { ChevronDown, Github, Loader2, ArrowDownToLine, RefreshCw, AlertTriangle, CircleDot, Check } from 'lucide-react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { SessionState } from '../store/createSessionStore'
import { useAppStore } from '../store/app'
import { useKeybindingStore } from '../store/keybinding'
import FlexLayoutPane from './FlexLayoutPane'
import TabContentPortals from './TabContentPortals'
import CreateChildDialog from './CreateChildDialog'
import KeybindingOverlay from './KeybindingOverlay'
import { ErrorBoundary } from './ErrorBoundary'
import WorkspaceErrorFallback from './WorkspaceErrorFallback'
import type { ReviewState, Platform, WorkspaceStore } from '../types'
import { getTabs } from '../types'
import { PromptDescriptionButton } from './PromptDescriptionButton'
import RunActionDropdown from './RunActionDropdown'
import ContextMenu from './ContextMenu'
import { useContextMenuStore } from '../store/contextMenu'

interface WorkspacePaneProps {
  sessionStore: StoreApi<SessionState>
  platform: Platform
}

export default function WorkspacePane({ sessionStore, platform }: WorkspacePaneProps) {
  const {
    workspaces,
    activeWorkspaceId,
    addChildWorkspace,
    adoptExistingWorktree,
    createWorktreeFromBranch,
    createWorktreeFromRemote,
    setActiveWorkspace,
    clearWorkspaceError,
    closeWorkspace,
  } = useStore(sessionStore)
  const enterWorkspaceFocus = useKeybindingStore(s => s.enterWorkspaceFocus)
  const applications = useAppStore((s) => s.applications)
  const clipboard = useAppStore((s) => s.clipboard)
  const menuApplications = Array.from(applications.values()).filter((app) => app.showInNewTabMenu)
  const openContextMenu = useContextMenuStore((s) => s.open)
  const closeContextMenu = useContextMenuStore((s) => s.close)
  const activeMenuId = useContextMenuStore((s) => s.activeMenuId)
  const menuPosition = useContextMenuStore((s) => s.position)
  const branchBadgeMenuId = 'branch-badge'

  const [branchCopied, setBranchCopied] = useState(false)

  const activeEntry = activeWorkspaceId ? workspaces.get(activeWorkspaceId) ?? null : null
  const activeWorkspace = activeEntry && (activeEntry.status === 'loaded' || activeEntry.status === 'operation-error') ? activeEntry.data : null
  const activeHandle = activeEntry && (activeEntry.status === 'loaded' || activeEntry.status === 'operation-error') ? activeEntry.store : null

  // Dialog state
  const [showCreateChildDialog, setShowCreateChildDialog] = useState(false)

  // Inline header edit state — independent for name and description
  const [isEditingName, setIsEditingName] = useState(false)
  const [isEditingDescription, setIsEditingDescription] = useState(false)
  const [editName, setEditName] = useState('')
  const [editDescription, setEditDescription] = useState('')

  const handleStartEditName = useCallback(() => {
    if (!activeWorkspace) return
    setEditName(activeWorkspace.metadata.displayName || activeWorkspace.name)
    setIsEditingName(true)
  }, [activeWorkspace])

  const handleSaveName = useCallback(() => {
    if (!activeHandle) return
    const trimmedName = editName.trim()
    if (trimmedName) {
      activeHandle.getState().updateMetadata('displayName', trimmedName)
    }
    setIsEditingName(false)
  }, [activeHandle, editName])

  const handleStartEditDescription = useCallback(() => {
    if (!activeWorkspace) return
    setEditDescription(activeWorkspace.metadata.description || '')
    setIsEditingDescription(true)
  }, [activeWorkspace])

  const handleSaveDescription = useCallback(() => {
    if (!activeHandle) return
    activeHandle.getState().updateMetadata('description', editDescription.trim())
    setIsEditingDescription(false)
  }, [activeHandle, editDescription])

  // Cancel edit mode when switching workspaces (setState-in-render pattern)
  const [prevActiveWorkspaceId, setPrevActiveWorkspaceId] = useState(activeWorkspaceId)
  if (activeWorkspaceId !== prevActiveWorkspaceId) {
    setPrevActiveWorkspaceId(activeWorkspaceId)
    setIsEditingName(false)
    setIsEditingDescription(false)
  }

  // Signal active tab to focus after workspace switch (keyboard navigation)
  useEffect(() => {
    if (!activeHandle) return
    activeHandle.getState().requestFocus()
  }, [activeWorkspaceId, activeHandle])

  // Create new tab using the first available application
  const handleNewDefaultTab = useCallback(() => {
    // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- menuApplications always has at least one entry
    const defaultApp = menuApplications[0]!
    if (activeHandle) {
      activeHandle.getState().addTab(defaultApp.id)
    }
  }, [activeHandle, menuApplications])

  const handleCloseTab = useCallback(
    (tabId: string) => {
      if (activeHandle) {
        void activeHandle.getState().removeTab(tabId)
      }
    },
    [activeHandle]
  )

  const handleSelectTab = useCallback(
    (tabId: string) => {
      if (activeHandle) {
        activeHandle.getState().setActiveTab(tabId)
      }
    },
    [activeHandle]
  )

  // Compute paths of already-open worktrees
  const openWorktreePaths = Array.from(workspaces.values())
    .filter((e): e is Extract<typeof e, { status: 'loaded' | 'operation-error' }> =>
      e.status === 'loaded' || e.status === 'operation-error')
    .filter(e => e.data.isWorktree)
    .map(e => e.data.path)

  // Fork handler - create new worktree
  const handleCreateChildSubmit = (name: string, isDetached: boolean, settings?: import('../types').WorktreeSettings, description?: string) => {
    if (!activeWorkspaceId) return { success: false, error: 'No workspace selected' }

    const result = addChildWorkspace(activeWorkspaceId, name, isDetached, settings, description)
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
  const handleCreateFromBranchSubmit = (branch: string, isDetached: boolean, settings?: import('../types').WorktreeSettings, description?: string) => {
    console.log('[WorkspacePane] handleCreateFromBranchSubmit called:', { branch, isDetached, activeWorkspaceId })
    if (!activeWorkspaceId) return { success: false, error: 'No workspace selected' }

    const result = createWorktreeFromBranch(activeWorkspaceId, branch, isDetached, settings, description)
    console.log('[WorkspacePane] handleCreateFromBranchSubmit result:', result)
    if (result.success) {
      setShowCreateChildDialog(false)
    }
    return result
  }

  // Create worktree from remote branch handler
  const handleCreateFromRemoteSubmit = (remoteBranch: string, isDetached: boolean, settings?: import('../types').WorktreeSettings, description?: string) => {
    console.log('[WorkspacePane] handleCreateFromRemoteSubmit called:', { remoteBranch, isDetached, activeWorkspaceId })
    if (!activeWorkspaceId) return { success: false, error: 'No workspace selected' }

    const result = createWorktreeFromRemote(activeWorkspaceId, remoteBranch, isDetached, settings, description)
    console.log('[WorkspacePane] handleCreateFromRemoteSubmit result:', result)
    if (result.success) {
      setShowCreateChildDialog(false)
    }
    return result
  }

  // Review handler
  const handleOpenReview = () => {
    if (!activeHandle || !activeWorkspace?.parentId) return
    activeHandle.getState().addTab<ReviewState>('review', {
      parentWorkspaceId: activeWorkspace.parentId
    })
  }


  // Compute flattened workspace list for navigation
  const flattenedWorkspaceIds = (() => {
    const result: string[] = []
    const parentMap = new Map<string | null, string[]>()
    for (const [id, entry] of Array.from(workspaces.entries())) {
      const parentId = (entry.status === 'loaded' || entry.status === 'operation-error') ? entry.data.parentId : null
      const children = parentMap.get(parentId) ?? []
      children.push(id)
      parentMap.set(parentId, children)
    }
    const traverse = (wsId: string) => {
      result.push(wsId)
      const children = parentMap.get(wsId) ?? []
      children.forEach(traverse)
    }
    const roots = parentMap.get(null) ?? []
    roots.forEach(traverse)
    return result
  })()

  // Handle legacy workspaces - migrate terminals to tabs format
  const tabs = activeWorkspace ? getTabs(activeWorkspace) : []

  // Set keybinding handlers — called every render so handlers always reference latest closures.
  // This is a synchronous Zustand setter via getState(), so it doesn't trigger re-renders.
  useKeybindingStore.getState().setHandlers({
    newTab: handleNewDefaultTab,
    closeTab: () => {
      if (activeWorkspace?.activeTabId && tabs.length > 1) {
        handleCloseTab(activeWorkspace.activeTabId)
      }
    },
    nextTab: () => {
      if (!activeWorkspace) return
      const currentIndex = tabs.findIndex((t) => t.id === activeWorkspace.activeTabId)
      const newIndex = currentIndex < tabs.length - 1 ? currentIndex + 1 : 0
      // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- newIndex is always in bounds
      handleSelectTab(tabs[newIndex]!.id)
    },
    prevTab: () => {
      if (!activeWorkspace) return
      const currentIndex = tabs.findIndex((t) => t.id === activeWorkspace.activeTabId)
      const newIndex = currentIndex > 0 ? currentIndex - 1 : tabs.length - 1
      // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- newIndex is always in bounds
      handleSelectTab(tabs[newIndex]!.id)
    },
    workspaceFocus: () => {
      const currentIndex = activeWorkspaceId
        ? flattenedWorkspaceIds.indexOf(activeWorkspaceId) : 0
      enterWorkspaceFocus(flattenedWorkspaceIds, currentIndex >= 0 ? currentIndex : 0)
    },
    setActiveWorkspace,
    switchToTab: (index: number) => {
      if (!activeWorkspace) return
      if (index < tabs.length) {
        // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- index is bounds-checked above
        handleSelectTab(tabs[index]!.id)
      }
    }
  })
  const activeTabId = activeWorkspace?.activeTabId || tabs[0]?.id

  // Prompt description: show button next to description for AI harness tabs that haven't been prompted yet
  const activeTab = tabs.find((t) => t.id === activeTabId)
  const showPromptDescriptionButton = activeTab?.applicationId.startsWith('aiharness-')
    && activeWorkspace?.metadata.description
    && !activeWorkspace.metadata.descriptionPrompted

  const handlePromptDescriptionDismiss = useCallback(() => {
    if (activeHandle) {
      activeHandle.getState().updateMetadata('descriptionPrompted', 'true')
    }
  }, [activeHandle])

  return (
    <ErrorBoundary FallbackComponent={WorkspaceErrorFallback}>
      <div className="workspace-content">
        {/* Show loading pane when a workspace is being created (e.g. fork / new worktree) */}
        {activeEntry?.status === 'loading' ? (
          <div className="workspace-loading">
            <div className="workspace-loading-header">
              <Loader2 size={16} className="spinning" />
              <span>{activeEntry.message}</span>
            </div>
            {activeEntry.output.length > 0 && (
              <AutoScrollPre className="workspace-loading-output">
                {activeEntry.output.join('')}
              </AutoScrollPre>
            )}
          </div>
        ) : !activeWorkspace ? (
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
                    autoFocus
                    className="workspace-edit-input"
                    value={editName}
                    onChange={(e) => { setEditName(e.target.value); }}
                    onFocus={(e) => { e.target.select(); }}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') handleSaveName()
                      else if (e.key === 'Escape') setIsEditingName(false)
                    }}
                    onBlur={handleSaveName}
                  />
                ) : (
                  <>
                    <span className="workspace-title">{activeWorkspace.metadata.displayName || activeWorkspace.name}</span>
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
                    <>
                      <span
                        className={`workspace-branch${branchCopied ? ' copied' : ''}`}
                        onClick={() => {
                          clipboard.writeText(activeWorkspace.gitBranch ?? '')
                          setBranchCopied(true)
                          setTimeout(() => { setBranchCopied(false); }, 1500)
                        }}
                        onContextMenu={(e) => {
                          e.preventDefault()
                          e.stopPropagation()
                          openContextMenu(branchBadgeMenuId, e.clientX, e.clientY)
                        }}
                        title="Copy branch name"
                      >{branchCopied ? 'Copied!' : activeWorkspace.gitBranch}</span>
                      <ContextMenu menuId={branchBadgeMenuId} activeMenuId={activeMenuId} position={menuPosition}>
                        <div className="context-menu-item" onClick={() => {
                          closeContextMenu()
                          clipboard.writeText(activeWorkspace.path)
                        }}>
                          Copy worktree path
                        </div>
                      </ContextMenu>
                    </>
                  )}
                  {activeWorkspace.isGitRepo && activeHandle && (
                    <GitStatusButton workspace={activeHandle} />
                  )}
                  {activeWorkspace.isGitRepo && activeHandle && (
                    <GitPullButton workspace={activeHandle} />
                  )}
                  {activeHandle && (
                    <RunActionDropdown
                      workspacePath={activeWorkspace.path}
                      runActions={activeHandle.getState().runActionsApi}
                      onRun={(ptyId, actionId) => {
                        const tabId = activeHandle.getState().addTab('terminal', { ptyId, ptyHandle: null, keepOnExit: true })
                        activeHandle.getState().updateTabTitle(tabId, actionId)
                      }}
                      onOpenApp={(applicationId) => {
                        activeHandle.getState().addTab(applicationId)
                      }}
                    />
                  )}
                  {activeWorkspace.isWorktree && activeWorkspace.parentId && activeWorkspace.gitBranch && activeHandle && (
                    <GitHubButton workspace={activeHandle} />
                  )}
                  {activeWorkspace.isWorktree && activeWorkspace.parentId && activeHandle && (
                    <MergeAbandonButton
                      workspace={activeHandle}
                      onOpenReview={handleOpenReview}
                    />
                  )}
                </div>
              </div>
              <div className="workspace-header-description-row">
                {isEditingDescription ? (
                  <textarea
                    autoFocus
                    className="workspace-edit-textarea"
                    value={editDescription}
                    onFocus={(e) => {
                      e.target.style.height = 'auto'
                      e.target.style.height = String(e.target.scrollHeight) + 'px'
                    }}
                    onChange={(e) => {
                      setEditDescription(e.target.value)
                      e.target.style.height = 'auto'
                      e.target.style.height = String(e.target.scrollHeight) + 'px'
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
                ) : activeWorkspace.metadata.description ? (
                  <span className="workspace-description">
                    {activeWorkspace.metadata.description}
                    {showPromptDescriptionButton && activeHandle && (
                      <PromptDescriptionButton
                        description={activeWorkspace.metadata.description}
                        workspace={activeHandle}
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
            {(activeEntry?.status === 'error' || activeEntry?.status === 'operation-error') && (
              <div className="workspace-load-error">
                <div className="workspace-load-error-content">
                  <h3>Operation failed</h3>
                  <p className="workspace-load-error-message">{activeEntry.error}</p>
                  <div className="workspace-load-error-actions">
                    {activeEntry.status === 'operation-error' && (
                      <button className="workspace-action-btn" onClick={() => { if (activeWorkspaceId) clearWorkspaceError(activeWorkspaceId); }}>
                        Cancel
                      </button>
                    )}
                    <button className="workspace-action-btn workspace-action-btn-danger" onClick={() => { if (activeWorkspaceId) closeWorkspace(activeWorkspaceId); }}>
                      Close Workspace
                    </button>
                  </div>
                </div>
              </div>
            )}
          </>
        )}
        <div className="workspace-terminal" style={{ display: activeEntry?.status === 'loaded' ? 'flex' : 'none' }}>
          {Array.from(workspaces.entries()).map(([wsId, entry]) => {
            if (entry.status !== 'loaded' && entry.status !== 'operation-error') return null
            const isActive = wsId === activeWorkspaceId
            return (
              <div key={wsId} style={{ display: isActive ? 'contents' : 'none', height: '100%', width: '100%' }}>
                <FlexLayoutPane
                  workspace={entry.store}
                  onNewTab={(applicationId: string) => {
                    if (applicationId === 'review') {
                      entry.store.getState().addTab('review', {
                        parentWorkspaceId: entry.data.parentId ?? undefined
                      } as Partial<ReviewState>)
                    } else {
                      entry.store.getState().addTab(applicationId)
                    }
                  }}
                />
              </div>
            )
          })}
          <TabContentPortals
            sessionStore={sessionStore}
            activeWorkspaceId={activeWorkspaceId}
          />
        </div>

        {/* Create Child Dialog (Fork) */}
        {showCreateChildDialog && activeHandle && (
          <CreateChildDialog
            parentWorkspace={activeHandle}
            onCreate={handleCreateChildSubmit}
            onAdopt={handleAdoptWorktreeSubmit}
            onCreateFromBranch={handleCreateFromBranchSubmit}
            onCreateFromRemote={handleCreateFromRemoteSubmit}
            onCancel={() => { setShowCreateChildDialog(false); }}
            openWorktreePaths={openWorktreePaths}
          />
        )}

        {/* Keybinding Overlay */}
        <KeybindingOverlay platform={platform} />
      </div>
    </ErrorBoundary>
  )
}

interface MergeAbandonButtonProps {
  workspace: WorkspaceStore
  onOpenReview: () => void
}

function MergeAbandonButton({ workspace, onOpenReview }: MergeAbandonButtonProps) {
  const { gitController } = useStore(workspace)
  const { isDiffCleanFromParent } = useStore(gitController)

  const [menuOpen, setMenuOpen] = useState(false)
  const menuRef = React.useRef<HTMLDivElement>(null)
  const buttonRef = React.useRef<HTMLButtonElement>(null)

  React.useEffect(() => {
    if (!menuOpen) return
    const handleClickOutside = (e: MouseEvent) => {
      if (
        menuRef.current && !menuRef.current.contains(e.target as Node) &&
        buttonRef.current && !buttonRef.current.contains(e.target as Node)
      ) {
        setMenuOpen(false)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => { document.removeEventListener('mousedown', handleClickOutside); }
  }, [menuOpen])

  const handleAbandon = async () => {
    if (!confirm('Are you sure you want to abandon this workspace? All changes will be discarded.')) return
    setMenuOpen(false)
    await workspace.getState().remove()
  }

  const handleAbandonKeepBranch = async () => {
    if (!confirm('Abandon this workspace but keep the branch? The worktree will be removed but the branch will be kept.')) return
    setMenuOpen(false)
    await workspace.getState().removeKeepBranch()
  }

  const handleAbandonKeepBoth = async () => {
    if (!confirm('Abandon this workspace but keep both the worktree and branch? They will remain but will no longer be tracked in TreeTerm.')) return
    setMenuOpen(false)
    await workspace.getState().removeKeepBoth()
  }

  const mainLabel = isDiffCleanFromParent ? 'Abandon' : 'Review & Merge'
  const mainAction = isDiffCleanFromParent ? () => { void handleAbandon() } : onOpenReview
  const mainTitle = isDiffCleanFromParent
    ? 'Abandon: No changes to merge — delete worktree and branch'
    : 'Review & Merge: Review changes and merge this workspace'

  return (
    <div className="abandon-dropdown-container">
      <button
        className="workspace-action-btn workspace-action-btn-merge abandon-split-btn"
        onClick={mainAction}
        title={mainTitle}
      >
        {mainLabel}
      </button>
      <button
        ref={buttonRef}
        className="workspace-action-btn workspace-action-btn-merge abandon-dropdown-btn"
        onClick={() => { setMenuOpen(!menuOpen); }}
        title="More options"
      >
        <ChevronDown size={14} />
      </button>
      {menuOpen && (
        <div className="abandon-menu" ref={menuRef}>
          {isDiffCleanFromParent && (
            <div className="abandon-menu-item" onClick={onOpenReview}>
              Review & Merge
              <span className="abandon-menu-hint">Review changes and merge</span>
            </div>
          )}
          {!isDiffCleanFromParent && (
            <div className="abandon-menu-item" onClick={() => { void handleAbandon() }}>
              Abandon
              <span className="abandon-menu-hint">Delete worktree and branch</span>
            </div>
          )}
          <div className="abandon-menu-item" onClick={() => { void handleAbandonKeepBranch() }}>
            Abandon (Keep Branch)
            <span className="abandon-menu-hint">Delete worktree, keep branch</span>
          </div>
          <div className="abandon-menu-item" onClick={() => { void handleAbandonKeepBoth() }}>
            Abandon (Keep Both)
            <span className="abandon-menu-hint">Keep worktree and branch</span>
          </div>
        </div>
      )}
    </div>
  )
}

/** Auto-scrolls to bottom whenever children change */
function AutoScrollPre({ className, children }: { className?: string; children: React.ReactNode }) {
  const ref = React.useRef<HTMLPreElement>(null)
  React.useEffect(() => {
    if (ref.current) {
      ref.current.scrollTop = ref.current.scrollHeight
    }
  })
  return <pre className={className} ref={ref}>{children}</pre>
}

interface GitStatusButtonProps {
  workspace: WorkspaceStore
}

function GitStatusButton({ workspace }: GitStatusButtonProps) {
  const { gitController } = useStore(workspace)
  const { gitRefreshing, hasUncommittedChanges, hasConflictsWithParent, refreshDiffStatus } = useStore(gitController)

  const handleRefresh = () => {
    void refreshDiffStatus()
  }

  let icon: React.ReactNode
  let statusClass: string
  let title: string

  if (gitRefreshing) {
    icon = <Loader2 size={14} className="spinning" />
    statusClass = ''
    title = 'Checking git status...'
  } else if (hasConflictsWithParent) {
    icon = <AlertTriangle size={14} />
    statusClass = 'workspace-action-btn-git-conflict'
    title = 'Merge conflicts with parent'
  } else if (hasUncommittedChanges) {
    icon = <CircleDot size={14} />
    statusClass = 'workspace-action-btn-git-dirty'
    title = 'Uncommitted changes'
  } else {
    icon = <Check size={14} />
    statusClass = 'workspace-action-btn-git-clean'
    title = 'Working tree clean'
  }

  return (
    <button
      className={`workspace-action-btn ${statusClass}`}
      onClick={handleRefresh}
      disabled={gitRefreshing}
      title={title}
    >
      {icon}
    </button>
  )
}

interface GitHubButtonProps {
  workspace: WorkspaceStore
}

function GitHubButton({ workspace }: GitHubButtonProps) {
  const { gitController, addTab } = useStore(workspace)
  const { prInfo, openGitHub } = useStore(gitController)
  const [loading, setLoading] = useState(false)

  const hasPr = prInfo !== null
  const unresolvedCount = prInfo?.unresolvedCount ?? 0
  const hasUnresolved = unresolvedCount > 0

  const handleClick = async () => {
    if (hasUnresolved) {
      addTab('github')
      return
    }
    setLoading(true)
    try {
      const result = await openGitHub()
      if ('url' in result) {
        window.open(result.url, '_blank')
      } else {
        console.error('[GitHubButton] error:', result.error)
        alert(result.error)
      }
    } catch (error) {
      console.error('[GitHubButton] error:', error)
    } finally {
      setLoading(false)
    }
  }

  let className = 'workspace-action-btn'
  if (hasUnresolved) {
    className += ' workspace-action-btn-github-comments'
  } else if (hasPr) {
    className += ' workspace-action-btn-github'
  }

  let title = 'Create GitHub PR'
  if (hasUnresolved) {
    title = `Address ${String(unresolvedCount)} comment${unresolvedCount === 1 ? '' : 's'}`
  } else if (hasPr) {
    title = 'Open GitHub PR'
  }

  return (
    <button
      className={className}
      onClick={() => { void handleClick() }}
      disabled={loading}
      title={title}
    >
      {/* eslint-disable-next-line @typescript-eslint/no-deprecated */}
      <Github size={14} />
      {hasUnresolved && <span className="github-comment-count">Address {String(unresolvedCount)} comment{unresolvedCount === 1 ? '' : 's'}</span>}
    </button>
  )
}

interface GitPullButtonProps {
  workspace: WorkspaceStore
}

function GitPullButton({ workspace }: GitPullButtonProps) {
  const { gitController } = useStore(workspace)
  const { behindCount, pullLoading, refreshRemoteStatus, pullFromRemote } = useStore(gitController)

  const handleRefresh = () => {
    void refreshRemoteStatus()
  }

  const handlePull = async () => {
    const result = await pullFromRemote()
    if (!result.success) {
      alert(result.error)
    }
  }

  if (behindCount > 0) {
    return (
      <button
        className="workspace-action-btn workspace-action-btn-pull"
        onClick={() => { void handlePull() }}
        disabled={pullLoading}
        title={`Pull ${String(behindCount)} commit${behindCount > 1 ? 's' : ''} from remote`}
      >
        {pullLoading ? <Loader2 size={14} className="spinning" /> : <ArrowDownToLine size={14} />}
        <span className="pull-count">{String(behindCount)}</span>
      </button>
    )
  }

  return (
    <button
      className="workspace-action-btn"
      onClick={handleRefresh}
      title="Check for remote updates"
    >
      <RefreshCw size={14} />
    </button>
  )
}
