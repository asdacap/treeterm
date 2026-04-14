import React, { useState, useEffect, useCallback, useRef } from 'react'
import { ChevronDown, RefreshCw, Loader2 } from 'lucide-react'
import { WorkerPoolContextProvider } from '@pierre/diffs/react'
import { useStore } from 'zustand'
import { findRunningHarness } from '../utils/findRunningHarnessPtyId'
import { getTabs } from '../types'
import type { DiffFile, DiffResult, UncommittedFile, UncommittedChanges, ConflictInfo, FileDiffContents, GitLogCommit, WorkspaceStore, ReviewState, ViewedFileStats } from '../types'
import { FileChangeStatus } from '../types'
import { useGitApi } from '../hooks/useWorkspaceApis'
import { CommittedDiffFileTree, UncommittedDiffFileTree } from './DiffFileTree'
import { StackedDiffList } from './StackedDiffList'
import { DiffToolbar } from './DiffToolbar'
import { createDiffsWorker } from '../pierre-diffs-config'

interface ReviewBrowserProps {
  workspace: WorkspaceStore
  tabId: string
  // parentWorkspaceId is optional - if undefined, this is a top-level worktree
  // and only uncommitted changes are shown (no merge functionality)
  parentWorkspaceId?: string
  isVisible: boolean
}

enum ViewMode {
  Committed = 'committed',
  Uncommitted = 'uncommitted',
  Commits = 'commits',
}

