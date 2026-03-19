import { useState, useCallback, useEffect } from 'react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { WorkspaceState } from '../store/createWorkspaceStore'
import { useGitApi } from '../contexts/GitApiContext'
import { FileTree } from './FileTree'
import { FileViewer } from './FileViewer'
import { CommentDisplay } from './CommentDisplay'
import type { FilesystemState } from '../types'

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
  const { workspaces, updateTabState, addReviewComment, deleteReviewComment, updateOutdatedReviewComments, getReviewComments } = useStore(workspaceStore)
  const git = useGitApi()
  const workspace = workspaces[workspaceId]
  const appState = workspace?.appStates[tabId]
  const state = appState?.state as FilesystemState | undefined

  // Resize state
  const [treeWidth, setTreeWidth] = useState(250)
  const [isResizing, setIsResizing] = useState(false)

  // Reviews state — derived from store
  const allComments = getReviewComments(workspaceId)
  const [commentInput, setCommentInput] = useState<{ lineNumber: number } | null>(null)
  const [currentCommitHash, setCurrentCommitHash] = useState<string | null>(null)
  const [commitHashError, setCommitHashError] = useState<string | null>(null)

  if (!appState || !state) {
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

  // Update outdated comments on mount
  useEffect(() => {
    const updateOutdated = async () => {
      try {
        const hashResult = await git.getHeadCommitHash(workspacePath)
        if (hashResult.success && hashResult.hash) {
          setCurrentCommitHash(hashResult.hash)
          updateOutdatedReviewComments(workspaceId, hashResult.hash)
        }
      } catch (error) {
        console.error('Failed to update outdated comments:', error)
        setCommitHashError(`Failed to get commit hash: ${error instanceof Error ? error.message : 'Unknown error'}`)
      }
    }
    updateOutdated()
  }, [workspacePath])

  // Clear comment input when selected file changes
  useEffect(() => {
    setCommentInput(null)
  }, [state.selectedPath])

  const handleLineClick = (lineNumber: number) => {
    setCommentInput({ lineNumber })
  }

  const handleCommentSubmit = (text: string) => {
    if (!commentInput || !currentCommitHash || !state.selectedPath) return
    addReviewComment(workspaceId, {
      filePath: state.selectedPath,
      lineNumber: commentInput.lineNumber,
      text,
      commitHash: currentCommitHash,
      isOutdated: false,
      addressed: false,
      side: 'modified'
    })
    setCommentInput(null)
  }

  const handleCommentDelete = (commentId: string) => {
    deleteReviewComment(workspaceId, commentId)
  }

  // Filter comments for current file
  const fileComments = state.selectedPath
    ? allComments.filter(c => c.filePath === state.selectedPath)
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
      {commitHashError && (
        <div className="review-load-error">{commitHashError}</div>
      )}
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
          scrollToLine={state.scrollToLine}
          onScrollToLineUsed={() => {
            updateTabState<FilesystemState>(workspaceId, tabId, (s) => ({
              ...s,
              scrollToLine: undefined
            }))
          }}
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
