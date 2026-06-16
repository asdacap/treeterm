import React from 'react'
import { Columns2, AlignJustify, Eye, EyeOff, MessageSquare, CheckCircle2, Space, Filter, X, ChevronUp, ChevronDown } from 'lucide-react'

interface DiffToolbarProps {
  isSplitView: boolean
  onToggleSplit: () => void
  hideUnchanged: boolean
  onToggleHideUnchanged: () => void
  ignoreWhitespace: boolean
  onToggleIgnoreWhitespace: () => void
  totalComments: number
  viewedCount?: number
  totalFiles?: number
  dirFilter?: string | null
  onClearDirFilter?: () => void
  onPrevUnviewed?: () => void
  onNextUnviewed?: () => void
  hasUnviewed?: boolean
}

export function DiffToolbar({
  isSplitView,
  onToggleSplit,
  hideUnchanged,
  onToggleHideUnchanged,
  ignoreWhitespace,
  onToggleIgnoreWhitespace,
  totalComments,
  viewedCount,
  totalFiles,
  dirFilter,
  onClearDirFilter,
  onPrevUnviewed,
  onNextUnviewed,
  hasUnviewed,
}: DiffToolbarProps): React.JSX.Element {
  return (
    <div className="diff-toolbar-global">
      <div className="diff-toolbar-viewed-progress">
        {totalFiles !== undefined && totalFiles > 0 && (
          <span className="diff-toolbar-viewed-count" title={`${String(viewedCount ?? 0)} of ${String(totalFiles)} files viewed`}>
            <CheckCircle2 size={14} />
            {viewedCount ?? 0}/{totalFiles} files viewed
          </span>
        )}
        {dirFilter && (
          <span className="diff-filter-chip" title={`Filtered to ${dirFilter}`}>
            <Filter size={12} />
            <span className="diff-filter-chip-label">{dirFilter}</span>
            <button
              className="diff-filter-chip-clear"
              onClick={onClearDirFilter}
              title="Clear directory filter"
            >
              <X size={12} />
            </button>
          </span>
        )}
      </div>
      <div className="diff-toolbar-controls">
        {onPrevUnviewed && (
          <button
            className="pierre-diff-btn"
            onClick={onPrevUnviewed}
            disabled={!hasUnviewed}
            title="Previous unviewed file"
          >
            <ChevronUp size={14} />
          </button>
        )}

        {onNextUnviewed && (
          <button
            className="pierre-diff-btn"
            onClick={onNextUnviewed}
            disabled={!hasUnviewed}
            title="Next unviewed file"
          >
            <ChevronDown size={14} />
          </button>
        )}

        <button
          className={`pierre-diff-btn ${isSplitView ? 'active' : ''}`}
          onClick={onToggleSplit}
          title={isSplitView ? 'Switch to unified view' : 'Switch to split view'}
        >
          {isSplitView ? <Columns2 size={14} /> : <AlignJustify size={14} />}
        </button>

        <button
          className={`pierre-diff-btn ${hideUnchanged ? 'active' : ''}`}
          onClick={onToggleHideUnchanged}
          title={hideUnchanged ? 'Show unchanged regions' : 'Hide unchanged regions'}
        >
          {hideUnchanged ? <EyeOff size={14} /> : <Eye size={14} />}
        </button>

        <button
          className={`pierre-diff-btn ${ignoreWhitespace ? 'active' : ''}`}
          onClick={onToggleIgnoreWhitespace}
          title={ignoreWhitespace ? 'Show whitespace changes' : 'Ignore whitespace changes'}
        >
          <Space size={14} />
        </button>

        {totalComments > 0 && (
          <span className="pierre-diff-comment-count" title={`${String(totalComments)} comment(s)`}>
            <MessageSquare size={14} />
            {totalComments}
          </span>
        )}
      </div>
    </div>
  )
}
