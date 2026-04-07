import { useEffect, useRef, useState } from 'react'

let nextLineId = 0
import { useStore } from 'zustand'
import type { StoreApi } from 'zustand'
import { Loader2 } from 'lucide-react'
import type { SessionState } from '../store/createSessionStore'
import { useAppStore } from '../store/app'
import { useSessionNamesStore } from '../store/sessionNames'
import type { PortForwardConfig, PortForwardInfo } from '../types'
import PortForwardDialog from './PortForwardDialog'
import JsonViewer from './JsonViewer'

type TabId = 'info' | 'ssh' | 'json'
type SshSubTab = 'bootstrap' | 'tunnel' | 'portforwards'

const BOOTSTRAP_PREFIXES = ['[bootstrap]', '[bootstrap:err]', '[ssh]', '[start]', '[start:err]']
const TUNNEL_PREFIXES = ['[tunnel]', '[tunnel:err]']

interface OutputLine {
  id: number
  line: string
}

function filterLines(lines: OutputLine[], prefixes: string[]): OutputLine[] {
  return lines.filter(item =>
    prefixes.some(prefix => item.line.includes(prefix))
  )
}

function getStatusColor(status: string | undefined): string {
  switch (status) {
    case 'connected': return '#4caf50'
    case 'connecting': return '#ff9800'
    case 'error': return '#f44336'
    case 'disconnected': return '#f44336'
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

interface SessionInfoPaneProps {
  sessionStore: StoreApi<SessionState>
}

export default function SessionInfoPane({ sessionStore }: SessionInfoPaneProps) {
  const sessionId = useStore(sessionStore, s => s.sessionId)
  const connection = useStore(sessionStore, s => s.connection)
  const isRemote = connection?.target.type === 'remote'
  const isConnected = connection?.status === 'connected'
  const connectionError = (connection?.status === 'error' || connection?.status === 'disconnected') ? connection.error : undefined

  const ssh = useAppStore(s => s.ssh)
  const disconnectSession = useAppStore(s => s.disconnectSession)
  const displayName = useSessionNamesStore(s => s.names[sessionId].name)

  const [activeTab, setActiveTab] = useState<TabId>(isRemote ? 'ssh' : 'info')
  const [sshSubTab, setSshSubTab] = useState<SshSubTab>('bootstrap')
  const [output, setOutput] = useState<OutputLine[]>([])
  const [portForwards, setPortForwards] = useState<PortForwardInfo[]>([])
  const [showPortForwardDialog, setShowPortForwardDialog] = useState(false)
  const [expandedPfOutput, setExpandedPfOutput] = useState<Record<string, OutputLine[]>>({})
  const scrollRef = useRef<HTMLDivElement>(null)

  // Session data for JSON tab
  const rawSessionId = useStore(sessionStore, s => s.sessionId)
  const rawConnection = useStore(sessionStore, s => s.connection)
  const rawActiveWorkspaceId = useStore(sessionStore, s => s.activeWorkspaceId)
  const rawSessionVersion = useStore(sessionStore, s => s.sessionVersion)
  const rawWorkspaces = useStore(sessionStore, s => s.workspaces)

  // SSH output watching
  useEffect(() => {
    if (!isRemote) return
    let unsubscribe: (() => void) | undefined
    void ssh.watchOutput(connection.id, (line) => {
      const id = nextLineId++
      setOutput(prev => [...prev, { id, line }])
    }).then(({ scrollback, unsubscribe: unsub }) => {
      setOutput(scrollback.map(line => ({ id: nextLineId++, line })))
      unsubscribe = unsub
    }).catch((e: unknown) => { console.error(e) })
    return () => { unsubscribe?.() }
  }, [isRemote, connection, ssh])

  const bootstrapLines = filterLines(output, BOOTSTRAP_PREFIXES)
  const tunnelLines = filterLines(output, TUNNEL_PREFIXES)

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [bootstrapLines.length, tunnelLines.length])

  // Port forwards (only when connected + remote)
  useEffect(() => {
    if (!isRemote || !isConnected) return
    void ssh.listPortForwards(connection.id).then(setPortForwards).catch((e: unknown) => { console.error(e) })
  }, [isRemote, isConnected, connection, ssh])

  useEffect(() => {
    if (!isRemote || !isConnected) return
    const unsubscribe = ssh.onPortForwardStatus((info) => {
      if (info.connectionId !== connection.id) return
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
  }, [isRemote, isConnected, connection, ssh])

  const handleTogglePfOutput = (pfId: string, currentlyExpanded: boolean) => {
    if (currentlyExpanded) {
      setExpandedPfOutput(prev => {
        const { [pfId]: _removed, ...rest } = prev
        return rest
      })
    } else {
      void ssh.watchPortForwardOutput(pfId, (line) => {
        const id = nextLineId++
        setExpandedPfOutput(prev => ({
          ...prev,
          [pfId]: [...(prev[pfId] ?? []), { id, line }]
        }))
      }).then(({ scrollback }) => {
        setExpandedPfOutput(prev => ({ ...prev, [pfId]: scrollback.map(line => ({ id: nextLineId++, line })) }))
      }).catch((e: unknown) => { console.error(e) })
      setExpandedPfOutput(prev => ({ ...prev, [pfId]: [] }))
    }
  }

  const handleRetry = () => {
    if (!isRemote || connection.target.type !== 'remote') return
    const config = connection.target.config
    // Reset connection to connecting state
    sessionStore.setState({ connection: { ...connection, status: 'connecting' as const } })
    void ssh.connect(config).then(({ info, session }) => {
      if (info.status !== 'connected' || !session) {
        useAppStore.getState().setSessionError(config.id, info.status === 'error' ? info.error : 'Connection failed')
        return
      }
      void useAppStore.getState().addRemoteSession(session, info)
    }).catch((err: unknown) => {
      useAppStore.getState().setSessionError(config.id, err instanceof Error ? err.message : String(err))
    })
  }

  // Derive label
  const label = isRemote && connection.target.type === 'remote'
    ? (connection.target.config.label || `${connection.target.config.user}@${connection.target.config.host}`)
    : displayName || sessionId

  // Tabs: Info + SSH (if remote) + JSON (if connected)
  const tabs: { id: TabId; label: string }[] = isRemote
    ? isConnected
      ? [{ id: 'info', label: 'Info' }, { id: 'ssh', label: 'SSH' }, { id: 'json', label: 'JSON' }]
      : [{ id: 'info', label: 'Info' }, { id: 'ssh', label: 'SSH' }]
    : [{ id: 'info', label: 'Info' }, { id: 'json', label: 'JSON' }]

  // SSH sub-tabs: Bootstrap + Tunnel always, Port Forwards when connected
  const sshSubTabs: { id: SshSubTab; label: string }[] = isConnected
    ? [{ id: 'bootstrap', label: 'Bootstrap' }, { id: 'tunnel', label: 'Tunnel' }, { id: 'portforwards', label: 'Port Forwards' }]
    : [{ id: 'bootstrap', label: 'Bootstrap' }, { id: 'tunnel', label: 'Tunnel' }]

  const sessionData = {
    sessionId: rawSessionId,
    connection: rawConnection,
    activeWorkspaceId: rawActiveWorkspaceId,
    sessionVersion: rawSessionVersion,
    workspaces: Object.fromEntries(
      Object.entries(rawWorkspaces).map(([id, entry]) => [
        id,
        entry.status === 'loaded' || entry.status === 'operation-error'
          ? { status: entry.status, data: entry.data }
          : entry.status === 'loading'
            ? { status: entry.status, name: entry.name, message: entry.message }
            : { status: entry.status, name: entry.name, error: entry.error },
      ]),
    ),
  }

  return (
    <div className="ssh-pane">
      <div className="ssh-pane-header">
        {isRemote && (
          <span
            className="ssh-pane-status-dot"
            style={{ backgroundColor: getStatusColor(connection.status) }}
          />
        )}
        <span className="ssh-pane-label">{label}</span>
        {isRemote && connection.status === 'connecting' && (
          <Loader2 size={14} className="spinning" style={{ marginLeft: 8 }} />
        )}
        {isRemote && connection.status && (
          <span className="ssh-pane-status-text">({connection.status})</span>
        )}
        {!isRemote && (
          <span className="ssh-pane-status-text">(Local)</span>
        )}
        {connectionError && (
          <span className="ssh-pane-error">{connectionError}</span>
        )}
        {connection && (connection.status === 'error' || connection.status === 'disconnected') && (
          <div style={{ marginLeft: 'auto', display: 'flex', gap: 8 }}>
            <button className="ssh-pane-tab" onClick={handleRetry}>Retry</button>
            <button className="ssh-pane-tab" style={{ color: '#f44336' }} onClick={() => { disconnectSession(sessionId); }}>Remove</button>
          </div>
        )}
      </div>
      <div className="ssh-pane-tabs">
        {tabs.map(tab => (
          <button
            key={tab.id}
            className={`ssh-pane-tab ${activeTab === tab.id ? 'active' : ''}`}
            onClick={() => { setActiveTab(tab.id); }}
          >
            {tab.label}
          </button>
        ))}
      </div>
      {showPortForwardDialog && isRemote && (
        <PortForwardDialog
          connectionId={connection.id}
          onClose={() => { setShowPortForwardDialog(false); }}
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
      {activeTab === 'ssh' && isRemote && (
        <div className="ssh-pane-subtabs">
          {sshSubTabs.map(tab => (
            <button
              key={tab.id}
              className={`ssh-pane-tab ${sshSubTab === tab.id ? 'active' : ''}`}
              onClick={() => { setSshSubTab(tab.id); }}
            >
              {tab.label}
            </button>
          ))}
          {sshSubTab === 'portforwards' && isConnected && (
            <button className="ssh-pane-tab active" style={{ marginLeft: 'auto' }} onClick={() => { setShowPortForwardDialog(true); }}>
              + Add Port Forward
            </button>
          )}
        </div>
      )}
      {activeTab === 'info' ? (
        <div className="ssh-pane-output">
          <div className="ssh-pane-output-line">Session ID: {sessionId}</div>
          {isRemote && connection.target.type === 'remote' ? (
            <>
              <div className="ssh-pane-output-line">Connection: SSH</div>
              <div className="ssh-pane-output-line">Host: {connection.target.config.host}</div>
              <div className="ssh-pane-output-line">User: {connection.target.config.user}</div>
              <div className="ssh-pane-output-line">Port: {String(connection.target.config.port)}</div>
              {connection.target.config.identityFile && (
                <div className="ssh-pane-output-line">Identity File: {connection.target.config.identityFile}</div>
              )}
              <div className="ssh-pane-output-line">Status: {connection.status}</div>
              {(connection.status === 'error' || connection.status === 'disconnected') && connection.error && (
                <div className="ssh-pane-output-line">Error: {connection.error}</div>
              )}
            </>
          ) : (
            <div className="ssh-pane-output-line">Connection: Local</div>
          )}
        </div>
      ) : activeTab === 'json' ? (
        <div className="ssh-pane-output">
          <JsonViewer data={sessionData} />
        </div>
      ) : sshSubTab === 'bootstrap' ? (
        <div className="ssh-pane-output" ref={scrollRef}>
          {bootstrapLines.length === 0 ? (
            <div className="ssh-pane-output-empty">No bootstrap output yet...</div>
          ) : (
            bootstrapLines.map((item) => (
              <div key={item.id} className="ssh-pane-output-line">{item.line}</div>
            ))
          )}
        </div>
      ) : sshSubTab === 'tunnel' ? (
        <div className="ssh-pane-output" ref={scrollRef}>
          {tunnelLines.length === 0 ? (
            <div className="ssh-pane-output-empty">No tunnel output yet...</div>
          ) : (
            tunnelLines.map((item) => (
              <div key={item.id} className="ssh-pane-output-line">{item.line}</div>
            ))
          )}
        </div>
      ) : (
        <div className="ssh-pane-output">
          {portForwards.length === 0 ? (
            <div className="ssh-pane-output-empty">No port forwards configured.</div>
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
                        localhost:{String(pf.localPort)} → {pf.remoteHost}:{String(pf.remotePort)}
                      </span>
                      <span className="ssh-pane-status-text">({pf.status})</span>
                      <button
                        className="ssh-pane-tab"
                        onClick={() => { handleTogglePfOutput(pf.id, isExpanded); }}
                        title={isExpanded ? 'Hide output' : 'Show output'}
                      >
                        {isExpanded ? 'Hide' : 'Log'}
                      </button>
                      {(pf.status === 'error' || pf.status === 'stopped') && (
                        <button
                          className="ssh-pane-tab"
                          style={{ color: '#ff9800' }}
                          onClick={() => {
                            void (async () => {
                              await ssh.removePortForward(pf.id).catch(() => {})
                              const config: PortForwardConfig = {
                                id: crypto.randomUUID(),
                                connectionId: pf.connectionId,
                                localPort: pf.localPort,
                                remoteHost: pf.remoteHost,
                                remotePort: pf.remotePort,
                              }
                              try {
                                const info = await ssh.addPortForward(config)
                                setPortForwards(prev => prev.filter(p => p.id !== pf.id).concat(info))
                              } catch (err) {
                                console.error('Failed to restart port forward:', err)
                              }
                            })()
                          }}
                        >
                          Restart
                        </button>
                      )}
                      {pf.status !== 'stopped' && pf.status !== 'error' && (
                        <button
                          className="ssh-pane-tab"
                          style={{ color: '#f44336' }}
                          onClick={() => {
                            void ssh.removePortForward(pf.id).catch((e: unknown) => { console.error(e) })
                            setPortForwards(prev => prev.filter(p => p.id !== pf.id))
                          }}
                        >
                          Stop
                        </button>
                      )}
                    </div>
                    {pf.status === 'error' && (
                      <div className="port-forward-item-error">{pf.error}</div>
                    )}
                    {isExpanded && (
                      <div className="port-forward-item-output">
                        {(expandedPfOutput[pf.id] ?? []).length === 0 ? (
                          <div className="ssh-pane-output-empty">No output yet...</div>
                        ) : (
                          (expandedPfOutput[pf.id] ?? []).map((item) => (
                            <div key={item.id} className="ssh-pane-output-line">{item.line}</div>
                          ))
                        )}
                      </div>
                    )}
                  </div>
                )
              })}
            </div>
          )}
        </div>
      )}
    </div>
  )
}
