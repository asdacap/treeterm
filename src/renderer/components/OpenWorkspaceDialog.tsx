import { useState, useMemo, useEffect } from 'react'
import { applicationRegistry } from '../registry/applicationRegistry'
import { useElectron } from '../store/ElectronContext'
import type { WorktreeSettings } from '../types'

interface OpenWorkspaceDialogProps {
  onOpen: (path: string, settings?: WorktreeSettings) => void
  onCancel: () => void
}

export default function OpenWorkspaceDialog({ onOpen, onCancel }: OpenWorkspaceDialogProps) {
  const { getRecentDirectories, selectFolder } = useElectron()
  const [selectedPath, setSelectedPath] = useState<string>('')
  const [isSelecting, setIsSelecting] = useState(false)
  const [selectedAppId, setSelectedAppId] = useState<string>('')
  const [recentDirectories, setRecentDirectories] = useState<string[]>([])
  const [isLoadingRecent, setIsLoadingRecent] = useState(false)

  // Fetch recent directories on mount
  useEffect(() => {
    const loadRecentDirectories = async () => {
      setIsLoadingRecent(true)
      try {
        const recent = await getRecentDirectories()
        setRecentDirectories(recent)
      } catch (error) {
        console.error('Failed to load recent directories:', error)
      } finally {
        setIsLoadingRecent(false)
      }
    }
    loadRecentDirectories()
  }, [])

  const availableApps = useMemo(() => {
    return applicationRegistry.getAll().filter(app => app.showInNewTabMenu)
  }, [])

  const handleSelectFolder = async () => {
    setIsSelecting(true)
    try {
      const path = await selectFolder()
      if (path) {
        setSelectedPath(path)
      }
    } finally {
      setIsSelecting(false)
    }
  }

  const handleOpen = () => {
    if (!selectedPath) return
    
    const settings: WorktreeSettings | undefined = selectedAppId
      ? { defaultApplicationId: selectedAppId }
      : undefined
    
    onOpen(selectedPath, settings)
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && selectedPath && !isSelecting) {
      handleOpen()
    }
    if (e.key === 'Escape') {
      onCancel()
    }
  }

  const handleSelectRecent = (path: string) => {
    setSelectedPath(path)
  }

  const getFolderName = (path: string) => {
    const parts = path.split(/[/\\]/)
    return parts[parts.length - 1] || path
  }

  return (
    <div className="dialog-overlay" onClick={onCancel}>
      <div className="open-workspace-dialog" onClick={(e) => e.stopPropagation()} onKeyDown={handleKeyDown}>
        <div className="open-workspace-dialog-header">
          <h2>Open Workspace</h2>
          <button className="dialog-close" onClick={onCancel}>
            &times;
          </button>
        </div>

        <div className="open-workspace-dialog-content">
          {recentDirectories.length > 0 && (
            <div className="open-workspace-field">
              <label>Recent Directories</label>
              <div className="recent-directories-list">
                {recentDirectories.map((dir) => (
                  <button
                    key={dir}
                    className="recent-directory-item"
                    onClick={() => handleSelectRecent(dir)}
                    title={dir}
                  >
                    <span className="recent-directory-name">{getFolderName(dir)}</span>
                    <span className="recent-directory-path">{dir}</span>
                  </button>
                ))}
              </div>
            </div>
          )}

          <div className="open-workspace-field">
            <label>Folder</label>
            <div className="folder-picker-row">
              <input
                type="text"
                value={selectedPath}
                readOnly
                placeholder="Select a folder..."
                className="folder-path-input"
              />
              <button
                className="dialog-btn browse"
                onClick={handleSelectFolder}
                disabled={isSelecting}
              >
                {isSelecting ? 'Opening...' : 'Browse...'}
              </button>
            </div>
          </div>

          <div className="open-workspace-field">
            <label>Default Application</label>
            <select
              className="settings-select"
              value={selectedAppId}
              onChange={(e) => setSelectedAppId(e.target.value)}
            >
              <option value="">Use Global Default</option>
              <option disabled>──────────</option>
              {availableApps.map((app) => (
                <option key={app.id} value={app.id}>
                  {app.name}
                </option>
              ))}
            </select>
            <p className="settings-hint">
              The application to open by default in this workspace. 
              Child worktrees will inherit this setting.
            </p>
          </div>
        </div>

        <div className="open-workspace-dialog-actions">
          <button className="dialog-btn cancel" onClick={onCancel}>
            Cancel
          </button>
          <button 
            className="dialog-btn create" 
            onClick={handleOpen}
            disabled={!selectedPath || isSelecting}
          >
            Open
          </button>
        </div>
      </div>
    </div>
  )
}
