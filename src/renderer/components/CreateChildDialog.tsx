import { useState, useEffect } from 'react'
import { useStore } from 'zustand'
import type { WorktreeInfo, BranchInfo, WorktreeSettings, WorkspaceStore } from '../types'
import { useAppStore } from '../store/app'
import { useGitApi } from '../hooks/useWorkspaceApis'

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

export enum TabMode {
  Create = 'create',
  Existing = 'existing',
  Branch = 'branch',
  Remote = 'remote',
}

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
  const { workspace: parentWsData } = useStore(parentWorkspace)
  const git = useGitApi(parentWorkspace)
  const [mode, setMode] = useState(initialMode ?? TabMode.Create)
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [isProcessing, setIsProcessing] = useState(false)
  const [processingMessage, setProcessingMessage] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [isDetached, setIsDetached] = useState(false)

  // For existing worktrees tab
  const [selectedWorktree, setSelectedWorktree] = useState<WorktreeInfo | null>(null)

  // For branch tab
  const [selectedBranch, setSelectedBranch] = useState<BranchInfo | null>(null)

  // For remote tab
  const [selectedRemoteBranch, setSelectedRemoteBranch] = useState<BranchInfo | null>(null)

  // Settings section state
  const [settingsExpanded, setSettingsExpanded] = useState(false)
  const [useCustomSettings, setUseCustomSettings] = useState(false)
  const [selectedAppId, setSelectedAppId] = useState('')

  const applications = useAppStore((s) => s.applications)

  // Get inherited app name
  const inheritedApp = parentWsData.settings.defaultApplicationId
     
    ? applications.get(parentWsData.settings.defaultApplicationId) ?? null
    : null

  // Get available apps
  const availableApps = Array.from(applications.values()).filter(app => app.showInNewTabMenu)

  // Validate name for '/' character
  const nameValidationError = !name.trim() ? null
    : name.includes('/') ? 'Name cannot contain "/" - use simple names only'
    : null

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
      selectedWorktree.branch,
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
      if (mode === TabMode.Create) {
        handleCreateSubmit()
      } else if (mode === TabMode.Existing && selectedWorktree) {
        void handleAdoptSubmit()
      } else if (mode === TabMode.Branch && selectedBranch) {
        handleBranchSubmit()
      } else if (mode === TabMode.Remote && selectedRemoteBranch) {
        handleRemoteSubmit()
      }
    }
    if (e.key === 'Escape') {
      onCancel()
    }
  }

  return (
    <div className="dialog-overlay" onClick={onCancel}>
      <div className="create-child-dialog" onClick={(e) => { e.stopPropagation(); }} onKeyDown={handleKeyDown}>
        <div className="create-child-dialog-header">
          <h2>Add Child Workspace</h2>
          <button className="dialog-close" onClick={onCancel}>
            x
          </button>
        </div>

        {/* Tab Switcher */}
        <div className="create-child-tabs">
          <button
            className={`create-child-tab ${mode === TabMode.Create ? 'active' : ''}`}
            onClick={() => { setMode(TabMode.Create); setError(null) }}
          >
            Create New
          </button>
          <button
            className={`create-child-tab ${mode === TabMode.Existing ? 'active' : ''}`}
            onClick={() => { setMode(TabMode.Existing); setError(null) }}
          >
            Open Existing
          </button>
          <button
            className={`create-child-tab ${mode === TabMode.Branch ? 'active' : ''}`}
            onClick={() => { setMode(TabMode.Branch); setError(null); setSelectedBranch(null) }}
          >
            Open Branch
          </button>
          <button
            className={`create-child-tab ${mode === TabMode.Remote ? 'active' : ''}`}
            onClick={() => { setMode(TabMode.Remote); setError(null); setSelectedRemoteBranch(null) }}
          >
            Open Remote
          </button>
        </div>

        <div className="create-child-dialog-content">
          <div className="create-child-dialog-info">
            <span className="create-child-label">Parent:</span>
            <span className="create-child-value">{parentWsData.name}</span>
          </div>

          {mode === TabMode.Create ? (
            /* Create New Tab */
            <>
              <div className="create-child-dialog-field">
                <label htmlFor="workspace-name">Name</label>
                <input
                  id="workspace-name"
                  type="text"
                  value={name}
                  onChange={(e) => { setName(e.target.value); }}
                  placeholder="Workspace name..."
                  autoFocus
                  disabled={isProcessing}
                />
                {nameValidationError && (
                  <div className="create-child-field-error">{nameValidationError}</div>
                )}
              </div>
            </>
          ) : mode === TabMode.Existing ? (
            /* Open Existing Tab */
            <ExistingWorktreesLoader
              key={parentWsData.gitRootPath}
              git={git}
              openWorktreePaths={openWorktreePaths}
              selectedWorktree={selectedWorktree}
              onSelect={setSelectedWorktree}
            />
          ) : mode === TabMode.Branch ? (
            /* Open Branch Tab */
            <LocalBranchesLoader
              key={parentWsData.gitRootPath}
              git={git}
              isProcessing={isProcessing}
              selectedBranch={selectedBranch}
              onSelect={setSelectedBranch}
              onError={setError}
            />
          ) : (
            /* Open Remote Tab */
            <RemoteBranchesLoader
              key={parentWsData.gitRootPath}
              git={git}
              isProcessing={isProcessing}
              selectedBranch={selectedRemoteBranch}
              onSelect={setSelectedRemoteBranch}
              onError={setError}
            />
          )}

          {/* Description */}
          <div className="create-child-dialog-field">
            <label htmlFor="workspace-description">Description</label>
            <textarea
              id="workspace-description"
              value={description}
              onChange={(e) => { setDescription(e.target.value); }}
              placeholder="Optional description..."
              disabled={isProcessing}
              rows={2}
              className="create-child-description"
            />
          </div>

          {/* Detached checkbox - shown for create, branch, and remote tabs */}
          {mode !== TabMode.Existing && (
            <div className="create-child-detached-checkbox">
              <label>
                <input
                  type="checkbox"
                  checked={isDetached}
                  onChange={(e) => { setIsDetached(e.target.checked); }}
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
              onClick={() => { setSettingsExpanded(!settingsExpanded); }}
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
                      onChange={(e) => { setSelectedAppId(e.target.value); }}
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
          {mode === TabMode.Create ? (
            <button
              className="dialog-btn create"
              onClick={handleCreateSubmit}
              disabled={isProcessing || !!nameValidationError}
            >
              {isProcessing ? 'Creating... Please wait' : 'Create'}
            </button>
          ) : mode === TabMode.Existing ? (
            <button
              className="dialog-btn create"
              onClick={() => { void handleAdoptSubmit(); }}
              disabled={isProcessing || !selectedWorktree}
            >
              {isProcessing ? 'Opening... Please wait' : 'Open'}
            </button>
          ) : mode === TabMode.Branch ? (
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

/** Loads existing worktrees on mount */
function ExistingWorktreesLoader({ git, openWorktreePaths, selectedWorktree, onSelect }: {
  git: ReturnType<typeof useGitApi>
  openWorktreePaths: string[]
  selectedWorktree: WorktreeInfo | null
  onSelect: (wt: WorktreeInfo) => void
}) {
  const [worktrees, setWorktrees] = useState<WorktreeInfo[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    void git.listWorktrees().then(result => {
      setWorktrees(result.filter(wt => !openWorktreePaths.includes(wt.path)))
      setLoading(false)
    }).catch(() => {
      setWorktrees([])
      setLoading(false)
    })
  }, [git, openWorktreePaths])

  return (
    <div className="create-child-existing-list">
      {loading ? (
        <div className="create-child-loading">Loading worktrees...</div>
      ) : worktrees.length === 0 ? (
        <div className="create-child-empty">No available child worktrees found</div>
      ) : (
        worktrees.map(wt => (
          <div
            key={wt.path}
            className={`create-child-worktree-item ${selectedWorktree?.path === wt.path ? 'selected' : ''}`}
            onClick={() => { onSelect(wt); }}
          >
            <span className="worktree-name">{wt.branch}</span>
            <span className="worktree-branch">{wt.branch}</span>
          </div>
        ))
      )}
    </div>
  )
}

/** Loads local branches on mount */
function LocalBranchesLoader({ git, isProcessing, selectedBranch, onSelect, onError }: {
  git: ReturnType<typeof useGitApi>
  isProcessing: boolean
  selectedBranch: BranchInfo | null
  onSelect: (b: BranchInfo) => void
  onError: (msg: string) => void
}) {
  const [branches, setBranches] = useState<BranchInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [search, setSearch] = useState('')

  useEffect(() => {
    void Promise.all([
      git.listLocalBranches(),
      git.getBranchesInWorktrees()
    ]).then(([allBranches, branchesInWorktrees]) => {
      setBranches(allBranches.map(name => ({ name, isInWorktree: branchesInWorktrees.includes(name) })))
      setLoading(false)
    }).catch((error: unknown) => {
      setBranches([])
      setLoading(false)
      onError(`Failed to load branches: ${error instanceof Error ? error.message : 'Unknown error'}`)
    })
  }, [git, onError])

  const filtered = !search.trim() ? branches : branches.filter(b => b.name.toLowerCase().includes(search.toLowerCase()))

  return (
    <>
      <div className="create-child-search-field">
        <input type="text" value={search} onChange={(e) => { setSearch(e.target.value); }} placeholder="Search branches..." disabled={isProcessing} />
      </div>
      <div className="create-child-existing-list">
        {loading ? (
          <div className="create-child-loading">Loading branches...</div>
        ) : filtered.length === 0 ? (
          <div className="create-child-empty">{search.trim() ? 'No branches match your search' : 'No local branches found'}</div>
        ) : (
          filtered.map(branch => (
            <div
              key={branch.name}
              className={`create-child-worktree-item ${selectedBranch?.name === branch.name ? 'selected' : ''} ${branch.isInWorktree ? 'disabled' : ''}`}
              onClick={() => { if (!branch.isInWorktree) onSelect(branch) }}
              title={branch.isInWorktree ? 'Branch is already in a worktree' : undefined}
            >
              <span className="worktree-name">{branch.name}</span>
              {branch.isInWorktree && <span className="worktree-badge">In Worktree</span>}
            </div>
          ))
        )}
      </div>
    </>
  )
}

/** Loads remote branches on mount */
function RemoteBranchesLoader({ git, isProcessing, selectedBranch, onSelect, onError }: {
  git: ReturnType<typeof useGitApi>
  isProcessing: boolean
  selectedBranch: BranchInfo | null
  onSelect: (b: BranchInfo) => void
  onError: (msg: string) => void
}) {
  const [branches, setBranches] = useState<BranchInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [search, setSearch] = useState('')

  useEffect(() => {
    void Promise.all([
      git.listRemoteBranches(),
      git.getBranchesInWorktrees(),
      git.listLocalBranches()
    ]).then(([remoteBranchNames, branchesInWorktrees, localBranches]) => {
      setBranches(remoteBranchNames.map(name => {
        const localName = name.split('/').slice(1).join('/')
        return {
          name,
          isInWorktree: branchesInWorktrees.includes(name) || branchesInWorktrees.includes(localName) || localBranches.includes(localName)
        }
      }))
      setLoading(false)
    }).catch((error: unknown) => {
      setBranches([])
      setLoading(false)
      onError(`Failed to load remote branches: ${error instanceof Error ? error.message : 'Unknown error'}`)
    })
  }, [git, onError])

  const filtered = !search.trim() ? branches : branches.filter(b => b.name.toLowerCase().includes(search.toLowerCase()))

  return (
    <>
      <div className="create-child-search-field">
        <input type="text" value={search} onChange={(e) => { setSearch(e.target.value); }} placeholder="Search remote branches..." disabled={isProcessing} />
      </div>
      <div className="create-child-existing-list">
        {loading ? (
          <div className="create-child-loading">Loading remote branches...</div>
        ) : filtered.length === 0 ? (
          <div className="create-child-empty">{search.trim() ? 'No branches match your search' : 'No remote branches found'}</div>
        ) : (
          filtered.map(branch => (
            <div
              key={branch.name}
              className={`create-child-worktree-item ${selectedBranch?.name === branch.name ? 'selected' : ''} ${branch.isInWorktree ? 'disabled' : ''}`}
              onClick={() => { if (!branch.isInWorktree) onSelect(branch) }}
              title={branch.isInWorktree ? 'Branch is already in a worktree' : undefined}
            >
              <span className="worktree-name">{branch.name}</span>
              {branch.isInWorktree && <span className="worktree-badge">In Worktree</span>}
            </div>
          ))
        )}
      </div>
    </>
  )
}
