import { useEffect, useRef, useState, useMemo } from 'react'
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import { Loader2 } from 'lucide-react'
import type { SessionState } from '../store/createSessionStore'
import { useAppStore } from '../store/app'
import { useSessionNamesStore } from '../store/sessionNames'
import type { SSHConnectionConfig, PortForwardInfo } from '../types'
import PortForwardDialog from './PortForwardDialog'

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

function getPfStatusColor(status: string | undefined): string {
  switch (status) {
    case 'active': return '#4caf50'
    case 'connecting': return '#ff9800'
    case 'error': return '#f44336'
    case 'stopped': return '#666'
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
  const [portForwards, setPortForwards] = useState<PortForwardInfo[]>([])
  const [showPortForwardDialog, setShowPortForwardDialog] = useState(false)
  const [expandedPfOutput, setExpandedPfOutput] = useState<Record<string, string[]>>({})
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

  useEffect(() => {
    if (!isRemote || !connection) return
    ssh.listPortForwards(connection.id).then(setPortForwards).catch(console.error)
  }, [isRemote, connection, ssh])

  useEffect(() => {
    if (!isRemote) return
    const unsubscribe = ssh.onPortForwardStatus((info) => {
      if (!connection || info.connectionId !== connection.id) return
      setPortForwards(prev => {
        const idx = prev.findIndex(p => p.id === info.id)
        if (idx >= 0) {
          const next = [...prev]
          next[idx] = info
          return next
        }
        return [...prev, info]
      })
    })
    return unsubscribe
  }, [isRemote, connection, ssh])

  const handleTogglePfOutput = (pfId: string, currentlyExpanded: boolean) => {
    if (currentlyExpanded) {
      setExpandedPfOutput(prev => {
        const next = { ...prev }
        delete next[pfId]
        return next
      })
    } else {
      ssh.watchPortForwardOutput(pfId, (line) => {
        setExpandedPfOutput(prev => ({
          ...prev,
          [pfId]: [...(prev[pfId] ?? []), line]
        }))
      }).then(({ scrollback, unsubscribe: _ }) => {
        setExpandedPfOutput(prev => ({ ...prev, [pfId]: scrollback }))
      }).catch(console.error)
      setExpandedPfOutput(prev => ({ ...prev, [pfId]: [] }))
    }
  }

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
      {showPortForwardDialog && isRemote && connection && (
        <PortForwardDialog
          connectionId={connection.id}
          onClose={() => setShowPortForwardDialog(false)}
          onCreated={(info) => {
            setPortForwards(prev => {
              const idx = prev.findIndex(p => p.id === info.id)
              if (idx >= 0) {
                const next = [...prev]
                next[idx] = info
                return next
              }
              return [...prev, info]
            })
          }}
          addPortForward={ssh.addPortForward}
        />
      )}
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
          {isRemote && (
            <>
              <div className="ssh-pane-section-header">── Port Forwards ──</div>
              <div className="port-forward-toolbar">
                <button className="ssh-pane-tab active" onClick={() => setShowPortForwardDialog(true)}>
                  + Add Port Forward
                </button>
              </div>
              {portForwards.length === 0 ? (
                <div className="ssh-pane-output-empty">No port forwards. Click "+ Add Port Forward" to start one.</div>
              ) : (
                <div className="port-forward-list">
                  {portForwards.map(pf => {
                    const isExpanded = pf.id in expandedPfOutput
                    return (
                      <div key={pf.id} className="port-forward-item">
                        <div className="port-forward-item-header">
                          <span
                            className="ssh-pane-status-dot"
                            style={{ backgroundColor: getPfStatusColor(pf.status) }}
                          />
                          <span className="port-forward-item-label">
                            localhost:{pf.localPort} → {pf.remoteHost}:{pf.remotePort}
                          </span>
                          <span className="ssh-pane-status-text">({pf.status})</span>
                          <button
                            className="ssh-pane-tab"
                            onClick={() => handleTogglePfOutput(pf.id, isExpanded)}
                            title={isExpanded ? 'Hide output' : 'Show output'}
                          >
                            {isExpanded ? 'Hide' : 'Log'}
                          </button>
                          {pf.status !== 'stopped' && (
                            <button
                              className="ssh-pane-tab"
                              style={{ color: '#f44336' }}
                              onClick={() => {
                                ssh.removePortForward(pf.id).catch(console.error)
                                setPortForwards(prev => prev.filter(p => p.id !== pf.id))
                              }}
                            >
                              Stop
                            </button>
                          )}
                        </div>
                        {pf.error && (
                          <div className="port-forward-item-error">{pf.error}</div>
                        )}
                        {isExpanded && (
                          <div className="port-forward-item-output">
                            {(expandedPfOutput[pf.id] ?? []).length === 0 ? (
                              <div className="ssh-pane-output-empty">No output yet...</div>
                            ) : (
                              (expandedPfOutput[pf.id] ?? []).map((line, i) => (
                                <div key={i} className="ssh-pane-output-line">{line}</div>
                              ))
                            )}
                          </div>
                        )}
                      </div>
                    )
                  })}
                </div>
              )}
            </>
          )}
        </div>
      )}
    </div>
  )
}
