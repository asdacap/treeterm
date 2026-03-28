import { useEffect, useRef, useState, useMemo } from 'react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import { Loader2 } from 'lucide-react'
import type { SessionState } from '../store/createSessionStore'
import { useAppStore } from '../store/app'
import { useSessionNamesStore } from '../store/sessionNames'
import type { SSHConnectionConfig } from '../types'

type TabId = 'info' | 'bootstrap' | 'start' | 'proxy' | 'json'

const BOOTSTRAP_PREFIXES = ['[bootstrap]', '[bootstrap:err]', '[ssh]']
const START_PREFIXES = ['[start]', '[start:err]']
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

export type SessionInfoPaneProps =
  | { sessionId: string; status: 'connecting'; connectionId: string; config: SSHConnectionConfig }
  | { sessionId: string; status: 'error'; connectionId: string; config: SSHConnectionConfig; error: string }
  | { sessionId: string; status: 'connected'; sessionStore: StoreApi<SessionState> }

export default function SessionInfoPane(props: SessionInfoPaneProps) {
  if (props.status === 'connected') {
    return <ConnectedSessionInfoPane sessionId={props.sessionId} sessionStore={props.sessionStore} />
  }
  return (
    <ConnectingSessionInfoPane
      sessionId={props.sessionId}
      connectionId={props.connectionId}
      config={props.config}
      initialError={props.status === 'error' ? props.error : undefined}
    />
  )
}

// --- ConnectingSessionInfoPane ---

interface ConnectingProps {
  sessionId: string
  connectionId: string
  config: SSHConnectionConfig
  initialError?: string
}

const CONNECTING_TABS: { id: TabId; label: string }[] = [
  { id: 'info', label: 'Info' },
  { id: 'bootstrap', label: 'Bootstrap' },
  { id: 'start', label: 'Start' },
  { id: 'proxy', label: 'Proxy' },
]

