import React, { useState, useEffect, useRef } from 'react'
import { ChevronDown, ChevronRight, Loader2 } from 'lucide-react'
import type { DiffFile, UncommittedFile, FileDiffContents, ReviewComment } from '../types'
import { FileChangeStatus } from '../types'
import { PierreDiffViewer } from './PierreDiffViewer'

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
  getStatusIcon: (status: FileChangeStatus) => React.JSX.Element
  comments: ReviewComment[]
  onLineClick: (lineNumber: number, side: 'original' | 'modified') => void
  inlineCommentInput: { lineNumber: number; side: 'original' | 'modified' } | null
  onCommentSubmit?: (text: string) => void
  onCommentCancel?: () => void
  onCommentDelete?: (commentId: string) => void
  stagingAction?: StagingAction
}

export function FileDiffSection({
  file,
  contents,
  loading,
  error,
  onRequestLoad,
  diffStyle,
  expandUnchanged,
  getStatusIcon,
  comments,
  onLineClick,
  inlineCommentInput,
  onCommentSubmit,
  onCommentCancel,
  onCommentDelete,
  stagingAction,
}: FileDiffSectionProps): React.JSX.Element {
  const [collapsed, setCollapsed] = useState(false)
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
            <PierreDiffViewer
              originalContent={contents.originalContent}
              modifiedContent={contents.modifiedContent}
              filePath={file.path}
              diffStyle={diffStyle}
              expandUnchanged={expandUnchanged}
              comments={comments}
              onLineClick={onLineClick}
              inlineCommentInput={inlineCommentInput}
              onCommentSubmit={onCommentSubmit}
              onCommentCancel={onCommentCancel}
              onCommentDelete={onCommentDelete}
            />
          ) : (
            <div className="file-diff-placeholder">Scroll to load diff</div>
          )}
        </div>
      )}
    </div>
  )
}
