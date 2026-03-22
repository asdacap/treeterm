import { useEffect, useRef, useState, useMemo } from 'react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import type { SessionState } from '../store/createSessionStore'
import { useAppStore } from '../store/app'
import { useSessionNamesStore } from '../store/sessionNames'

type TabId = 'info' | 'daemon' | 'proxy'

const DAEMON_PREFIXES = ['[bootstrap]', '[bootstrap:err]', '[ssh]']
const PROXY_PREFIXES = ['[tunnel]', '[tunnel:err]']

function filterLines(lines: string[], prefixes: string[]): string[] {
  return lines.filter(line =>
    prefixes.some(prefix => line.includes(prefix))
  )
}

function getStatusColor(status: string | undefined): string {
  switch (status) {
    case 'connected': return '#4caf50'
    case 'connecting': return '#ff9800'
    case 'error': return '#f44336'
    default: return '#666'
  }
}

interface SessionInfoPaneProps {
  sessionId: string
  sessionStore: StoreApi<SessionState>
}

export default function SessionInfoPane({ sessionId, sessionStore }: SessionInfoPaneProps) {
  const connection = useStore(sessionStore, s => s.connection)
  const isRemote = connection?.target.type === 'remote'

  const [activeTab, setActiveTab] = useState<TabId>('info')
  const [output, setOutput] = useState<string[]>([])
  const ssh = useAppStore(s => s.ssh)
  const scrollRef = useRef<HTMLDivElement>(null)
  const displayName = useSessionNamesStore(s => s.names[sessionId]?.name)

  // Watch SSH output (only for remote connections)
  useEffect(() => {
    if (!isRemote || !connection) return
    let unsubscribe: (() => void) | undefined

    ssh.watchOutput(connection.id, (line) => {
      setOutput(prev => [...prev, line])
    }).then(({ scrollback, unsubscribe: unsub }) => {
      setOutput(scrollback)
      unsubscribe = unsub
    }).catch(console.error)

    return () => { unsubscribe?.() }
  }, [isRemote, connection, ssh])

  const filteredOutput = useMemo(() => {
    if (activeTab === 'daemon') return filterLines(output, DAEMON_PREFIXES)
    if (activeTab === 'proxy') return filterLines(output, PROXY_PREFIXES)
    return []
  }, [output, activeTab])

  // Auto-scroll
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [filteredOutput.length])

  const label = isRemote && connection.target.type === 'remote'
    ? `${connection.target.config.user}@${connection.target.config.host}`
    : displayName || sessionId

  const tabs: { id: TabId; label: string }[] = isRemote
    ? [{ id: 'info', label: 'Info' }, { id: 'daemon', label: 'Daemon' }, { id: 'proxy', label: 'Proxy' }]
    : [{ id: 'info', label: 'Info' }]

  return (
    <div className="ssh-pane">
      <div className="ssh-pane-header">
        {isRemote && (
          <span
            className="ssh-pane-status-dot"
            style={{ backgroundColor: getStatusColor(connection?.status) }}
          />
        )}
        <span className="ssh-pane-label">{label}</span>
        {isRemote && connection?.status && (
          <span className="ssh-pane-status-text">({connection.status})</span>
        )}
        {isRemote && connection?.error && (
          <span className="ssh-pane-error">{connection.error}</span>
        )}
        {!isRemote && (
          <span className="ssh-pane-status-text">(Local)</span>
        )}
      </div>
      <div className="ssh-pane-tabs">
        {tabs.map(tab => (
          <button
            key={tab.id}
            className={`ssh-pane-tab ${activeTab === tab.id ? 'active' : ''}`}
            onClick={() => setActiveTab(tab.id)}
          >
            {tab.label}
          </button>
        ))}
      </div>
      {activeTab === 'info' ? (
        <div className="ssh-pane-output">
          <div className="ssh-pane-output-line">Session ID: {sessionId}</div>
          {isRemote && connection.target.type === 'remote' ? (
            <>
              <div className="ssh-pane-output-line">Connection: SSH</div>
              <div className="ssh-pane-output-line">Host: {connection.target.config.host}</div>
              <div className="ssh-pane-output-line">User: {connection.target.config.user}</div>
              <div className="ssh-pane-output-line">Port: {connection.target.config.port}</div>
              {connection.target.config.identityFile && (
                <div className="ssh-pane-output-line">Identity File: {connection.target.config.identityFile}</div>
              )}
              <div className="ssh-pane-output-line">Status: {connection.status}</div>
              {connection.error && (
                <div className="ssh-pane-output-line">Error: {connection.error}</div>
              )}
            </>
          ) : (
            <div className="ssh-pane-output-line">Connection: Local</div>
          )}
        </div>
      ) : (
        <div className="ssh-pane-output" ref={scrollRef}>
          {filteredOutput.length === 0 && (
            <div className="ssh-pane-output-empty">
              No {activeTab === 'daemon' ? 'daemon' : 'proxy'} output yet...
            </div>
          )}
          {filteredOutput.map((line, i) => (
            <div key={i} className="ssh-pane-output-line">{line}</div>
          ))}
        </div>
      )}
    </div>
  )
}
