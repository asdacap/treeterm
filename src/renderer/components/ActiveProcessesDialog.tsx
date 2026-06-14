import { useState, useEffect, useCallback } from 'react'
import { useAppStore } from '../store/app'
import type { TTYSessionInfo, Workspace } from '../types'
import PtyViewer from './PtyViewer'
import { formatRelativeTime, getDisplayName } from '../utils/ttyDisplay'

interface ActiveProcessesDialogProps {
  workspaces: Record<string, Workspace>
  connectionId: string
  onClose: () => void
}

export default function ActiveProcessesDialog({ workspaces, connectionId, onClose }: ActiveProcessesDialogProps) {
  const terminalApi = useAppStore(s => s.terminal)
  const [sessions, setSessions] = useState<TTYSessionInfo[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    void terminalApi.list(connectionId).then(list => {
      if (cancelled) return
      setSessions(list)
      // Clear selection if selected PTY no longer exists
      if (selectedId && !list.find((s) => s.id === selectedId)) {
        setSelectedId(null)
      }
    })
    return () => { cancelled = true }
  }, [terminalApi, connectionId, selectedId])

  const handleStop = useCallback(async () => {
    if (!selectedId) return
    terminalApi.kill(connectionId, selectedId)
    setSelectedId(null)
    const list = await terminalApi.list(connectionId)
    setSessions(list)
  }, [selectedId, connectionId, terminalApi])

  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) onClose()
    },
    [onClose]
  )

  return (
    <div className="dialog-overlay" onClick={handleOverlayClick}>
      <div className="active-processes-dialog">
        <div className="active-processes-header">
          <h2>Active Processes</h2>
          <button className="dialog-close" onClick={onClose}>
            &times;
          </button>
        </div>
        <div className="active-processes-body">
          <div className="active-processes-list">
            {sessions.length === 0 && (
              <div className="active-processes-empty">No active PTY sessions</div>
            )}
            {sessions.map((session) => (
              <div
                key={session.id}
                className={`active-processes-item ${selectedId === session.id ? 'selected' : ''}`}
                onClick={() => { setSelectedId(session.id); }}
              >
                <div className="active-processes-item-name">{getDisplayName(session.cwd, workspaces)}</div>
                <div className="active-processes-item-meta">
                  <span className="active-processes-item-id">{session.id.slice(0, 8)}</span>
                  <span className="active-processes-item-time">
                    {formatRelativeTime(session.createdAt)}
                  </span>
                </div>
              </div>
            ))}
          </div>
          <div className="active-processes-detail">
            {selectedId ? (
              <>
                <div className="active-processes-toolbar">
                  <button className="active-processes-stop-btn" onClick={() => { void handleStop(); }}>
                    Stop Process
                  </button>
                </div>
                <PtyViewer key={selectedId} ptyId={selectedId} connectionId={connectionId} terminalApi={terminalApi} />
              </>
            ) : (
              <div className="active-processes-empty">Select a process to view its terminal</div>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
