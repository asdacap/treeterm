import React from 'react'
import { Columns2, AlignJustify, Eye, EyeOff, MessageSquare, CheckCircle2, Space } from 'lucide-react'

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
      </div>
      <div className="diff-toolbar-controls">
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
