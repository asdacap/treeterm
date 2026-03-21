import { useState, useEffect, useRef, useCallback } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { useAppStore } from '../store/app'
import type { TerminalApi, SessionInfo, Workspace } from '../types'

interface ActiveProcessesDialogProps {
  workspaces: Record<string, Workspace>
  connectionId: string
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

function PtyViewer({ ptyId, connectionId, terminalApi }: { ptyId: string; connectionId: string; terminalApi: TerminalApi }) {
  const containerRef = useRef<HTMLDivElement>(null)
  const termRef = useRef<Terminal | null>(null)
  const fitAddonRef = useRef<FitAddon | null>(null)
  const [status, setStatus] = useState<'loading' | 'ready' | 'error'>('loading')
  const [errorMessage, setErrorMessage] = useState<string | null>(null)

  useEffect(() => {
    const container = containerRef.current
    if (!container) return

    let cancelled = false
    const cleanups: (() => void)[] = []

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

    const resizeObserver = new ResizeObserver(() => {
      fitAddon.fit()
    })
    resizeObserver.observe(container)
    cleanups.push(() => resizeObserver.disconnect())

    // Attach to PTY and get scrollback
    terminalApi.attach(connectionId, ptyId).then((result) => {
      if (cancelled) return

      if (!result.success) {
        setStatus('error')
        setErrorMessage(result.error ?? `Failed to attach to PTY session ${ptyId}`)
        return
      }

      const handle = result.handle
      if (!handle) {
        setStatus('error')
        setErrorMessage(`Attach succeeded but no handle returned for session ${ptyId}`)
        return
      }

      if (result.scrollback) {
        for (const line of result.scrollback) {
          term.write(line)
        }
      }

      // Subscribe to data using handle
      const unsubData = terminalApi.onData(handle, (data) => {
        term.write(data)
      })
      cleanups.push(unsubData)

      // Subscribe to exit using handle
      const unsubExit = terminalApi.onExit(handle, () => {
        term.write('\r\n\x1b[90m[Process exited]\x1b[0m\r\n')
      })
      cleanups.push(unsubExit)

      // Forward input using handle
      const onDataDisposable = term.onData((data) => {
        terminalApi.write(handle, data)
      })
      cleanups.push(() => onDataDisposable.dispose())

      // Handle resize using handle
      const onResizeDisposable = term.onResize(({ cols, rows }) => {
        terminalApi.resize(handle, cols, rows)
      })
      cleanups.push(() => onResizeDisposable.dispose())

      setStatus('ready')
    }).catch((err: unknown) => {
      if (cancelled) return
      setStatus('error')
      setErrorMessage(err instanceof Error ? err.message : `Failed to attach to PTY session ${ptyId}`)
    })

    return () => {
      cancelled = true
      for (const cleanup of cleanups) cleanup()
      term.dispose()
      termRef.current = null
      fitAddonRef.current = null
    }
  }, [ptyId, terminalApi])

  if (status === 'error') {
    return (
      <div className="active-processes-pty-viewer" style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', color: '#f44336', padding: '24px', textAlign: 'center' }}>
        <div>
          <div style={{ fontSize: '16px', fontWeight: 'bold', marginBottom: '8px' }}>Failed to attach to process</div>
          <div style={{ fontSize: '13px', opacity: 0.8 }}>{errorMessage}</div>
        </div>
      </div>
    )
  }

  return (
    <div style={{ position: 'relative', width: '100%', height: '100%' }}>
      {status === 'loading' && (
        <div style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', color: '#888', zIndex: 1 }}>
          Attaching to process...
        </div>
      )}
      <div ref={containerRef} className="active-processes-pty-viewer" />
    </div>
  )
}

function getDisplayName(cwd: string, workspaces: Record<string, Workspace>): string {
  const workspace = Object.values(workspaces).find((ws) => ws.path === cwd)
  if (workspace) return workspace.name
  return lastSegment(cwd)
}

export default function ActiveProcessesDialog({ workspaces, connectionId, onClose }: ActiveProcessesDialogProps) {
  const terminalApi = useAppStore(s => s.terminal)
  const [sessions, setSessions] = useState<SessionInfo[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)

  const fetchSessions = useCallback(async () => {
    const list = await terminalApi.list(connectionId)
    setSessions(list)
    // Clear selection if selected PTY no longer exists
    if (selectedId && !list.find((s) => s.id === selectedId)) {
      setSelectedId(null)
    }
  }, [terminalApi, connectionId, selectedId])

  useEffect(() => {
    fetchSessions()
  }, [])

  const handleStop = useCallback(async () => {
    if (!selectedId) return
    await terminalApi.kill(connectionId, selectedId)
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
                onClick={() => setSelectedId(session.id)}
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
                  <button className="active-processes-stop-btn" onClick={handleStop}>
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
