import { useState } from 'react'

interface CommentInputProps {
  lineNumber: number
  side?: 'original' | 'modified'
  onSubmit: (text: string) => void
  onCancel: () => void
}

export function CommentInput({
  lineNumber,
  side,
  onSubmit,
  onCancel
}: CommentInputProps): JSX.Element {
  const [text, setText] = useState('')

  const handleSubmit = () => {
    if (text.trim()) {
      onSubmit(text.trim())
      setText('')
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault()
      handleSubmit()
    } else if (e.key === 'Escape') {
      e.preventDefault()
      onCancel()
    }
  }

  return (
    <div className="comment-input-container">
      <div className="comment-input-header">
        Comment on line {lineNumber}{side ? ` (${side})` : ''}
      </div>
      <textarea
        className="comment-input-textarea"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={handleKeyDown}
        placeholder="Add your comment... (Cmd/Ctrl+Enter to submit, Esc to cancel)"
        autoFocus
      />
      <div className="comment-input-actions">
        <button className="comment-btn comment-btn-cancel" onClick={onCancel}>
          Cancel
        </button>
        <button
          className="comment-btn comment-btn-submit"
          onClick={handleSubmit}
          disabled={!text.trim()}
        >
          Add Comment
        </button>
      </div>
    </div>
  )
}
