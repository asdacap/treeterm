import { useState, useEffect } from 'react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { WorkspaceState } from '../store/createWorkspaceStore'
import { useFilesystemApi } from '../contexts/FilesystemApiContext'
import { generateReviewPrompt } from '../utils/reviewPrompt'
import type { ReviewComment, FilesystemState } from '../types'

interface CommentsListProps {
  workspacePath: string
  workspaceId: string
  workspaceStore: StoreApi<WorkspaceState>
}

const CONTEXT_LINES = 3

function extractCodeContext(
  fileContent: string,
  lineNumber: number
): { lines: { num: number; text: string }[]; targetLine: number } | null {
  const allLines = fileContent.split('\n')
  if (lineNumber < 1 || lineNumber > allLines.length) return null

  const start = Math.max(0, lineNumber - 1 - CONTEXT_LINES)
  const end = Math.min(allLines.length, lineNumber + CONTEXT_LINES)
  const lines = allLines.slice(start, end).map((text, i) => ({
    num: start + i + 1,
    text
  }))

  return { lines, targetLine: lineNumber }
}

export default function CommentsList({
  workspacePath,
  workspaceId,
  workspaceStore
}: CommentsListProps): JSX.Element {
  const filesystem = useFilesystemApi()
  const { workspaces, addTab, deleteReviewComment, toggleReviewCommentAddressed, getReviewComments } = useStore(workspaceStore)
  const workspace = workspaces[workspaceId]
  const comments: ReviewComment[] = getReviewComments(workspaceId)
  const [fileContents, setFileContents] = useState<Map<string, string>>(new Map())
  const [promptExpanded, setPromptExpanded] = useState(false)

  // Batch-fetch file contents for code context
  useEffect(() => {
    if (comments.length === 0) return

    const uniquePaths = Array.from(new Set(comments.map(c => c.filePath)))
    const missing = uniquePaths.filter(p => !fileContents.has(p))
    if (missing.length === 0) return

    const fetchFiles = async () => {
      const newContents = new Map(fileContents)
      await Promise.all(
        missing.map(async (filePath) => {
          try {
            const result = await filesystem.readFile(workspacePath, filePath)
            if (result.success && result.file) {
              newContents.set(filePath, result.file.content)
            }
          } catch {
            // Skip files that fail to load
          }
        })
      )
      setFileContents(newContents)
    }

    fetchFiles()
  }, [comments, workspacePath, filesystem])

  const handleToggleAddressed = (commentId: string) => {
    toggleReviewCommentAddressed(workspaceId, commentId)
  }

  const handleDelete = (commentId: string) => {
    deleteReviewComment(workspaceId, commentId)
  }

  const handleGoToFile = (comment: ReviewComment) => {
    addTab<FilesystemState>(workspaceId, 'filesystem', {
      selectedPath: comment.filePath,
      scrollToLine: comment.lineNumber
    })
  }

  const handleCopyPrompt = async () => {
    const prompt = generateReviewPrompt(comments)
    if (prompt) {
      await navigator.clipboard.writeText(prompt)
    }
  }

  if (comments.length === 0) {
    return <div className="comments-list"><div className="comments-empty">No review comments yet</div></div>
  }

  const prompt = generateReviewPrompt(comments)

  return (
    <div className="comments-list">
      <div className="comments-header">
        <span>Comments ({comments.length})</span>
      </div>
      <div className="comments-cards">
        {comments.map(comment => {
          const content = fileContents.get(comment.filePath)
          const codeContext = content ? extractCodeContext(content, comment.lineNumber) : null

          return (
            <div
              key={comment.id}
              className={`comments-card ${comment.isOutdated ? 'outdated' : ''} ${comment.addressed ? 'addressed' : ''}`}
            >
              <div className="comments-card-header">
                <span className="comments-card-file" title={comment.filePath}>
                  {comment.filePath}
                </span>
                <span className="comment-line-ref">
                  L{comment.lineNumber} ({comment.side})
                </span>
                {comment.isOutdated && (
                  <span className="comment-outdated-badge" title="This comment may be outdated">
                    Outdated
                  </span>
                )}
                <input
                  type="checkbox"
                  className="comments-addressed"
                  checked={comment.addressed}
                  onChange={() => handleToggleAddressed(comment.id)}
                  title="Mark as addressed"
                />
                <button
                  className="comment-delete-btn"
                  onClick={() => handleDelete(comment.id)}
                  title="Delete comment"
                >
                  ×
                </button>
              </div>

              {codeContext && (
                <pre className="comments-code-context">
                  {codeContext.lines.map(line => (
                    <div
                      key={line.num}
                      className={`comments-code-line ${line.num === codeContext.targetLine ? 'highlight' : ''}`}
                    >
                      <span className="comments-code-line-number">{line.num}</span>
                      <span>{line.text}</span>
                    </div>
                  ))}
                </pre>
              )}

              <div className="comments-card-text">{comment.text}</div>

              <div className="comments-card-footer">
                <span className="comments-card-time">
                  {new Date(comment.createdAt).toLocaleString()}
                </span>
                <button
                  className="comments-goto-btn"
                  onClick={() => handleGoToFile(comment)}
                  title="Open file at this line"
                >
                  Go to file
                </button>
              </div>
            </div>
          )
        })}
      </div>

      {prompt && (
        <div className="comments-prompt-preview">
          <div
            className="comments-prompt-header"
            onClick={() => setPromptExpanded(!promptExpanded)}
          >
            <span>{promptExpanded ? '▼' : '▶'} Generated Prompt</span>
            <button
              className="comments-copy-btn"
              onClick={(e) => {
                e.stopPropagation()
                handleCopyPrompt()
              }}
              title="Copy to clipboard"
            >
              Copy to Clipboard
            </button>
          </div>
          {promptExpanded && (
            <pre className="comments-prompt-content">{prompt}</pre>
          )}
        </div>
      )}
    </div>
  )
}
