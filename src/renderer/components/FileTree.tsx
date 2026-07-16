import React, { useState, useEffect, useCallback } from 'react'
import { Star } from 'lucide-react'
import { useStore } from 'zustand'
import type { FileEntry, WorkspaceStore } from '../types'
import { useFilesystemApi } from '../hooks/useWorkspaceApis'
import { resolveFavouriteFiles, type FavouriteFile } from '../utils/favouriteFiles'
import { normalizeFileEntryRelativePath } from '../../shared/workspaceFavourites'

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
}: FileTreeProps): React.JSX.Element {
  const wsData = useStore(workspace, s => s.workspace)
  const favouritePathsRevision = useStore(workspace, s => s.favouritePathsRevision)
  const getFavouritePaths = useStore(workspace, s => s.getFavouritePaths)
  const checkFavouritePath = useStore(workspace, s => s.isFavouritePath)
  const addFavouritePath = useStore(workspace, s => s.addFavouritePath)
  const removeFavouritePath = useStore(workspace, s => s.removeFavouritePath)
  const workspacePath = wsData.path
  const filesystem = useFilesystemApi(workspace)
  const favouritePaths = getFavouritePaths()
  const localFavouritePaths = wsData.favouritePaths
  const favouritePathsKey = favouritePaths.join('\0')
  const [dirContents, setDirContents] = useState(new Map<string, DirectoryState>())
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
    return () => { clearTimeout(timer); }
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
          error: `Search error: ${String(err)}`
        }))
      }
    }

    void performSearch()
  }, [debouncedQuery, workspacePath, filesystem])

  const loadDirectory = useCallback(
    async (dirPath: string) => {
      // Atomically check-and-set loading state to prevent duplicate requests
      let alreadyRequested = false
      setDirContents((prev) => {
        const existing = prev.get(dirPath)
        if (existing && !existing.error) {
          alreadyRequested = true
          return prev
        }
        return new Map(prev).set(dirPath, { entries: [], loading: true, error: null })
      })
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- mutated synchronously in setState callback above
      if (alreadyRequested) return

      try {
        const result = await filesystem.readDirectory(dirPath)

        if (result.success) {
          setDirContents((prev) =>
            new Map(prev).set(dirPath, { entries: result.contents.entries, loading: false, error: null })
          )
        } else {
          setDirContents((prev) =>
            new Map(prev).set(dirPath, { entries: [], loading: false, error: result.error || 'Failed to load' })
          )
        }
      } catch (err) {
        setDirContents((prev) =>
          new Map(prev).set(dirPath, { entries: [], loading: false, error: `Error: ${String(err)}` })
        )
      }
    },
    [filesystem]
  )

  // Load root directory on mount
  useEffect(() => {
    void loadDirectory(workspacePath)
  }, [workspacePath, loadDirectory])

  const handleToggleDir = (dirPath: string) => {
    onToggleDir(dirPath)
    if (!expandedDirs.includes(dirPath)) {
      void loadDirectory(dirPath)
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

  const renderFavouriteButton = (entry: FileEntry): React.JSX.Element => {
    const relativePath = normalizeFileEntryRelativePath(entry.relativePath, workspacePath)
    const isFavourite = checkFavouritePath(relativePath)
    const isLocalFavourite = localFavouritePaths.includes(relativePath)
    const title = isLocalFavourite
      ? 'Remove from favourites'
      : isFavourite
        ? 'Inherited or included by a favourite folder'
        : `Add ${entry.isDirectory ? 'folder' : 'file'} to favourites`
    return (
      <button
        className={`file-tree-favourite ${isFavourite ? 'active' : ''}`}
        title={title}
        aria-label={title}
        onClick={(event) => {
          event.stopPropagation()
          if (isLocalFavourite) removeFavouritePath(relativePath)
          else if (!isFavourite) addFavouritePath(relativePath)
        }}
      >
        <Star size={13} fill={isFavourite ? 'currentColor' : 'none'} />
      </button>
    )
  }

  // Highlight matching text in filename
  const highlightMatch = (name: string, query: string): React.JSX.Element => {
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
  const renderEntry = (entry: FileEntry, depth: number): React.JSX.Element => {
    const isExpanded = expandedDirs.includes(entry.path)
    const isSelected = selectedPath === entry.path
    const dirState = dirContents.get(entry.path)

    return (
      <div key={entry.path}>
        <div
          className={`file-tree-item ${isSelected ? 'selected' : ''}`}
          style={{ paddingLeft: `${String(depth * 16 + 8)}px` }}
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
          {renderFavouriteButton(entry)}
        </div>
        {entry.isDirectory && isExpanded && (
          <div className="file-tree-children">
            {dirState?.loading && (
              <div className="file-tree-item" style={{ paddingLeft: `${String((depth + 1) * 16 + 8)}px` }}>
                <span className="file-tree-loading">Loading...</span>
              </div>
            )}
            {dirState?.error && (
              <div
                className="file-tree-item file-tree-error"
                style={{ paddingLeft: `${String((depth + 1) * 16 + 8)}px` }}
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
  const renderSearchResult = (entry: FileEntry): React.JSX.Element => {
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
        {renderFavouriteButton(entry)}
      </div>
    )
  }

  const rootState = dirContents.get(workspacePath)
  const isSearching = search.query.trim().length > 0

  return (
    <div className="file-tree">
      {/* Load expanded directories — each DirectoryLoader triggers loadDirectory on mount */}
      {expandedDirs.map(dirPath => (
        <DirectoryLoader key={dirPath} dirPath={dirPath} loadDirectory={loadDirectory} />
      ))}
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

      {favouritePathsKey && (
        <FavouriteFilesSection
          key={`${String(favouritePathsRevision)}:${favouritePathsKey}`}
          workspacePath={workspacePath}
          favouritePathsKey={favouritePathsKey}
          filesystem={filesystem}
          selectedPath={selectedPath}
          onSelectFile={onSelectFile}
        />
      )}

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

enum FavouriteFilesStatus {
  Loading = 'loading',
  Ready = 'ready',
  Error = 'error',
}

type FavouriteFilesState =
  | { status: FavouriteFilesStatus.Loading }
  | { status: FavouriteFilesStatus.Ready; files: FavouriteFile[] }
  | { status: FavouriteFilesStatus.Error; error: string }

function FavouriteFilesSection({
  workspacePath,
  favouritePathsKey,
  filesystem,
  selectedPath,
  onSelectFile,
}: {
  workspacePath: string
  favouritePathsKey: string
  filesystem: ReturnType<typeof useFilesystemApi>
  selectedPath: string | null
  onSelectFile: (path: string) => void
}): React.JSX.Element | null {
  const [state, setState] = useState<FavouriteFilesState>({ status: FavouriteFilesStatus.Loading })
  const [retryRevision, setRetryRevision] = useState(0)

  useEffect(() => {
    const favouritePaths = favouritePathsKey.split('\0')
    let cancelled = false
    void resolveFavouriteFiles(workspacePath, favouritePaths, filesystem, () => cancelled).then(
      (files) => { if (!cancelled) setState({ status: FavouriteFilesStatus.Ready, files }) },
      (error: unknown) => {
        if (!cancelled) setState({
          status: FavouriteFilesStatus.Error,
          error: error instanceof Error ? error.message : String(error),
        })
      }
    )
    return () => { cancelled = true }
  }, [workspacePath, favouritePathsKey, filesystem, retryRevision])

  return (
    <div className="file-tree-favourites">
      <div className="file-tree-section-title"><Star size={12} fill="currentColor" /> Favourites</div>
      {state.status === FavouriteFilesStatus.Loading && <div className="file-tree-favourites-message">Loading favourites...</div>}
      {state.status === FavouriteFilesStatus.Error && (
        <div className="file-tree-favourites-message file-tree-error">
          <span>{state.error}</span>
          <button
            className="file-tree-favourites-retry"
            onClick={() => {
              setState({ status: FavouriteFilesStatus.Loading })
              setRetryRevision((revision) => revision + 1)
            }}
          >
            Retry
          </button>
        </div>
      )}
      {state.status === FavouriteFilesStatus.Ready && state.files.length === 0 && (
        <div className="file-tree-favourites-message">No favourite files exist in this workspace</div>
      )}
      {state.status === FavouriteFilesStatus.Ready && state.files.map((file) => (
        <div
          key={file.relativePath}
          className={`file-tree-item file-tree-favourite-file ${selectedPath === file.path ? 'selected' : ''}`}
          onClick={() => { onSelectFile(file.path) }}
          title={file.relativePath}
        >
          <span className="file-tree-icon">\uD83D\uDCC4</span>
          <span className="file-tree-name">{file.relativePath}</span>
        </div>
      ))}
    </div>
  )
}

/** Triggers directory load on mount — renders nothing */
function DirectoryLoader({ dirPath, loadDirectory }: { dirPath: string; loadDirectory: (path: string) => Promise<void> }) {
  useEffect(() => {
    void loadDirectory(dirPath)
  }, [dirPath, loadDirectory])

  return null
}
