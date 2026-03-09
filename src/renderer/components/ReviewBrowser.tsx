import { useState, useEffect } from 'react'
import { useWorkspaceStore } from '../store/workspace'
import type { DiffFile, DiffResult, UncommittedFile, UncommittedChanges, ConflictInfo, FileDiffContents } from '../types'
import { MonacoDiffViewer } from './MonacoDiffViewer'

interface ReviewBrowserProps {
  workspaceId: string
  workspacePath: string
  tabId: string
  parentWorkspaceId: string
}

type ViewMode = 'committed' | 'uncommitted'

export default function ReviewBrowser({
  workspaceId,
  workspacePath,
  tabId,
  parentWorkspaceId
}: ReviewBrowserProps) {
  const { workspaces, mergeAndRemoveWorkspace, removeWorkspace, removeTab } = useWorkspaceStore()
  const workspace = workspaces[workspaceId]
  const parentWorkspace = workspaces[parentWorkspaceId]

  // Diff state
  const [diff, setDiff] = useState<DiffResult | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [selectedFile, setSelectedFile] = useState<string | null>(null)
  const [fileDiffContents, setFileDiffContents] = useState<FileDiffContents | null>(null)
  const [loadingFileDiff, setLoadingFileDiff] = useState(false)

  // Uncommitted changes state
  const [uncommitted, setUncommitted] = useState<UncommittedChanges | null>(null)
  const [viewMode, setViewMode] = useState<ViewMode>('committed')
  const [selectedUncommittedFile, setSelectedUncommittedFile] = useState<UncommittedFile | null>(null)

  // Commit state
  const [commitMessage, setCommitMessage] = useState('')
  const [committing, setCommitting] = useState(false)
  const [commitError, setCommitError] = useState<string | null>(null)

  // Conflict state
  const [conflictInfo, setConflictInfo] = useState<ConflictInfo | null>(null)
  const [isCheckingConflicts, setIsCheckingConflicts] = useState(false)

  // Action state
  const [isProcessing, setIsProcessing] = useState(false)
  const [processingAction, setProcessingAction] = useState<'merge' | 'squash' | 'abandon' | null>(null)

  useEffect(() => {
    if (workspace && parentWorkspace) {
      loadDiff()
      loadUncommittedChanges()
      checkConflicts()
    }
  }, [workspace, parentWorkspace])

  const loadDiff = async () => {
    if (!workspace?.gitBranch || !parentWorkspace?.gitBranch) return

    setLoading(true)
    setError(null)
    try {
      const result = await window.electron.git.getDiffAgainstHead(workspacePath, parentWorkspace.gitBranch)
      if (result.success && result.diff) {
        setDiff(result.diff)
      } else {
        setError(result.error || 'Failed to load diff')
      }
    } catch (err) {
      setError('Failed to load diff')
    }
    setLoading(false)
  }

  const loadUncommittedChanges = async () => {
    try {
      const result = await window.electron.git.getUncommittedChanges(workspacePath)
      if (result.success && result.changes) {
        setUncommitted(result.changes)
      }
    } catch {
      // Ignore errors for uncommitted changes
    }
  }

  const checkConflicts = async () => {
    if (!workspace?.gitBranch || !parentWorkspace?.gitBranch || !parentWorkspace.gitRootPath) return

    setIsCheckingConflicts(true)
    try {
      const result = await window.electron.git.checkMergeConflicts(
        parentWorkspace.gitRootPath,
        workspace.gitBranch,
        parentWorkspace.gitBranch
      )
      if (result.success && result.conflicts) {
        setConflictInfo(result.conflicts)
      }
    } catch {
      // Ignore conflict check errors
    }
    setIsCheckingConflicts(false)
  }

  const loadFileDiff = async (filePath: string) => {
    if (!parentWorkspace?.gitBranch) return

    setSelectedFile(filePath)
    setSelectedUncommittedFile(null)
    setLoadingFileDiff(true)
    setFileDiffContents(null)
    try {
      const result = await window.electron.git.getFileContentsForDiffAgainstHead(workspacePath, parentWorkspace.gitBranch, filePath)
      if (result.success && result.contents) {
        setFileDiffContents(result.contents)
      } else {
        setFileDiffContents(null)
      }
    } catch {
      setFileDiffContents(null)
    }
    setLoadingFileDiff(false)
  }

  const loadUncommittedFileDiff = async (file: UncommittedFile) => {
    setSelectedUncommittedFile(file)
    setSelectedFile(null)
    setLoadingFileDiff(true)
    setFileDiffContents(null)
    try {
      const result = await window.electron.git.getUncommittedFileContentsForDiff(workspacePath, file.path, file.staged)
      if (result.success && result.contents) {
        setFileDiffContents(result.contents)
      } else {
        setFileDiffContents(null)
      }
    } catch {
      setFileDiffContents(null)
    }
    setLoadingFileDiff(false)
  }

  const handleStageFile = async (filePath: string) => {
    const result = await window.electron.git.stageFile(workspacePath, filePath)
    if (result.success) {
      await loadUncommittedChanges()
    }
  }

  const handleUnstageFile = async (filePath: string) => {
    const result = await window.electron.git.unstageFile(workspacePath, filePath)
    if (result.success) {
      await loadUncommittedChanges()
    }
  }

  const handleStageAll = async () => {
    const result = await window.electron.git.stageAll(workspacePath)
    if (result.success) {
      await loadUncommittedChanges()
    }
  }

  const handleUnstageAll = async () => {
    const result = await window.electron.git.unstageAll(workspacePath)
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
      const result = await window.electron.git.commitStaged(workspacePath, commitMessage.trim())
      if (result.success) {
        setCommitMessage('')
        await loadUncommittedChanges()
        await loadDiff()
      } else {
        setCommitError(result.error || 'Failed to commit')
      }
    } catch {
      setCommitError('Failed to commit')
    }

    setCommitting(false)
  }

  const handleMerge = async (squash: boolean) => {
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

  if (!workspace || !parentWorkspace) {
    return <div className="review-browser-error">Workspace or parent workspace not found</div>
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

  return (
    <div className="review-browser">
      {/* Header */}
      <div className="review-header">
        <div className="review-header-info">
          <span className="review-workspace-name">{workspace.name}</span>
          <span className="review-branch-info">
            <span className="review-branch">{workspace.gitBranch}</span>
            <span className="review-arrow">→</span>
            <span className="review-branch">{parentWorkspace.gitBranch}</span>
          </span>
        </div>
        {isCheckingConflicts && (
          <span className="review-checking">Checking for conflicts...</span>
        )}
      </div>

      {/* Conflict Warning */}
      {hasConflicts && (
        <div className="review-conflict-banner">
          <span className="review-conflict-icon">⚠️</span>
          <div className="review-conflict-text">
            <strong>{conflictInfo.conflictedFiles.length} conflict(s) detected</strong>
            <div className="review-conflict-files">
              {conflictInfo.conflictedFiles.slice(0, 3).join(', ')}
              {conflictInfo.conflictedFiles.length > 3 && ` and ${conflictInfo.conflictedFiles.length - 3} more`}
            </div>
          </div>
        </div>
      )}

      {/* View Mode Tabs */}
      <div className="diff-tabs">
        <button
          className={`diff-tab ${viewMode === 'committed' ? 'active' : ''}`}
          onClick={() => setViewMode('committed')}
        >
          Committed Changes
          {hasCommittedChanges && (
            <span className="diff-tab-count">{diff.files.length}</span>
          )}
        </button>
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

                <div className="diff-content">
                  <div className="diff-file-list">
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

                  <div className="diff-file-content">
                    {selectedFile ? (
                      loadingFileDiff ? (
                        <div className="diff-loading">Loading...</div>
                      ) : fileDiffContents ? (
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

                <div className="diff-content">
                  <div className="diff-file-list">
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
        <button
          className="review-action-btn review-abandon-btn"
          onClick={handleAbandon}
          disabled={isProcessing}
          title="Discard all changes and remove this workspace"
        >
          {processingAction === 'abandon' ? 'Abandoning...' : 'Abandon'}
        </button>
        <button
          className="review-action-btn review-cancel-btn"
          onClick={handleCancel}
          disabled={isProcessing}
        >
          Cancel
        </button>
      </div>
    </div>
  )
}
