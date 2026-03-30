import { useEffect, useCallback, useState, useRef, useMemo } from 'react'
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
  const menuApplications = useMemo(() => Object.values(applications).filter((app) => app.showInNewTabMenu), [applications])

  const [branchCopied, setBranchCopied] = useState(false)

  const activeEntry = activeWorkspaceId ? workspaces[activeWorkspaceId] ?? null : null
  const activeWorkspace = activeEntry && (activeEntry.status === 'loaded' || activeEntry.status === 'operation-error') ? activeEntry.data : null
  const activeHandle = activeEntry && (activeEntry.status === 'loaded' || activeEntry.status === 'operation-error') ? activeEntry.store : null
  const outputRef = useRef<HTMLPreElement>(null)

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
    if (!activeHandle) return
    const trimmedName = editName.trim()
    if (trimmedName) {
      activeHandle.getState().updateMetadata('displayName', trimmedName)
    }
    setIsEditingName(false)
  }, [activeHandle, editName])

  const handleStartEditDescription = useCallback(() => {
    if (!activeWorkspace) return
    setEditDescription(activeWorkspace.metadata?.description || '')
    setIsEditingDescription(true)
  }, [activeWorkspace])

  const handleSaveDescription = useCallback(() => {
    if (!activeHandle) return
    activeHandle.getState().updateMetadata('description', editDescription.trim())
    setIsEditingDescription(false)
  }, [activeHandle, editDescription])

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

  // Signal active tab to focus after workspace switch (keyboard navigation)
  useEffect(() => {
    if (!activeHandle) return
    activeHandle.getState().requestFocus()
  }, [activeWorkspaceId])

  // Auto-scroll loading output
  const outputLength = activeEntry?.status === 'loading' ? activeEntry.output.length : 0
  useEffect(() => {
    if (outputRef.current) {
      outputRef.current.scrollTop = outputRef.current.scrollHeight
    }
  }, [outputLength])

  // Create new tab using the first available application
  const handleNewDefaultTab = useCallback(() => {
    const defaultApp = menuApplications[0]
    if (activeHandle && defaultApp) {
      activeHandle.getState().addTab(defaultApp.id)
    }
  }, [activeHandle, menuApplications])

  const handleCloseTab = useCallback(
    (tabId: string) => {
      if (activeHandle) {
        activeHandle.getState().removeTab(tabId)
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
  const openWorktreePaths = useMemo(() => {
    return Object.values(workspaces)
      .filter((e): e is Extract<typeof e, { status: 'loaded' | 'operation-error' }> =>
        e.status === 'loaded' || e.status === 'operation-error')
      .filter(e => e.data.isWorktree)
      .map(e => e.data.path)
  }, [workspaces])

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

  // Abandon handlers — these are only callable when activeHandle exists (past the early return)
  const handleAbandon = async () => {
    if (!confirm('Are you sure you want to abandon this workspace? All changes will be discarded.')) {
      return
    }
    setAbandonMenuOpen(false)
    await activeHandle!.getState().remove()
  }

  const handleAbandonKeepBranch = async () => {
    if (!confirm('Abandon this workspace but keep the branch? The worktree will be removed but the branch will be kept.')) {
      return
    }
    setAbandonMenuOpen(false)
    await activeHandle!.getState().removeKeepBranch()
  }

  const handleAbandonKeepBoth = async () => {
    if (!confirm('Abandon this workspace but keep both the worktree and branch? They will remain but will no longer be tracked in TreeTerm.')) {
      return
    }
    setAbandonMenuOpen(false)
    await activeHandle!.getState().removeKeepBoth()
  }

  // Compute flattened workspace list for navigation
  const flattenedWorkspaceIds = useMemo(() => {
    const result: string[] = []
    // Build parent lookup for loaded entries
    const parentMap = new Map<string | null, string[]>()
    for (const [id, entry] of Object.entries(workspaces)) {
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
    // Start from root workspaces
    const roots = parentMap.get(null) ?? []
    roots.forEach(traverse)
    return result
  }, [workspaces])

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
      handleSelectTab(tabs[newIndex].id)
    },
    prevTab: () => {
      if (!activeWorkspace) return
      const currentIndex = tabs.findIndex((t) => t.id === activeWorkspace.activeTabId)
      const newIndex = currentIndex > 0 ? currentIndex - 1 : tabs.length - 1
      handleSelectTab(tabs[newIndex].id)
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
        handleSelectTab(tabs[index].id)
      }
    }
  })
  const activeTabId = activeWorkspace?.activeTabId || tabs[0]?.id

  // Prompt description: show button next to description for AI harness tabs that haven't been prompted yet
  const activeTab = tabs.find((t) => t.id === activeTabId)
  const showPromptDescriptionButton = activeTab?.applicationId.startsWith('aiharness-')
    && activeWorkspace?.metadata?.description
    && !activeWorkspace?.metadata?.descriptionPrompted

  const handlePromptDescriptionDismiss = useCallback(() => {
    if (activeHandle) {
      activeHandle.getState().updateMetadata('descriptionPrompted', 'true')
    }
  }, [activeHandle])

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
                    <span
                      className={`workspace-branch${branchCopied ? ' copied' : ''}`}
                      onClick={() => {
                        clipboard.writeText(activeWorkspace.gitBranch!)
                        setBranchCopied(true)
                        setTimeout(() => setBranchCopied(false), 1500)
                      }}
                      title="Copy branch name"
                    >{branchCopied ? 'Copied!' : activeWorkspace.gitBranch}</span>
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
                      runActions={activeHandle.getState().getRunActionsApi()}
                      onRun={async (ptyId, actionId) => {
                        const tabId = activeHandle.getState().addTab('terminal', { ptyId, ptyHandle: null, keepOnExit: true })
                        activeHandle.getState().updateTabTitle(tabId, actionId)
                      }}
                    />
                  )}
                  {activeWorkspace.isWorktree && activeWorkspace.parentId && activeWorkspace.gitBranch && activeHandle && (
                    <GitHubButton workspace={activeHandle} />
                  )}
                  {activeWorkspace.isWorktree && activeWorkspace.parentId && activeHandle && (
                    <MergeAbandonButton
                      workspace={activeHandle}
                      abandonMenuOpen={abandonMenuOpen}
                      abandonMenuRef={abandonMenuRef}
                      abandonButtonRef={abandonButtonRef}
                      onToggleMenu={() => setAbandonMenuOpen(!abandonMenuOpen)}
                      onOpenReview={handleOpenReview}
                      onAbandon={handleAbandon}
                      onAbandonKeepBranch={handleAbandonKeepBranch}
                      onAbandonKeepBoth={handleAbandonKeepBoth}
                    />
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
                        workspace={activeHandle!}
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
            {activeEntry?.status === 'loading' && (
              <div className="workspace-loading">
                <div className="workspace-loading-header">
                  <Loader2 size={16} className="spinning" />
                  <span>{activeEntry.message}</span>
                </div>
                {activeEntry.output.length > 0 && (
                  <pre className="workspace-loading-output" ref={outputRef}>
                    {activeEntry.output.join('')}
                  </pre>
                )}
              </div>
            )}
            {(activeEntry?.status === 'error' || activeEntry?.status === 'operation-error') && (
              <div className="workspace-load-error">
                <div className="workspace-load-error-content">
                  <h3>Operation failed</h3>
                  <p className="workspace-load-error-message">{activeEntry.error}</p>
                  <div className="workspace-load-error-actions">
                    {activeEntry.status === 'operation-error' && (
                      <button className="workspace-action-btn" onClick={() => clearWorkspaceError(activeWorkspaceId!)}>
                        Dismiss
                      </button>
                    )}
                    <button className="workspace-action-btn workspace-action-btn-danger" onClick={() => closeWorkspace(activeWorkspaceId!)}>
                      Close
                    </button>
                  </div>
                </div>
              </div>
            )}
          </>
        )}
        <div className="workspace-terminal" style={{ display: activeEntry?.status === 'loaded' ? 'flex' : 'none' }}>
          {Object.entries(workspaces).map(([wsId, entry]) => {
            if (entry.status !== 'loaded' && entry.status !== 'operation-error') return null
            const isActive = wsId === activeWorkspaceId
            return (
              <div key={wsId} style={{ display: isActive ? 'contents' : 'none', height: '100%', width: '100%' }}>
                <FlexLayoutPane
                  workspace={entry.store}
                  onNewTab={(applicationId: string) => {
                    if (applicationId === 'review') {
                      entry.store.getState().addTab<ReviewState>('review', {
                        parentWorkspaceId: entry.data.parentId ?? undefined
                      })
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

interface MergeAbandonButtonProps {
  workspace: WorkspaceStore
  abandonMenuOpen: boolean
  abandonMenuRef: React.Ref<HTMLDivElement>
  abandonButtonRef: React.Ref<HTMLButtonElement>
  onToggleMenu: () => void
  onOpenReview: () => void
  onAbandon: () => void
  onAbandonKeepBranch: () => void
  onAbandonKeepBoth: () => void
}

function MergeAbandonButton({
  workspace, abandonMenuOpen, abandonMenuRef, abandonButtonRef,
  onToggleMenu, onOpenReview, onAbandon, onAbandonKeepBranch, onAbandonKeepBoth,
}: MergeAbandonButtonProps) {
  const { gitController } = useStore(workspace)
  const { isDiffCleanFromParent } = useStore(gitController)

  const mainLabel = isDiffCleanFromParent ? 'Abandon' : 'Review & Merge'
  const mainAction = isDiffCleanFromParent ? onAbandon : onOpenReview
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
        ref={abandonButtonRef}
        className="workspace-action-btn workspace-action-btn-merge abandon-dropdown-btn"
        onClick={onToggleMenu}
        title="More options"
      >
        <ChevronDown size={14} />
      </button>
      {abandonMenuOpen && (
        <div className="abandon-menu" ref={abandonMenuRef}>
          {isDiffCleanFromParent && (
            <div className="abandon-menu-item" onClick={onOpenReview}>
              Review & Merge
              <span className="abandon-menu-hint">Review changes and merge</span>
            </div>
          )}
          {!isDiffCleanFromParent && (
            <div className="abandon-menu-item" onClick={onAbandon}>
              Abandon
              <span className="abandon-menu-hint">Delete worktree and branch</span>
            </div>
          )}
          <div className="abandon-menu-item" onClick={onAbandonKeepBranch}>
            Abandon (Keep Branch)
            <span className="abandon-menu-hint">Delete worktree, keep branch</span>
          </div>
          <div className="abandon-menu-item" onClick={onAbandonKeepBoth}>
            Abandon (Keep Both)
            <span className="abandon-menu-hint">Keep worktree and branch</span>
          </div>
        </div>
      )}
    </div>
  )
}

interface GitStatusButtonProps {
  workspace: WorkspaceStore
}

function GitStatusButton({ workspace }: GitStatusButtonProps) {
  const { gitController } = useStore(workspace)
  const { gitRefreshing, hasUncommittedChanges, hasConflictsWithParent, refreshDiffStatus } = useStore(gitController)

  const handleRefresh = async () => {
    await refreshDiffStatus()
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
    title = `Address ${unresolvedCount} comment${unresolvedCount === 1 ? '' : 's'}`
  } else if (hasPr) {
    title = 'Open GitHub PR'
  }

  return (
    <button
      className={className}
      onClick={handleClick}
      disabled={loading}
      title={title}
    >
      <Github size={14} />
      {hasUnresolved && <span className="github-comment-count">Address {unresolvedCount} comment{unresolvedCount === 1 ? '' : 's'}</span>}
    </button>
  )
}

interface GitPullButtonProps {
  workspace: WorkspaceStore
}

function GitPullButton({ workspace }: GitPullButtonProps) {
  const { gitController } = useStore(workspace)
  const { behindCount, pullLoading, refreshRemoteStatus, pullFromRemote } = useStore(gitController)

  const handleRefresh = async () => {
    await refreshRemoteStatus()
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
        onClick={handlePull}
        disabled={pullLoading}
        title={`Pull ${behindCount} commit${behindCount > 1 ? 's' : ''} from remote`}
      >
        {pullLoading ? <Loader2 size={14} className="spinning" /> : <ArrowDownToLine size={14} />}
        <span className="pull-count">{behindCount}</span>
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
