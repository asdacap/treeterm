import { useState, useEffect, useCallback } from 'react'
import type { DiffFile, DiffResult, UncommittedFile, UncommittedChanges, FileDiffContents } from '../types'
import { MonacoDiffViewer } from './MonacoDiffViewer'

interface DiffViewProps {
  worktreePath: string
  parentBranch: string
}

type ViewMode = 'committed' | 'uncommitted'

export default function DiffView({ worktreePath, parentBranch }: DiffViewProps) {
  const [diff, setDiff] = useState<DiffResult | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [selectedFile, setSelectedFile] = useState<string | null>(null)
  const [fileDiffContents, setFileDiffContents] = useState<FileDiffContents | null>(null)
  const [loadingFileDiff, setLoadingFileDiff] = useState(false)

  // Uncommitted changes state
  const [uncommitted, setUncommitted] = useState<UncommittedChanges | null>(null)
  const [loadingUncommitted, setLoadingUncommitted] = useState(false)
  const [viewMode, setViewMode] = useState<ViewMode>('committed')
  const [selectedUncommittedFile, setSelectedUncommittedFile] = useState<UncommittedFile | null>(null)

  // Commit state
  const [commitMessage, setCommitMessage] = useState('')
  const [committing, setCommitting] = useState(false)
  const [commitError, setCommitError] = useState<string | null>(null)

  // Resize state
  const [fileListWidth, setFileListWidth] = useState(250)
  const [isResizing, setIsResizing] = useState(false)

  useEffect(() => {
    loadDiff()
    loadUncommittedChanges()
  }, [worktreePath, parentBranch])

  const loadDiff = async () => {
    setLoading(true)
    setError(null)
    try {
      const result = await window.electron.git.getDiff(worktreePath, parentBranch)
      if (result.success && result.diff) {
        setDiff(result.diff)
      } else {
        setError(result.error || 'Failed to load diff')
      }
    } catch (err) {
      console.error('[DiffView] Error loading diff:', err)
      setError(`Failed to load diff: ${err instanceof Error ? err.message : 'Unknown error'}`)
    }
    setLoading(false)
  }

  const loadUncommittedChanges = async () => {
    setLoadingUncommitted(true)
    try {
      const result = await window.electron.git.getUncommittedChanges(worktreePath)
      if (result.success && result.changes) {
        setUncommitted(result.changes)
      }
    } catch {
      // Ignore errors for uncommitted changes
    }
    setLoadingUncommitted(false)
  }

  const loadFileDiff = async (filePath: string) => {
    setSelectedFile(filePath)
    setSelectedUncommittedFile(null)
    setLoadingFileDiff(true)
    setFileDiffContents(null)
    try {
      const result = await window.electron.git.getFileContentsForDiff(worktreePath, parentBranch, filePath)
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
      const result = await window.electron.git.getUncommittedFileContentsForDiff(worktreePath, file.path, file.staged)
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
    const result = await window.electron.git.stageFile(worktreePath, filePath)
    if (result.success) {
      await loadUncommittedChanges()
    }
  }

  const handleUnstageFile = async (filePath: string) => {
    const result = await window.electron.git.unstageFile(worktreePath, filePath)
    if (result.success) {
      await loadUncommittedChanges()
    }
  }

  const handleStageAll = async () => {
    const result = await window.electron.git.stageAll(worktreePath)
    if (result.success) {
      await loadUncommittedChanges()
    }
  }

  const handleUnstageAll = async () => {
    const result = await window.electron.git.unstageAll(worktreePath)
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
      const result = await window.electron.git.commitStaged(worktreePath, commitMessage.trim())
      if (result.success) {
        setCommitMessage('')
        await loadUncommittedChanges()
        await loadDiff() // Refresh committed diff too
      } else {
        setCommitError(result.error || 'Failed to commit')
      }
    } catch {
      setCommitError('Failed to commit')
    }

    setCommitting(false)
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
      setFileListWidth(newWidth)
    },
    [isResizing]
  )

  const handleMouseUp = useCallback(() => {
    setIsResizing(false)
  }, [])

  if (loading) {
    return <div className="diff-loading">Loading diff...</div>
  }

  if (error) {
    return <div className="diff-error">{error}</div>
  }

  const hasCommittedChanges = diff && diff.files.length > 0

  return (
    <div className="diff-view">
      {/* View mode tabs */}
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

      {viewMode === 'committed' ? (
        <>
          {!hasCommittedChanges ? (
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

              <div className="diff-content" onMouseMove={handleMouseMove} onMouseUp={handleMouseUp} onMouseLeave={handleMouseUp}>
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

                <div className={`divider ${isResizing ? 'active' : ''}`} onMouseDown={handleMouseDown} />

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
          )}
        </>
      ) : (
        <>
          {loadingUncommitted ? (
            <div className="diff-loading">Loading uncommitted changes...</div>
          ) : !hasUncommitted ? (
            <div className="diff-empty">No uncommitted changes</div>
          ) : (
            <>
              {/* Commit section */}
              <div className="commit-section">
                <div className="commit-input-row">
                  <input
                    type="text"
                    className="commit-message-input"
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
                    className="commit-button"
                    onClick={handleCommit}
                    disabled={committing || stagedFiles.length === 0}
                  >
                    {committing ? 'Committing...' : 'Commit'}
                  </button>
                </div>
                {commitError && <div className="commit-error">{commitError}</div>}
              </div>

              <div className="diff-content" onMouseMove={handleMouseMove} onMouseUp={handleMouseUp} onMouseLeave={handleMouseUp}>
                <div className="diff-file-list" style={{ width: fileListWidth }}>
                  {/* Staged files */}
                  {stagedFiles.length > 0 && (
                    <div className="diff-file-group">
                      <div className="diff-file-group-header">
                        <span>Staged Changes</span>
                        <button className="diff-stage-btn" onClick={handleUnstageAll}>
                          Unstage All
                        </button>
                      </div>
                      {stagedFiles.map((file) => (
                        <div
                          key={`staged-${file.path}`}
                          className={`diff-file-item ${selectedUncommittedFile?.path === file.path && selectedUncommittedFile?.staged ? 'selected' : ''}`}
                          onClick={() => loadUncommittedFileDiff(file)}
                        >
                          {getStatusIcon(file.status)}
                          <span className="diff-file-path">{file.path}</span>
                          <span className="diff-file-actions">
                            <button
                              className="diff-action-btn"
                              onClick={(e) => {
                                e.stopPropagation()
                                handleUnstageFile(file.path)
                              }}
                              title="Unstage"
                            >
                              -
                            </button>
                          </span>
                        </div>
                      ))}
                    </div>
                  )}

                  {/* Unstaged files */}
                  {unstagedFiles.length > 0 && (
                    <div className="diff-file-group">
                      <div className="diff-file-group-header">
                        <span>Changes</span>
                        <button className="diff-stage-btn" onClick={handleStageAll}>
                          Stage All
                        </button>
                      </div>
                      {unstagedFiles.map((file) => (
                        <div
                          key={`unstaged-${file.path}`}
                          className={`diff-file-item ${selectedUncommittedFile?.path === file.path && !selectedUncommittedFile?.staged ? 'selected' : ''}`}
                          onClick={() => loadUncommittedFileDiff(file)}
                        >
                          {getStatusIcon(file.status)}
                          <span className="diff-file-path">{file.path}</span>
                          <span className="diff-file-actions">
                            <button
                              className="diff-action-btn"
                              onClick={(e) => {
                                e.stopPropagation()
                                handleStageFile(file.path)
                              }}
                              title="Stage"
                            >
                              +
                            </button>
                          </span>
                        </div>
                      ))}
                    </div>
                  )}
                </div>

                <div className={`divider ${isResizing ? 'active' : ''}`} onMouseDown={handleMouseDown} />

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
            </>
          )}
        </>
      )}
    </div>
  )
}
