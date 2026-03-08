import { useState, useEffect, useCallback } from 'react'
import type { FileEntry } from '../types'

interface FileTreeProps {
  workspacePath: string
  selectedPath: string | null
  expandedDirs: string[]
  onSelectFile: (path: string) => void
  onToggleDir: (path: string) => void
}

interface DirectoryState {
  entries: FileEntry[]
  loading: boolean
  error: string | null
}

export function FileTree({
  workspacePath,
  selectedPath,
  expandedDirs,
  onSelectFile,
  onToggleDir
}: FileTreeProps): JSX.Element {
  const [dirContents, setDirContents] = useState<Record<string, DirectoryState>>({})

  const loadDirectory = useCallback(
    async (dirPath: string) => {
      // Don't reload if already loaded
      if (dirContents[dirPath]?.entries.length > 0) return

      setDirContents((prev) => ({
        ...prev,
        [dirPath]: { entries: [], loading: true, error: null }
      }))

      try {
        const result = await window.electron.filesystem.readDirectory(workspacePath, dirPath)

        if (result.success && result.contents) {
          setDirContents((prev) => ({
            ...prev,
            [dirPath]: { entries: result.contents!.entries, loading: false, error: null }
          }))
        } else {
          setDirContents((prev) => ({
            ...prev,
            [dirPath]: { entries: [], loading: false, error: result.error || 'Failed to load' }
          }))
        }
      } catch (err) {
        setDirContents((prev) => ({
          ...prev,
          [dirPath]: { entries: [], loading: false, error: `Error: ${err}` }
        }))
      }
    },
    [workspacePath, dirContents]
  )

  // Load root directory on mount
  useEffect(() => {
    loadDirectory(workspacePath)
  }, [workspacePath])

  // Load expanded directories
  useEffect(() => {
    for (const dirPath of expandedDirs) {
      if (!dirContents[dirPath]) {
        loadDirectory(dirPath)
      }
    }
  }, [expandedDirs, loadDirectory])

  const handleToggleDir = (dirPath: string) => {
    onToggleDir(dirPath)
    if (!expandedDirs.includes(dirPath)) {
      loadDirectory(dirPath)
    }
  }

  const renderEntry = (entry: FileEntry, depth: number): JSX.Element => {
    const isExpanded = expandedDirs.includes(entry.path)
    const isSelected = selectedPath === entry.path
    const dirState = dirContents[entry.path]

    return (
      <div key={entry.path}>
        <div
          className={`file-tree-item ${isSelected ? 'selected' : ''}`}
          style={{ paddingLeft: `${depth * 16 + 8}px` }}
          onClick={() => {
            if (entry.isDirectory) {
              handleToggleDir(entry.path)
            } else {
              onSelectFile(entry.path)
            }
          }}
        >
          <span className="file-tree-expand">
            {entry.isDirectory ? (isExpanded ? '\u25BC' : '\u25B6') : ''}
          </span>
          <span className="file-tree-icon">{entry.isDirectory ? '\uD83D\uDCC1' : '\uD83D\uDCC4'}</span>
          <span className="file-tree-name">{entry.name}</span>
        </div>
        {entry.isDirectory && isExpanded && (
          <div className="file-tree-children">
            {dirState?.loading && (
              <div className="file-tree-item" style={{ paddingLeft: `${(depth + 1) * 16 + 8}px` }}>
                <span className="file-tree-loading">Loading...</span>
              </div>
            )}
            {dirState?.error && (
              <div
                className="file-tree-item file-tree-error"
                style={{ paddingLeft: `${(depth + 1) * 16 + 8}px` }}
              >
                <span>{dirState.error}</span>
              </div>
            )}
            {dirState?.entries.map((child) => renderEntry(child, depth + 1))}
          </div>
        )}
      </div>
    )
  }

  const rootState = dirContents[workspacePath]

  return (
    <div className="file-tree">
      {rootState?.loading && (
        <div className="file-tree-item">
          <span className="file-tree-loading">Loading...</span>
        </div>
      )}
      {rootState?.error && (
        <div className="file-tree-item file-tree-error">
          <span>{rootState.error}</span>
        </div>
      )}
      {rootState?.entries.map((entry) => renderEntry(entry, 0))}
    </div>
  )
}
