import { useEffect, useRef, useState, useMemo } from 'react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import { Loader2 } from 'lucide-react'
import type { SessionState } from '../store/createSessionStore'
import { useAppStore } from '../store/app'
import { useSessionNamesStore } from '../store/sessionNames'
import type { SSHConnectionConfig } from '../types'

type TabId = 'info' | 'ssh' | 'json'

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
  { id: 'ssh', label: 'SSH' },
]

function ConnectingSessionInfoPane({ sessionId, connectionId, config, initialError }: ConnectingProps) {
  const ssh = useAppStore(s => s.ssh)
  const removeSession = useAppStore(s => s.removeSession)
  const startRemoteConnect = useAppStore(s => s.startRemoteConnect)
  const disconnectSession = useAppStore(s => s.disconnectSession)
  const [activeTab, setActiveTab] = useState<TabId>('ssh')
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

  const sshOutput = useMemo(() => ({
    bootstrap: filterLines(output, BOOTSTRAP_PREFIXES),
    start: filterLines(output, START_PREFIXES),
    proxy: filterLines(output, PROXY_PREFIXES),
  }), [output])

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [sshOutput.bootstrap.length, sshOutput.start.length, sshOutput.proxy.length])

  const handleRetry = () => {
    removeSession(sessionId)
    startRemoteConnect(config)
    ssh.connect(config).then(({ info, session }) => {
      if (info.status !== 'connected' || !session) {
        useAppStore.getState().setSessionError(config.id, info.error || 'Connection failed')
        return
      }
      useAppStore.getState().addRemoteSession(session, info)
    }).catch((err) => {
      useAppStore.getState().setSessionError(config.id, err instanceof Error ? err.message : String(err))
    })
  }

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
        {status === 'error' && (
          <div style={{ marginLeft: 'auto', display: 'flex', gap: 8 }}>
            <button className="ssh-pane-tab" onClick={handleRetry}>Retry</button>
            <button className="ssh-pane-tab" style={{ color: '#f44336' }} onClick={() => disconnectSession(sessionId)}>Remove</button>
          </div>
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
          {sshOutput.bootstrap.length === 0 && sshOutput.start.length === 0 && sshOutput.proxy.length === 0 && (
            <div className="ssh-pane-output-empty">
              Waiting for SSH output...
            </div>
          )}
          {sshOutput.bootstrap.length > 0 && (
            <>
              <div className="ssh-pane-section-header">── Bootstrap ──</div>
              {sshOutput.bootstrap.map((line, i) => (
                <div key={`bootstrap-${i}`} className="ssh-pane-output-line">{line}</div>
              ))}
            </>
          )}
          {sshOutput.start.length > 0 && (
            <>
              <div className="ssh-pane-section-header">── Start ──</div>
              {sshOutput.start.map((line, i) => (
                <div key={`start-${i}`} className="ssh-pane-output-line">{line}</div>
              ))}
            </>
          )}
          {sshOutput.proxy.length > 0 && (
            <>
              <div className="ssh-pane-section-header">── Proxy ──</div>
              {sshOutput.proxy.map((line, i) => (
                <div key={`proxy-${i}`} className="ssh-pane-output-line">{line}</div>
              ))}
            </>
          )}
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

  const sshOutput = useMemo(() => ({
    bootstrap: filterLines(output, BOOTSTRAP_PREFIXES),
    start: filterLines(output, START_PREFIXES),
    proxy: filterLines(output, PROXY_PREFIXES),
  }), [output])

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [sshOutput.bootstrap.length, sshOutput.start.length, sshOutput.proxy.length])

  const label = isRemote && connection.target.type === 'remote'
    ? `${connection.target.config.user}@${connection.target.config.host}`
    : displayName || sessionId

  const tabs: { id: TabId; label: string }[] = isRemote
    ? [{ id: 'info', label: 'Info' }, { id: 'ssh', label: 'SSH' }, { id: 'json', label: 'JSON' }]
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
          {sshOutput.bootstrap.length === 0 && sshOutput.start.length === 0 && sshOutput.proxy.length === 0 && (
            <div className="ssh-pane-output-empty">
              Waiting for SSH output...
            </div>
          )}
          {sshOutput.bootstrap.length > 0 && (
            <>
              <div className="ssh-pane-section-header">── Bootstrap ──</div>
              {sshOutput.bootstrap.map((line, i) => (
                <div key={`bootstrap-${i}`} className="ssh-pane-output-line">{line}</div>
              ))}
            </>
          )}
          {sshOutput.start.length > 0 && (
            <>
              <div className="ssh-pane-section-header">── Start ──</div>
              {sshOutput.start.map((line, i) => (
                <div key={`start-${i}`} className="ssh-pane-output-line">{line}</div>
              ))}
            </>
          )}
          {sshOutput.proxy.length > 0 && (
            <>
              <div className="ssh-pane-section-header">── Proxy ──</div>
              {sshOutput.proxy.map((line, i) => (
                <div key={`proxy-${i}`} className="ssh-pane-output-line">{line}</div>
              ))}
            </>
          )}
        </div>
      )}
    </div>
  )
}
