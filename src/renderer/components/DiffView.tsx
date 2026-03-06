import { useState, useEffect } from 'react'
import type { DiffFile, DiffResult } from '../types'

interface DiffViewProps {
  worktreePath: string
  parentBranch: string
}

export default function DiffView({ worktreePath, parentBranch }: DiffViewProps) {
  const [diff, setDiff] = useState<DiffResult | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [selectedFile, setSelectedFile] = useState<string | null>(null)
  const [fileDiff, setFileDiff] = useState<string | null>(null)
  const [loadingFileDiff, setLoadingFileDiff] = useState(false)

  useEffect(() => {
    loadDiff()
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
      setError('Failed to load diff')
    }
    setLoading(false)
  }

  const loadFileDiff = async (filePath: string) => {
    setSelectedFile(filePath)
    setLoadingFileDiff(true)
    try {
      const result = await window.electron.git.getFileDiff(worktreePath, parentBranch, filePath)
      if (result.success) {
        setFileDiff(result.diff || '')
      } else {
        setFileDiff(`Error: ${result.error}`)
      }
    } catch {
      setFileDiff('Failed to load file diff')
    }
    setLoadingFileDiff(false)
  }

  const getStatusIcon = (status: DiffFile['status']) => {
    switch (status) {
      case 'added':
        return <span className="diff-status added">A</span>
      case 'modified':
        return <span className="diff-status modified">M</span>
      case 'deleted':
        return <span className="diff-status deleted">D</span>
      case 'renamed':
        return <span className="diff-status renamed">R</span>
    }
  }

  if (loading) {
    return <div className="diff-loading">Loading diff...</div>
  }

  if (error) {
    return <div className="diff-error">{error}</div>
  }

  if (!diff || diff.files.length === 0) {
    return <div className="diff-empty">No changes to show</div>
  }

  return (
    <div className="diff-view">
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
            ) : (
              <pre className="diff-code">{fileDiff}</pre>
            )
          ) : (
            <div className="diff-placeholder">Select a file to view changes</div>
          )}
        </div>
      </div>
    </div>
  )
}
