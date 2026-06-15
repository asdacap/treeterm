import React, { useState, useEffect, useRef } from 'react'
import { ChevronDown, ChevronRight, Loader2 } from 'lucide-react'
import type { DiffFile, UncommittedFile, FileDiffContents, ReviewComment } from '../types'
import { FileChangeStatus } from '../types'
import { PierreDiffViewer } from './PierreDiffViewer'

// Rendering an entire file's diff into the DOM in one shot can exhaust renderer
// memory and crash the window. Files past these thresholds require an explicit
// "Load anyway" click before the heavy viewer is mounted.
const MAX_DIFF_LINES = 20000
const MAX_DIFF_BYTES = 2 * 1024 * 1024

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

  // Auto-collapse when isViewed transitions false→true (React previous-value-in-state pattern)
  const [prevIsViewed, setPrevIsViewed] = useState(isViewed ?? false)
  if (isViewed !== undefined && isViewed !== prevIsViewed) {
    setPrevIsViewed(isViewed)
    if (isViewed) {
      setCollapsed(true)
    }
  }
  const sectionRef = useRef<HTMLDivElement>(null)
  const loadRequestedRef = useRef(false)

  // IntersectionObserver to trigger lazy loading when section enters viewport
  useEffect(() => {
    const el = sectionRef.current
    if (!el || loadRequestedRef.current) return

    const observer = new IntersectionObserver(
      (entries) => {
        const entry = entries[0]
        if (entry && entry.isIntersecting) {
          loadRequestedRef.current = true
          onRequestLoad()
          observer.disconnect()
        }
      },
      { rootMargin: '200px' }
    )

    observer.observe(el)
    return () => { observer.disconnect() }
  }, [onRequestLoad])

  return (
    <div className="file-diff-section" ref={sectionRef} data-file-path={file.path}>
      <div
        className={`file-diff-header ${collapsed ? 'collapsed' : ''}`}
        onClick={() => { setCollapsed(!collapsed) }}
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
                  if (headerEl) {
                    const headerHeight = headerEl.getBoundingClientRect().height
                    requestAnimationFrame(() => {
                      const scrollParent = headerEl.closest('.stacked-diff-list')
                      if (scrollParent) {
                        scrollParent.scrollTop += headerHeight
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
        <div className="file-diff-body">
          {loading ? (
            <div className="file-diff-loading">
              <Loader2 size={16} className="spinning" />
              <span>Loading diff...</span>
            </div>
          ) : error ? (
            <div className="file-diff-error">{error}</div>
          ) : contents ? (
            (() => {
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
