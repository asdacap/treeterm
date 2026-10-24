import React, { useEffect, useRef, useState } from 'react'
import { ChevronDown, ChevronUp, Loader2, Search, X } from 'lucide-react'
import { useStore } from 'zustand'
import type { FilesystemState, WorkspaceStore } from '../types'
import { useExecApi } from '../hooks/useWorkspaceApis'
import {
  RepoSearchEventType,
  startRepoSearch,
  toWorkspaceAbsolute,
  type RepoHit,
  type RepoSearchTool,
} from '../utils/repoSearch'

// Typing (or a new double-click) restarts the repo search; wait for a pause first.
export const REPO_SEARCH_DEBOUNCE_MS = 250

// Open a workspace-relative location in a new file browser tab.
export function openInFileBrowser(workspace: WorkspaceStore, relativePath: string, lineNumber: number): void {
  const { addTab, workspace: wsData } = workspace.getState()
  addTab<FilesystemState>('filesystem', {
    selectedPath: toWorkspaceAbsolute(wsData.path, relativePath),
    scrollToLine: lineNumber,
  })
}

export interface InViewHit {
  filePath: string
  // Shown before the preview, e.g. the line number.
  label: string
  preview: string
}

interface SearchPanelProps {
  workspace: WorkspaceStore
  query: string
  onQueryChange: (query: string) => void
  // Bumped whenever the panel is (re)opened so the input refocuses.
  focusNonce: number
  onClose: () => void
  // Matches in what is on screen (the diff, or the open file), in navigation order.
  inViewTitle: string
  inViewHits: InViewHit[]
  activeIndex: number | null
  // Some of the in-view content is still loading, so the hits may grow.
  inViewPending: boolean
  onSelectInView: (index: number) => void
  onNext: () => void
  onPrev: () => void
  // Files whose matches are already listed in view, left out of the repo results.
  excludeFilePaths: string[]
  onSelectRepo: (hit: RepoHit) => void
}

function countLabel(query: string, current: number | null, total: number, pending: boolean): string {
  if (!query) return ''
  if (pending && total === 0) return 'Searching…'
  const count = current === null ? `${String(total)} found` : `${String(current)}/${String(total)}`
  return pending ? `${count}…` : count
}

// Consecutive hits in the same file share one heading.
function groupByFile<T extends { filePath: string }>(hits: T[]): { filePath: string; items: { hit: T; index: number }[] }[] {
  const groups: { filePath: string; items: { hit: T; index: number }[] }[] = []
  hits.forEach((hit, index) => {
    const last = groups.at(-1)
    if (last?.filePath === hit.filePath) last.items.push({ hit, index })
    else groups.push({ filePath: hit.filePath, items: [{ hit, index }] })
  })
  return groups
}

export function SearchPanel({
  workspace,
  query,
  onQueryChange,
  focusNonce,
  onClose,
  inViewTitle,
  inViewHits,
  activeIndex,
  inViewPending,
  onSelectInView,
  onNext,
  onPrev,
  excludeFilePaths,
  onSelectRepo,
}: SearchPanelProps): React.JSX.Element {
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    inputRef.current?.focus()
    inputRef.current?.select()
  }, [focusNonce])

  const total = inViewHits.length

  return (
    <div className="search-panel">
      <div className="diff-find-bar">
        <Search size={12} className="diff-find-icon" />
        <input
          ref={inputRef}
          className="diff-find-input"
          placeholder="Search"
          value={query}
          onChange={(e) => { onQueryChange(e.target.value) }}
          onKeyDown={(e) => {
            // eslint-disable-next-line custom/no-string-literal-comparison -- DOM key names are external
            if (e.key === 'Enter') {
              e.preventDefault()
              if (e.shiftKey) onPrev()
              else onNext()
            // eslint-disable-next-line custom/no-string-literal-comparison -- DOM key names are external
            } else if (e.key === 'Escape') {
              e.preventDefault()
              onClose()
            }
          }}
        />
        <span className="diff-find-count">
          {countLabel(query, activeIndex === null ? null : activeIndex + 1, total, inViewPending)}
        </span>
        <button className="diff-find-btn" onClick={onPrev} disabled={total === 0} title="Previous match (Shift+Enter)">
          <ChevronUp size={14} />
        </button>
        <button className="diff-find-btn" onClick={onNext} disabled={total === 0} title="Next match (Enter)">
          <ChevronDown size={14} />
        </button>
        <button className="diff-find-btn" onClick={onClose} title="Close (Escape)">
          <X size={14} />
        </button>
      </div>
      {query && (
        <div className="search-panel-results">
          <div className="search-panel-section-title">{inViewTitle}</div>
          {total === 0 && !inViewPending && <div className="search-panel-note">No matches</div>}
          {groupByFile(inViewHits).map(group => (
            <div key={group.filePath} className="search-panel-group">
              <div className="search-panel-file">{group.filePath}</div>
              {group.items.map(({ hit, index }) => (
                <button
                  key={index}
                  className={`search-panel-hit ${index === activeIndex ? 'active' : ''}`}
                  onClick={() => { onSelectInView(index) }}
                >
                  <span className="search-panel-hit-label">{hit.label}</span>
                  <span className="search-panel-hit-preview">{hit.preview}</span>
                </button>
              ))}
            </div>
          ))}
          <RepoResults
            key={query}
            workspace={workspace}
            query={query}
            excludeFilePaths={excludeFilePaths}
            onSelect={onSelectRepo}
          />
        </div>
      )}
    </div>
  )
}

