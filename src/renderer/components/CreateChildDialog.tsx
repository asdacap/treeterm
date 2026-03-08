import { useState, useEffect, useMemo } from 'react'
import type { Workspace, ChildWorktreeInfo } from '../types'

interface CreateChildDialogProps {
  parentWorkspace: Workspace
  onCreate: (name: string) => Promise<{ success: boolean; error?: string }>
  onAdopt: (worktreePath: string, branch: string, name: string) => Promise<{ success: boolean; error?: string }>
  onCancel: () => void
  openWorktreePaths: string[]
}

type TabMode = 'create' | 'existing'

export default function CreateChildDialog({
  parentWorkspace,
  onCreate,
  onAdopt,
  onCancel,
  openWorktreePaths
}: CreateChildDialogProps) {
  const [mode, setMode] = useState<TabMode>('create')
  const [name, setName] = useState('')
  const [isProcessing, setIsProcessing] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // For existing worktrees tab
  const [existingWorktrees, setExistingWorktrees] = useState<ChildWorktreeInfo[]>([])
  const [isLoadingWorktrees, setIsLoadingWorktrees] = useState(false)
  const [selectedWorktree, setSelectedWorktree] = useState<ChildWorktreeInfo | null>(null)

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

    const result = await onCreate(name.trim())
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

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !isProcessing) {
      if (mode === 'create') {
        handleCreateSubmit()
      } else if (selectedWorktree) {
        handleAdoptSubmit()
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
        </div>

        <div className="create-child-dialog-content">
          <div className="create-child-dialog-info">
            <span className="create-child-label">Parent:</span>
            <span className="create-child-value">{parentWorkspace.name}</span>
          </div>

          {mode === 'create' ? (
            /* Create New Tab */
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
          ) : (
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
          ) : (
            <button
              className="dialog-btn create"
              onClick={handleAdoptSubmit}
              disabled={isProcessing || !selectedWorktree}
            >
              {isProcessing ? 'Opening...' : 'Open'}
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
