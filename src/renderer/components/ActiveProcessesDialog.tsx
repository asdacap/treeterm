import { useState, useEffect, useRef, useCallback } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import type { TerminalApi, SessionInfo, Workspace } from '../types'

interface ActiveProcessesDialogProps {
  terminalApi: TerminalApi
  workspaces: Record<string, Workspace>
  onClose: () => void
}

function formatRelativeTime(timestamp: number): string {
  const seconds = Math.floor((Date.now() - timestamp) / 1000)
  if (seconds < 60) return `${seconds}s ago`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  const days = Math.floor(hours / 24)
  return `${days}d ago`
}

function lastSegment(cwd: string): string {
  const parts = cwd.split('/')
  return parts[parts.length - 1] || cwd
}

function PtyViewer({ ptyId, terminalApi }: { ptyId: string; terminalApi: TerminalApi }) {
  const containerRef = useRef<HTMLDivElement>(null)
  const termRef = useRef<Terminal | null>(null)
  const fitAddonRef = useRef<FitAddon | null>(null)

  useEffect(() => {
    const container = containerRef.current
    if (!container) return

    const term = new Terminal({
      fontSize: 13,
      fontFamily: 'monospace',
      theme: {
        background: '#1e1e1e',
        foreground: '#d4d4d4',
      },
      cursorBlink: true,
      scrollback: 5000,
    })

    const fitAddon = new FitAddon()
    term.loadAddon(fitAddon)
    term.open(container)
    fitAddon.fit()

    termRef.current = term
    fitAddonRef.current = fitAddon

    // Attach to PTY and get scrollback
    terminalApi.attach(ptyId).then((result) => {
      if (result.success && result.scrollback) {
        for (const line of result.scrollback) {
          term.write(line)
        }
      }
    })

    // Subscribe to data
    const unsubData = terminalApi.onData(ptyId, (data) => {
      term.write(data)
    })

    // Subscribe to exit
    const unsubExit = terminalApi.onExit(ptyId, () => {
      term.write('\r\n\x1b[90m[Process exited]\x1b[0m\r\n')
    })

    // Forward input
    const onDataDisposable = term.onData((data) => {
      terminalApi.write(ptyId, data)
    })

    // Handle resize
    const onResizeDisposable = term.onResize(({ cols, rows }) => {
      terminalApi.resize(ptyId, cols, rows)
    })

    // Observe container size changes
    const resizeObserver = new ResizeObserver(() => {
      fitAddon.fit()
    })
    resizeObserver.observe(container)

    return () => {
      resizeObserver.disconnect()
      onDataDisposable.dispose()
      onResizeDisposable.dispose()
      unsubData()
      unsubExit()
      terminalApi.detach(ptyId)
      term.dispose()
      termRef.current = null
      fitAddonRef.current = null
    }
  }, [ptyId, terminalApi])

  return <div ref={containerRef} className="active-processes-pty-viewer" />
}

function getDisplayName(cwd: string, workspaces: Record<string, Workspace>): string {
  const workspace = Object.values(workspaces).find((ws) => ws.path === cwd)
  if (workspace) return workspace.name
  return lastSegment(cwd)
}

export default function ActiveProcessesDialog({ terminalApi, workspaces, onClose }: ActiveProcessesDialogProps) {
  const [sessions, setSessions] = useState<SessionInfo[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)

  const fetchSessions = useCallback(async () => {
    const list = await terminalApi.list()
    setSessions(list)
    // Clear selection if selected PTY no longer exists
    if (selectedId && !list.find((s) => s.id === selectedId)) {
      setSelectedId(null)
    }
  }, [terminalApi, selectedId])

  useEffect(() => {
    fetchSessions()
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  const handleStop = useCallback(async () => {
    if (!selectedId) return
    terminalApi.kill(selectedId)
    setSelectedId(null)
    // Refresh list after a small delay to let the kill propagate
    setTimeout(async () => {
      const list = await terminalApi.list()
      setSessions(list)
    }, 200)
  }, [selectedId, terminalApi])

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
                onClick={() => setSelectedId(session.id)}
              >
                <div className="active-processes-item-name">{getDisplayName(session.cwd, workspaces)}</div>
                <div className="active-processes-item-meta">
                  <span className="active-processes-item-id">{session.id.slice(0, 8)}</span>
                  <span className="active-processes-item-time">
                    {formatRelativeTime(session.createdAt)}
                  </span>
                  <span className="active-processes-item-clients">
                    {session.attachedClients} client{session.attachedClients !== 1 ? 's' : ''}
                  </span>
                </div>
              </div>
            ))}
          </div>
          <div className="active-processes-detail">
            {selectedId ? (
              <>
                <div className="active-processes-toolbar">
                  <button className="active-processes-stop-btn" onClick={handleStop}>
                    Stop Process
                  </button>
                </div>
                <PtyViewer key={selectedId} ptyId={selectedId} terminalApi={terminalApi} />
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
