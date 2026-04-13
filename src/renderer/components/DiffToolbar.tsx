import React from 'react'
import { Columns2, AlignJustify, Eye, EyeOff, MessageSquare } from 'lucide-react'

interface DiffToolbarProps {
  isSplitView: boolean
  onToggleSplit: () => void
  hideUnchanged: boolean
  onToggleHideUnchanged: () => void
  totalComments: number
}

export function DiffToolbar({
  isSplitView,
  onToggleSplit,
  hideUnchanged,
  onToggleHideUnchanged,
  totalComments,
}: DiffToolbarProps): React.JSX.Element {
  return (
    <div className="diff-toolbar-global">
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
