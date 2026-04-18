/* eslint-disable custom/no-string-literal-comparison -- TODO: migrate existing string-literal comparisons to enums */
import { useState } from 'react'
import { useAppStore } from '../store/app'
import { useRecentDirectoriesStore } from '../store/recentDirectories'
import RemoteDirectoryBrowser from './RemoteDirectoryBrowser'
import type { WorktreeSettings, IpcResult, DirectoryContents } from '../types'

const EMPTY_RECENT: string[] = []

interface OpenWorkspaceDialogProps {
  onOpen: (path: string, settings?: WorktreeSettings) => void
  onCancel: () => void
  selectFolder: () => Promise<string | null>
  connectionKey: string
  isRemote: boolean
  readDirectory?: (dirPath: string) => Promise<IpcResult<{ contents: DirectoryContents }>>
}

export default function OpenWorkspaceDialog({ onOpen, onCancel, selectFolder, connectionKey, isRemote, readDirectory }: OpenWorkspaceDialogProps) {
  const [selectedPath, setSelectedPath] = useState('')
  const [isSelecting, setIsSelecting] = useState(false)
  const [selectedAppId, setSelectedAppId] = useState('')
  const [showBrowser, setShowBrowser] = useState(false)

  const recentDirectories = useRecentDirectoriesStore(s => s.directories.get(connectionKey)) ?? EMPTY_RECENT
  const addRecent = useRecentDirectoriesStore(s => s.addRecent)

  const applications = useAppStore((s) => s.applications)
  const availableApps = Array.from(applications.values()).filter(app => app.showInNewTabMenu)

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

    addRecent(connectionKey, selectedPath)

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
      <div className="open-workspace-dialog" onClick={(e) => { e.stopPropagation(); }} onKeyDown={handleKeyDown}>
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
                    onClick={() => { handleSelectRecent(dir); }}
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
                onChange={(e) => { setSelectedPath(e.target.value); }}
                placeholder={isRemote ? "Enter remote path (e.g. /home/user/project)" : "Enter or browse for a folder..."}
                className="folder-path-input"
                autoFocus
              />
              {!isRemote && (
                <button
                  className="dialog-btn browse"
                  onClick={() => { void handleSelectFolder(); }}
                  disabled={isSelecting}
                >
                  {isSelecting ? 'Opening...' : 'Browse...'}
                </button>
              )}
              {isRemote && readDirectory && (
                <button
                  className="dialog-btn browse"
                  onClick={() => { setShowBrowser(!showBrowser); }}
                >
                  {showBrowser ? 'Hide' : 'Browse...'}
                </button>
              )}
            </div>
          </div>

          {showBrowser && readDirectory && (
            <RemoteDirectoryBrowser
              readDirectory={readDirectory}
              initialPath={selectedPath || '/'}
              onSelect={(path) => {
                setSelectedPath(path)
                setShowBrowser(false)
              }}
              onCancel={() => { setShowBrowser(false); }}
            />
          )}

          <div className="open-workspace-field">
            <label>Default Application</label>
            <select
              className="settings-select"
              value={selectedAppId}
              onChange={(e) => { setSelectedAppId(e.target.value); }}
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
