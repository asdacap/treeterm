import React, { useState, useEffect, useRef } from 'react'
import { ChevronDown, ChevronRight, Loader2, RefreshCw } from 'lucide-react'
import type { DiffFile, UncommittedFile, FileDiffContents, ReviewComment } from '../types'
import { FileChangeStatus } from '../types'
import { PierreDiffViewer } from './PierreDiffViewer'

// Rendering an entire file's diff into the DOM in one shot can exhaust renderer
// memory and crash the window. Files past these thresholds require an explicit
// "Load anyway" click before the heavy viewer is mounted.
const MAX_DIFF_LINES = 20000
const MAX_DIFF_BYTES = 2 * 1024 * 1024

// Matches .file-diff-body min-height in index.css. Used as the floor for the
// off-screen spacer height.
const MIN_BODY_HEIGHT = 40

// Count lines without allocating a huge intermediate array, so this stays cheap
// even for files large enough to be the reason we're guarding in the first place.
function countLines(text: string): number {
  if (text.length === 0) return 0
  let count = 1
  let idx = text.indexOf('\n')
  while (idx !== -1) {
    count++
    idx = text.indexOf('\n', idx + 1)
  }
  return count
}

interface DiffSize {
  lines: number
  bytes: number
  oversized: boolean
}

function measureDiff(contents: FileDiffContents): DiffSize {
  const lines = countLines(contents.originalContent) + countLines(contents.modifiedContent)
  const bytes = contents.originalContent.length + contents.modifiedContent.length
  return { lines, bytes, oversized: lines > MAX_DIFF_LINES || bytes > MAX_DIFF_BYTES }
}

interface StagingAction {
  label: string
  onAction: () => void
  disabled: boolean
}

interface FileDiffSectionProps {
  file: DiffFile | UncommittedFile
  contents: FileDiffContents | null
  loading: boolean
  error: string | null
  onRequestLoad: () => void
  onRefresh?: () => void
  diffStyle: 'split' | 'unified'
  expandUnchanged: boolean
  ignoreWhitespace: boolean
  getStatusIcon: (status: FileChangeStatus) => React.JSX.Element
  comments: ReviewComment[]
  onLineClick: (lineNumber: number, side: 'original' | 'modified') => void
  inlineCommentInput: { lineNumber: number; side: 'original' | 'modified' } | null
  onCommentSubmit?: (text: string) => void
  onCommentCancel?: () => void
  onCommentDelete?: (commentId: string) => void
  stagingAction?: StagingAction
  isViewed?: boolean
  onToggleViewed?: () => void
  onMarkViewedAbove?: () => void
  isFirstFile?: boolean
}

