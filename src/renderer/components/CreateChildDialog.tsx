import { useState } from 'react'
import type { Workspace } from '../types'

interface CreateChildDialogProps {
  parentWorkspace: Workspace
  onCreate: (name: string, sandboxed: boolean) => Promise<{ success: boolean; error?: string }>
  onCancel: () => void
}

export default function CreateChildDialog({
  parentWorkspace,
  onCreate,
  onCancel
}: CreateChildDialogProps) {
  const [name, setName] = useState('')
  const [sandboxed, setSandboxed] = useState(false)
  const [isCreating, setIsCreating] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const handleSubmit = async () => {
    if (!name.trim()) {
      setError('Please enter a workspace name')
      return
    }

    setIsCreating(true)
    setError(null)

    const result = await onCreate(name.trim(), sandboxed)
    if (!result.success) {
      setError(result.error || 'Failed to create workspace')
      setIsCreating(false)
    }
    // Dialog will be closed by parent on success
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !isCreating) {
      handleSubmit()
    }
    if (e.key === 'Escape') {
      onCancel()
    }
  }

  return (
    <div className="dialog-overlay" onClick={onCancel}>
      <div className="create-child-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="create-child-dialog-header">
          <h2>New Child Workspace</h2>
          <button className="dialog-close" onClick={onCancel}>
            x
          </button>
        </div>

        <div className="create-child-dialog-content">
          <div className="create-child-dialog-info">
            <span className="create-child-label">Parent:</span>
            <span className="create-child-value">{parentWorkspace.name}</span>
          </div>

          <div className="create-child-dialog-field">
            <label htmlFor="workspace-name">Name</label>
            <input
              id="workspace-name"
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Workspace name..."
              autoFocus
              disabled={isCreating}
            />
          </div>

          <div className="create-child-dialog-checkbox">
            <label>
              <input
                type="checkbox"
                checked={sandboxed}
                onChange={(e) => setSandboxed(e.target.checked)}
                disabled={isCreating}
              />
              Enable Sandbox
            </label>
          </div>

          {error && <div className="create-child-error">{error}</div>}
        </div>

        <div className="create-child-dialog-actions">
          <button className="dialog-btn cancel" onClick={onCancel} disabled={isCreating}>
            Cancel
          </button>
          <button className="dialog-btn create" onClick={handleSubmit} disabled={isCreating}>
            {isCreating ? 'Creating...' : 'Create'}
          </button>
        </div>
      </div>
    </div>
  )
}
