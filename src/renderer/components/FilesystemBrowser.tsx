import { useState, useCallback, useEffect } from 'react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { WorkspaceState } from '../store/createWorkspaceStore'
import { useGitApi } from '../contexts/GitApiContext'
import { useReviewsApi } from '../contexts/ReviewsApiContext'
import { FileTree } from './FileTree'
import { FileViewer } from './FileViewer'
import { CommentDisplay } from './CommentDisplay'
import type { FilesystemState, ReviewsData } from '../types'

interface FilesystemBrowserProps {
  workspacePath: string
  workspaceId: string
  tabId: string
  workspaceStore: StoreApi<WorkspaceState>
}

export function FilesystemBrowser({
  workspacePath,
  workspaceId,
  tabId,
  workspaceStore
}: FilesystemBrowserProps): JSX.Element {
  const { workspaces, updateTabState } = useStore(workspaceStore)
  const git = useGitApi()
  const reviewsApi = useReviewsApi()
  const workspace = workspaces[workspaceId]
  const tab = workspace?.tabs.find((t) => t.id === tabId)
  const state = tab?.state as FilesystemState | undefined

  // Resize state
  const [treeWidth, setTreeWidth] = useState(250)
  const [isResizing, setIsResizing] = useState(false)

  // Reviews state
  const [reviews, setReviews] = useState<ReviewsData | null>(null)
  const [commentInput, setCommentInput] = useState<{ lineNumber: number } | null>(null)
  const [currentCommitHash, setCurrentCommitHash] = useState<string | null>(null)

  if (!tab || !state) {
    return <div className="filesystem-browser-error">Invalid tab</div>
  }

  const setSelectedPath = (path: string | null) => {
    updateTabState<FilesystemState>(workspaceId, tabId, (s) => ({
      ...s,
      selectedPath: path
    }))
  }

  const toggleExpandedDir = (dirPath: string) => {
    updateTabState<FilesystemState>(workspaceId, tabId, (s) => {
      const isExpanded = s.expandedDirs.includes(dirPath)
      return {
        ...s,
        expandedDirs: isExpanded
          ? s.expandedDirs.filter((d) => d !== dirPath)
          : [...s.expandedDirs, dirPath]
      }
    })
  }

  // Load reviews on mount
  useEffect(() => {
    loadReviews()
  }, [workspacePath])

  // Clear comment input when selected file changes
  useEffect(() => {
    setCommentInput(null)
  }, [state.selectedPath])

  const loadReviews = async () => {
    try {
      const hashResult = await git.getHeadCommitHash(workspacePath)
      if (hashResult.success && hashResult.hash) {
        setCurrentCommitHash(hashResult.hash)

        const result = await reviewsApi.updateOutdated(workspacePath, hashResult.hash)
        if (result.success && result.reviews) {
          setReviews(result.reviews)
        }
      }
    } catch (error) {
      console.error('Failed to load reviews:', error)
    }
  }

  const handleLineClick = (lineNumber: number) => {
    setCommentInput({ lineNumber })
  }

  const handleCommentSubmit = async (text: string) => {
    if (!commentInput || !currentCommitHash || !state.selectedPath) return

    try {
      const result = await reviewsApi.addComment(workspacePath, {
        filePath: state.selectedPath,
        lineNumber: commentInput.lineNumber,
        text,
        commitHash: currentCommitHash,
        isOutdated: false,
        side: 'modified'
      })
      if (result.success && result.comment) {
        setReviews(prev => prev ? {
          ...prev,
          comments: [...prev.comments, result.comment!]
        } : { version: 1, comments: [result.comment!] })
      }
      setCommentInput(null)
    } catch (error) {
      console.error('Failed to add comment:', error)
      alert(`Failed to add comment: ${error instanceof Error ? error.message : 'Unknown error'}`)
    }
  }

  const handleCommentDelete = async (commentId: string) => {
    try {
      const result = await reviewsApi.deleteComment(workspacePath, commentId)
      if (result.success) {
        setReviews(prev => prev ? {
          ...prev,
          comments: prev.comments.filter(c => c.id !== commentId)
        } : null)
      }
    } catch (error) {
      console.error('Failed to delete comment:', error)
      alert(`Failed to delete comment: ${error instanceof Error ? error.message : 'Unknown error'}`)
    }
  }

  // Filter comments for current file
  const fileComments = state.selectedPath && reviews
    ? reviews.comments.filter(c => c.filePath === state.selectedPath)
    : []

  // Resize handlers
  const handleMouseDown = useCallback(() => {
    setIsResizing(true)
  }, [])

  const handleMouseMove = useCallback(
    (e: React.MouseEvent) => {
      if (!isResizing) return
      const container = e.currentTarget as HTMLElement
      const rect = container.getBoundingClientRect()
      const newWidth = Math.max(150, Math.min(500, e.clientX - rect.left))
      setTreeWidth(newWidth)
    },
    [isResizing]
  )

  const handleMouseUp = useCallback(() => {
    setIsResizing(false)
  }, [])

  return (
    <div className="filesystem-browser" onMouseMove={handleMouseMove} onMouseUp={handleMouseUp} onMouseLeave={handleMouseUp}>
      <div style={{ width: treeWidth }}>
        <FileTree
          workspacePath={workspacePath}
          selectedPath={state.selectedPath}
          expandedDirs={state.expandedDirs}
          onSelectFile={setSelectedPath}
          onToggleDir={toggleExpandedDir}
        />
      </div>
      <div className={`divider ${isResizing ? 'active' : ''}`} onMouseDown={handleMouseDown} />
      <div className="filesystem-browser-content">
        <FileViewer
          workspacePath={workspacePath}
          workspaceId={workspaceId}
          filePath={state.selectedPath}
          workspaceStore={workspaceStore}
          comments={fileComments}
          onLineClick={handleLineClick}
          inlineCommentInput={commentInput}
          onCommentSubmit={handleCommentSubmit}
          onCommentCancel={() => setCommentInput(null)}
        />
        {state.selectedPath && fileComments.length > 0 && (
          <div className="diff-comments-panel">
            <div className="diff-comments-header">
              Comments ({fileComments.length})
            </div>
            <div className="diff-comments-list">
              {fileComments.map((comment) => (
                <CommentDisplay
                  key={comment.id}
                  comment={comment}
                  onDelete={handleCommentDelete}
                />
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