export function FileDiffSection({
  file,
  contents,
  loading,
  error,
  onRequestLoad,
  onRefresh,
  diffStyle,
  expandUnchanged,
  ignoreWhitespace,
  getStatusIcon,
  comments,
  onLineClick,
  inlineCommentInput,
  onCommentSubmit,
  onCommentCancel,
  onCommentDelete,
  stagingAction,
  isViewed,
  onToggleViewed,
  onMarkViewedAbove,
  isFirstFile,
}: FileDiffSectionProps): React.JSX.Element {
  const [collapsed, setCollapsed] = useState(isViewed ?? false)
  const [forceRender, setForceRender] = useState(false)
  // Whether the heavy diff viewer should be mounted. Starts true so the
  // initially-visible sections render immediately; the observer flips it false
  // once the section scrolls far enough out of view, unmounting the viewer's DOM
  // to bound memory. Far-down sections stay cheap regardless because their
  // contents are null until lazy-loaded.
  const [shouldRender, setShouldRender] = useState(true)
  // Last measured body height, used to size the spacer that replaces the viewer
  // while off-screen so the scroll position doesn't jump. Floored at MIN_BODY_HEIGHT.
  const [placeholderHeight, setPlaceholderHeight] = useState(MIN_BODY_HEIGHT)

  // Auto-collapse when isViewed transitions false→true (React previous-value-in-state pattern)
  const [prevIsViewed, setPrevIsViewed] = useState(isViewed ?? false)
  if (isViewed !== undefined && isViewed !== prevIsViewed) {
    setPrevIsViewed(isViewed)
    if (isViewed) {
      setCollapsed(true)
    }
  }

  // A global layout change (split↔unified, expand toggle) invalidates the cached
  // off-screen height; reset so the section re-measures on next entry.
  const [prevLayoutKey, setPrevLayoutKey] = useState(`${diffStyle}-${String(expandUnchanged)}`)
  const layoutKey = `${diffStyle}-${String(expandUnchanged)}`
  if (layoutKey !== prevLayoutKey) {
    setPrevLayoutKey(layoutKey)
    setPlaceholderHeight(MIN_BODY_HEIGHT)
  }

  const sectionRef = useRef<HTMLDivElement>(null)
  const bodyRef = useRef<HTMLDivElement>(null)
  // Hold the latest onRequestLoad so the once-created observer always calls the
  // current callback without re-subscribing. A stale closure would bypass the
  // parent's load dedupe and trigger redundant git refetches. Pure mutation.
  const onRequestLoadRef = useRef(onRequestLoad)
  onRequestLoadRef.current = onRequestLoad

  // Continuous IntersectionObserver: lazy-loads on entry and unmounts the heavy
  // viewer on exit. Created once; reads the latest onRequestLoad via the ref.
  useEffect(() => {
    const el = sectionRef.current
    if (!el) return

    const observer = new IntersectionObserver(
      (entries) => {
        const entry = entries[0]
        if (!entry) return
        if (entry.isIntersecting) {
          onRequestLoadRef.current()
          setShouldRender(true)
        } else {
          const bodyEl = bodyRef.current
          if (bodyEl) {
            const height = bodyEl.getBoundingClientRect().height
            // Floor guard against measure-before-layout / fast-scroll zero height.
            if (height > MIN_BODY_HEIGHT) setPlaceholderHeight(height)
          }
          setShouldRender(false)
        }
      },
      { rootMargin: '1500px' }
    )

    observer.observe(el)
    return () => { observer.disconnect() }
  }, [])

  return (
    <div className="file-diff-section" ref={sectionRef} data-file-path={file.path}>
      <div
        className={`file-diff-header ${collapsed ? 'collapsed' : ''}`}
        onClick={() => {
          // Expanding implies the section is near the viewport; ensure the viewer
          // mounts rather than showing a stale off-screen spacer.
          if (collapsed) setShouldRender(true)
          setCollapsed(!collapsed)
        }}
      >
        <span className="file-diff-collapse-icon">
          {collapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
        </span>
        {getStatusIcon(file.status)}
        <span className="file-diff-path" title={file.path}>{file.path}</span>
        <span className="file-diff-stats">
          <span className="additions">+{file.additions}</span>
          <span className="deletions">-{file.deletions}</span>
        </span>
        {stagingAction && (
          <button
            className="file-diff-stage-btn"
            onClick={(e) => {
              e.stopPropagation()
              stagingAction.onAction()
            }}
            disabled={stagingAction.disabled}
          >
            {stagingAction.label}
          </button>
        )}
        {onRefresh && (
          <button
            className="file-diff-refresh-btn"
            onClick={(e) => { e.stopPropagation(); onRefresh() }}
            disabled={loading}
            title="Refresh this file's diff"
          >
            <RefreshCw size={12} className={loading ? 'spinning' : ''} />
          </button>
        )}
        {onToggleViewed && (
          <label
            className={`file-diff-viewed-label ${isViewed ? 'viewed' : ''}`}
            onClick={(e) => { e.stopPropagation() }}
          >
            <input
              type="checkbox"
              checked={isViewed ?? false}
              onChange={(e) => {
                if (!isViewed && !collapsed) {
                  const headerEl = (e.target as HTMLElement).closest('.file-diff-header')
                  // Capture the clicked (sticky) header's position now — this is where the
                  // cursor is. After the file collapses we bring the next file's header here,
                  // so its "Viewed" checkbox (same column) lands under the cursor for rapid
                  // sequential checking. Because the sticky header is pinned at the viewport
                  // top while scrolled into the file, this also lands at the next file's
                  // start with no overshoot, regardless of how far in we'd scrolled.
                  const anchorTop = headerEl?.getBoundingClientRect().top ?? 0
                  const nextHeader = sectionRef.current
                    ?.parentElement?.nextElementSibling
                    ?.querySelector('.file-diff-header')
                  if (headerEl && nextHeader) {
                    requestAnimationFrame(() => {
                      const scrollParent = headerEl.closest('.stacked-diff-list')
                      if (scrollParent) {
                        scrollParent.scrollTop += nextHeader.getBoundingClientRect().top - anchorTop
                      }
                    })
                  }
                }
                onToggleViewed()
              }}
              className="file-diff-viewed-checkbox"
            />
            <span
              onContextMenu={!isFirstFile && onMarkViewedAbove ? (e) => {
                e.preventDefault()
                e.stopPropagation()
                onMarkViewedAbove()
              } : undefined}
              title={!isFirstFile && onMarkViewedAbove ? 'Right-click to mark all above as viewed' : undefined}
            >
              Viewed
            </span>
          </label>
        )}
      </div>

      {!collapsed && (
        <div className="file-diff-body" ref={bodyRef}>
          {loading ? (
            <div className="file-diff-loading">
              <Loader2 size={16} className="spinning" />
              <span>Loading diff...</span>
            </div>
          ) : error ? (
            <div className="file-diff-error">{error}</div>
          ) : contents ? (
            !shouldRender ? (
              <div
                className="file-diff-offscreen-placeholder"
                style={{ height: placeholderHeight }}
              />
            ) : (() => {
              const size = measureDiff(contents)
              if (size.oversized && !forceRender) {
                return (
                  <div className="file-diff-too-large">
                    <p>
                      This diff is large ({size.lines.toLocaleString()} lines,{' '}
                      {Math.round(size.bytes / 1024).toLocaleString()} KB) and may slow down or crash the app.
                    </p>
                    <button
                      className="file-diff-load-anyway-btn"
                      onClick={() => { setForceRender(true) }}
                    >
                      Load anyway
                    </button>
                  </div>
                )
              }
              return (
                <PierreDiffViewer
                  originalContent={contents.originalContent}
                  modifiedContent={contents.modifiedContent}
                  filePath={file.path}
                  diffStyle={diffStyle}
                  expandUnchanged={expandUnchanged}
                  ignoreWhitespace={ignoreWhitespace}
                  comments={comments}
                  onLineClick={onLineClick}
                  inlineCommentInput={inlineCommentInput}
                  onCommentSubmit={onCommentSubmit}
                  onCommentCancel={onCommentCancel}
                  onCommentDelete={onCommentDelete}
                />
              )
            })()
          ) : (
            <div className="file-diff-placeholder">Scroll to load diff</div>
          )}
        </div>
      )}
    </div>
  )
}
