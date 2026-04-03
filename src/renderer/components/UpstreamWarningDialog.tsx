interface UpstreamWarningDialogProps {
  behindCount: number
  workspaceName: string
  onConfirm: () => void
  onCancel: () => void
}

export default function UpstreamWarningDialog({
  behindCount,
  workspaceName,
  onConfirm,
  onCancel
}: UpstreamWarningDialogProps) {
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      onCancel()
    }
  }

  return (
    <div className="dialog-overlay" onClick={onCancel} onKeyDown={handleKeyDown}>
      <div className="upstream-warning-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="upstream-warning-dialog-header">
          <h2>Upstream Updates Available</h2>
          <button className="dialog-close" onClick={onCancel}>
            x
          </button>
        </div>

        <div className="upstream-warning-dialog-content">
          <p className="upstream-warning-text">
            <strong>{workspaceName}</strong> is {behindCount} commit{behindCount > 1 ? 's' : ''} behind upstream.
            Forking now means the new worktree will be based on an outdated branch.
          </p>
          <p className="upstream-warning-note">
            Consider pulling the latest changes before forking to avoid potential merge conflicts.
          </p>
        </div>

        <div className="upstream-warning-dialog-actions">
          <button className="dialog-btn cancel" onClick={onCancel}>
            Cancel
          </button>
          <button className="dialog-btn fork-anyway" onClick={onConfirm}>
            Fork Anyway
          </button>
        </div>
      </div>
    </div>
  )
}
