import { useState, useEffect } from 'react'
import DiffView from './DiffView'
import type { Workspace, ConflictInfo, UncommittedChanges } from '../types'

interface MergeDialogProps {
  workspace: Workspace
  parentWorkspace: Workspace
  onMerge: (squash: boolean) => Promise<void>
  onAbandon: () => Promise<void>
  onCloseAndClean: () => Promise<void>
  onCancel: () => void
}

export default function MergeDialog({
  workspace,
  parentWorkspace,
  onMerge,
  onAbandon,
  onCloseAndClean,
  onCancel
}: MergeDialogProps): JSX.Element {
  const [isProcessing, setIsProcessing] = useState(false)
  const [isCheckingConflicts, setIsCheckingConflicts] = useState(true)
  const [conflictInfo, setConflictInfo] = useState<ConflictInfo | null>(null)
  const [conflictError, setConflictError] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [uncommittedChanges, setUncommittedChanges] = useState<UncommittedChanges | null>(null)
  const [isCheckingUncommitted, setIsCheckingUncommitted] = useState(true)
  const [checkError, setCheckError] = useState<string | null>(null)

  // Check for conflicts when dialog opens
  useEffect(() => {
    async function checkConflicts() {
      if (!parentWorkspace.gitRootPath || !workspace.gitBranch || !parentWorkspace.gitBranch) {
        setIsCheckingConflicts(false)
        return
      }

      try {
        const result = await window.electron.git.checkMergeConflicts(
          parentWorkspace.gitRootPath,
          workspace.gitBranch,
          parentWorkspace.gitBranch
        )

        if (result.success && result.conflicts) {
          setConflictInfo(result.conflicts)
        } else if (!result.success) {
          setConflictError(result.error || 'Failed to check for conflicts')
        }
      } catch (err) {
        setConflictError(err instanceof Error ? err.message : 'Failed to check for conflicts')
      } finally {
        setIsCheckingConflicts(false)
      }
    }

    checkConflicts()
  }, [workspace, parentWorkspace])

  // Check for uncommitted changes when dialog opens
  useEffect(() => {
    async function checkUncommitted() {
      try {
        const result = await window.electron.git.getUncommittedChanges(workspace.path)
        if (result.success && result.changes) {
          setUncommittedChanges(result.changes)
        } else if (!result.success) {
          setCheckError(result.error || 'Failed to check for uncommitted changes')
        }
      } catch (err) {
        setCheckError(err instanceof Error ? err.message : 'Failed to check for uncommitted changes')
      } finally {
        setIsCheckingUncommitted(false)
      }
    }

    checkUncommitted()
  }, [workspace.path])

  const hasConflicts = conflictInfo?.hasConflicts ?? false
  const hasUncommitted = uncommittedChanges && uncommittedChanges.files.length > 0

  const handleMerge = async (squash: boolean): Promise<void> => {
    setIsProcessing(true)
    setError(null)
    try {
      await onMerge(squash)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Merge failed')
      setIsProcessing(false)
    }
  }

  const handleAbandon = async (): Promise<void> => {
    setIsProcessing(true)
    setError(null)
    try {
      await onAbandon()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to abandon')
      setIsProcessing(false)
    }
  }

  const handleCloseAndClean = async (): Promise<void> => {
    setIsProcessing(true)
    setError(null)
    try {
      await onCloseAndClean()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to close and clean')
      setIsProcessing(false)
    }
  }

  const isDetached = workspace.isDetached ?? false

  return (
    <div className="dialog-overlay" onClick={onCancel}>
      <div className="merge-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="merge-dialog-header">
          <h2>Close Workspace: {workspace.name}{isDetached && ' (Detached)'}</h2>
          <button className="dialog-close" onClick={onCancel}>
            x
          </button>
        </div>

        <div className="merge-dialog-info">
          <div className="merge-info-row">
            <span className="merge-label">Branch:</span>
            <span className="merge-value">{workspace.gitBranch}</span>
          </div>
          <div className="merge-info-row">
            <span className="merge-label">Merge into:</span>
            <span className="merge-value">{parentWorkspace.gitBranch}</span>
          </div>
        </div>

        {/* Conflict Status Section */}
        <div className="merge-conflict-status">
          {isCheckingConflicts ? (
            <span className="merge-conflict-loading">Checking for conflicts...</span>
          ) : conflictError ? (
            <div className="merge-conflict-error">
              <span>Could not check for conflicts: {conflictError}</span>
            </div>
          ) : hasConflicts ? (
            <div className="merge-conflict-warning">
              <div className="merge-conflict-header">
                <span className="merge-conflict-icon">⚠</span>
                <span className="merge-conflict-title">Merge conflicts detected</span>
              </div>
              {conflictInfo && conflictInfo.conflictedFiles.length > 0 && (
                <div className="merge-conflict-files">
                  <span className="merge-conflict-files-label">Conflicted files:</span>
                  <ul className="merge-conflict-files-list">
                    {conflictInfo.conflictedFiles.map((file, idx) => (
                      <li key={idx} className="merge-conflict-file">{file}</li>
                    ))}
                  </ul>
                </div>
              )}
              <div className="merge-conflict-help">
                Merging will leave conflict markers in the parent workspace.
                You will need to resolve them manually.
              </div>
            </div>
          ) : (
            <div className="merge-conflict-clean">
              <span className="merge-conflict-icon">✓</span>
              <span>No conflicts detected - merge should be clean</span>
            </div>
          )}
        </div>

        {/* Uncommitted Changes Check Error */}
        {!isCheckingUncommitted && checkError && (
          <div className="merge-uncommitted-error">
            <span className="merge-conflict-icon">⚠</span>
            <span>Could not check for uncommitted changes: {checkError}</span>
          </div>
        )}

        {/* Uncommitted Changes Warning */}
        {!isCheckingUncommitted && hasUncommitted && (
          <div className="merge-uncommitted-warning">
            <div className="merge-conflict-header">
              <span className="merge-conflict-icon">⚠</span>
              <span className="merge-conflict-title">Uncommitted changes detected</span>
            </div>
            <div className="merge-conflict-files">
              <span className="merge-conflict-files-label">
                {uncommittedChanges.files.length} file{uncommittedChanges.files.length !== 1 ? 's' : ''} with uncommitted changes:
              </span>
              <ul className="merge-conflict-files-list">
                {uncommittedChanges.files.slice(0, 5).map((file, idx) => (
                  <li key={idx} className="merge-conflict-file">{file.path}</li>
                ))}
                {uncommittedChanges.files.length > 5 && (
                  <li className="merge-conflict-file">...and {uncommittedChanges.files.length - 5} more</li>
                )}
              </ul>
            </div>
            <div className="merge-conflict-help">
              <strong>Merge/Squash:</strong> Changes will be auto-committed with a "WIP" message before merging.
              <br />
              <strong>Abandon:</strong> All uncommitted changes will be permanently lost.
            </div>
          </div>
        )}

        <div className="merge-dialog-diff">
          <DiffView
            worktreePath={workspace.path}
            parentBranch={parentWorkspace.gitBranch || 'main'}
          />
        </div>

        {error && <div className="merge-error">{error}</div>}

        <div className="merge-dialog-actions">
          {isDetached ? (
            <>
              <button
                className="merge-btn close-and-clean"
                onClick={handleCloseAndClean}
                disabled={isProcessing}
              >
                {isProcessing ? 'Processing...' : 'Close and Clean'}
              </button>
              <button
                className="merge-btn abandon"
                onClick={handleAbandon}
                disabled={isProcessing}
              >
                Abandon
              </button>
              <button className="merge-btn cancel" onClick={onCancel} disabled={isProcessing}>
                Cancel
              </button>
            </>
          ) : (
            <>
              <button
                className={`merge-btn merge ${hasConflicts ? 'merge-btn-warning' : ''}`}
                onClick={() => handleMerge(false)}
                disabled={isProcessing || isCheckingConflicts}
                title={hasConflicts ? 'Warning: This merge has conflicts' : undefined}
              >
                {isProcessing ? 'Processing...' : hasConflicts ? 'Merge (has conflicts)' : 'Merge'}
              </button>
              <button
                className={`merge-btn squash ${hasConflicts ? 'merge-btn-warning' : ''}`}
                onClick={() => handleMerge(true)}
                disabled={isProcessing || isCheckingConflicts}
                title={hasConflicts ? 'Warning: This merge has conflicts' : undefined}
              >
                {hasConflicts ? 'Squash (has conflicts)' : 'Squash Merge'}
              </button>
              <button
                className="merge-btn abandon"
                onClick={handleAbandon}
                disabled={isProcessing}
              >
                Abandon
              </button>
              <button className="merge-btn cancel" onClick={onCancel} disabled={isProcessing}>
                Cancel
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  )
}
