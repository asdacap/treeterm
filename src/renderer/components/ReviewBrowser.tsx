import React, { useState, useEffect, useCallback, useRef } from 'react'
import { ChevronDown, RefreshCw, Loader2 } from 'lucide-react'
import { useStore } from 'zustand'
import { findRunningHarness } from '../utils/findRunningHarnessPtyId'
import { getTabs } from '../types'
import type { DiffFile, DiffResult, UncommittedFile, UncommittedChanges, ConflictInfo, FileDiffContents, GitLogCommit, WorkspaceStore, ReviewState } from '../types'
import { useGitApi } from '../hooks/useWorkspaceApis'
import { MonacoDiffViewer } from './MonacoDiffViewer'
import { CommittedDiffFileTree, UncommittedDiffFileTree, getSortedFilePaths } from './DiffFileTree'

interface ReviewBrowserProps {
  workspace: WorkspaceStore
  tabId: string
  // parentWorkspaceId is optional - if undefined, this is a top-level worktree
  // and only uncommitted changes are shown (no merge functionality)
  parentWorkspaceId?: string
  isVisible: boolean
}

type ViewMode = 'committed' | 'uncommitted' | 'commits'

export default function ReviewBrowser({
  workspace,
  tabId,
  parentWorkspaceId,
  isVisible,
}: ReviewBrowserProps) {
  const {
    workspace: wsData, lookupWorkspace,
    promptHarness, mergeAndRemove, mergeAndKeep, closeAndClean, removeTab,
    reviewComments: reviewCommentStore, gitController, updateTabState,
  } = useStore(workspace)
  const { getReviewComments, addReviewComment, deleteReviewComment, updateOutdatedReviewComments } = useStore(reviewCommentStore)
  const { refreshDiffStatus } = useStore(gitController)
  const git = useGitApi(workspace)
  const workspaceId = wsData.id
  const parentWorkspace = parentWorkspaceId ? lookupWorkspace(parentWorkspaceId) : undefined
  // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- tabId guaranteed to exist in appStates
  const reviewState = wsData.appStates[tabId]!.state as ReviewState | undefined

  // For top-level worktrees, we only show uncommitted changes (no parent to compare against)
  const hasParent = !!parentWorkspaceId

  // Diff state
  const [diff, setDiff] = useState<DiffResult | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [selectedFile, setSelectedFile] = useState(reviewState?.selectedFilePath ?? null)
  const [fileDiffContents, setFileDiffContents] = useState<FileDiffContents | null>(null)
  const [loadingFileDiff, setLoadingFileDiff] = useState(false)

  // Uncommitted changes state
  const [uncommitted, setUncommitted] = useState<UncommittedChanges | null>(null)
  const [viewMode, setViewMode] = useState<ViewMode>(reviewState?.viewMode ?? (hasParent ? 'committed' : 'uncommitted'))
  const [selectedUncommittedFile, setSelectedUncommittedFile] = useState<UncommittedFile | null>(null)

  // Staging state
  const [stagingInProgress, setStagingInProgress] = useState(false)
  const [stageError, setStageError] = useState<string | null>(null)

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
  const [processingAction, setProcessingAction] = useState<'merge' | 'squash' | 'merge-keep' | 'squash-keep' | null>(null)
  const [mergeDropdownOpen, setMergeDropdownOpen] = useState(false)

  // Reviews state
  const reviews = getReviewComments()
  const [commentInput, setCommentInput] = useState<{
    visible: boolean
    lineNumber: number
    side: 'original' | 'modified'
  } | null>(null)
  const [currentCommitHash, setCurrentCommitHash] = useState<string | null>(null)

  // Resize state
  const [fileListWidth, setFileListWidth] = useState(250)
  const [isResizing, setIsResizing] = useState(false)

  // Commits state
  const [commits, setCommits] = useState<GitLogCommit[]>([])
  const [commitsLoading, setCommitsLoading] = useState(false)
  const [commitsError, setCommitsError] = useState<string | null>(null)
  const [commitsHasMore, setCommitsHasMore] = useState(false)
  const [selectedCommit, setSelectedCommit] = useState<GitLogCommit | null>(null)
  const [commitDiffFiles, setCommitDiffFiles] = useState<DiffFile[]>([])
  const [commitDiffLoading, setCommitDiffLoading] = useState(false)
  const [selectedCommitFile, setSelectedCommitFile] = useState<string | null>(null)

  // AI harness prompt support
  const runningHarness = findRunningHarness(getTabs(wsData))

  const [refreshing, setRefreshing] = useState(false)

  // Persist view state to tab state for restoration across workspace switches
  const persistViewState = useCallback((updates: Partial<ReviewState>) => {
    updateTabState<ReviewState>(tabId, (s) => ({ ...s, ...updates }))
  }, [tabId, updateTabState])

  const handleScrollPositionChange = useCallback((scrollTop: number) => {
    persistViewState({ scrollTop })
  }, [persistViewState])

  const handleRefresh = useCallback(async () => {
    setRefreshing(true)
    try {
      const h = helpersRef.current
      await Promise.all([
        h.loadUncommittedChanges(),
        h.loadReviews(),
        ...(hasParent ? [h.loadDiff(), h.checkConflicts()] : []),
      ])
      void refreshDiffStatus()
    } finally {
      setRefreshing(false)
    }
  }, [hasParent, refreshDiffStatus])

  // Auto-refresh when tab becomes visible (e.g., switching back from terminal)
  const wasVisibleRef = useRef<boolean | null>(null)
  useEffect(() => {
    if (wasVisibleRef.current === false && isVisible) {
      void handleRefresh()
    }
    wasVisibleRef.current = isVisible
  }, [isVisible, handleRefresh])

  const handlePromptCommit = useCallback(() => {
    void promptHarness('commit')
  }, [promptHarness])

  const handlePromptRebase = useCallback(() => {
    if (parentWorkspace?.gitBranch) {
      void promptHarness(`rebase local branch ${wsData.gitBranch ?? ''} onto ${parentWorkspace.gitBranch}`)
    }
  }, [promptHarness, wsData.gitBranch, parentWorkspace?.gitBranch])

  // Resize handlers
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

  // Track whether we need to restore persisted file selection after initial data load
  const pendingRestoreRef = useRef(
    !!(reviewState?.selectedFilePath || reviewState?.selectedUncommittedFilePath)
  )
  // Capture initial viewMode for mount-only commits loading (tab click handler loads on interaction)
  const [initialViewMode] = useState(viewMode)

  // Use stable primitives instead of object references to avoid re-running
  // on every workspace store update (tab switches, scroll persistence, etc.)
  const currentGitBranch = wsData.gitBranch
  const parentGitBranch = parentWorkspace?.gitBranch

  useEffect(() => {
    const h = helpersRef.current
    void h.loadUncommittedChanges()
    void h.loadReviews()

    if (parentWorkspaceId) {
      void h.loadDiff()
      void h.checkConflicts()
    } else {
      setLoading(false)
    }

    // If commits tab was persisted, load commits on mount
    if (initialViewMode === 'commits') {
      void h.loadCommits()
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps -- initialViewMode is intentionally excluded: it captures the mount-time value and should not re-trigger the effect
  }, [workspaceId, parentWorkspaceId, currentGitBranch, parentGitBranch])

  // Auto-restore persisted file selection after initial data load
  useEffect(() => {
    if (!pendingRestoreRef.current) return

    if (viewMode === 'committed' && diff && reviewState?.selectedFilePath) {
      const fileExists = diff.files.some(f => f.path === reviewState.selectedFilePath)
      if (fileExists) {
        pendingRestoreRef.current = false
        void helpersRef.current.loadFileDiff(reviewState.selectedFilePath)
      }
    }
  }, [diff, viewMode, reviewState?.selectedFilePath])

  useEffect(() => {
    if (!pendingRestoreRef.current) return

    if (viewMode === 'uncommitted' && uncommitted && reviewState?.selectedUncommittedFilePath) {
      const file = uncommitted.files.find(f => f.path === reviewState.selectedUncommittedFilePath)
      if (file) {
        pendingRestoreRef.current = false
        void helpersRef.current.loadUncommittedFileDiff(file)
      }
    }
  }, [uncommitted, viewMode, reviewState?.selectedUncommittedFilePath])

  const loadReviews = async () => {
    try {
      const hashResult = await git.getHeadCommitHash()
      if (hashResult.success && hashResult.hash) {
        setCurrentCommitHash(hashResult.hash)
        updateOutdatedReviewComments(hashResult.hash)
      }
    } catch (error) {
      console.error('Failed to load reviews:', error)
      setLoadError(`Failed to load review state: ${error instanceof Error ? error.message : 'Unknown error'}`)
    }
  }

  const loadDiff = async () => {
    if (!wsData.gitBranch || !parentWorkspace?.gitBranch) return

    if (!diff) {
      setLoading(true)
    }
    setError(null)
    try {
      const result = await git.getDiff(parentWorkspace.gitBranch)
      if (result.success) {
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
      const result = await git.getUncommittedChanges()
      if (result.success) {
        setUncommitted(result.changes)
      }
    } catch (err) {
      setLoadError(`Failed to load uncommitted changes: ${err instanceof Error ? err.message : 'Unknown error'}`)
    }
  }

  const loadCommits = async (skip: number = 0) => {
    setCommitsLoading(true)
    setCommitsError(null)
    try {
      const parentBranch = parentWorkspace?.gitBranch || null
      const result = await git.getLog(parentBranch, skip, 50)
      if (result.success) {
        setCommits(prev => skip === 0 ? result.result.commits : [...prev, ...result.result.commits])
        setCommitsHasMore(result.result.hasMore)
      } else {
        setCommitsError(result.error || 'Failed to load commits')
      }
    } catch (err) {
      setCommitsError(`Failed to load commits: ${err instanceof Error ? err.message : 'Unknown error'}`)
    }
    setCommitsLoading(false)
  }

  const loadCommitDiff = async (commit: GitLogCommit) => {
    setSelectedCommit(commit)
    setCommitDiffFiles([])
    setSelectedCommitFile(null)
    setFileDiffContents(null)
    setCommitDiffLoading(true)
    try {
      const result = await git.getCommitDiff(commit.hash)
      if (result.success) {
        setCommitDiffFiles(result.files)
      }
    } catch (err) {
      setLoadError(`Failed to load commit diff: ${err instanceof Error ? err.message : 'Unknown error'}`)
    }
    setCommitDiffLoading(false)
  }

  const loadCommitFileDiff = async (commitHash: string, filePath: string) => {
    setSelectedCommitFile(filePath)
    setLoadingFileDiff(true)
    setFileDiffContents(null)
    setLoadError(null)
    try {
      const result = await git.getCommitFileDiff(commitHash, filePath)
      if (result.success) {
        setFileDiffContents(result.contents)
      } else {
        setLoadError(result.error || 'Failed to load commit file diff')
      }
    } catch (err) {
      setLoadError(`Failed to load commit file diff: ${err instanceof Error ? err.message : 'Unknown error'}`)
    }
    setLoadingFileDiff(false)
  }

  const checkConflicts = async () => {
    if (!wsData.gitBranch || !parentWorkspace?.gitBranch || !parentWorkspace.gitRootPath) return

    setIsCheckingConflicts(true)
    setConflictError(null)
    try {
      const result = await git.checkMergeConflicts(
        wsData.gitBranch,
        parentWorkspace.gitBranch
      )
      if (result.success) {
        setConflictInfo(result.conflicts)
      } else {
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
    // Don't clear scrollTop during restore — only on user-initiated file selection
    if (!pendingRestoreRef.current) {
      persistViewState({ selectedFilePath: filePath, selectedUncommittedFilePath: undefined, scrollTop: undefined })
    }
    setLoadingFileDiff(true)
    setFileDiffContents(null)
    setLoadError(null)
    try {
      const result = await git.getFileContentsForDiff(parentWorkspace.gitBranch, filePath)
      if (result.success) {
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
    if (!pendingRestoreRef.current) {
      persistViewState({ selectedUncommittedFilePath: file.path, selectedFilePath: undefined, scrollTop: undefined })
    }
    setLoadingFileDiff(true)
    setFileDiffContents(null)
    setLoadError(null)
    try {
      const result = await git.getUncommittedFileContentsForDiff(file.path, file.staged)
      if (result.success) {
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

  const helpersRef = useRef({ loadDiff, loadReviews, loadUncommittedChanges, checkConflicts, loadCommits, loadFileDiff, loadUncommittedFileDiff })
  helpersRef.current = { loadDiff, loadReviews, loadUncommittedChanges, checkConflicts, loadCommits, loadFileDiff, loadUncommittedFileDiff }

  const handleStageFile = async (filePath: string) => {
    setStagingInProgress(true)
    setStageError(null)
    try {
      const result = await git.stageFile(filePath)
      if (result.success) {
        await loadUncommittedChanges()
        void refreshDiffStatus()
      } else {
        setStageError(result.error || `Failed to stage ${filePath}`)
      }
    } catch (err) {
      setStageError(`Failed to stage ${filePath}: ${err instanceof Error ? err.message : 'Unknown error'}`)
    }
    setStagingInProgress(false)
  }

  const handleUnstageFile = async (filePath: string) => {
    setStagingInProgress(true)
    setStageError(null)
    try {
      const result = await git.unstageFile(filePath)
      if (result.success) {
        await loadUncommittedChanges()
        void refreshDiffStatus()
      } else {
        setStageError(result.error || `Failed to unstage ${filePath}`)
      }
    } catch (err) {
      setStageError(`Failed to unstage ${filePath}: ${err instanceof Error ? err.message : 'Unknown error'}`)
    }
    setStagingInProgress(false)
  }

  const handleStageAll = async () => {
    setStagingInProgress(true)
    setStageError(null)
    try {
      const result = await git.stageAll()
      if (result.success) {
        await loadUncommittedChanges()
        void refreshDiffStatus()
      } else {
        setStageError(result.error || 'Failed to stage all files')
      }
    } catch (err) {
      setStageError(`Failed to stage all files: ${err instanceof Error ? err.message : 'Unknown error'}`)
    }
    setStagingInProgress(false)
  }

  const handleUnstageAll = async () => {
    setStagingInProgress(true)
    setStageError(null)
    try {
      const result = await git.unstageAll()
      if (result.success) {
        await loadUncommittedChanges()
        void refreshDiffStatus()
      } else {
        setStageError(result.error || 'Failed to unstage all files')
      }
    } catch (err) {
      setStageError(`Failed to unstage all files: ${err instanceof Error ? err.message : 'Unknown error'}`)
    }
    setStagingInProgress(false)
  }

  const handleCommit = async () => {
    if (!commitMessage.trim()) {
      setCommitError('Commit message is required')
      return
    }

    setCommitting(true)
    setCommitError(null)

    try {
      const result = await git.commitStaged(commitMessage.trim())
      if (result.success) {
        setCommitMessage('')
        await loadUncommittedChanges()
        await loadDiff()
        void refreshDiffStatus()
      } else {
        setCommitError(result.error || 'Failed to commit')
      }
    } catch (err) {
      setCommitError(`Failed to commit: ${err instanceof Error ? err.message : 'Unknown error'}`)
    }

    setCommitting(false)
  }

  const handleMerge = async (squash: boolean) => {
    // Dry-run: re-check conflicts before merging to catch stale state
    if (wsData.gitBranch && parentWorkspace?.gitBranch) {
      const freshCheck = await git.checkMergeConflicts(
        wsData.gitBranch,
        parentWorkspace.gitBranch
      )
      if (freshCheck.success && freshCheck.conflicts.hasConflicts) {
        setConflictInfo(freshCheck.conflicts)
        alert(`Cannot merge: ${String(freshCheck.conflicts.conflictedFiles.length)} conflict(s) detected. Resolve conflicts first.`)
        return
      }
    }

    if (hasUncommitted) {
      const fileCount = uncommitted.files.length
      const confirmed = confirm(
        `You have ${String(fileCount)} uncommitted file${fileCount !== 1 ? 's' : ''}. ` +
        `These changes will be auto-committed before merging. Continue?`
      )
      if (!confirmed) {
        return
      }
    }

    setIsProcessing(true)
    setProcessingAction(squash ? 'squash' : 'merge')

    try {
      const result = await mergeAndRemove(squash)
      if (!result.success) {
        alert(`Merge failed: ${String(result.error)}`)
        return
      }
    } catch (err) {
      alert(`Merge failed: ${err instanceof Error ? err.message : 'Unknown error'}`)
    }

    setIsProcessing(false)
    setProcessingAction(null)
  }

  const handleMergeAndKeep = async (squash: boolean) => {
    if (wsData.gitBranch && parentWorkspace?.gitBranch) {
      const freshCheck = await git.checkMergeConflicts(
        wsData.gitBranch,
        parentWorkspace.gitBranch
      )
      if (freshCheck.success && freshCheck.conflicts.hasConflicts) {
        setConflictInfo(freshCheck.conflicts)
        alert(`Cannot merge: ${String(freshCheck.conflicts.conflictedFiles.length)} conflict(s) detected. Resolve conflicts first.`)
        return
      }
    }

    if (hasUncommitted) {
      const fileCount = uncommitted.files.length
      const confirmed = confirm(
        `You have ${String(fileCount)} uncommitted file${fileCount !== 1 ? 's' : ''}. ` +
        `These changes will be auto-committed before merging. Continue?`
      )
      if (!confirmed) {
        return
      }
    }

    setIsProcessing(true)
    setProcessingAction(squash ? 'squash-keep' : 'merge-keep')

    try {
      const result = await mergeAndKeep(squash)
      if (!result.success) {
        alert(`Merge failed: ${String(result.error)}`)
        setIsProcessing(false)
        setProcessingAction(null)
        return
      }
      // Workspace still alive — refresh the review view
      await Promise.all([loadDiff(), loadUncommittedChanges()])
      void refreshDiffStatus()
    } catch (err) {
      alert(`Merge failed: ${err instanceof Error ? err.message : 'Unknown error'}`)
    }

    setIsProcessing(false)
    setProcessingAction(null)
  }

  const handleCloseAndClean = async () => {
    if (!confirm('Close this workspace? The worktree will be removed but the branch will be kept.')) {
      return
    }

    setIsProcessing(true)

    try {
      const result = await closeAndClean()
      if (!result.success) {
        alert(`Close failed: ${String(result.error)}`)
      }
    } catch (err) {
      alert(`Close failed: ${err instanceof Error ? err.message : 'Unknown error'}`)
    }

    setIsProcessing(false)
  }

  const handleCancel = () => {
    void removeTab(tabId)
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

  const stagedFiles = uncommitted?.files.filter((f) => f.staged) || []
  const unstagedFiles = uncommitted?.files.filter((f) => !f.staged) || []
  const hasUncommitted = uncommitted && uncommitted.files.length > 0
  const hasCommittedChanges = diff && diff.files.length > 0
  const hasConflicts = conflictInfo?.hasConflicts || false

  const fileList = viewMode === 'committed'
    ? getSortedFilePaths(diff?.files || [])
    : getSortedFilePaths([...stagedFiles, ...unstagedFiles])

  const currentFileIndex = selectedFile
    ? fileList.indexOf(selectedFile)
    : selectedUncommittedFile
      ? fileList.indexOf(selectedUncommittedFile.path)
      : -1

  const handlePreviousFile = () => {
    if (currentFileIndex > 0) {
      // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- currentFileIndex > 0 guarantees valid index
      const prevFilePath = fileList[currentFileIndex - 1]!
      if (viewMode === 'committed') {
        void loadFileDiff(prevFilePath)
      } else {
        const prevFile = [...stagedFiles, ...unstagedFiles].find(f => f.path === prevFilePath)
        if (prevFile) void loadUncommittedFileDiff(prevFile)
      }
    }
  }

  const handleNextFile = () => {
    if (currentFileIndex < fileList.length - 1) {
      // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- currentFileIndex < length - 1 guarantees valid index
      const nextFilePath = fileList[currentFileIndex + 1]!
      if (viewMode === 'committed') {
        void loadFileDiff(nextFilePath)
      } else {
        const nextFile = [...stagedFiles, ...unstagedFiles].find(f => f.path === nextFilePath)
        if (nextFile) void loadUncommittedFileDiff(nextFile)
      }
    }
  }

  const handleLineClick = (lineNumber: number, side: 'original' | 'modified') => {
    setCommentInput({ visible: true, lineNumber, side })
  }

  const handleCommentSubmit = (text: string) => {
    const filePath = selectedFile || selectedUncommittedFile?.path
    if (!commentInput || !currentCommitHash || !filePath) return

    try {
      addReviewComment({
        filePath,
        lineNumber: commentInput.lineNumber,
        text,
        commitHash: currentCommitHash,
        isOutdated: false,
        addressed: false,
        side: commentInput.side
      })
      setCommentInput(null)
    } catch (error) {
      console.error('Failed to add comment:', error)
      alert(`Failed to add comment: ${error instanceof Error ? error.message : 'Unknown error'}`)
    }
  }

  const handleCommentDelete = (commentId: string) => {
    deleteReviewComment(commentId)
  }

  const currentFilePath = selectedFile || selectedUncommittedFile?.path
  const fileComments = currentFilePath
    ? reviews.filter(c => c.filePath === currentFilePath)
    : []

  return (
    <div className="review-browser">
      {/* Header */}
      <div className="review-header">
        <div className="review-header-info">
          <span className="review-workspace-name">{wsData.name}</span>
          <span className="review-branch-info">
            <span className="review-branch">{wsData.gitBranch}</span>
            {parentWorkspace && (
              <>
                <span className="review-arrow">→</span>
                <span className="review-branch">{parentWorkspace.gitBranch}</span>
              </>
            )}
          </span>
        </div>
        <div className="review-header-actions">
          {isCheckingConflicts && (
            <span className="review-checking">Checking for conflicts...</span>
          )}
          <button
            className="review-refresh-btn"
            onClick={() => { void handleRefresh(); }}
            disabled={refreshing}
            title="Refresh changes"
          >
            {refreshing ? <Loader2 size={14} className="spinning" /> : <RefreshCw size={14} />}
          </button>
          {!hasParent ? (
            <>
              {hasUncommitted && runningHarness && (
                <button
                  className="review-action-btn review-prompt-commit-btn"
                  onClick={handlePromptCommit}
                  disabled={isProcessing}
                  title="Send commit command to the AI harness"
                >
                  Prompt Commit
                </button>
              )}
              <button
                className="review-action-btn review-cancel-btn"
                onClick={handleCancel}
                disabled={isProcessing}
              >
                Close
              </button>
            </>
          ) : wsData.isDetached ? (
            <>
              {hasUncommitted && runningHarness && (
                <button
                  className="review-action-btn review-prompt-commit-btn"
                  onClick={handlePromptCommit}
                  disabled={isProcessing}
                  title="Send commit command to the AI harness"
                >
                  Prompt Commit
                </button>
              )}
              <button
                className="review-action-btn review-close-and-clean-btn"
                onClick={() => { void handleCloseAndClean(); }}
                disabled={isProcessing}
                title="Remove worktree but keep the branch"
              >
                {isProcessing ? <><span className="btn-spinner" />Closing...</> : 'Close and Clean'}
              </button>
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
              {hasUncommitted && runningHarness && (
                <button
                  className="review-action-btn review-prompt-commit-btn"
                  onClick={handlePromptCommit}
                  disabled={isProcessing}
                  title="Send commit command to the AI harness"
                >
                  Prompt Commit
                </button>
              )}
              {hasConflicts && runningHarness && parentWorkspace?.gitBranch && (
                <button
                  className="review-action-btn review-prompt-rebase-btn"
                  onClick={handlePromptRebase}
                  disabled={isProcessing}
                  title="Send rebase command to the AI harness"
                >
                  Prompt Rebase
                </button>
              )}
              <div className="merge-btn-group">
                <button
                  className="review-action-btn review-merge-btn merge-btn-main"
                  onClick={() => { void handleMerge(false); }}
                  disabled={isProcessing || hasConflicts}
                  title={hasConflicts ? 'Cannot merge: resolve conflicts first' : 'Merge changes into parent branch'}
                >
                  {processingAction === 'merge' ? <><span className="btn-spinner" />Merging...</> : 'Merge'}
                  {hasConflicts && ' (has conflicts)'}
                </button>
                <button
                  className="review-action-btn review-merge-btn merge-btn-dropdown-toggle"
                  onClick={() => { setMergeDropdownOpen(!mergeDropdownOpen); }}
                  disabled={isProcessing || hasConflicts}
                  title="More merge options"
                >
                  <ChevronDown size={14} />
                </button>
                {mergeDropdownOpen && (
                  <ClickOutsideDiv className="merge-dropdown-menu" onClickOutside={() => { setMergeDropdownOpen(false); }}>
                    <button
                      className="merge-dropdown-item"
                      onClick={() => { setMergeDropdownOpen(false); void handleMerge(true); }}
                      disabled={isProcessing || hasConflicts}
                      title={hasConflicts ? 'Cannot merge: resolve conflicts first' : 'Squash all commits into one'}
                    >
                      {processingAction === 'squash' ? <><span className="btn-spinner" />Squashing...</> : 'Squash Merge'}
                      {hasConflicts && ' (has conflicts)'}
                    </button>
                    <button
                      className="merge-dropdown-item"
                      onClick={() => { setMergeDropdownOpen(false); void handleMergeAndKeep(false); }}
                      disabled={isProcessing || hasConflicts}
                      title={hasConflicts ? 'Cannot merge: resolve conflicts first' : 'Merge into parent but keep this workspace'}
                    >
                      {processingAction === 'merge-keep' ? <><span className="btn-spinner" />Merging...</> : 'Merge and Keep'}
                      {hasConflicts && ' (has conflicts)'}
                    </button>
                    <button
                      className="merge-dropdown-item"
                      onClick={() => { setMergeDropdownOpen(false); void handleMergeAndKeep(true); }}
                      disabled={isProcessing || hasConflicts}
                      title={hasConflicts ? 'Cannot merge: resolve conflicts first' : 'Squash merge into parent but keep this workspace'}
                    >
                      {processingAction === 'squash-keep' ? <><span className="btn-spinner" />Squashing...</> : 'Squash Merge and Keep'}
                      {hasConflicts && ' (has conflicts)'}
                    </button>
                  </ClickOutsideDiv>
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

      {loadError && (
        <div className="review-load-error">{loadError}</div>
      )}

      {conflictError && (
        <div className="review-conflict-error">Could not check for conflicts: {conflictError}</div>
      )}

      {hasConflicts && (
        <div className="review-conflict-banner">
          <span className="review-conflict-icon">⚠️</span>
          <div className="review-conflict-text">
            <strong>{conflictInfo?.conflictedFiles.length} conflict(s) detected</strong>
            <div className="review-conflict-files">
              {conflictInfo?.conflictedFiles.slice(0, 3).join(', ')}
              {conflictInfo && conflictInfo.conflictedFiles.length > 3 && ` and ${String(conflictInfo.conflictedFiles.length - 3)} more`}
            </div>
          </div>
        </div>
      )}

      <div className="diff-tabs">
        {hasParent && (
          <button
            className={`diff-tab ${viewMode === 'committed' ? 'active' : ''}`}
            onClick={() => { setViewMode('committed'); persistViewState({ viewMode: 'committed' }) }}
          >
            Committed Changes
            {hasCommittedChanges && (
              <span className="diff-tab-count">{String(diff.files.length)}</span>
            )}
          </button>
        )}
        <button
          className={`diff-tab ${viewMode === 'uncommitted' ? 'active' : ''}`}
          onClick={() => { setViewMode('uncommitted'); persistViewState({ viewMode: 'uncommitted' }) }}
        >
          Uncommitted
          {hasUncommitted && (
            <span className="diff-tab-count">{String(uncommitted.files.length)}</span>
          )}
        </button>
        <button
          className={`diff-tab ${viewMode === 'commits' ? 'active' : ''}`}
          onClick={() => { setViewMode('commits'); persistViewState({ viewMode: 'commits' }); if (commits.length === 0) void loadCommits(); }}
        >
          Commits
          {commits.length > 0 && (
            <span className="diff-tab-count">{commits.length}{commitsHasMore ? '+' : ''}</span>
          )}
        </button>
      </div>

      {loading ? (
        <div className="review-loading">Loading changes...</div>
      ) : error ? (
        <div className="review-error">{error}</div>
      ) : (
        <>
          {viewMode === 'commits' ? (
            <div className="commits-view">
              <div className="commits-pane">
                {commitsLoading && commits.length === 0 ? (
                  <div className="review-loading">Loading commits...</div>
                ) : commitsError ? (
                  <div className="review-error">{commitsError}</div>
                ) : commits.length === 0 ? (
                  <div className="diff-empty">No commits on this branch</div>
                ) : (
                  <div className="commits-list">
                    {commits.map(commit => (
                      <div
                        key={commit.hash}
                        className={`commit-row ${selectedCommit?.hash === commit.hash ? 'selected' : ''}`}
                        onClick={() => { void loadCommitDiff(commit); }}
                      >
                        <span className="commit-hash">{commit.shortHash}</span>
                        <span className="commit-message">{commit.message}</span>
                        <span className="commit-author">{commit.author}</span>
                        <span className="commit-date">{new Date(commit.date).toLocaleDateString()}</span>
                      </div>
                    ))}
                    {commitsHasMore && (
                      <button
                        className="commits-load-more"
                        onClick={() => { void loadCommits(commits.length); }}
                        disabled={commitsLoading}
                      >
                        {commitsLoading ? 'Loading...' : 'Load more'}
                      </button>
                    )}
                  </div>
                )}
              </div>
              {selectedCommit && (
                <div
                  className="diff-content"
                  onMouseMove={handleResizeMouseMove}
                  onMouseUp={handleResizeMouseUp}
                  onMouseLeave={handleResizeMouseUp}
                >
                  <div className="diff-file-list" style={{ width: fileListWidth }}>
                    {commitDiffLoading ? (
                      <div className="diff-loading">Loading...</div>
                    ) : (
                      <CommittedDiffFileTree
                        files={commitDiffFiles}
                        selectedFile={selectedCommitFile}
                        onSelectFile={(path) => { void loadCommitFileDiff(selectedCommit.hash, path); }}
                        getStatusIcon={getStatusIcon}
                      />
                    )}
                  </div>
                  <div
                    className={`divider ${isResizing ? 'active' : ''}`}
                    onMouseDown={handleResizeMouseDown}
                  />
                  <div className="diff-file-content">
                    {selectedCommitFile ? (
                      loadingFileDiff ? (
                        <div className="diff-loading">Loading...</div>
                      ) : fileDiffContents ? (
                        <MonacoDiffViewer
                          originalContent={fileDiffContents.originalContent}
                          modifiedContent={fileDiffContents.modifiedContent}
                          language={fileDiffContents.language}
                          originalLabel={`${selectedCommit.shortHash}~1`}
                          modifiedLabel={selectedCommit.shortHash}
                        />
                      ) : (
                        <div className="diff-placeholder">Failed to load diff contents</div>
                      )
                    ) : (
                      <div className="diff-placeholder">Select a file to view changes</div>
                    )}
                  </div>
                </div>
              )}
            </div>
          ) : viewMode === 'committed' ? (
            !hasCommittedChanges ? (
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
                    <CommittedDiffFileTree
                      files={diff.files}
                      selectedFile={selectedFile}
                      onSelectFile={(path) => { void loadFileDiff(path); }}
                      getStatusIcon={getStatusIcon}
                    />
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
                        <MonacoDiffViewer
                          originalContent={fileDiffContents.originalContent}
                          modifiedContent={fileDiffContents.modifiedContent}
                          language={fileDiffContents.language}
                          originalLabel={diff.baseBranch || 'Original'}
                          modifiedLabel={diff.headBranch || 'Modified'}
                          onPreviousFile={handlePreviousFile}
                          onNextFile={handleNextFile}
                          hasPreviousFile={currentFileIndex > 0}
                          hasNextFile={currentFileIndex < fileList.length - 1}
                          comments={fileComments}
                          onLineClick={handleLineClick}
                          inlineCommentInput={commentInput}
                          onCommentSubmit={handleCommentSubmit}
                          onCommentCancel={() => { setCommentInput(null); }}
                          onCommentDelete={handleCommentDelete}
                          initialScrollTop={reviewState?.scrollTop}
                          onScrollPositionChange={handleScrollPositionChange}
                        />
                      ) : (
                        <div className="diff-placeholder">Failed to load diff contents</div>
                      )
                    ) : (
                      <div className="diff-placeholder">Select a file to view changes</div>
                    )}
                  </div>

                </div>
              </>
            )
          ) : (
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
                    <button onClick={() => { void handleStageAll(); }} className="diff-action-btn" disabled={stagingInProgress}>
                      {stagingInProgress ? 'Processing...' : 'Stage All'}
                    </button>
                    <button onClick={() => { void handleUnstageAll(); }} className="diff-action-btn" disabled={stagingInProgress}>
                      {stagingInProgress ? 'Processing...' : 'Unstage All'}
                    </button>
                  </div>
                </div>
                {stageError && <div className="review-load-error">{stageError}</div>}

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
                        <UncommittedDiffFileTree
                          files={stagedFiles}
                          selectedFile={selectedUncommittedFile}
                          onSelectFile={(file) => { void loadUncommittedFileDiff(file); }}
                          getStatusIcon={getStatusIcon}
                          onAction={(path) => { void handleUnstageFile(path); }}
                          actionLabel="Unstage"
                          stagingInProgress={stagingInProgress}
                        />
                      </>
                    )}
                    {unstagedFiles.length > 0 && (
                      <>
                        <div className="diff-file-section">Unstaged</div>
                        <UncommittedDiffFileTree
                          files={unstagedFiles}
                          selectedFile={selectedUncommittedFile}
                          onSelectFile={(file) => { void loadUncommittedFileDiff(file); }}
                          getStatusIcon={getStatusIcon}
                          onAction={(path) => { void handleStageFile(path); }}
                          actionLabel="Stage"
                          stagingInProgress={stagingInProgress}
                        />
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
                          onCommentCancel={() => { setCommentInput(null); }}
                          onCommentDelete={handleCommentDelete}
                          initialScrollTop={reviewState?.scrollTop}
                          onScrollPositionChange={handleScrollPositionChange}
                        />
                      ) : (
                        <div className="diff-placeholder">Failed to load diff contents</div>
                      )
                    ) : (
                      <div className="diff-placeholder">Select a file to view changes</div>
                    )}
                  </div>
                </div>

                {stagedFiles.length > 0 && (
                  <div className="review-commit-section">
                    <input
                      type="text"
                      className="review-commit-input"
                      placeholder="Commit message..."
                      value={commitMessage}
                      onChange={(e) => { setCommitMessage(e.target.value); }}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter' && !e.shiftKey) {
                          e.preventDefault()
                          void handleCommit()
                        }
                      }}
                    />
                    <button
                      className="review-commit-btn"
                      onClick={() => { void handleCommit(); }}
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

    </div>
  )
}

/** Div that detects clicks outside itself and calls onClickOutside */
function ClickOutsideDiv({ className, onClickOutside, children }: {
  className?: string
  onClickOutside: () => void
  children: React.ReactNode
}) {
  const ref = React.useRef<HTMLDivElement>(null)

  React.useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        onClickOutside()
      }
    }
    document.addEventListener('mousedown', handler)
    return () => { document.removeEventListener('mousedown', handler); }
  }, [onClickOutside])

  return <div className={className} ref={ref}>{children}</div>
}
