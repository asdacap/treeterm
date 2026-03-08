import type { Workspace } from '../types'

interface CloseConfirmDialogProps {
  unmergedWorkspaces: Workspace[]
  onConfirm: () => void
  onCancel: () => void
}

export default function CloseConfirmDialog({
  unmergedWorkspaces,
  onConfirm,
  onCancel
}: CloseConfirmDialogProps) {
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      onCancel()
    }
  }

  return (
    <div className="dialog-overlay" onClick={onCancel} onKeyDown={handleKeyDown}>
      <div className="close-confirm-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="close-confirm-dialog-header">
          <h2>Unmerged Workspaces</h2>
          <button className="dialog-close" onClick={onCancel}>
            x
          </button>
        </div>

        <div className="close-confirm-dialog-content">
          <p className="close-confirm-warning">
            The following sub-workspaces have not been merged:
          </p>

          <ul className="close-confirm-list">
            {unmergedWorkspaces.map((ws) => (
              <li key={ws.id} className="close-confirm-item">
                <span className="close-confirm-name">{ws.name}</span>
                {ws.gitBranch && (
                  <span className="close-confirm-branch">({ws.gitBranch})</span>
                )}
              </li>
            ))}
          </ul>

          <p className="close-confirm-note">
            Closing the application will not delete these workspaces, but their work remains
            unmerged.
          </p>
        </div>

        <div className="close-confirm-dialog-actions">
          <button className="dialog-btn cancel" onClick={onCancel}>
            Cancel
          </button>
          <button className="dialog-btn close-anyway" onClick={onConfirm}>
            Close Anyway
          </button>
        </div>
      </div>
    </div>
  )
}
