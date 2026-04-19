/* eslint-disable custom/no-string-literal-comparison -- TODO: migrate existing string-literal comparisons to enums */
import { useState, useEffect } from 'react'
import { useStore } from 'zustand'
import type { WorktreeInfo, BranchInfo, WorktreeSettings, WorkspaceStore } from '../types'
import { useAppStore } from '../store/app'
import { useGitApi, useWorktreeRegistryApi } from '../hooks/useWorkspaceApis'
import type { WorktreeRegistryApi, WorktreeRegistryEntry } from '../lib/worktreeRegistry'

interface CreateChildDialogProps {
  parentWorkspace: WorkspaceStore
  onCreate: (name: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => { success: boolean; error?: string }
  onAdopt: (worktreePath: string, branch: string, name: string, settings?: WorktreeSettings, description?: string, displayName?: string) => Promise<{ success: boolean; error?: string }>
  onCreateFromBranch: (branch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => { success: boolean; error?: string }
  onCreateFromRemote: (remoteBranch: string, isDetached: boolean, settings?: WorktreeSettings, description?: string) => { success: boolean; error?: string }
  onCancel: () => void
  openWorktreePaths: string[]
  initialMode?: TabMode
}

export enum TabMode {
  Create = 'create',
  Existing = 'existing',
  Recent = 'recent',
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
  const parentWsData = useStore(parentWorkspace, s => s.workspace)
  const parentDefaultAppId = useStore(parentWorkspace, s => s.settings.defaultApplicationId)
  const git = useGitApi(parentWorkspace)
  const worktreeRegistry = useWorktreeRegistryApi(parentWorkspace)
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

  // For recent tab
  const [selectedRecent, setSelectedRecent] = useState<{ worktree: WorktreeInfo; entry: WorktreeRegistryEntry } | null>(null)

  // Settings section state
  const [settingsExpanded, setSettingsExpanded] = useState(false)
  const [useCustomSettings, setUseCustomSettings] = useState(false)
  const [selectedAppId, setSelectedAppId] = useState('')

  const applications = useAppStore((s) => s.applications)

  // Get inherited app name
  const inheritedApp = parentDefaultAppId
    ? applications.get(parentDefaultAppId) ?? null
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
    const trimmedName = name.trim()
    const displayName = trimmedName && trimmedName !== selectedWorktree.branch ? trimmedName : undefined
    const result = await onAdopt(
      selectedWorktree.path,
      selectedWorktree.branch,
      selectedWorktree.branch,
      settings,
      desc,
      displayName
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

  const handleRecentSubmit = async () => {
    if (!selectedRecent) {
      setError('Please select a recent workspace')
      return
    }

    setIsProcessing(true)
    setProcessingMessage('Opening recent workspace...')
    setError(null)

    const settings = buildSettings()
    const desc = description.trim() || undefined
    const trimmedName = name.trim()
    const displayName = trimmedName && trimmedName !== selectedRecent.worktree.branch ? trimmedName : undefined
    const result = await onAdopt(
      selectedRecent.worktree.path,
      selectedRecent.worktree.branch,
      selectedRecent.worktree.branch,
      settings,
      desc,
      displayName
    )
    if (!result.success) {
      console.error('[CreateChildDialog] Failed to adopt recent workspace:', result.error)
      setError(result.error || 'Failed to open recent workspace')
      setIsProcessing(false)
      setProcessingMessage('')
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
      } else if (mode === TabMode.Recent && selectedRecent) {
        void handleRecentSubmit()
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
            className={`create-child-tab ${mode === TabMode.Recent ? 'active' : ''}`}
            onClick={() => { setMode(TabMode.Recent); setError(null); setSelectedRecent(null) }}
          >
            Recent Workspace
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
            <>
              <div className="create-child-dialog-field">
                <label htmlFor="workspace-name-existing">Name (optional)</label>
                <input
                  id="workspace-name-existing"
                  type="text"
                  value={name}
                  onChange={(e) => { setName(e.target.value); }}
                  placeholder="Leave empty to use branch name..."
                  disabled={isProcessing}
                />
              </div>
              <ExistingWorktreesLoader
                key={parentWsData.gitRootPath}
                git={git}
                worktreeRegistry={worktreeRegistry}
                openWorktreePaths={openWorktreePaths}
                selectedWorktree={selectedWorktree}
                onSelect={(wt, entry) => {
                  setSelectedWorktree(wt)
                  if (entry) {
                    if (!name.trim() && entry.displayName) setName(entry.displayName)
                    if (!description.trim() && entry.description) setDescription(entry.description)
                  }
                }}
                onError={setError}
              />
            </>
          ) : mode === TabMode.Recent ? (
            /* Recent Workspace Tab */
            <>
              <div className="create-child-dialog-field">
                <label htmlFor="workspace-name-recent">Name</label>
                <input
                  id="workspace-name-recent"
                  type="text"
                  value={name}
                  onChange={(e) => { setName(e.target.value); }}
                  placeholder="Workspace name..."
                  disabled={isProcessing}
                />
              </div>
              <RecentWorkspacesLoader
                key={parentWsData.gitRootPath}
                git={git}
                worktreeRegistry={worktreeRegistry}
                openWorktreePaths={openWorktreePaths}
                selectedPath={selectedRecent?.worktree.path ?? null}
                onSelect={(worktree, entry) => {
                  setSelectedRecent({ worktree, entry })
                  setName(entry.displayName ?? worktree.branch)
                  setDescription(entry.description ?? '')
                }}
                onError={setError}
              />
            </>
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
          {mode !== TabMode.Existing && mode !== TabMode.Recent && (
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
          ) : mode === TabMode.Recent ? (
            <button
              className="dialog-btn create"
              onClick={() => { void handleRecentSubmit(); }}
              disabled={isProcessing || !selectedRecent}
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
function ExistingWorktreesLoader({ git, worktreeRegistry, openWorktreePaths, selectedWorktree, onSelect, onError }: {
  git: ReturnType<typeof useGitApi>
  worktreeRegistry: WorktreeRegistryApi
  openWorktreePaths: string[]
  selectedWorktree: WorktreeInfo | null
  onSelect: (wt: WorktreeInfo, entry: WorktreeRegistryEntry | null) => void
  onError: (msg: string) => void
}) {
  const [worktrees, setWorktrees] = useState<WorktreeInfo[]>([])
  const [entriesByPath, setEntriesByPath] = useState<Map<string, WorktreeRegistryEntry>>(new Map())
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    void (async () => {
      try {
        const [wts, registry] = await Promise.all([
          git.listWorktrees(),
          worktreeRegistry.list().catch((err: unknown) => {
            onError(`Failed to load worktree registry: ${err instanceof Error ? err.message : String(err)}`)
            return [] as WorktreeRegistryEntry[]
          }),
        ])
        setWorktrees(wts.filter(wt => !openWorktreePaths.includes(wt.path)))
        setEntriesByPath(new Map(registry.map(e => [e.path, e])))
      } catch {
        setWorktrees([])
      } finally {
        setLoading(false)
      }
    })()
  }, [git, worktreeRegistry, openWorktreePaths, onError])

  return (
    <div className="create-child-existing-list">
      {loading ? (
        <div className="create-child-loading">Loading worktrees...</div>
      ) : worktrees.length === 0 ? (
        <div className="create-child-empty">No available child worktrees found</div>
      ) : (
        worktrees.map(wt => {
          const entry = entriesByPath.get(wt.path) ?? null
          const displayName = entry?.displayName ?? wt.branch
          return (
            <div
              key={wt.path}
              className={`create-child-worktree-item ${selectedWorktree?.path === wt.path ? 'selected' : ''}`}
              onClick={() => { onSelect(wt, entry); }}
            >
              <span className="worktree-name">{displayName}</span>
              <span className="worktree-branch">{wt.branch}</span>
            </div>
          )
        })
      )}
    </div>
  )
}

/** Loads worktrees with registry entries (intersect git worktree list with stored registry) */
function RecentWorkspacesLoader({ git, worktreeRegistry, openWorktreePaths, selectedPath, onSelect, onError }: {
  git: ReturnType<typeof useGitApi>
  worktreeRegistry: WorktreeRegistryApi
  openWorktreePaths: string[]
  selectedPath: string | null
  onSelect: (wt: WorktreeInfo, entry: WorktreeRegistryEntry) => void
  onError: (msg: string) => void
}) {
  const [items, setItems] = useState<{ worktree: WorktreeInfo; entry: WorktreeRegistryEntry }[]>([])
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    void (async () => {
      try {
        const [wts, registry] = await Promise.all([
          git.listWorktrees(),
          worktreeRegistry.list(),
        ])
        const wtByPath = new Map(wts.map(w => [w.path, w]))
        const openSet = new Set(openWorktreePaths)
        const intersected: { worktree: WorktreeInfo; entry: WorktreeRegistryEntry }[] = []
        for (const entry of registry) {
          const worktree = wtByPath.get(entry.path)
          if (!worktree) continue
          if (openSet.has(entry.path)) continue
          intersected.push({ worktree, entry })
        }
        intersected.sort((a, b) => b.entry.lastUsedAt - a.entry.lastUsedAt)
        setItems(intersected)
      } catch (err) {
        onError(`Failed to load recent workspaces: ${err instanceof Error ? err.message : String(err)}`)
        setItems([])
      } finally {
        setLoading(false)
      }
    })()
  }, [git, worktreeRegistry, openWorktreePaths, onError])

  return (
    <div className="create-child-existing-list">
      {loading ? (
        <div className="create-child-loading">Loading recent workspaces...</div>
      ) : items.length === 0 ? (
        <div className="create-child-empty">No recent workspaces found</div>
      ) : (
        items.map(({ worktree, entry }) => {
          const displayName = entry.displayName ?? worktree.branch
          return (
            <div
              key={worktree.path}
              className={`create-child-worktree-item ${selectedPath === worktree.path ? 'selected' : ''}`}
              onClick={() => { onSelect(worktree, entry); }}
            >
              <span className="worktree-name">{displayName}</span>
              <span className="worktree-branch">{worktree.branch}</span>
              {entry.description && <span className="worktree-branch" style={{ opacity: 0.7 }}>{entry.description}</span>}
            </div>
          )
        })
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
