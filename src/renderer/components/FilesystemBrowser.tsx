import React, { useState, useCallback, useEffect } from 'react'
import { useStore } from 'zustand'
import { FileTree } from './FileTree'
import { FileViewer } from './FileViewer'
import type { FilesystemState, WorkspaceStore } from '../types'
import { useGitApi } from '../hooks/useWorkspaceApis'

interface FilesystemBrowserProps {
  workspace: WorkspaceStore
  tabId: string
}

export function FilesystemBrowser({
  workspace,
  tabId,
}: FilesystemBrowserProps): React.JSX.Element {
  const wsData = useStore(workspace, s => s.workspace)
  // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- tabId guaranteed to exist in appStates
  const appState = wsData.appStates[tabId]!
  const state = appState.state as FilesystemState | undefined

  if (!state) {
    return <div className="filesystem-browser-error">Invalid tab</div>
  }

  return <FilesystemBrowserContent workspace={workspace} tabId={tabId} state={state} />
}

function FilesystemBrowserContent({
  workspace,
  tabId,
  state,
}: {
  workspace: WorkspaceStore
  tabId: string
  state: FilesystemState
}): React.JSX.Element {
  const wsData = useStore(workspace, s => s.workspace)
  const updateTabState = useStore(workspace, s => s.updateTabState)
  const reviewCommentStore = useStore(workspace, s => s.reviewComments)
  const git = useGitApi(workspace)
  const getReviewComments = useStore(reviewCommentStore, s => s.getReviewComments)
  const addReviewComment = useStore(reviewCommentStore, s => s.addReviewComment)
  const deleteReviewComment = useStore(reviewCommentStore, s => s.deleteReviewComment)
  const updateOutdatedReviewComments = useStore(reviewCommentStore, s => s.updateOutdatedReviewComments)
  const workspacePath = wsData.path

  // Resize state
  const [treeWidth, setTreeWidth] = useState(250)
  const [isResizing, setIsResizing] = useState(false)

  // Reviews state — derived from store
  const allComments = getReviewComments()
  const [commentInput, setCommentInput] = useState<{ lineNumber: number } | null>(null)
  const [currentCommitHash, setCurrentCommitHash] = useState<string | null>(null)

  const setSelectedPath = (path: string | null) => {
    updateTabState<FilesystemState>(tabId, (s) => ({
      ...s,
      selectedPath: path,
      scrollTop: undefined
    }))
  }

  const handleScrollPositionChange = useCallback((scrollTop: number) => {
    updateTabState<FilesystemState>(tabId, (s) => ({
      ...s,
      scrollTop
    }))
  }, [tabId, updateTabState])

  const toggleExpandedDir = (dirPath: string) => {
    updateTabState<FilesystemState>(tabId, (s) => {
      const isExpanded = s.expandedDirs.includes(dirPath)
      return {
        ...s,
        expandedDirs: isExpanded
          ? s.expandedDirs.filter((d) => d !== dirPath)
          : [...s.expandedDirs, dirPath]
      }
    })
  }

  // Update outdated comments on mount (git repos only)
  useEffect(() => {
    if (!wsData.isGitRepo) return
    const updateOutdated = async () => {
      const hashResult = await git.getHeadCommitHash()
      if (hashResult.success && hashResult.hash) {
        setCurrentCommitHash(hashResult.hash)
        updateOutdatedReviewComments(hashResult.hash)
      }
    }
    void updateOutdated()
  }, [workspacePath, git, updateOutdatedReviewComments, wsData.isGitRepo])

  // Clear comment input when selected file changes
  const [prevSelectedPath, setPrevSelectedPath] = useState(state.selectedPath)
  if (state.selectedPath !== prevSelectedPath) {
    setPrevSelectedPath(state.selectedPath)
    setCommentInput(null)
  }

  const handleLineClick = (lineNumber: number) => {
    setCommentInput({ lineNumber })
  }

  const handleCommentSubmit = (text: string) => {
    if (!commentInput || !state.selectedPath) return
    addReviewComment({
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
    deleteReviewComment(commentId)
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
      <div style={{ width: treeWidth }}>
        <FileTree
          workspace={workspace}
          selectedPath={state.selectedPath}
          expandedDirs={state.expandedDirs}
          onSelectFile={setSelectedPath}
          onToggleDir={toggleExpandedDir}
        />
      </div>
      <div className={`divider ${isResizing ? 'active' : ''}`} onMouseDown={handleMouseDown} />
      <div className="filesystem-browser-content">
        <FileViewer
          workspace={workspace}
          filePath={state.selectedPath}
          comments={fileComments}
          onLineClick={handleLineClick}
          inlineCommentInput={commentInput}
          onCommentSubmit={handleCommentSubmit}
          onCommentCancel={() => { setCommentInput(null); }}
          onCommentDelete={handleCommentDelete}
          scrollToLine={state.scrollToLine}
          onScrollToLineUsed={() => {
            updateTabState<FilesystemState>(tabId, (s) => ({
              ...s,
              scrollToLine: undefined
            }))
          }}
          initialScrollTop={state.scrollTop}
          onScrollPositionChange={handleScrollPositionChange}
        />
      </div>
    </div>
  )
}
