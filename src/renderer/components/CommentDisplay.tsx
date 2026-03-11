import type { ReviewComment } from '../types'

interface CommentDisplayProps {
  comment: ReviewComment
  onDelete: (id: string) => void
}

export function CommentDisplay({ comment, onDelete }: CommentDisplayProps): JSX.Element {
  return (
    <div className={`comment-display ${comment.isOutdated ? 'outdated' : ''}`}>
      <div className="comment-display-header">
        <span className="comment-line-ref">
          Line {comment.lineNumber} ({comment.side})
        </span>
        {comment.isOutdated && (
          <span className="comment-outdated-badge" title="This comment may be outdated">
            Outdated
          </span>
        )}
        <button
          className="comment-delete-btn"
          onClick={() => onDelete(comment.id)}
          title="Delete comment"
        >
          ×
        </button>
      </div>
      <div className="comment-display-text">{comment.text}</div>
      <div className="comment-display-meta">
        {new Date(comment.createdAt).toLocaleString()}
      </div>
    </div>
  )
}
