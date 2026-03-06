import { useState } from 'react'
import DiffView from './DiffView'
import type { Workspace } from '../types'

interface MergeDialogProps {
  workspace: Workspace
  parentWorkspace: Workspace
  onMerge: (squash: boolean) => Promise<void>
  onAbandon: () => Promise<void>
  onCancel: () => void
}

export default function MergeDialog({
  workspace,
  parentWorkspace,
  onMerge,
  onAbandon,
  onCancel
}: MergeDialogProps) {
  const [isProcessing, setIsProcessing] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const handleMerge = async (squash: boolean) => {
    setIsProcessing(true)
    setError(null)
    try {
      await onMerge(squash)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Merge failed')
      setIsProcessing(false)
    }
  }

  const handleAbandon = async () => {
    setIsProcessing(true)
    setError(null)
    try {
      await onAbandon()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to abandon')
      setIsProcessing(false)
    }
  }

  return (
    <div className="dialog-overlay" onClick={onCancel}>
      <div className="merge-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="merge-dialog-header">
          <h2>Close Workspace: {workspace.name}</h2>
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

        <div className="merge-dialog-diff">
          <DiffView
            worktreePath={workspace.path}
            parentBranch={parentWorkspace.gitBranch || 'main'}
          />
        </div>

        {error && <div className="merge-error">{error}</div>}

        <div className="merge-dialog-actions">
          <button
            className="merge-btn merge"
            onClick={() => handleMerge(false)}
            disabled={isProcessing}
          >
            {isProcessing ? 'Processing...' : 'Merge'}
          </button>
          <button
            className="merge-btn squash"
            onClick={() => handleMerge(true)}
            disabled={isProcessing}
          >
            Squash Merge
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
        </div>
      </div>
    </div>
  )
}