export default function ReviewBrowser({
  workspace,
  tabId,
  parentWorkspaceId,
  isVisible,
}: ReviewBrowserProps) {
  const wsData = useStore(workspace, s => s.workspace)
  const lookupWorkspace = useStore(workspace, s => s.lookupWorkspace)
  const promptHarness = useStore(workspace, s => s.promptHarness)
  const mergeAndRemove = useStore(workspace, s => s.mergeAndRemove)
  const mergeAndKeep = useStore(workspace, s => s.mergeAndKeep)
  const closeAndClean = useStore(workspace, s => s.closeAndClean)
  const removeTab = useStore(workspace, s => s.removeTab)
  const reviewCommentStore = useStore(workspace, s => s.reviewComments)
  const gitController = useStore(workspace, s => s.gitController)
  const updateTabState = useStore(workspace, s => s.updateTabState)
  const getReviewComments = useStore(reviewCommentStore, s => s.getReviewComments)
  const addReviewComment = useStore(reviewCommentStore, s => s.addReviewComment)
  const deleteReviewComment = useStore(reviewCommentStore, s => s.deleteReviewComment)
  const updateOutdatedReviewComments = useStore(reviewCommentStore, s => s.updateOutdatedReviewComments)
  const refreshDiffStatus = useStore(gitController, s => s.refreshDiffStatus)
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

  // Stacked diff view state
  const [isSplitView, setIsSplitView] = useState(true)
  const [hideUnchangedRegions, setHideUnchangedRegions] = useState(true)
  const [ignoreWhitespace, setIgnoreWhitespace] = useState(false)
  const [scrollToFile, setScrollToFile] = useState<string | null>(reviewState?.selectedFilePath ?? null)
  const [activeFile, setActiveFile] = useState<string | null>(null)

  // Viewed files state — maps file path to diff stats at time of marking viewed
  const [viewedFiles, setViewedFiles] = useState<Record<string, ViewedFileStats>>(
    reviewState?.viewedFiles ?? {}
  )

  // Uncommitted changes state
  const [uncommitted, setUncommitted] = useState<UncommittedChanges | null>(null)
  const [viewMode, setViewMode] = useState((reviewState?.viewMode as ViewMode | undefined) ?? (hasParent ? ViewMode.Committed : ViewMode.Uncommitted))

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
    filePath: string
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

  // AI harness prompt support
  const runningHarness = findRunningHarness(getTabs(wsData))

  const [refreshing, setRefreshing] = useState(false)

  // Persist view state to tab state for restoration across workspace switches
  const persistViewState = useCallback((updates: Partial<ReviewState>) => {
    updateTabState<ReviewState>(tabId, (s) => ({ ...s, ...updates }))
  }, [tabId, updateTabState])

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
    if (initialViewMode === ViewMode.Commits) {
      void h.loadCommits()
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps -- initialViewMode is intentionally excluded: it captures the mount-time value and should not re-trigger the effect
  }, [workspaceId, parentWorkspaceId, currentGitBranch, parentGitBranch])

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

  // Filter viewed files: invalidate entries whose stats changed, but preserve entries
  // for files not in the provided list (they belong to a different view mode).
  const reconcileViewedFiles = (files: (DiffFile | UncommittedFile)[]) => {
    setViewedFiles(prev => {
      const fileMap = new Map(files.map(f => [f.path, f]))
      const next: Record<string, ViewedFileStats> = {}
      let changed = false
      for (const [path, stats] of Object.entries(prev)) {
        const file = fileMap.get(path)
        if (file) {
          if (file.additions === stats.additions && file.deletions === stats.deletions) {
            next[path] = stats
          } else {
            changed = true
          }
        } else {
          // File not in this list — preserve (belongs to another view mode)
          next[path] = stats
        }
      }
      if (!changed) return prev
      persistViewState({ viewedFiles: next })
      return next
    })
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
        reconcileViewedFiles(result.diff.files)
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
        reconcileViewedFiles(result.changes.files)
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

  // Load file contents callbacks for StackedDiffList
  const loadCommittedFileContents = useCallback(async (filePath: string): Promise<FileDiffContents> => {
    if (!parentWorkspace?.gitBranch) throw new Error('No parent branch')
    const result = await git.getFileContentsForDiff(parentWorkspace.gitBranch, filePath)
    if (result.success) return result.contents
    throw new Error(result.error || 'Failed to load file diff')
  }, [git, parentWorkspace?.gitBranch])

  const loadUncommittedFileContents = useCallback(async (filePath: string): Promise<FileDiffContents> => {
    const file = uncommitted?.files.find(f => f.path === filePath)
    if (!file) throw new Error(`File not found: ${filePath}`)
    const result = await git.getUncommittedFileContentsForDiff(filePath, file.staged)
    if (result.success) return result.contents
    throw new Error(result.error || 'Failed to load uncommitted file diff')
  }, [git, uncommitted?.files])

  const loadCommitFileContents = useCallback(async (filePath: string): Promise<FileDiffContents> => {
    if (!selectedCommit) throw new Error('No commit selected')
    const result = await git.getCommitFileDiff(selectedCommit.hash, filePath)
    if (result.success) return result.contents
    throw new Error(result.error || 'Failed to load commit file diff')
  }, [git, selectedCommit])

  const helpersRef = useRef({ loadDiff, loadReviews, loadUncommittedChanges, checkConflicts, loadCommits })
  helpersRef.current = { loadDiff, loadReviews, loadUncommittedChanges, checkConflicts, loadCommits }

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

  const getStatusIcon = (status: FileChangeStatus) => {
    switch (status) {
      case FileChangeStatus.Added:
        return <span className="diff-status added">A</span>
      case FileChangeStatus.Modified:
        return <span className="diff-status modified">M</span>
      case FileChangeStatus.Deleted:
        return <span className="diff-status deleted">D</span>
      case FileChangeStatus.Renamed:
        return <span className="diff-status renamed">R</span>
      case FileChangeStatus.Untracked:
        return <span className="diff-status untracked">?</span>
    }
  }

  const stagedFiles = uncommitted?.files.filter((f) => f.staged) || []
  const unstagedFiles = uncommitted?.files.filter((f) => !f.staged) || []
  const hasUncommitted = uncommitted && uncommitted.files.length > 0
  const hasCommittedChanges = diff && diff.files.length > 0
  const hasConflicts = conflictInfo?.hasConflicts || false

  // Viewed files computed values
  const viewedFilePaths = new Set(Object.keys(viewedFiles))
  const committedViewedCount = diff?.files.filter(f => f.path in viewedFiles).length ?? 0
  const uncommittedViewedCount = uncommitted?.files.filter(f => f.path in viewedFiles).length ?? 0
  const commitDiffViewedCount = commitDiffFiles.filter(f => f.path in viewedFiles).length

  const handleLineClick = (filePath: string, lineNumber: number, side: 'original' | 'modified') => {
    setCommentInput({ filePath, lineNumber, side })
  }

  const handleScrollToFileHandled = useCallback(() => {
    setScrollToFile(null)
  }, [])

  const handleToggleViewed = useCallback((file: DiffFile | UncommittedFile) => {
    setViewedFiles(prev => {
      let next: Record<string, ViewedFileStats>
      if (prev[file.path]) {
        next = Object.fromEntries(Object.entries(prev).filter(([k]) => k !== file.path))
      } else {
        next = { ...prev, [file.path]: { additions: file.additions, deletions: file.deletions } }
      }
      persistViewState({ viewedFiles: next })
      return next
    })
  }, [persistViewState])

  const handleMarkViewedAbove = useCallback((filesToMark: (DiffFile | UncommittedFile)[]) => {
    setViewedFiles(prev => {
      const next = { ...prev }
      for (const file of filesToMark) {
        if (!next[file.path]) {
          next[file.path] = { additions: file.additions, deletions: file.deletions }
        }
      }
      persistViewState({ viewedFiles: next })
      return next
    })
  }, [persistViewState])

  const isFileViewed = useCallback((filePath: string): boolean => {
    return filePath in viewedFiles
  }, [viewedFiles])

  const handleCommentSubmit = (text: string) => {
    if (!commentInput || !currentCommitHash) return
    const filePath = commentInput.filePath

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

  // Staging action factory for uncommitted files
  const getUncommittedStagingAction = (file: DiffFile | UncommittedFile) => {
    const ucFile = file as UncommittedFile
    if (ucFile.staged) {
      return {
        label: 'Unstage',
        onAction: () => { void handleUnstageFile(ucFile.path) },
        disabled: stagingInProgress,
      }
    }
    return {
      label: 'Stage',
      onAction: () => { void handleStageFile(ucFile.path) },
      disabled: stagingInProgress,
    }
  }

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
            className={`diff-tab ${viewMode === ViewMode.Committed ? 'active' : ''}`}
            onClick={() => { setViewMode(ViewMode.Committed); persistViewState({ viewMode: ViewMode.Committed }) }}
          >
            Committed Changes
            {hasCommittedChanges && (
              <span className="diff-tab-count">{String(diff.files.length)}</span>
            )}
          </button>
        )}
        <button
          className={`diff-tab ${viewMode === ViewMode.Uncommitted ? 'active' : ''}`}
          onClick={() => { setViewMode(ViewMode.Uncommitted); persistViewState({ viewMode: ViewMode.Uncommitted }) }}
        >
          Uncommitted
          {hasUncommitted && (
            <span className="diff-tab-count">{String(uncommitted.files.length)}</span>
          )}
        </button>
        <button
          className={`diff-tab ${viewMode === ViewMode.Commits ? 'active' : ''}`}
          onClick={() => { setViewMode(ViewMode.Commits); persistViewState({ viewMode: ViewMode.Commits }); if (commits.length === 0) void loadCommits(); }}
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
          {viewMode === ViewMode.Commits ? (
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
                <>
                  <DiffToolbar
                    isSplitView={isSplitView}
                    onToggleSplit={() => { setIsSplitView(!isSplitView) }}
                    hideUnchanged={hideUnchangedRegions}
                    onToggleHideUnchanged={() => { setHideUnchangedRegions(!hideUnchangedRegions) }}
                    ignoreWhitespace={ignoreWhitespace}
                    onToggleIgnoreWhitespace={() => { setIgnoreWhitespace(!ignoreWhitespace) }}
                    totalComments={0}
                    viewedCount={commitDiffViewedCount}
                    totalFiles={commitDiffFiles.length}
                  />
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
                          selectedFile={activeFile}
                          onSelectFile={(path) => { setScrollToFile(path) }}
                          getStatusIcon={getStatusIcon}
                          viewedFiles={viewedFilePaths}
                        />
                      )}
                    </div>
                    <div
                      className={`divider ${isResizing ? 'active' : ''}`}
                      onMouseDown={handleResizeMouseDown}
                    />
                    <WorkerPoolContextProvider
                      poolOptions={{ workerFactory: createDiffsWorker, poolSize: 2 }}
                      highlighterOptions={{ preferredHighlighter: 'shiki-wasm' }}
                    >
                      <StackedDiffList
                        files={commitDiffFiles}
                        loadFileContents={loadCommitFileContents}
                        diffStyle={isSplitView ? 'split' : 'unified'}
                        expandUnchanged={!hideUnchangedRegions}
                        ignoreWhitespace={ignoreWhitespace}
                        getStatusIcon={getStatusIcon}
                        reviews={[]}
                        onLineClick={handleLineClick}
                        commentInput={null}
                        scrollToFile={scrollToFile}
                        onActiveFileChange={setActiveFile}
                        onScrollToFileHandled={handleScrollToFileHandled}
                        isFileViewed={isFileViewed}
                        onToggleViewed={handleToggleViewed}
                        onMarkViewedAbove={handleMarkViewedAbove}
                      />
                    </WorkerPoolContextProvider>
                  </div>
                </>
              )}
            </div>
          ) : viewMode === ViewMode.Committed ? (
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

                <DiffToolbar
                  isSplitView={isSplitView}
                  onToggleSplit={() => { setIsSplitView(!isSplitView) }}
                  hideUnchanged={hideUnchangedRegions}
                  onToggleHideUnchanged={() => { setHideUnchangedRegions(!hideUnchangedRegions) }}
                  ignoreWhitespace={ignoreWhitespace}
                  onToggleIgnoreWhitespace={() => { setIgnoreWhitespace(!ignoreWhitespace) }}
                  totalComments={reviews.length}
                  viewedCount={committedViewedCount}
                  totalFiles={diff.files.length}
                />

                <div
                  className="diff-content"
                  onMouseMove={handleResizeMouseMove}
                  onMouseUp={handleResizeMouseUp}
                  onMouseLeave={handleResizeMouseUp}
                >
                  <div className="diff-file-list" style={{ width: fileListWidth }}>
                    <CommittedDiffFileTree
                      files={diff.files}
                      selectedFile={activeFile}
                      onSelectFile={(path) => { setScrollToFile(path); persistViewState({ selectedFilePath: path }) }}
                      getStatusIcon={getStatusIcon}
                      viewedFiles={viewedFilePaths}
                    />
                  </div>

                  <div
                    className={`divider ${isResizing ? 'active' : ''}`}
                    onMouseDown={handleResizeMouseDown}
                  />

                  <WorkerPoolContextProvider
                    poolOptions={{ workerFactory: createDiffsWorker, poolSize: 2 }}
                    highlighterOptions={{ preferredHighlighter: 'shiki-wasm' }}
                  >
                    <StackedDiffList
                      files={diff.files}
                      loadFileContents={loadCommittedFileContents}
                      diffStyle={isSplitView ? 'split' : 'unified'}
                      expandUnchanged={!hideUnchangedRegions}
                      ignoreWhitespace={ignoreWhitespace}
                      getStatusIcon={getStatusIcon}
                      reviews={reviews}
                      onLineClick={handleLineClick}
                      commentInput={commentInput}
                      onCommentSubmit={handleCommentSubmit}
                      onCommentCancel={() => { setCommentInput(null) }}
                      onCommentDelete={handleCommentDelete}
                      scrollToFile={scrollToFile}
                      onActiveFileChange={setActiveFile}
                      onScrollToFileHandled={handleScrollToFileHandled}
                      isFileViewed={isFileViewed}
                      onToggleViewed={handleToggleViewed}
                      onMarkViewedAbove={handleMarkViewedAbove}
                    />
                  </WorkerPoolContextProvider>
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

                <DiffToolbar
                  isSplitView={isSplitView}
                  onToggleSplit={() => { setIsSplitView(!isSplitView) }}
                  hideUnchanged={hideUnchangedRegions}
                  onToggleHideUnchanged={() => { setHideUnchangedRegions(!hideUnchangedRegions) }}
                  ignoreWhitespace={ignoreWhitespace}
                  onToggleIgnoreWhitespace={() => { setIgnoreWhitespace(!ignoreWhitespace) }}
                  totalComments={reviews.length}
                  viewedCount={uncommittedViewedCount}
                  totalFiles={uncommitted.files.length}
                />

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
                          selectedFile={null}
                          onSelectFile={(file) => { setScrollToFile(file.path) }}
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
                          selectedFile={null}
                          onSelectFile={(file) => { setScrollToFile(file.path) }}
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

                  <WorkerPoolContextProvider
                    poolOptions={{ workerFactory: createDiffsWorker, poolSize: 2 }}
                    highlighterOptions={{ preferredHighlighter: 'shiki-wasm' }}
                  >
                    <StackedDiffList
                      files={[...stagedFiles, ...unstagedFiles]}
                      loadFileContents={loadUncommittedFileContents}
                      diffStyle={isSplitView ? 'split' : 'unified'}
                      expandUnchanged={!hideUnchangedRegions}
                      ignoreWhitespace={ignoreWhitespace}
                      getStatusIcon={getStatusIcon}
                      reviews={reviews}
                      onLineClick={handleLineClick}
                      commentInput={commentInput}
                      onCommentSubmit={handleCommentSubmit}
                      onCommentCancel={() => { setCommentInput(null) }}
                      onCommentDelete={handleCommentDelete}
                      getStagingAction={getUncommittedStagingAction}
                      scrollToFile={scrollToFile}
                      onActiveFileChange={setActiveFile}
                      onScrollToFileHandled={handleScrollToFileHandled}
                      isFileViewed={isFileViewed}
                      onToggleViewed={handleToggleViewed}
                      onMarkViewedAbove={handleMarkViewedAbove}
                    />
                  </WorkerPoolContextProvider>
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
