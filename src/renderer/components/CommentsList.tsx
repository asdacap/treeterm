import React, { useState, useEffect } from 'react'
import { useStore } from 'zustand'
import { useAppStore } from '../store/app'
import { generateReviewPrompt, buildPromptForComments } from '../utils/reviewPrompt'
import type { ReviewComment, FilesystemState, WorkspaceStore } from '../types'
import { useFilesystemApi } from '../hooks/useWorkspaceApis'

interface CommentsListProps {
  workspace: WorkspaceStore
}

const CONTEXT_LINES = 3

enum CommentFilter {
  Unprompted = 'unprompted',
  Prompted = 'prompted',
}

const FILTER_PREDICATES: Record<CommentFilter, (c: ReviewComment) => boolean> = {
  [CommentFilter.Unprompted]: (c) => !c.addressed,
  [CommentFilter.Prompted]: (c) => c.addressed,
}

enum PushStatus {
  Idle = 'idle',
  Loading = 'loading',
  Result = 'result',
}

type PushState =
  | { status: PushStatus.Idle }
  | { status: PushStatus.Loading }
  | { status: PushStatus.Result; message: string }

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
  workspace,
}: CommentsListProps): React.JSX.Element {
  const wsData = useStore(workspace, s => s.workspace)
  const reviewCommentStore = useStore(workspace, s => s.reviewComments)
  const gitController = useStore(workspace, s => s.gitController)
  const addTab = useStore(workspace, s => s.addTab)
  const promptHarness = useStore(workspace, s => s.promptHarness)
  const getReviewComments = useStore(reviewCommentStore, s => s.getReviewComments)
  const markReviewCommentsAddressed = useStore(reviewCommentStore, s => s.markReviewCommentsAddressed)
  const deleteReviewComment = useStore(reviewCommentStore, s => s.deleteReviewComment)
  const pushReviewCommentsToGitHub = useStore(gitController, s => s.pushReviewCommentsToGitHub)
  const clipboard = useAppStore((state) => state.clipboard)
  const filesystem = useFilesystemApi(workspace)
  const comments: ReviewComment[] = getReviewComments()
  const [fileContents, setFileContents] = useState(new Map<string, string>())
  const [promptExpanded, setPromptExpanded] = useState(false)
  const [filter, setFilter] = useState<CommentFilter>(CommentFilter.Unprompted)
  const [pushState, setPushState] = useState<PushState>({ status: PushStatus.Idle })

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
            const result = await filesystem.readFile(filePath)
            if (result.success) {
              newContents.set(filePath, result.file.content)
            }
          } catch {
            // Skip files that fail to load
          }
        })
      )
      setFileContents(newContents)
    }

    void fetchFiles()
  }, [comments, wsData.path, fileContents, filesystem])

  const handleDelete = (commentId: string) => {
    deleteReviewComment(commentId)
  }

  const handleGoToFile = (comment: ReviewComment) => {
    addTab<FilesystemState>('filesystem', {
      selectedPath: comment.filePath,
      scrollToLine: comment.lineNumber
    })
  }

  const handleReprompt = async (comment: ReviewComment) => {
    const sent = await promptHarness(buildPromptForComments([comment]))
    if (sent) {
      markReviewCommentsAddressed([comment.id])
    }
  }

  const handleCopyPrompt = () => {
    const prompt = generateReviewPrompt(comments)
    if (prompt) {
      clipboard.writeText(prompt)
    }
  }

  const unprompted = comments.filter(c => !c.addressed)

  const handlePush = async () => {
    if (unprompted.length === 0) return
    setPushState({ status: PushStatus.Loading })
    const result = await pushReviewCommentsToGitHub(unprompted)
    if ('error' in result) {
      setPushState({ status: PushStatus.Result, message: `Push failed: ${result.error}` })
      return
    }
    const failedIds = new Set(result.failed.map(f => f.id))
    const postedIds = unprompted.filter(c => !failedIds.has(c.id)).map(c => c.id)
    markReviewCommentsAddressed(postedIds)
    const message = result.failed.length === 0
      ? `Pushed ${String(result.posted)} comment(s) to GitHub`
      : `Pushed ${String(result.posted)}, ${String(result.failed.length)} failed`
    setPushState({ status: PushStatus.Result, message })
  }

  if (comments.length === 0) {
    return <div className="comments-list"><div className="comments-empty">No review comments yet</div></div>
  }

  const promptedCount = comments.length - unprompted.length
  const filtered = comments.filter(FILTER_PREDICATES[filter])
  const prompt = generateReviewPrompt(comments)

  return (
    <div className="comments-list">
      <div className="comments-header">
        <div className="comments-filter">
          <button
            className={`comments-filter-tab ${filter === CommentFilter.Unprompted ? 'active' : ''}`}
            onClick={() => { setFilter(CommentFilter.Unprompted); }}
          >
            Unprompted ({unprompted.length})
          </button>
          <button
            className={`comments-filter-tab ${filter === CommentFilter.Prompted ? 'active' : ''}`}
            onClick={() => { setFilter(CommentFilter.Prompted); }}
          >
            Prompted ({promptedCount})
          </button>
        </div>
        <button
          className="comments-push-btn"
          onClick={() => { void handlePush(); }}
          disabled={unprompted.length === 0 || pushState.status === PushStatus.Loading}
          title="Push unprompted comments to GitHub as inline PR review comments"
        >
          {pushState.status === PushStatus.Loading ? 'Pushing…' : `Push to GitHub (${String(unprompted.length)})`}
        </button>
      </div>

      {pushState.status === PushStatus.Result && (
        <div className="comments-push-status">{pushState.message}</div>
      )}

      <div className="comments-cards">
        {filtered.length === 0 && (
          <div className="comments-empty">No {filter} comments</div>
        )}
        {filtered.map(comment => {
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
                <button
                  className="comment-delete-btn"
                  onClick={() => { handleDelete(comment.id); }}
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
                  className="comments-reprompt-btn"
                  onClick={() => { void handleReprompt(comment); }}
                  title={comment.addressed ? 'Re-send this comment to the AI harness' : 'Send this comment to the AI harness'}
                >
                  {comment.addressed ? 'Re-prompt' : 'Prompt'}
                </button>
                <button
                  className="comments-goto-btn"
                  onClick={() => { handleGoToFile(comment); }}
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
            onClick={() => { setPromptExpanded(!promptExpanded); }}
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
