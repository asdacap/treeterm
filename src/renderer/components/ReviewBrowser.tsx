import { useState, useEffect, useCallback, useRef } from 'react'
import { ChevronDown } from 'lucide-react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { WorkspaceState } from '../store/createWorkspaceStore'
import { useGitApi } from '../contexts/GitApiContext'
import { useReviewsApi } from '../contexts/ReviewsApiContext'
import type { DiffFile, DiffResult, UncommittedFile, UncommittedChanges, ConflictInfo, FileDiffContents, ReviewsData, ReviewComment } from '../types'
import { MonacoDiffViewer } from './MonacoDiffViewer'
import { CommentInput } from './CommentInput'
import { CommentDisplay } from './CommentDisplay'

interface ReviewBrowserProps {
  workspaceId: string
  workspacePath: string
  tabId: string
  // parentWorkspaceId is optional - if undefined, this is a top-level worktree
  // and only uncommitted changes are shown (no merge functionality)
  parentWorkspaceId?: string
  workspaceStore: StoreApi<WorkspaceState>
}

type ViewMode = 'committed' | 'uncommitted'

export default function ReviewBrowser({
  workspaceId,
  workspacePath,
  tabId,
  parentWorkspaceId,
  workspaceStore
}: ReviewBrowserProps) {
  const git = useGitApi()
  const reviewsApi = useReviewsApi()
  const { workspaces, mergeAndRemoveWorkspace, removeWorkspace, removeWorkspaceKeepBranch, removeWorkspaceKeepWorktree, closeAndCleanWorkspace, removeTab } = useStore(workspaceStore)
  const workspace = workspaces[workspaceId]
  const parentWorkspace = parentWorkspaceId ? workspaces[parentWorkspaceId] : undefined
  
  // For top-level worktrees, we only show uncommitted changes (no parent to compare against)
  const hasParent = !!parentWorkspaceId

  // Diff state
  const [diff, setDiff] = useState<DiffResult | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [selectedFile, setSelectedFile] = useState<string | null>(null)
  const [fileDiffContents, setFileDiffContents] = useState<FileDiffContents | null>(null)
  const [loadingFileDiff, setLoadingFileDiff] = useState(false)

  // Uncommitted changes state
  const [uncommitted, setUncommitted] = useState<UncommittedChanges | null>(null)
  // Default to 'committed' view, but for top-level worktrees we only show uncommitted
  const [viewMode, setViewMode] = useState<ViewMode>('committed')
  const [selectedUncommittedFile, setSelectedUncommittedFile] = useState<UncommittedFile | null>(null)

  // Commit state
  const [commitMessage, setCommitMessage] = useState('')
  const [committing, setCommitting] = useState(false)
  const [commitError, setCommitError] = useState<string | null>(null)

  // Conflict state
  const [conflictInfo, setConflictInfo] = useState<ConflictInfo | null>(null)
  const [conflictError, setConflictError] = useState<string | null>(null)
  const [isCheckingConflicts, setIsCheckingConflicts] = useState(false)
  const [loadError, setLoadError] = useState<string | null>(null)

  // Action state
  const [isProcessing, setIsProcessing] = useState(false)
  const [processingAction, setProcessingAction] = useState<'merge' | 'squash' | 'abandon' | null>(null)

  // Reviews state
  const [reviews, setReviews] = useState<ReviewsData | null>(null)
  const [commentInput, setCommentInput] = useState<{
    visible: boolean
    lineNumber: number
    side: 'original' | 'modified'
  } | null>(null)
  const [currentCommitHash, setCurrentCommitHash] = useState<string | null>(null)

  // Resize state
  const [fileListWidth, setFileListWidth] = useState(250)
  const [isResizing, setIsResizing] = useState(false)

  // Abandon dropdown state
  const [abandonMenuOpen, setAbandonMenuOpen] = useState(false)
  const abandonMenuRef = useRef<HTMLDivElement>(null)
  const abandonButtonRef = useRef<HTMLButtonElement>(null)

  // Resize handlers (must be defined before any early returns)
  const handleResizeMouseDown = useCallback(() => {
    setIsResizing(true)
  }, [])

  const handleResizeMouseMove = useCallback(
    (e: React.MouseEvent) => {
      if (!isResizing) return
      const container = e.currentTarget as HTMLElement
      const rect = container.getBoundingClientRect()
      const newWidth = Math.max(150, Math.min(500, e.clientX - rect.left))
      setFileListWidth(newWidth)
    },
    [isResizing]
  )

  const handleResizeMouseUp = useCallback(() => {
    setIsResizing(false)
  }, [])

  useEffect(() => {
    if (!workspace) return
    
    // Always load uncommitted changes
    loadUncommittedChanges()
    loadReviews()
    
    if (parentWorkspace) {
      // For child worktrees: load diff and check conflicts for merge
      loadDiff()
      checkConflicts()
    } else {
      // For top-level worktrees: no diff to show, just clear loading state
      setLoading(false)
    }
  }, [workspace, parentWorkspace])

  // Close abandon dropdown when clicking outside
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (
        abandonMenuOpen &&
        abandonMenuRef.current &&
        !abandonMenuRef.current.contains(e.target as Node) &&
        abandonButtonRef.current &&
        !abandonButtonRef.current.contains(e.target as Node)
      ) {
        setAbandonMenuOpen(false)
      }
    }

    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [abandonMenuOpen])

  const loadReviews = async () => {
    try {
      // Get current commit hash
      const hashResult = await git.getHeadCommitHash(workspacePath)
      if (hashResult.success && hashResult.hash) {
        setCurrentCommitHash(hashResult.hash)

        // Load reviews and mark outdated
        const result = await reviewsApi.updateOutdated(workspacePath, hashResult.hash)
        if (result.success && result.reviews) {
          setReviews(result.reviews)
        }
      }
    } catch (error) {
      console.error('Failed to load reviews:', error)
    }
  }

  const loadDiff = async () => {
    if (!workspace?.gitBranch || !parentWorkspace?.gitBranch) return

    setLoading(true)
    setError(null)
    try {
      const result = await git.getDiff(workspacePath, parentWorkspace.gitBranch)
      if (result.success && result.diff) {
        setDiff(result.diff)
      } else {
        setError(result.error || 'Failed to load diff')
      }
    } catch (err) {
      console.error('[ReviewBrowser] Error loading diff:', err)
      setError(`Failed to load diff: ${err instanceof Error ? err.message : 'Unknown error'}`)
    }
    setLoading(false)
  }

  const loadUncommittedChanges = async () => {
    try {
      const result = await git.getUncommittedChanges(workspacePath)
      if (result.success && result.changes) {
        setUncommitted(result.changes)
      }
    } catch (err) {
      setLoadError(`Failed to load uncommitted changes: ${err instanceof Error ? err.message : 'Unknown error'}`)
    }
  }

  const checkConflicts = async () => {
    if (!workspace?.gitBranch || !parentWorkspace?.gitBranch || !parentWorkspace.gitRootPath) return

    setIsCheckingConflicts(true)
    setConflictError(null)
    try {
      const result = await git.checkMergeConflicts(
        parentWorkspace.gitRootPath,
        workspace.gitBranch,
        parentWorkspace.gitBranch
      )
      if (result.success && result.conflicts) {
        setConflictInfo(result.conflicts)
      } else if (!result.success) {
        setConflictError(result.error || 'Failed to check for conflicts')
      }
    } catch (err) {
      setConflictError(err instanceof Error ? err.message : 'Failed to check for conflicts')
    }
    setIsCheckingConflicts(false)
  }

  const loadFileDiff = async (filePath: string) => {
    if (!parentWorkspace?.gitBranch) return

    setSelectedFile(filePath)
    setSelectedUncommittedFile(null)
    setLoadingFileDiff(true)
    setFileDiffContents(null)
    setLoadError(null)
    try {
      const result = await git.getFileContentsForDiff(workspacePath, parentWorkspace.gitBranch, filePath)
      if (result.success && result.contents) {
        setFileDiffContents(result.contents)
      } else {
        setFileDiffContents(null)
        setLoadError(result.error || 'Failed to load file diff')
      }
    } catch (err) {
      setFileDiffContents(null)
      setLoadError(`Failed to load file diff: ${err instanceof Error ? err.message : 'Unknown error'}`)
    }
    setLoadingFileDiff(false)
  }

  const loadUncommittedFileDiff = async (file: UncommittedFile) => {
    setSelectedUncommittedFile(file)
    setSelectedFile(null)
    setLoadingFileDiff(true)
    setFileDiffContents(null)
    setLoadError(null)
    try {
      const result = await git.getUncommittedFileContentsForDiff(workspacePath, file.path, file.staged)
      if (result.success && result.contents) {
        setFileDiffContents(result.contents)
      } else {
        setFileDiffContents(null)
        setLoadError(result.error || 'Failed to load uncommitted file diff')
      }
    } catch (err) {
      setFileDiffContents(null)
      setLoadError(`Failed to load uncommitted file diff: ${err instanceof Error ? err.message : 'Unknown error'}`)
    }
    setLoadingFileDiff(false)
  }

  const handleStageFile = async (filePath: string) => {
    const result = await git.stageFile(workspacePath, filePath)
    if (result.success) {
      await loadUncommittedChanges()
    }
  }

  const handleUnstageFile = async (filePath: string) => {
    const result = await git.unstageFile(workspacePath, filePath)
    if (result.success) {
      await loadUncommittedChanges()
    }
  }

  const handleStageAll = async () => {
    const result = await git.stageAll(workspacePath)
    if (result.success) {
      await loadUncommittedChanges()
    }
  }

  const handleUnstageAll = async () => {
    const result = await git.unstageAll(workspacePath)
    if (result.success) {
      await loadUncommittedChanges()
    }
  }

  const handleCommit = async () => {
    if (!commitMessage.trim()) {
      setCommitError('Commit message is required')
      return
    }

    setCommitting(true)
    setCommitError(null)

    try {
      const result = await git.commitStaged(workspacePath, commitMessage.trim())
      if (result.success) {
        setCommitMessage('')
        await loadUncommittedChanges()
        await loadDiff()
      } else {
        setCommitError(result.error || 'Failed to commit')
      }
    } catch (err) {
      setCommitError(`Failed to commit: ${err instanceof Error ? err.message : 'Unknown error'}`)
    }

    setCommitting(false)
  }

  const handleMerge = async (squash: boolean) => {
    // Check for uncommitted changes and warn user
    if (hasUncommitted) {
      const fileCount = uncommitted!.files.length
      const confirmed = confirm(
        `You have ${fileCount} uncommitted file${fileCount !== 1 ? 's' : ''}. ` +
        `These changes will be auto-committed before merging. Continue?`
      )
      if (!confirmed) {
        return
      }
    }

    setIsProcessing(true)
    setProcessingAction(squash ? 'squash' : 'merge')

    try {
      const result = await mergeAndRemoveWorkspace(workspaceId, squash)
      if (!result.success) {
        alert(`Merge failed: ${result.error}`)
        setIsProcessing(false)
        setProcessingAction(null)
        return
      }
      // Tab will close automatically when workspace is removed
    } catch (err) {
      alert(`Merge failed: ${err instanceof Error ? err.message : 'Unknown error'}`)
      setIsProcessing(false)
      setProcessingAction(null)
    }
  }

  const handleAbandon = async () => {
    if (!confirm('Are you sure you want to abandon this workspace? All changes will be discarded.')) {
      return
    }

    setIsProcessing(true)
    setProcessingAction('abandon')

    try {
      await removeWorkspace(workspaceId)
      // Tab will close automatically when workspace is removed
    } catch (err) {
      alert(`Abandon failed: ${err instanceof Error ? err.message : 'Unknown error'}`)
      setIsProcessing(false)
      setProcessingAction(null)
    }
  }

  const handleAbandonKeepBranch = async () => {
    if (!confirm('Abandon this workspace but keep the branch? The worktree will be removed but the branch will be kept.')) {
      return
    }

    setIsProcessing(true)
    setProcessingAction('abandon')

    try {
      await removeWorkspaceKeepBranch(workspaceId)
      // Tab will close automatically when workspace is removed
    } catch (err) {
      alert(`Abandon (Keep Branch) failed: ${err instanceof Error ? err.message : 'Unknown error'}`)
      setIsProcessing(false)
      setProcessingAction(null)
    }
  }

  const handleAbandonKeepWorktree = async () => {
    if (!confirm('Abandon this workspace but keep the worktree on disk? The worktree will remain but will no longer be tracked in TreeTerm.')) {
      return
    }

    setIsProcessing(true)
    setProcessingAction('abandon')
    setAbandonMenuOpen(false)

    try {
      await removeWorkspaceKeepWorktree(workspaceId)
      // Tab will close automatically when workspace is removed
    } catch (err) {
      alert(`Abandon (Keep Worktree) failed: ${err instanceof Error ? err.message : 'Unknown error'}`)
      setIsProcessing(false)
      setProcessingAction(null)
    }
  }

  const handleCloseAndClean = async () => {
    if (!confirm('Close this workspace? The worktree will be removed but the branch will be kept.')) {
      return
    }

    setIsProcessing(true)

    try {
      const result = await closeAndCleanWorkspace(workspaceId)
      if (!result.success) {
        alert(`Close failed: ${result.error}`)
        setIsProcessing(false)
        return
      }
      // Tab will close automatically when workspace is removed
    } catch (err) {
      alert(`Close failed: ${err instanceof Error ? err.message : 'Unknown error'}`)
      setIsProcessing(false)
    }
  }

  const handleCancel = () => {
    removeTab(workspaceId, tabId)
  }

  const getStatusIcon = (status: DiffFile['status'] | UncommittedFile['status']) => {
    switch (status) {
      case 'added':
        return <span className="diff-status added">A</span>
      case 'modified':
        return <span className="diff-status modified">M</span>
      case 'deleted':
        return <span className="diff-status deleted">D</span>
      case 'renamed':
        return <span className="diff-status renamed">R</span>
      case 'untracked':
        return <span className="diff-status untracked">?</span>
    }
  }

  if (!workspace) {
    return <div className="review-browser-error">Workspace not found</div>
  }

  const stagedFiles = uncommitted?.files.filter((f) => f.staged) || []
  const unstagedFiles = uncommitted?.files.filter((f) => !f.staged) || []
  const hasUncommitted = uncommitted && uncommitted.files.length > 0
  const hasCommittedChanges = diff && diff.files.length > 0
  const hasConflicts = conflictInfo?.hasConflicts || false

  // File navigation helpers
  const fileList = viewMode === 'committed'
    ? diff?.files.map(f => f.path) || []
    : [...stagedFiles, ...unstagedFiles].map(f => f.path)

  const currentFileIndex = selectedFile
    ? fileList.indexOf(selectedFile)
    : selectedUncommittedFile
      ? fileList.indexOf(selectedUncommittedFile.path)
      : -1

  const handlePreviousFile = () => {
    if (currentFileIndex > 0) {
      const prevFilePath = fileList[currentFileIndex - 1]
      if (viewMode === 'committed') {
        loadFileDiff(prevFilePath)
      } else {
        const prevFile = [...stagedFiles, ...unstagedFiles].find(f => f.path === prevFilePath)
        if (prevFile) loadUncommittedFileDiff(prevFile)
      }
    }
  }

  const handleNextFile = () => {
    if (currentFileIndex < fileList.length - 1) {
      const nextFilePath = fileList[currentFileIndex + 1]
      if (viewMode === 'committed') {
        loadFileDiff(nextFilePath)
      } else {
        const nextFile = [...stagedFiles, ...unstagedFiles].find(f => f.path === nextFilePath)
        if (nextFile) loadUncommittedFileDiff(nextFile)
      }
    }
  }

  // Comment handlers
  const handleLineClick = (lineNumber: number, side: 'original' | 'modified') => {
    setCommentInput({ visible: true, lineNumber, side })
  }

  const handleCommentSubmit = async (text: string) => {
    const filePath = selectedFile || selectedUncommittedFile?.path
    if (!commentInput || !currentCommitHash || !filePath) return

    try {
      const comment = {
        filePath,
        lineNumber: commentInput.lineNumber,
        text,
        commitHash: currentCommitHash,
        isOutdated: false,
        side: commentInput.side
      }

      const result = await reviewsApi.addComment(workspacePath, comment)
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
  const currentFilePath = selectedFile || selectedUncommittedFile?.path
  const fileComments = currentFilePath && reviews
    ? reviews.comments.filter(c => c.filePath === currentFilePath)
    : []

  return (
    <div className="review-browser">
      {/* Header */}
      <div className="review-header">
        <div className="review-header-info">
          <span className="review-workspace-name">{workspace.name}</span>
          <span className="review-branch-info">
            <span className="review-branch">{workspace.gitBranch}</span>
            {parentWorkspace && (
              <>
                <span className="review-arrow">→</span>
                <span className="review-branch">{parentWorkspace.gitBranch}</span>
              </>
            )}
          </span>
        </div>
        {isCheckingConflicts && (
          <span className="review-checking">Checking for conflicts...</span>
        )}
      </div>

      {/* Load Error */}
      {loadError && (
        <div className="review-load-error">{loadError}</div>
      )}

      {/* Conflict Check Error */}
      {conflictError && (
        <div className="review-conflict-error">Could not check for conflicts: {conflictError}</div>
      )}

      {/* Conflict Warning */}
      {hasConflicts && (
        <div className="review-conflict-banner">
          <span className="review-conflict-icon">⚠️</span>
          <div className="review-conflict-text">
            <strong>{conflictInfo?.conflictedFiles.length} conflict(s) detected</strong>
            <div className="review-conflict-files">
              {conflictInfo?.conflictedFiles.slice(0, 3).join(', ')}
              {conflictInfo && conflictInfo.conflictedFiles.length > 3 && ` and ${conflictInfo.conflictedFiles.length - 3} more`}
            </div>
          </div>
        </div>
      )}

      {/* View Mode Tabs */}
      <div className="diff-tabs">
        {hasParent ? (
          <button
            className={`diff-tab ${viewMode === 'committed' ? 'active' : ''}`}
            onClick={() => setViewMode('committed')}
          >
            Committed Changes
            {hasCommittedChanges && (
              <span className="diff-tab-count">{diff.files.length}</span>
            )}
          </button>
        ) : (
          <button
            className="diff-tab disabled"
            disabled
            title="Top-level worktree - no parent branch to compare against"
          >
            Committed Changes
          </button>
        )}
        <button
          className={`diff-tab ${viewMode === 'uncommitted' ? 'active' : ''}`}
          onClick={() => setViewMode('uncommitted')}
        >
          Uncommitted
          {hasUncommitted && (
            <span className="diff-tab-count">{uncommitted.files.length}</span>
          )}
        </button>
      </div>

      {/* Content */}
      {loading ? (
        <div className="review-loading">Loading changes...</div>
      ) : error ? (
        <div className="review-error">{error}</div>
      ) : (
        <>
          {viewMode === 'committed' ? (
            // Committed view
            !hasParent ? (
              <div className="diff-empty">Top-level worktree - no parent branch to compare committed changes against</div>
            ) : !hasCommittedChanges ? (
              <div className="diff-empty">No committed changes to show</div>
            ) : (
              <>
                <div className="diff-summary">
                  <span className="diff-branch">{diff.baseBranch}</span>
                  <span className="diff-arrow">...</span>
                  <span className="diff-branch">{diff.headBranch}</span>
                  <span className="diff-stats">
                    <span className="additions">+{diff.totalAdditions}</span>
                    <span className="deletions">-{diff.totalDeletions}</span>
                  </span>
                </div>

                <div
                  className="diff-content"
                  onMouseMove={handleResizeMouseMove}
                  onMouseUp={handleResizeMouseUp}
                  onMouseLeave={handleResizeMouseUp}
                >
                  <div className="diff-file-list" style={{ width: fileListWidth }}>
                    {diff.files.map((file) => (
                      <div
                        key={file.path}
                        className={`diff-file-item ${selectedFile === file.path ? 'selected' : ''}`}
                        onClick={() => loadFileDiff(file.path)}
                      >
                        {getStatusIcon(file.status)}
                        <span className="diff-file-path">{file.path}</span>
                        <span className="diff-file-stats">
                          <span className="additions">+{file.additions}</span>
                          <span className="deletions">-{file.deletions}</span>
                        </span>
                      </div>
                    ))}
                  </div>

                  <div
                    className={`divider ${isResizing ? 'active' : ''}`}
                    onMouseDown={handleResizeMouseDown}
                  />

                  <div className="diff-file-content">
                    {selectedFile ? (
                      loadingFileDiff ? (
                        <div className="diff-loading">Loading...</div>
                      ) : fileDiffContents ? (
                        <>
                          <MonacoDiffViewer
                            originalContent={fileDiffContents.originalContent}
                            modifiedContent={fileDiffContents.modifiedContent}
                            language={fileDiffContents.language}
                            originalLabel={diff?.baseBranch || 'Original'}
                            modifiedLabel={diff?.headBranch || 'Modified'}
                            onPreviousFile={handlePreviousFile}
                            onNextFile={handleNextFile}
                            hasPreviousFile={currentFileIndex > 0}
                            hasNextFile={currentFileIndex < fileList.length - 1}
                            comments={fileComments}
                            onLineClick={handleLineClick}
                            inlineCommentInput={commentInput}
                            onCommentSubmit={handleCommentSubmit}
                            onCommentCancel={() => setCommentInput(null)}
                          />
                        </>
                      ) : (
                        <div className="diff-placeholder">Failed to load diff contents</div>
                      )
                    ) : (
                      <div className="diff-placeholder">Select a file to view changes</div>
                    )}
                  </div>

                  {selectedFile && fileComments.length > 0 && (
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
              </>
            )
          ) : (
            // Uncommitted view
            !hasUncommitted ? (
              <div className="diff-empty">No uncommitted changes</div>
            ) : (
              <>
                <div className="diff-summary">
                  <span className="diff-branch">Working directory</span>
                  <span className="diff-stats">
                    <span className="additions">+{uncommitted.totalAdditions}</span>
                    <span className="deletions">-{uncommitted.totalDeletions}</span>
                  </span>
                  <div className="diff-actions">
                    <button onClick={handleStageAll} className="diff-action-btn">Stage All</button>
                    <button onClick={handleUnstageAll} className="diff-action-btn">Unstage All</button>
                  </div>
                </div>

                <div
                  className="diff-content"
                  onMouseMove={handleResizeMouseMove}
                  onMouseUp={handleResizeMouseUp}
                  onMouseLeave={handleResizeMouseUp}
                >
                  <div className="diff-file-list" style={{ width: fileListWidth }}>
                    {stagedFiles.length > 0 && (
                      <>
                        <div className="diff-file-section">Staged</div>
                        {stagedFiles.map((file) => (
                          <div
                            key={file.path}
                            className={`diff-file-item ${selectedUncommittedFile?.path === file.path ? 'selected' : ''}`}
                            onClick={() => loadUncommittedFileDiff(file)}
                          >
                            {getStatusIcon(file.status)}
                            <span className="diff-file-path">{file.path}</span>
                            <span className="diff-file-stats">
                              <span className="additions">+{file.additions}</span>
                              <span className="deletions">-{file.deletions}</span>
                            </span>
                            <button
                              className="diff-file-action"
                              onClick={(e) => {
                                e.stopPropagation()
                                handleUnstageFile(file.path)
                              }}
                            >
                              Unstage
                            </button>
                          </div>
                        ))}
                      </>
                    )}
                    {unstagedFiles.length > 0 && (
                      <>
                        <div className="diff-file-section">Unstaged</div>
                        {unstagedFiles.map((file) => (
                          <div
                            key={file.path}
                            className={`diff-file-item ${selectedUncommittedFile?.path === file.path ? 'selected' : ''}`}
                            onClick={() => loadUncommittedFileDiff(file)}
                          >
                            {getStatusIcon(file.status)}
                            <span className="diff-file-path">{file.path}</span>
                            <span className="diff-file-stats">
                              <span className="additions">+{file.additions}</span>
                              <span className="deletions">-{file.deletions}</span>
                            </span>
                            <button
                              className="diff-file-action"
                              onClick={(e) => {
                                e.stopPropagation()
                                handleStageFile(file.path)
                              }}
                            >
                              Stage
                            </button>
                          </div>
                        ))}
                      </>
                    )}
                  </div>

                  <div
                    className={`divider ${isResizing ? 'active' : ''}`}
                    onMouseDown={handleResizeMouseDown}
                  />

                  <div className="diff-file-content">
                    {selectedUncommittedFile ? (
                      loadingFileDiff ? (
                        <div className="diff-loading">Loading...</div>
                      ) : fileDiffContents ? (
                        <MonacoDiffViewer
                          originalContent={fileDiffContents.originalContent}
                          modifiedContent={fileDiffContents.modifiedContent}
                          language={fileDiffContents.language}
                          originalLabel={selectedUncommittedFile.staged ? 'HEAD' : 'Index/HEAD'}
                          modifiedLabel={selectedUncommittedFile.staged ? 'Staged' : 'Working Tree'}
                          onPreviousFile={handlePreviousFile}
                          onNextFile={handleNextFile}
                          hasPreviousFile={currentFileIndex > 0}
                          hasNextFile={currentFileIndex < fileList.length - 1}
                          comments={fileComments}
                          onLineClick={handleLineClick}
                          inlineCommentInput={commentInput}
                          onCommentSubmit={handleCommentSubmit}
                          onCommentCancel={() => setCommentInput(null)}
                        />
                      ) : (
                        <div className="diff-placeholder">Failed to load diff contents</div>
                      )
                    ) : (
                      <div className="diff-placeholder">Select a file to view changes</div>
                    )}
                  </div>
                </div>

                {/* Commit Section */}
                {stagedFiles.length > 0 && (
                  <div className="review-commit-section">
                    <input
                      type="text"
                      className="review-commit-input"
                      placeholder="Commit message..."
                      value={commitMessage}
                      onChange={(e) => setCommitMessage(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter' && !e.shiftKey) {
                          e.preventDefault()
                          handleCommit()
                        }
                      }}
                    />
                    <button
                      className="review-commit-btn"
                      onClick={handleCommit}
                      disabled={committing || !commitMessage.trim()}
                    >
                      {committing ? 'Committing...' : 'Commit'}
                    </button>
                    {commitError && (
                      <div className="review-commit-error">{commitError}</div>
                    )}
                  </div>
                )}
              </>
            )
          )}
        </>
      )}

      {/* Action Bar */}
      <div className="review-actions">
        {!hasParent ? (
          // Top-level worktree: only show Cancel (no merge/abandon actions)
          <>
            <span className="review-top-level-message">Top-level worktree - review only</span>
            <button
              className="review-action-btn review-cancel-btn"
              onClick={handleCancel}
              disabled={isProcessing}
            >
              Close Review
            </button>
          </>
        ) : workspace?.isDetached ? (
          <>
            <button
              className="review-action-btn review-close-and-clean-btn"
              onClick={handleCloseAndClean}
              disabled={isProcessing}
              title="Remove worktree but keep the branch"
            >
              {isProcessing ? 'Closing...' : 'Close and Clean'}
            </button>
            <div className="abandon-dropdown-container">
              <button
                ref={abandonButtonRef}
                className="review-action-btn review-abandon-btn abandon-split-btn"
                onClick={handleAbandon}
                disabled={isProcessing}
                title="Discard all changes and remove this workspace"
              >
                {processingAction === 'abandon' ? 'Abandoning...' : 'Abandon'}
              </button>
              <button
                className="review-action-btn review-abandon-btn abandon-dropdown-btn"
                onClick={() => setAbandonMenuOpen(!abandonMenuOpen)}
                disabled={isProcessing}
                title="More abandon options"
              >
                <ChevronDown size={14} />
              </button>
              {abandonMenuOpen && (
                <div className="abandon-menu" ref={abandonMenuRef}>
                  <div
                    className="abandon-menu-item"
                    onClick={handleAbandon}
                  >
                    Abandon
                    <span className="abandon-menu-hint">Delete worktree and branch</span>
                  </div>
                  <div
                    className="abandon-menu-item"
                    onClick={handleAbandonKeepBranch}
                  >
                    Abandon (Keep Branch)
                    <span className="abandon-menu-hint">Delete worktree, keep branch</span>
                  </div>
                  <div
                    className="abandon-menu-item"
                    onClick={handleAbandonKeepWorktree}
                  >
                    Abandon (Keep Worktree)
                    <span className="abandon-menu-hint">Keep worktree on disk</span>
                  </div>
                </div>
              )}
            </div>
            <button
              className="review-action-btn review-cancel-btn"
              onClick={handleCancel}
              disabled={isProcessing}
            >
              Cancel
            </button>
          </>
        ) : (
          <>
            <button
              className="review-action-btn review-merge-btn"
              onClick={() => handleMerge(false)}
              disabled={isProcessing}
              title={hasConflicts ? 'Merge (conflicts will need to be resolved)' : 'Merge changes into parent branch'}
            >
              {processingAction === 'merge' ? 'Merging...' : 'Merge'}
              {hasConflicts && ' (has conflicts)'}
            </button>
            <button
              className="review-action-btn review-squash-btn"
              onClick={() => handleMerge(true)}
              disabled={isProcessing}
              title={hasConflicts ? 'Squash merge (conflicts will need to be resolved)' : 'Squash all commits into one'}
            >
              {processingAction === 'squash' ? 'Squashing...' : 'Squash Merge'}
              {hasConflicts && ' (has conflicts)'}
            </button>
            <div className="abandon-dropdown-container">
              <button
                ref={abandonButtonRef}
                className="review-action-btn review-abandon-btn abandon-split-btn"
                onClick={handleAbandon}
                disabled={isProcessing}
                title="Discard all changes and remove this workspace"
              >
                {processingAction === 'abandon' ? 'Abandoning...' : 'Abandon'}
              </button>
              <button
                className="review-action-btn review-abandon-btn abandon-dropdown-btn"
                onClick={() => setAbandonMenuOpen(!abandonMenuOpen)}
                disabled={isProcessing}
                title="More abandon options"
              >
                <ChevronDown size={14} />
              </button>
              {abandonMenuOpen && (
                <div className="abandon-menu" ref={abandonMenuRef}>
                  <div
                    className="abandon-menu-item"
                    onClick={handleAbandon}
                  >
                    Abandon
                    <span className="abandon-menu-hint">Delete worktree and branch</span>
                  </div>
                  <div
                    className="abandon-menu-item"
                    onClick={handleAbandonKeepBranch}
                  >
                    Abandon (Keep Branch)
                    <span className="abandon-menu-hint">Delete worktree, keep branch</span>
                  </div>
                  <div
                    className="abandon-menu-item"
                    onClick={handleAbandonKeepWorktree}
                  >
                    Abandon (Keep Worktree)
                    <span className="abandon-menu-hint">Keep worktree on disk</span>
                  </div>
                </div>
              )}
            </div>
            <button
              className="review-action-btn review-cancel-btn"
              onClick={handleCancel}
              disabled={isProcessing}
            >
              Cancel
            </button>
          </>
        )}
      </div>
    </div>
  )
}
