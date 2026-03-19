import { useState, useMemo } from 'react'
import type { Workspace, Session } from '../types'

interface WorkspacePickerDialogProps {
  sessions: Session[]
  onSelect: (session: Session) => void
  onOpenInNewWindow: (session: Session) => void
  onCreateNew: () => void
  onCancel: () => void
}

export default function WorkspacePickerDialog({
  sessions,
  onSelect,
  onOpenInNewWindow,
  onCreateNew,
  onCancel
}: WorkspacePickerDialogProps) {
  const [selectedSession, setSelectedSession] = useState<Session | null>(null)
  const [searchQuery, setSearchQuery] = useState('')

  const filteredSessions = useMemo(() => {
    if (!searchQuery.trim()) return sessions
    const query = searchQuery.toLowerCase()
    return sessions.filter(session =>
      session.workspaces.some(w =>
        w.name.toLowerCase().includes(query) ||
        w.path.toLowerCase().includes(query)
      )
    )
  }, [sessions, searchQuery])

  // Build workspace hierarchy for each session
  const buildWorkspaceHierarchy = (workspaces: Workspace[]) => {
    const roots: Workspace[] = []
    const children: Map<string, Workspace[]> = new Map()

    for (const workspace of workspaces) {
      if (workspace.parentId === null) {
        roots.push(workspace)
      } else {
        const siblings = children.get(workspace.parentId) || []
        siblings.push(workspace)
        children.set(workspace.parentId, siblings)
      }
    }

    return { rootWorkspaces: roots, childWorkspaces: children }
  }

  const formatTime = (timestamp: number) => {
    const now = Date.now()
    const diff = now - timestamp
    const minutes = Math.floor(diff / 60000)
    const hours = Math.floor(diff / 3600000)
    const days = Math.floor(diff / 86400000)

    if (minutes < 1) return 'just now'
    if (minutes < 60) return `${minutes}m ago`
    if (hours < 24) return `${hours}h ago`
    return `${days}d ago`
  }

  const getStatusIcon = (status: string) => {
    switch (status) {
      case 'active': return '●'
      case 'merged': return '✓'
      case 'abandoned': return '×'
      default: return '○'
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && selectedSession) {
      onSelect(selectedSession)
    }
    if (e.key === 'Escape') {
      onCancel()
    }
  }

  const renderWorkspace = (workspace: Workspace, childWorkspaces: Map<string, Workspace[]>, isChild: boolean = false) => {
    const children = childWorkspaces.get(workspace.id) || []

    return (
      <div key={workspace.path} style={{ marginLeft: isChild ? '20px' : '0' }}>
        <div className="workspace-info">
          <span className="workspace-status">{getStatusIcon(workspace.status)}</span>
          <span className="workspace-name">{workspace.name}</span>
          {workspace.isGitRepo && workspace.gitBranch && (
            <span className="workspace-branch">({workspace.gitBranch})</span>
          )}
          <span className="workspace-tabs">
            {Object.keys(workspace.appStates).length} tab{Object.keys(workspace.appStates).length !== 1 ? 's' : ''}
          </span>
        </div>
        {children.length > 0 && (
          <div className="workspace-children">
            {children.map(child => renderWorkspace(child, childWorkspaces, true))}
          </div>
        )}
      </div>
    )
  }

  const renderSession = (session: Session) => {
    const { rootWorkspaces, childWorkspaces } = buildWorkspaceHierarchy(session.workspaces)
    const totalTabs = session.workspaces.reduce((sum, w) => sum + Object.keys(w.appStates).length, 0)

    return (
      <div
        key={session.id}
        className={`session-picker-item ${selectedSession?.id === session.id ? 'selected' : ''}`}
        onClick={() => setSelectedSession(session)}
        onDoubleClick={() => onSelect(session)}
      >
        <div className="session-header">
          <div className="session-info">
            <span className="session-workspaces">
              {session.workspaces.length} workspace{session.workspaces.length !== 1 ? 's' : ''}
            </span>
            <span className="session-tabs">
              {totalTabs} tab{totalTabs !== 1 ? 's' : ''}
            </span>
            <span className="session-time">Active {formatTime(session.lastActivity)}</span>
          </div>
        </div>
        <div className="session-workspaces-list">
          {rootWorkspaces.map(workspace => renderWorkspace(workspace, childWorkspaces))}
        </div>
      </div>
    )
  }

  return (
    <div className="dialog-overlay" onClick={onCancel}>
      <div
        className="workspace-picker-dialog"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        <div className="workspace-picker-header">
          <h2>Restore Session</h2>
          <button className="dialog-close" onClick={onCancel}>×</button>
        </div>

        <div className="workspace-picker-content">
          <p className="workspace-picker-description">
            Found {sessions.length} session{sessions.length !== 1 ? 's' : ''} in the daemon.
          </p>

          <div className="workspace-picker-search">
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search by workspace name or path..."
              autoFocus
            />
          </div>

          <div className="workspace-picker-list">
            {filteredSessions.length === 0 ? (
              <div className="workspace-picker-empty">No sessions match your search</div>
            ) : (
              filteredSessions.map(session => renderSession(session))
            )}
          </div>
        </div>

        <div className="workspace-picker-actions">
          <button className="dialog-btn secondary" onClick={onCreateNew}>
            Create New
          </button>
          <button className="dialog-btn cancel" onClick={onCancel}>
            Cancel
          </button>
          <button
            className="dialog-btn secondary"
            onClick={() => selectedSession && onOpenInNewWindow(selectedSession)}
            disabled={!selectedSession}
          >
            Open in New Window
          </button>
          <button
            className="dialog-btn primary"
            onClick={() => selectedSession && onSelect(selectedSession)}
            disabled={!selectedSession}
          >
            Restore
          </button>
        </div>
      </div>
    </div>
  )
}
