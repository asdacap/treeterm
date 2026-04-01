import { useState, useEffect, useCallback } from 'react'
import { useStore } from 'zustand'
import type { FileEntry, WorkspaceStore } from '../types'

interface FileTreeProps {
  workspace: WorkspaceStore
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

interface SearchState {
  query: string
  entries: FileEntry[]
  loading: boolean
  error: string | null
}

export function FileTree({
  workspace,
  selectedPath,
  expandedDirs,
  onSelectFile,
  onToggleDir
}: FileTreeProps): JSX.Element {
  const { workspace: wsData, getFilesystemApi } = useStore(workspace)
  const workspacePath = wsData.path
  const filesystem = getFilesystemApi()
  const [dirContents, setDirContents] = useState<Record<string, DirectoryState>>({})
  const [search, setSearch] = useState<SearchState>({
    query: '',
    entries: [],
    loading: false,
    error: null
  })
  const [debouncedQuery, setDebouncedQuery] = useState('')

  // Debounce search query
  useEffect(() => {
    const timer = setTimeout(() => {
      setDebouncedQuery(search.query)
    }, 300)
    return () => clearTimeout(timer)
  }, [search.query])

  // Perform search when debounced query changes
  useEffect(() => {
    if (!debouncedQuery.trim()) {
      setSearch((prev) => ({ ...prev, entries: [], loading: false, error: null }))
      return
    }

    const performSearch = async () => {
      setSearch((prev) => ({ ...prev, loading: true, error: null }))

      try {
        const result = await filesystem.searchFiles(debouncedQuery)

        if (result.success) {
          setSearch((prev) => ({
            ...prev,
            entries: result.entries,
            loading: false,
            error: null
          }))
        } else {
          setSearch((prev) => ({
            ...prev,
            entries: [],
            loading: false,
            error: result.error || 'Search failed'
          }))
        }
      } catch (err) {
        setSearch((prev) => ({
          ...prev,
          entries: [],
          loading: false,
          error: `Search error: ${err}`
        }))
      }
    }

    performSearch()
  }, [debouncedQuery, workspacePath, filesystem])

  const loadDirectory = useCallback(
    async (dirPath: string) => {
      // Don't reload if already loaded
      if (dirContents[dirPath]?.entries.length > 0) return

      setDirContents((prev) => ({
        ...prev,
        [dirPath]: { entries: [], loading: true, error: null }
      }))

      try {
        const result = await filesystem.readDirectory(dirPath)

        if (result.success) {
          setDirContents((prev) => ({
            ...prev,
            [dirPath]: { entries: result.contents.entries, loading: false, error: null }
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
    [dirContents, filesystem]
  )

  // Load root directory on mount
  useEffect(() => {
    loadDirectory(workspacePath)
  }, [workspacePath, loadDirectory])

  // Load expanded directories
  useEffect(() => {
    for (const dirPath of expandedDirs) {
      if (!dirContents[dirPath]) {
        loadDirectory(dirPath)
      }
    }
  }, [expandedDirs, loadDirectory, dirContents])

  const handleToggleDir = (dirPath: string) => {
    onToggleDir(dirPath)
    if (!expandedDirs.includes(dirPath)) {
      loadDirectory(dirPath)
    }
  }

  const handleSearchChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setSearch((prev) => ({ ...prev, query: e.target.value }))
  }

  const clearSearch = () => {
    setSearch({
      query: '',
      entries: [],
      loading: false,
      error: null
    })
  }

  // Highlight matching text in filename
  const highlightMatch = (name: string, query: string): JSX.Element => {
    if (!query.trim()) return <>{name}</>

    const lowerName = name.toLowerCase()
    const lowerQuery = query.toLowerCase()
    const index = lowerName.indexOf(lowerQuery)

    if (index === -1) return <>{name}</>

    const before = name.slice(0, index)
    const match = name.slice(index, index + query.length)
    const after = name.slice(index + query.length)

    return (
      <>
        {before}
        <span className="file-tree-search-match">{match}</span>
        {after}
      </>
    )
  }

  // Render a tree entry (file or directory)
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

  // Render a search result entry
  const renderSearchResult = (entry: FileEntry): JSX.Element => {
    const isSelected = selectedPath === entry.path

    return (
      <div
        key={entry.path}
        className={`file-tree-item file-tree-search-result ${isSelected ? 'selected' : ''}`}
        style={{ paddingLeft: '8px' }}
        onClick={() => {
          if (entry.isDirectory) {
            // Expand the directory and select it
            handleToggleDir(entry.path)
            onSelectFile(entry.path)
          } else {
            onSelectFile(entry.path)
          }
        }}
        title={entry.relativePath}
      >
        <span className="file-tree-icon">{entry.isDirectory ? '\uD83D\uDCC1' : '\uD83D\uDCC4'}</span>
        <span className="file-tree-name file-tree-search-name">
          {highlightMatch(entry.name, search.query)}
        </span>
        <span className="file-tree-search-path">{entry.relativePath}</span>
      </div>
    )
  }

  const rootState = dirContents[workspacePath]
  const isSearching = search.query.trim().length > 0

  return (
    <div className="file-tree">
      {/* Search Input */}
      <div className="file-tree-search">
        <input
          type="text"
          className="file-tree-search-input"
          placeholder="Search files..."
          value={search.query}
          onChange={handleSearchChange}
        />
        {search.query && (
          <button className="file-tree-search-clear" onClick={clearSearch} title="Clear search">
            \u2715
          </button>
        )}
      </div>

      {/* Search Results */}
      {isSearching && (
        <div className="file-tree-search-results">
          {search.loading && (
            <div className="file-tree-item">
              <span className="file-tree-loading">Searching...</span>
            </div>
          )}

          {search.error && (
            <div className="file-tree-item file-tree-error">
              <span>{search.error}</span>
            </div>
          )}

          {!search.loading && !search.error && search.entries.length === 0 && (
            <div className="file-tree-item file-tree-search-empty">
              <span>No files found</span>
            </div>
          )}

          {!search.loading &&
            !search.error &&
            search.entries.length > 0 &&
            search.entries.map((entry) => renderSearchResult(entry))}
        </div>
      )}

      {/* Normal Tree View (shown when not searching) */}
      {!isSearching && (
        <>
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
        </>
      )}
    </div>
  )
}
