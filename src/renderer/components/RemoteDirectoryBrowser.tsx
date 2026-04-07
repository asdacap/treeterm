import { useState, useEffect } from 'react'
import type { IpcResult, DirectoryContents } from '../types'

interface RemoteDirectoryBrowserProps {
  readDirectory: (dirPath: string) => Promise<IpcResult<{ contents: DirectoryContents }>>
  initialPath: string
  onSelect: (path: string) => void
  onCancel: () => void
}

interface DirState {
  entries: { name: string; path: string }[]
  loading: boolean
  error: string | null
}

export default function RemoteDirectoryBrowser({
  readDirectory,
  initialPath,
  onSelect,
  onCancel,
}: RemoteDirectoryBrowserProps): JSX.Element {
  const [currentPath, setCurrentPath] = useState(initialPath)
  const [pathInput, setPathInput] = useState(initialPath)
  const [dirState, setDirState] = useState<DirState>({ entries: [], loading: true, error: null })

  // Set loading when path changes during render
  const [prevCurrentPath, setPrevCurrentPath] = useState(currentPath)
  if (currentPath !== prevCurrentPath) {
    setPrevCurrentPath(currentPath)
    setDirState({ entries: [], loading: true, error: null })
  }

  useEffect(() => {
    void readDirectory(currentPath).then(result => {
      if (result.success) {
        const dirs = result.contents.entries
          .filter((e) => e.isDirectory)
          .map((e) => ({ name: e.name, path: e.path }))
        setDirState({ entries: dirs, loading: false, error: null })
      } else {
        setDirState({ entries: [], loading: false, error: result.error })
      }
    }).catch((err: unknown) => {
      setDirState({ entries: [], loading: false, error: String(err) })
    })
  }, [currentPath, readDirectory])

  const navigateTo = (path: string) => {
    setCurrentPath(path)
    setPathInput(path)
  }

  const handleGoUp = () => {
    const parent = currentPath === '/' ? '/' : currentPath.replace(/\/[^/]+\/?$/, '') || '/'
    navigateTo(parent)
  }

  const handlePathSubmit = () => {
    const trimmed = pathInput.trim()
    if (trimmed && trimmed.startsWith('/')) {
      navigateTo(trimmed)
    }
  }

  const handlePathKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      handlePathSubmit()
    }
    if (e.key === 'Escape') {
      onCancel()
    }
  }

  // Build breadcrumb segments from current path
  const segments = currentPath === '/' ? ['/'] : ['/', ...currentPath.split('/').filter(Boolean)]

  return (
    <div className="remote-dir-browser">
      {/* Path input */}
      <div className="remote-dir-path-row">
        <button className="remote-dir-up-btn" onClick={handleGoUp} disabled={currentPath === '/'} title="Go up">
          ..
        </button>
        <input
          className="remote-dir-path-input"
          value={pathInput}
          onChange={(e) => { setPathInput(e.target.value); }}
          onKeyDown={handlePathKeyDown}
          onBlur={handlePathSubmit}
          placeholder="/path/to/directory"
        />
      </div>

      {/* Breadcrumbs */}
      <div className="remote-dir-breadcrumb">
        {segments.map((seg, i) => {
          const path = i === 0 ? '/' : '/' + segments.slice(1, i + 1).join('/')
          const isLast = i === segments.length - 1
          return (
            <span key={path}>
              <button
                className={`remote-dir-breadcrumb-seg ${isLast ? 'active' : ''}`}
                onClick={() => !isLast && navigateTo(path)}
                disabled={isLast}
              >
                {seg === '/' ? '/' : seg}
              </button>
              {!isLast && <span className="remote-dir-breadcrumb-sep">/</span>}
            </span>
          )
        })}
      </div>

      {/* Directory listing */}
      <div className="remote-dir-list">
        {dirState.loading && <div className="remote-dir-status">Loading...</div>}
        {dirState.error && <div className="remote-dir-status remote-dir-error">{dirState.error}</div>}
        {!dirState.loading && !dirState.error && dirState.entries.length === 0 && (
          <div className="remote-dir-status">No subdirectories</div>
        )}
        {dirState.entries.map((entry) => (
          <button
            key={entry.path}
            className="remote-dir-entry"
            onClick={() => { navigateTo(entry.path); }}
            title={entry.path}
          >
            <span className="remote-dir-entry-name">{entry.name}</span>
          </button>
        ))}
      </div>

      {/* Actions */}
      <div className="remote-dir-actions">
        <button className="dialog-btn cancel" onClick={onCancel}>
          Cancel
        </button>
        <button className="dialog-btn create" onClick={() => { onSelect(currentPath); }}>
          Select
        </button>
      </div>
    </div>
  )
}