enum RepoResultsStatus {
  Searching = 'searching',
  Done = 'done',
  Error = 'error',
}

type RepoResultsState =
  | { status: RepoResultsStatus.Searching; hits: RepoHit[] }
  | { status: RepoResultsStatus.Done; hits: RepoHit[]; tool: RepoSearchTool; truncated: boolean }
  | { status: RepoResultsStatus.Error; hits: RepoHit[]; message: string }

function renderRepoFooter(state: RepoResultsState, shownCount: number): React.ReactNode {
  const views: Record<RepoResultsStatus, () => React.ReactNode> = {
    [RepoResultsStatus.Searching]: () => (
      <div className="search-panel-note"><Loader2 size={12} className="spinning" /> Searching repo…</div>
    ),
    [RepoResultsStatus.Done]: () => state.status === RepoResultsStatus.Done && (
      <div className="search-panel-note">
        {shownCount === 0 ? 'No matches' : `${String(shownCount)} found`}
        {state.truncated ? ' (stopped at the result limit)' : ''} · {state.tool}
      </div>
    ),
    [RepoResultsStatus.Error]: () => state.status === RepoResultsStatus.Error && (
      <div className="search-panel-error">Search failed: {state.message}</div>
    ),
  }
  return views[state.status]()
}

// Keyed by query, so each query starts from empty results. Unmounting (query
// changed, panel closed) cancels the running search.
function RepoResults({
  workspace,
  query,
  excludeFilePaths,
  onSelect,
}: {
  workspace: WorkspaceStore
  query: string
  excludeFilePaths: string[]
  onSelect: (hit: RepoHit) => void
}): React.JSX.Element {
  const execApi = useExecApi(workspace)
  const connectionId = useStore(workspace, s => s.connectionId)
  const cwd = useStore(workspace, s => s.workspace.path)
  const [state, setState] = useState<RepoResultsState>({ status: RepoResultsStatus.Searching, hits: [] })

  useEffect(() => {
    let cancel = (): void => {}
    const timer = setTimeout(() => {
      cancel = startRepoSearch(execApi, connectionId, cwd, query, (event) => {
        if (event.type === RepoSearchEventType.Hits) {
          setState(s => ({ ...s, hits: [...s.hits, ...event.hits] }))
        } else if (event.type === RepoSearchEventType.Done) {
          setState(s => ({ status: RepoResultsStatus.Done, hits: s.hits, tool: event.tool, truncated: event.truncated }))
        } else {
          setState(s => ({ status: RepoResultsStatus.Error, hits: s.hits, message: event.message }))
        }
      })
    }, REPO_SEARCH_DEBOUNCE_MS)
    return () => {
      clearTimeout(timer)
      cancel()
    }
  }, [execApi, connectionId, cwd, query])

  const hits = state.hits.filter(hit => !excludeFilePaths.includes(hit.filePath))

  return (
    <>
      <div className="search-panel-section-title">In repo</div>
      {groupByFile(hits).map(group => (
        <div key={group.filePath} className="search-panel-group">
          <div className="search-panel-file">{group.filePath}</div>
          {group.items.map(({ hit, index }) => (
            <button key={index} className="search-panel-hit" onClick={() => { onSelect(hit) }}>
              <span className="search-panel-hit-label">{hit.lineNumber}</span>
              <span className="search-panel-hit-preview">{hit.preview}</span>
            </button>
          ))}
        </div>
      ))}
      {renderRepoFooter(state, hits.length)}
    </>
  )
}
