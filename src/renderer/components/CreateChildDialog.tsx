import { useState, useEffect, useMemo } from 'react'
import type { Workspace, ChildWorktreeInfo, BranchInfo } from '../types'

interface CreateChildDialogProps {
  parentWorkspace: Workspace
  onCreate: (name: string, isDetached: boolean) => Promise<{ success: boolean; error?: string }>
  onAdopt: (worktreePath: string, branch: string, name: string) => Promise<{ success: boolean; error?: string }>
  onCreateFromBranch: (branch: string, isDetached: boolean) => Promise<{ success: boolean; error?: string }>
  onCreateFromRemote: (remoteBranch: string, isDetached: boolean) => Promise<{ success: boolean; error?: string }>
  onCancel: () => void
  openWorktreePaths: string[]
}

type TabMode = 'create' | 'existing' | 'branch' | 'remote'

export default function CreateChildDialog({
  parentWorkspace,
  onCreate,
  onAdopt,
  onCreateFromBranch,
  onCreateFromRemote,
  onCancel,
  openWorktreePaths
}: CreateChildDialogProps) {
  const [mode, setMode] = useState<TabMode>('create')
  const [name, setName] = useState('')
  const [isProcessing, setIsProcessing] = useState(false)
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

  // Load existing worktrees when "existing" tab is selected
  useEffect(() => {
    if (mode === 'existing' && parentWorkspace.gitRootPath) {
      console.log('[CreateChildDialog] Loading worktrees for:', {
        gitRootPath: parentWorkspace.gitRootPath,
        gitBranch: parentWorkspace.gitBranch,
        openWorktreePaths
      })
      setIsLoadingWorktrees(true)
      window.electron.git.getChildWorktrees(
        parentWorkspace.gitRootPath,
        parentWorkspace.gitBranch
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
        hasGitRootPath: !!parentWorkspace.gitRootPath
      })
    }
  }, [mode, parentWorkspace.gitRootPath, parentWorkspace.gitBranch, openWorktreePaths])

  // Load local branches when "branch" tab is selected
  useEffect(() => {
    if (mode === 'branch' && parentWorkspace.gitRootPath) {
      setIsLoadingBranches(true)
      Promise.all([
        window.electron.git.listLocalBranches(parentWorkspace.gitRootPath),
        window.electron.git.getBranchesInWorktrees(parentWorkspace.gitRootPath)
      ]).then(([allBranches, branchesInWorktrees]) => {
        const branchInfos: BranchInfo[] = allBranches.map(name => ({
          name,
          isInWorktree: branchesInWorktrees.includes(name)
        }))
        setBranches(branchInfos)
        setIsLoadingBranches(false)
      }).catch(() => {
        setBranches([])
        setIsLoadingBranches(false)
      })
    }
  }, [mode, parentWorkspace.gitRootPath])

  // Load remote branches when "remote" tab is selected
  useEffect(() => {
    if (mode === 'remote' && parentWorkspace.gitRootPath) {
      setIsLoadingRemoteBranches(true)
      Promise.all([
        window.electron.git.listRemoteBranches(parentWorkspace.gitRootPath),
        window.electron.git.getBranchesInWorktrees(parentWorkspace.gitRootPath),
        window.electron.git.listLocalBranches(parentWorkspace.gitRootPath)
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
      }).catch(() => {
        setRemoteBranches([])
        setIsLoadingRemoteBranches(false)
      })
    }
  }, [mode, parentWorkspace.gitRootPath])

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

  const handleCreateSubmit = async () => {
    if (!name.trim()) {
      setError('Please enter a workspace name')
      return
    }

    if (nameValidationError) {
      setError(nameValidationError)
      return
    }

    setIsProcessing(true)
    setError(null)

    const result = await onCreate(name.trim(), isDetached)
    if (!result.success) {
      setError(result.error || 'Failed to create workspace')
      setIsProcessing(false)
    }
  }

  const handleAdoptSubmit = async () => {
    if (!selectedWorktree) {
      setError('Please select a worktree')
      return
    }

    setIsProcessing(true)
    setError(null)

    const result = await onAdopt(
      selectedWorktree.path,
      selectedWorktree.branch,
      selectedWorktree.displayName
    )
    if (!result.success) {
      setError(result.error || 'Failed to open worktree')
      setIsProcessing(false)
    }
  }

  const handleBranchSubmit = async () => {
    if (!selectedBranch) {
      setError('Please select a branch')
      return
    }

    setIsProcessing(true)
    setError(null)

    const result = await onCreateFromBranch(selectedBranch.name, isDetached)
    if (!result.success) {
      setError(result.error || 'Failed to create worktree from branch')
      setIsProcessing(false)
    }
  }

  const handleRemoteSubmit = async () => {
    if (!selectedRemoteBranch) {
      setError('Please select a remote branch')
      return
    }

    setIsProcessing(true)
    setError(null)

    const result = await onCreateFromRemote(selectedRemoteBranch.name, isDetached)
    if (!result.success) {
      setError(result.error || 'Failed to create worktree from remote branch')
      setIsProcessing(false)
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
            <span className="create-child-value">{parentWorkspace.name}</span>
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
            </>
          )}

          {error && <div className="create-child-error">{error}</div>}
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
              {isProcessing ? 'Creating...' : 'Create'}
            </button>
          ) : mode === 'existing' ? (
            <button
              className="dialog-btn create"
              onClick={handleAdoptSubmit}
              disabled={isProcessing || !selectedWorktree}
            >
              {isProcessing ? 'Opening...' : 'Open'}
            </button>
          ) : mode === 'branch' ? (
            <button
              className="dialog-btn create"
              onClick={handleBranchSubmit}
              disabled={isProcessing || !selectedBranch}
            >
              {isProcessing ? 'Opening...' : 'Open'}
            </button>
          ) : (
            <button
              className="dialog-btn create"
              onClick={handleRemoteSubmit}
              disabled={isProcessing || !selectedRemoteBranch}
            >
              {isProcessing ? 'Opening...' : 'Open'}
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
