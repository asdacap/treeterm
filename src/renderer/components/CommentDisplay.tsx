import type { ReviewComment } from '../types'

interface CommentDisplayProps {
  comment: ReviewComment
  onDelete: (id: string) => void
  hideLineRef?: boolean
}

export function CommentDisplay({ comment, onDelete, hideLineRef }: CommentDisplayProps): JSX.Element {
  const showHeader = !hideLineRef || comment.isOutdated
  return (
    <div className={`comment-display ${comment.isOutdated ? 'outdated' : ''}`}>
      {showHeader && (
        <div className="comment-display-header">
          {!hideLineRef && (
            <span className="comment-line-ref">
              Line {String(comment.lineNumber)} ({comment.side})
            </span>
          )}
          {comment.isOutdated && (
            <span className="comment-outdated-badge" title="This comment may be outdated">
              Outdated
            </span>
          )}
          <button
            className="comment-delete-btn"
            onClick={() => { onDelete(comment.id); }}
            title="Delete comment"
          >
            ×
          </button>
        </div>
      )}
      <div className="comment-display-text">
        {comment.text}
        {!showHeader && (
          <button
            className="comment-delete-btn inline"
            onClick={() => { onDelete(comment.id); }}
            title="Delete comment"
          >
            ×
          </button>
        )}
      </div>
      <div className="comment-display-meta">
        {new Date(comment.createdAt).toLocaleString()}
      </div>
    </div>
  )
}