function ConnectingSessionInfoPane({ sessionId, connectionId, config, initialError }: ConnectingProps) {
  const ssh = useAppStore(s => s.ssh)
  const [activeTab, setActiveTab] = useState<TabId>('bootstrap')
  const [output, setOutput] = useState<string[]>([])
  const [status, setStatus] = useState<string>(initialError ? 'error' : 'connecting')
  const [error, setError] = useState<string | undefined>(initialError)
  const scrollRef = useRef<HTMLDivElement>(null)

  const label = config.label || `${config.user}@${config.host}`

  useEffect(() => {
    let unsubscribe: (() => void) | undefined
    ssh.watchOutput(connectionId, (line) => {
      setOutput(prev => [...prev, line])
    }).then(({ scrollback, unsubscribe: unsub }) => {
      setOutput(scrollback)
      unsubscribe = unsub
    }).catch(console.error)
    return () => { unsubscribe?.() }
  }, [connectionId, ssh])

  useEffect(() => {
    let unsubscribe: (() => void) | undefined
    ssh.watchConnectionStatus(connectionId, (info) => {
      setStatus(info.status)
      setError(info.error)
    }).then(({ initial, unsubscribe: unsub }) => {
      if (initial) {
        setStatus(initial.status)
        setError(initial.error)
      }
      unsubscribe = unsub
    }).catch(console.error)
    return () => { unsubscribe?.() }
  }, [connectionId, ssh])

  const filteredOutput = useMemo(() => {
    if (activeTab === 'bootstrap') return filterLines(output, BOOTSTRAP_PREFIXES)
    if (activeTab === 'start') return filterLines(output, START_PREFIXES)
    if (activeTab === 'proxy') return filterLines(output, PROXY_PREFIXES)
    return []
  }, [output, activeTab])

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [filteredOutput.length])

  return (
    <div className="ssh-pane">
      <div className="ssh-pane-header">
        <span
          className="ssh-pane-status-dot"
          style={{ backgroundColor: getStatusColor(status) }}
        />
        <span className="ssh-pane-label">{label}</span>
        {status === 'connecting' && (
          <Loader2 size={14} className="spinning" style={{ marginLeft: 8 }} />
        )}
        <span className="ssh-pane-status-text">({status})</span>
        {error && (
          <span className="ssh-pane-error">{error}</span>
        )}
      </div>
      <div className="ssh-pane-tabs">
        {CONNECTING_TABS.map(tab => (
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
          <div className="ssh-pane-output-line">Connection: SSH</div>
          <div className="ssh-pane-output-line">Host: {config.host}</div>
          <div className="ssh-pane-output-line">User: {config.user}</div>
          <div className="ssh-pane-output-line">Port: {config.port}</div>
          {config.identityFile && (
            <div className="ssh-pane-output-line">Identity File: {config.identityFile}</div>
          )}
          <div className="ssh-pane-output-line">Status: {status}</div>
          {error && (
            <div className="ssh-pane-output-line">Error: {error}</div>
          )}
        </div>
      ) : (
        <div className="ssh-pane-output" ref={scrollRef}>
          {filteredOutput.length === 0 && (
            <div className="ssh-pane-output-empty">
              No {activeTab} output yet...
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

// --- ConnectedSessionInfoPane ---

interface ConnectedProps {
  sessionId: string
  sessionStore: StoreApi<SessionState>
}

function ConnectedSessionInfoPane({ sessionId, sessionStore }: ConnectedProps) {
  const connection = useStore(sessionStore, s => s.connection)
  const isRemote = connection?.target.type === 'remote'

  const [activeTab, setActiveTab] = useState<TabId>('info')
  const [output, setOutput] = useState<string[]>([])
  const [sessionJson, setSessionJson] = useState<string | null>(null)
  const [jsonLoading, setJsonLoading] = useState(false)
  const [jsonError, setJsonError] = useState<string | null>(null)
  const ssh = useAppStore(s => s.ssh)
  const sessionApi = useAppStore(s => s.sessionApi)
  const scrollRef = useRef<HTMLDivElement>(null)
  const displayName = useSessionNamesStore(s => s.names[sessionId]?.name)

  const fetchSessionJson = async () => {
    setJsonLoading(true)
    setJsonError(null)
    try {
      const result = await sessionApi.get(sessionId)
      if (result.success && result.session) {
        setSessionJson(JSON.stringify(result.session, null, 2))
      } else {
        setJsonError(result.error || 'Failed to fetch session')
      }
    } catch (err) {
      setJsonError(err instanceof Error ? err.message : 'Unknown error')
    } finally {
      setJsonLoading(false)
    }
  }

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
    if (activeTab === 'bootstrap') return filterLines(output, BOOTSTRAP_PREFIXES)
    if (activeTab === 'start') return filterLines(output, START_PREFIXES)
    if (activeTab === 'proxy') return filterLines(output, PROXY_PREFIXES)
    return []
  }, [output, activeTab])

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [filteredOutput.length])

  const label = isRemote && connection.target.type === 'remote'
    ? `${connection.target.config.user}@${connection.target.config.host}`
    : displayName || sessionId

  const tabs: { id: TabId; label: string }[] = isRemote
    ? [{ id: 'info', label: 'Info' }, { id: 'bootstrap', label: 'Bootstrap' }, { id: 'start', label: 'Start' }, { id: 'proxy', label: 'Proxy' }, { id: 'json', label: 'JSON' }]
    : [{ id: 'info', label: 'Info' }, { id: 'json', label: 'JSON' }]

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
      ) : activeTab === 'json' ? (
        <div className="ssh-pane-output">
          <div style={{ padding: '8px', borderBottom: '1px solid var(--border-color)' }}>
            <button
              className="ssh-pane-tab active"
              onClick={fetchSessionJson}
              disabled={jsonLoading}
              style={{ borderBottom: 'none' }}
            >
              {jsonLoading ? 'Loading...' : 'Refresh'}
            </button>
          </div>
          {jsonError && (
            <div className="ssh-pane-output-line" style={{ color: '#f44336' }}>
              Error: {jsonError}
            </div>
          )}
          {sessionJson ? (
            <pre style={{ margin: 0, padding: '8px', fontSize: '12px', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
              {sessionJson}
            </pre>
          ) : !jsonLoading && !jsonError && (
            <div className="ssh-pane-output-empty">
              Click Refresh to load session JSON
            </div>
          )}
        </div>
      ) : (
        <div className="ssh-pane-output" ref={scrollRef}>
          {filteredOutput.length === 0 && (
            <div className="ssh-pane-output-empty">
              No {activeTab === 'bootstrap' ? 'bootstrap' : activeTab === 'start' ? 'start' : 'proxy'} output yet...
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
