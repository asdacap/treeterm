import { useState, useEffect, useMemo } from 'react'
import { useStore } from 'zustand'
import type { ChildWorktreeInfo, BranchInfo, WorktreeSettings, WorkspaceStore } from '../types'
import { useAppStore } from '../store/app'

interface CreateChildDialogProps {
  parentWorkspace: WorkspaceStore
  onCreate: (name: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => { success: boolean; error?: string }
  onAdopt: (worktreePath: string, branch: string, name: string, settings?: WorktreeSettings, description?: string) => Promise<{ success: boolean; error?: string }>
  onCreateFromBranch: (branch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => { success: boolean; error?: string }
  onCreateFromRemote: (remoteBranch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => { success: boolean; error?: string }
  onCancel: () => void
  openWorktreePaths: string[]
  initialMode?: TabMode
}

type TabMode = 'create' | 'existing' | 'branch' | 'remote'

export default function CreateChildDialog({
  parentWorkspace,
  onCreate,
  onAdopt,
  onCreateFromBranch,
  onCreateFromRemote,
  onCancel,
  openWorktreePaths,
  initialMode
}: CreateChildDialogProps) {
  const { workspace: parentWsData, getGitApi } = useStore(parentWorkspace)
  const git = getGitApi()
  const [mode, setMode] = useState<TabMode>(initialMode ?? 'create')
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [isProcessing, setIsProcessing] = useState(false)
  const [processingMessage, setProcessingMessage] = useState<string>('')
  const [error, setError] = useState<string | null>(null)
  const [isDetached, setIsDetached] = useState(false)

  // For existing worktrees tab
  const [existingWorktrees, setExistingWorktrees] = useState<ChildWorktreeInfo[]>([])
  const [isLoadingWorktrees, setIsLoadingWorktrees] = useState(false)
  const [selectedWorktree, setSelectedWorktree] = useState<ChildWorktreeInfo | null>(null)

  // For branch tab
  const [branches, setBranches] = useState<BranchInfo[]>([])
  const [isLoadingBranches, setIsLoadingBranches] = useState(false)
  const [selectedBranch, setSelectedBranch] = useState<BranchInfo | null>(null)
  const [branchSearch, setBranchSearch] = useState('')

  // For remote tab
  const [remoteBranches, setRemoteBranches] = useState<BranchInfo[]>([])
  const [isLoadingRemoteBranches, setIsLoadingRemoteBranches] = useState(false)
  const [selectedRemoteBranch, setSelectedRemoteBranch] = useState<BranchInfo | null>(null)
  const [remoteBranchSearch, setRemoteBranchSearch] = useState('')

  // Settings section state
  const [settingsExpanded, setSettingsExpanded] = useState(false)
  const [useCustomSettings, setUseCustomSettings] = useState(false)
  const [selectedAppId, setSelectedAppId] = useState<string>('')

  const applications = useAppStore((s) => s.applications)

  // Get inherited app name
  const inheritedApp = useMemo(() => {
    if (parentWsData.settings?.defaultApplicationId) {
      const app = applications[parentWsData.settings.defaultApplicationId]
      if (app) return app
    }
    return null
  }, [parentWsData.settings, applications])

  // Get available apps
  const availableApps = useMemo(() => Object.values(applications).filter(app => app.showInNewTabMenu), [applications])

  // Load existing worktrees when "existing" tab is selected
  useEffect(() => {
    if (mode === 'existing' && parentWsData.gitRootPath) {
      console.log('[CreateChildDialog] Loading worktrees for:', {
        gitRootPath: parentWsData.gitRootPath,
        gitBranch: parentWsData.gitBranch,
        openWorktreePaths
      })
      setIsLoadingWorktrees(true)
      git.getChildWorktrees(
        parentWsData.parentId === null ? null : parentWsData.gitBranch
      ).then(worktrees => {
        console.log('[CreateChildDialog] Received worktrees:', worktrees)
        // Filter out worktrees that are already open
        const available = worktrees.filter(wt => !openWorktreePaths.includes(wt.path))
        console.log('[CreateChildDialog] Available worktrees after filtering:', available)
        setExistingWorktrees(available)
        setIsLoadingWorktrees(false)
      }).catch((error) => {
        console.error('[CreateChildDialog] Error loading worktrees:', error)
        setExistingWorktrees([])
        setIsLoadingWorktrees(false)
      })
    } else {
      console.log('[CreateChildDialog] Skipping worktree load:', {
        mode,
        hasGitRootPath: !!parentWsData.gitRootPath
      })
    }
  }, [mode, parentWsData.gitRootPath, parentWsData.gitBranch, parentWsData.parentId, openWorktreePaths])

  // Load local branches when "branch" tab is selected
  useEffect(() => {
    if (mode === 'branch' && parentWsData.gitRootPath) {
      setIsLoadingBranches(true)
      Promise.all([
        git.listLocalBranches(),
        git.getBranchesInWorktrees()
      ]).then(([allBranches, branchesInWorktrees]) => {
        const branchInfos: BranchInfo[] = allBranches.map(name => ({
          name,
          isInWorktree: branchesInWorktrees.includes(name)
        }))
        setBranches(branchInfos)
        setIsLoadingBranches(false)
      }).catch((error) => {
        console.error('[CreateChildDialog] Error loading local branches:', error)
        setBranches([])
        setIsLoadingBranches(false)
        setError(`Failed to load branches: ${error instanceof Error ? error.message : 'Unknown error'}`)
      })
    }
  }, [mode, parentWsData.gitRootPath])

  // Load remote branches when "remote" tab is selected
  useEffect(() => {
    if (mode === 'remote' && parentWsData.gitRootPath) {
      setIsLoadingRemoteBranches(true)
      Promise.all([
        git.listRemoteBranches(),
        git.getBranchesInWorktrees(),
        git.listLocalBranches()
      ]).then(([remoteBranchNames, branchesInWorktrees, localBranches]) => {
        const branchInfos: BranchInfo[] = remoteBranchNames.map(name => {
          // Extract local name from remote branch (e.g., origin/feature -> feature)
          const localName = name.split('/').slice(1).join('/')
          // Check if already in worktree by checking both remote and local names
          const isInWorktree = branchesInWorktrees.includes(name) ||
                               branchesInWorktrees.includes(localName) ||
                               localBranches.includes(localName)
          return {
            name,
            isInWorktree
          }
        })
        setRemoteBranches(branchInfos)
        setIsLoadingRemoteBranches(false)
      }).catch((error) => {
        console.error('[CreateChildDialog] Error loading remote branches:', error)
        setRemoteBranches([])
        setIsLoadingRemoteBranches(false)
        setError(`Failed to load remote branches: ${error instanceof Error ? error.message : 'Unknown error'}`)
      })
    }
  }, [mode, parentWsData.gitRootPath])

  // Filter branches based on search
  const filteredBranches = useMemo(() => {
    if (!branchSearch.trim()) return branches
    const searchLower = branchSearch.toLowerCase()
    return branches.filter(b => b.name.toLowerCase().includes(searchLower))
  }, [branches, branchSearch])

  // Filter remote branches based on search
  const filteredRemoteBranches = useMemo(() => {
    if (!remoteBranchSearch.trim()) return remoteBranches
    const searchLower = remoteBranchSearch.toLowerCase()
    return remoteBranches.filter(b => b.name.toLowerCase().includes(searchLower))
  }, [remoteBranches, remoteBranchSearch])

  // Validate name for '/' character
  const nameValidationError = useMemo(() => {
    if (!name.trim()) return null
    if (name.includes('/')) {
      return 'Name cannot contain "/" - use simple names only'
    }
    return null
  }, [name])

  // Build settings object for child worktree
  const buildSettings = (): WorktreeSettings | undefined => {
    if (useCustomSettings && selectedAppId) {
      return { defaultApplicationId: selectedAppId }
    }
    return undefined
  }

  const handleCreateSubmit = () => {
    if (!name.trim()) {
      setError('Please enter a workspace name')
      return
    }

    if (nameValidationError) {
      setError(nameValidationError)
      return
    }

    setError(null)

    console.log('[CreateChildDialog] Creating new worktree:', name.trim())
    const settings = buildSettings()
    const desc = description.trim() || undefined
    const result = onCreate(name.trim(), isDetached, settings, desc)
    if (!result.success) {
      console.error('[CreateChildDialog] Failed to create worktree:', result.error)
      setError(result.error || 'Failed to create workspace')
    }
  }

  const handleAdoptSubmit = async () => {
    if (!selectedWorktree) {
      setError('Please select a worktree')
      return
    }

    setIsProcessing(true)
    setProcessingMessage('Opening existing worktree...')
    setError(null)

    console.log('[CreateChildDialog] Adopting existing worktree:', selectedWorktree.path)
    const settings = buildSettings()
    const desc = description.trim() || undefined
    const result = await onAdopt(
      selectedWorktree.path,
      selectedWorktree.branch,
      selectedWorktree.displayName,
      settings,
      desc
    )
    if (!result.success) {
      console.error('[CreateChildDialog] Failed to adopt worktree:', result.error)
      setError(result.error || 'Failed to open worktree')
      setIsProcessing(false)
      setProcessingMessage('')
    } else {
      console.log('[CreateChildDialog] Successfully adopted worktree')
    }
  }

  const handleBranchSubmit = () => {
    if (!selectedBranch) {
      setError('Please select a branch')
      return
    }

    setError(null)

    console.log('[CreateChildDialog] Creating worktree from branch:', selectedBranch.name)
    const settings = buildSettings()
    const desc = description.trim() || undefined
    const result = onCreateFromBranch(selectedBranch.name, isDetached, settings, desc)
    if (!result.success) {
      console.error('[CreateChildDialog] Failed to create worktree from branch:', result.error)
      setError(result.error || 'Failed to create worktree from branch')
    }
  }

  const handleRemoteSubmit = () => {
    if (!selectedRemoteBranch) {
      setError('Please select a remote branch')
      return
    }

    setError(null)

    console.log('[CreateChildDialog] Creating worktree from remote branch:', selectedRemoteBranch.name)
    const settings = buildSettings()
    const desc = description.trim() || undefined
    const result = onCreateFromRemote(selectedRemoteBranch.name, isDetached, settings, desc)
    if (!result.success) {
      console.error('[CreateChildDialog] Failed to create worktree:', result.error)
      setError(result.error || 'Failed to create worktree from remote branch')
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !isProcessing) {
      if (mode === 'create') {
        handleCreateSubmit()
      } else if (mode === 'existing' && selectedWorktree) {
        handleAdoptSubmit()
      } else if (mode === 'branch' && selectedBranch) {
        handleBranchSubmit()
      } else if (mode === 'remote' && selectedRemoteBranch) {
        handleRemoteSubmit()
      }
    }
    if (e.key === 'Escape') {
      onCancel()
    }
  }

  return (
    <div className="dialog-overlay" onClick={onCancel}>
      <div className="create-child-dialog" onClick={(e) => e.stopPropagation()} onKeyDown={handleKeyDown}>
        <div className="create-child-dialog-header">
          <h2>Add Child Workspace</h2>
          <button className="dialog-close" onClick={onCancel}>
            x
          </button>
        </div>

        {/* Tab Switcher */}
        <div className="create-child-tabs">
          <button
            className={`create-child-tab ${mode === 'create' ? 'active' : ''}`}
            onClick={() => { setMode('create'); setError(null) }}
          >
            Create New
          </button>
          <button
            className={`create-child-tab ${mode === 'existing' ? 'active' : ''}`}
            onClick={() => { setMode('existing'); setError(null) }}
          >
            Open Existing
          </button>
          <button
            className={`create-child-tab ${mode === 'branch' ? 'active' : ''}`}
            onClick={() => { setMode('branch'); setError(null); setSelectedBranch(null); setBranchSearch('') }}
          >
            Open Branch
          </button>
          <button
            className={`create-child-tab ${mode === 'remote' ? 'active' : ''}`}
            onClick={() => { setMode('remote'); setError(null); setSelectedRemoteBranch(null); setRemoteBranchSearch('') }}
          >
            Open Remote
          </button>
        </div>

        <div className="create-child-dialog-content">
          <div className="create-child-dialog-info">
            <span className="create-child-label">Parent:</span>
            <span className="create-child-value">{parentWsData.name}</span>
          </div>

          {mode === 'create' ? (
            /* Create New Tab */
            <>
              <div className="create-child-dialog-field">
                <label htmlFor="workspace-name">Name</label>
                <input
                  id="workspace-name"
                  type="text"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="Workspace name..."
                  autoFocus
                  disabled={isProcessing}
                />
                {nameValidationError && (
                  <div className="create-child-field-error">{nameValidationError}</div>
                )}
              </div>
            </>
          ) : mode === 'existing' ? (
            /* Open Existing Tab */
            <div className="create-child-existing-list">
              {isLoadingWorktrees ? (
                <div className="create-child-loading">Loading worktrees...</div>
              ) : existingWorktrees.length === 0 ? (
                <div className="create-child-empty">
                  No available child worktrees found
                </div>
              ) : (
                existingWorktrees.map(wt => (
                  <div
                    key={wt.path}
                    className={`create-child-worktree-item ${selectedWorktree?.path === wt.path ? 'selected' : ''}`}
                    onClick={() => setSelectedWorktree(wt)}
                  >
                    <span className="worktree-name">{wt.displayName}</span>
                    <span className="worktree-branch">{wt.branch}</span>
                  </div>
                ))
              )}
            </div>
          ) : mode === 'branch' ? (
            /* Open Branch Tab */
            <>
              <div className="create-child-search-field">
                <input
                  type="text"
                  value={branchSearch}
                  onChange={(e) => setBranchSearch(e.target.value)}
                  placeholder="Search branches..."
                  disabled={isProcessing}
                />
              </div>
              <div className="create-child-existing-list">
                {isLoadingBranches ? (
                  <div className="create-child-loading">Loading branches...</div>
                ) : filteredBranches.length === 0 ? (
                  <div className="create-child-empty">
                    {branchSearch.trim() ? 'No branches match your search' : 'No local branches found'}
                  </div>
                ) : (
                  filteredBranches.map(branch => (
                    <div
                      key={branch.name}
                      className={`create-child-worktree-item ${selectedBranch?.name === branch.name ? 'selected' : ''} ${branch.isInWorktree ? 'disabled' : ''}`}
                      onClick={() => !branch.isInWorktree && setSelectedBranch(branch)}
                      title={branch.isInWorktree ? 'Branch is already in a worktree' : undefined}
                    >
                      <span className="worktree-name">{branch.name}</span>
                      {branch.isInWorktree && <span className="worktree-badge">In Worktree</span>}
                    </div>
                  ))
                )}
              </div>
            </>
          ) : (
            /* Open Remote Tab */
            <>
              <div className="create-child-search-field">
                <input
                  type="text"
                  value={remoteBranchSearch}
                  onChange={(e) => setRemoteBranchSearch(e.target.value)}
                  placeholder="Search remote branches..."
                  disabled={isProcessing}
                />
              </div>
              <div className="create-child-existing-list">
                {isLoadingRemoteBranches ? (
                  <div className="create-child-loading">Loading remote branches...</div>
                ) : filteredRemoteBranches.length === 0 ? (
                  <div className="create-child-empty">
                    {remoteBranchSearch.trim() ? 'No branches match your search' : 'No remote branches found'}
                  </div>
                ) : (
                  filteredRemoteBranches.map(branch => (
                    <div
                      key={branch.name}
                      className={`create-child-worktree-item ${selectedRemoteBranch?.name === branch.name ? 'selected' : ''} ${branch.isInWorktree ? 'disabled' : ''}`}
                      onClick={() => !branch.isInWorktree && setSelectedRemoteBranch(branch)}
                      title={branch.isInWorktree ? 'Branch is already in a worktree' : undefined}
                    >
                      <span className="worktree-name">{branch.name}</span>
                      {branch.isInWorktree && <span className="worktree-badge">In Worktree</span>}
                    </div>
                  ))
                )}
              </div>
            </>
          )}

          {/* Description */}
          <div className="create-child-dialog-field">
            <label htmlFor="workspace-description">Description</label>
            <textarea
              id="workspace-description"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="Optional description..."
              disabled={isProcessing}
              rows={2}
              className="create-child-description"
            />
          </div>

          {/* Detached checkbox - shown for create, branch, and remote tabs */}
          {mode !== 'existing' && (
            <div className="create-child-detached-checkbox">
              <label>
                <input
                  type="checkbox"
                  checked={isDetached}
                  onChange={(e) => setIsDetached(e.target.checked)}
                  disabled={isProcessing}
                />
                <span>Detached worktree (no merge, only close and clean)</span>
              </label>
            </div>
          )}

          {/* Settings Section */}
          <div className="create-child-settings-section">
            <button
              className="create-child-settings-toggle"
              onClick={() => setSettingsExpanded(!settingsExpanded)}
              type="button"
            >
              <span>{settingsExpanded ? '▼' : '▶'}</span>
              <span>Settings</span>
            </button>

            {settingsExpanded && (
              <div className="create-child-settings-content">
                <div className="create-child-settings-inherited">
                  <span className="settings-label">Inherited:</span>
                  <span className="settings-value">
                    {inheritedApp ? inheritedApp.name : 'Global Default'}
                  </span>
                </div>

                <div className="create-child-settings-option">
                  <label className="settings-checkbox-label">
                    <input
                      type="checkbox"
                      checked={useCustomSettings}
                      onChange={(e) => {
                        setUseCustomSettings(e.target.checked)
                        if (!e.target.checked) {
                          setSelectedAppId('')
                        }
                      }}
                      disabled={isProcessing}
                    />
                    Use custom default application
                  </label>
                </div>

                {useCustomSettings && (
                  <div className="create-child-settings-select">
                    <label>Default Application</label>
                    <select
                      value={selectedAppId}
                      onChange={(e) => setSelectedAppId(e.target.value)}
                      disabled={isProcessing}
                    >
                      <option value="">Select an application...</option>
                      {availableApps.map((app) => (
                        <option key={app.id} value={app.id}>
                          {app.name}
                        </option>
                      ))}
                    </select>
                  </div>
                )}
              </div>
            )}
          </div>

          {error && <div className="create-child-error">{error}</div>}
          {isProcessing && processingMessage && (
            <div className="create-child-processing">{processingMessage}</div>
          )}
        </div>

        <div className="create-child-dialog-actions">
          <button className="dialog-btn cancel" onClick={onCancel} disabled={isProcessing}>
            Cancel
          </button>
          {mode === 'create' ? (
            <button
              className="dialog-btn create"
              onClick={handleCreateSubmit}
              disabled={isProcessing || !!nameValidationError}
            >
              {isProcessing ? 'Creating... Please wait' : 'Create'}
            </button>
          ) : mode === 'existing' ? (
            <button
              className="dialog-btn create"
              onClick={handleAdoptSubmit}
              disabled={isProcessing || !selectedWorktree}
            >
              {isProcessing ? 'Opening... Please wait' : 'Open'}
            </button>
          ) : mode === 'branch' ? (
            <button
              className="dialog-btn create"
              onClick={handleBranchSubmit}
              disabled={isProcessing || !selectedBranch}
            >
              {isProcessing ? 'Opening... Please wait' : 'Open'}
            </button>
          ) : (
            <button
              className="dialog-btn create"
              onClick={handleRemoteSubmit}
              disabled={isProcessing || !selectedRemoteBranch}
            >
              {isProcessing ? 'Opening... Please wait' : 'Open'}
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
