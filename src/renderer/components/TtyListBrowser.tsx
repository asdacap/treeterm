import { useCallback, useEffect, useState } from 'react'
import type { ReactNode } from 'react'
import { useStore } from 'zustand'
import { useAppStore } from '../store/app'
import type { ApplicationRenderProps, TTYSessionInfo, TerminalState } from '../types'

enum LoadStatus {
  Loading = 'loading',
  Ready = 'ready',
  Error = 'error',
}

type LoadState =
  | { status: LoadStatus.Loading }
  | { status: LoadStatus.Ready; sessions: TTYSessionInfo[] }
  | { status: LoadStatus.Error; message: string }

type LoadStateRenderers = {
  [K in LoadStatus]: (
    state: Extract<LoadState, { status: K }>,
    workspacePath: string,
    onOpen: (s: TTYSessionInfo) => void,
    onKill: (s: TTYSessionInfo) => void
  ) => ReactNode
}

function formatRelativeTime(timestamp: number): string {
  const seconds = Math.floor((Date.now() - timestamp) / 1000)
  if (seconds < 60) return `${String(seconds)}s ago`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${String(minutes)}m ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${String(hours)}h ago`
  const days = Math.floor(hours / 24)
  return `${String(days)}d ago`
}

const BODY_RENDERERS: LoadStateRenderers = {
  [LoadStatus.Loading]: () => (
    <div className="tty-list-empty">Loading TTY sessions…</div>
  ),
  [LoadStatus.Error]: (state) => (
    <div className="tty-list-error">
      <div className="tty-list-error-title">Failed to load TTY sessions</div>
      <div className="tty-list-error-message">{state.message}</div>
    </div>
  ),
  [LoadStatus.Ready]: (state, workspacePath, onOpen, onKill) => {
    const filtered = state.sessions.filter((s) => s.cwd === workspacePath)
    if (filtered.length === 0) {
      return <div className="tty-list-empty">No TTY sessions for this workspace</div>
    }
    return (
      <div className="tty-list-rows">
        {filtered.map((session) => (
          <div key={session.id} className="tty-list-row">
            <div className="tty-list-row-info">
              <div className="tty-list-row-id">{session.id.slice(0, 8)}</div>
              <div className="tty-list-row-meta">
                <span>{String(session.cols)}×{String(session.rows)}</span>
                <span>created {formatRelativeTime(session.createdAt)}</span>
                <span>active {formatRelativeTime(session.lastActivity)}</span>
              </div>
            </div>
            <div className="tty-list-row-actions">
              <button className="tty-list-open-btn" onClick={() => { onOpen(session); }}>Open</button>
              <button className="tty-list-kill-btn" onClick={() => { onKill(session); }}>Kill</button>
            </div>
          </div>
        ))}
      </div>
    )
  },
}

function renderBody(
  state: LoadState,
  workspacePath: string,
  onOpen: (s: TTYSessionInfo) => void,
  onKill: (s: TTYSessionInfo) => void
): ReactNode {
  switch (state.status) {
    case LoadStatus.Loading:
      return BODY_RENDERERS[LoadStatus.Loading](state, workspacePath, onOpen, onKill)
    case LoadStatus.Ready:
      return BODY_RENDERERS[LoadStatus.Ready](state, workspacePath, onOpen, onKill)
    case LoadStatus.Error:
      return BODY_RENDERERS[LoadStatus.Error](state, workspacePath, onOpen, onKill)
  }
}

export default function TtyListBrowser({ workspace }: ApplicationRenderProps) {
  const terminalApi = useAppStore(s => s.terminal)
  const workspacePath = useStore(workspace, s => s.workspace.path)
  const connectionId = useStore(workspace, s => s.connectionId)
  const addTab = useStore(workspace, s => s.addTab)
  const [loadState, setLoadState] = useState<LoadState>({ status: LoadStatus.Loading })
  const [refreshNonce, setRefreshNonce] = useState(0)

  useEffect(() => {
    let cancelled = false
    terminalApi
      .list(connectionId)
      .then((sessions) => {
        if (cancelled) return
        setLoadState({ status: LoadStatus.Ready, sessions })
      })
      .catch((err: unknown) => {
        if (cancelled) return
        const message = err instanceof Error ? err.message : 'Failed to list TTY sessions'
        setLoadState({ status: LoadStatus.Error, message })
      })
    return () => { cancelled = true }
  }, [terminalApi, connectionId, refreshNonce])

  const refresh = useCallback(() => {
    setLoadState({ status: LoadStatus.Loading })
    setRefreshNonce((n) => n + 1)
  }, [])

  const openInTerminal = useCallback(
    (session: TTYSessionInfo) => {
      addTab<TerminalState>('terminal', {
        ptyId: session.id,
        connectionId,
      })
    },
    [addTab, connectionId]
  )

  const killSession = useCallback(
    (session: TTYSessionInfo) => {
      terminalApi.kill(connectionId, session.id)
      refresh()
    },
    [terminalApi, connectionId, refresh]
  )

  return (
    <div className="tty-list-browser">
      <div className="tty-list-toolbar">
        <h3 className="tty-list-title">TTY sessions for {workspacePath}</h3>
        <button className="tty-list-refresh-btn" onClick={refresh}>Refresh</button>
      </div>
      {renderBody(loadState, workspacePath, openInTerminal, killSession)}
    </div>
  )
}
