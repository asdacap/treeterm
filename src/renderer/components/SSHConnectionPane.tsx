import { useEffect, useRef, useState, useMemo } from 'react'
import { useAppStore } from '../store/app'
import type { ConnectionInfo } from '../types'

interface SSHConnectionPaneProps {
  connectionId: string
}

type TabId = 'daemon' | 'proxy'

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

export default function SSHConnectionPane({ connectionId }: SSHConnectionPaneProps) {
  const [activeTab, setActiveTab] = useState<TabId>('daemon')
  const [output, setOutput] = useState<string[]>([])
  const [connection, setConnection] = useState<ConnectionInfo | undefined>()
  const ssh = useAppStore(s => s.ssh)
  const disconnectRemote = useAppStore(s => s.disconnectRemote)
  const scrollRef = useRef<HTMLDivElement>(null)

  // Watch connection status
  useEffect(() => {
    let unsubscribe: (() => void) | undefined

    ssh.watchConnectionStatus(connectionId, (info) => {
      setConnection(info)
    }).then(({ initial, unsubscribe: unsub }) => {
      if (initial) setConnection(initial)
      unsubscribe = unsub
    }).catch(console.error)

    return () => { unsubscribe?.() }
  }, [connectionId, ssh])

  // Watch output
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

  const filteredOutput = useMemo(() => {
    if (activeTab === 'daemon') {
      return filterLines(output, DAEMON_PREFIXES)
    }
    return filterLines(output, PROXY_PREFIXES)
  }, [output, activeTab])

  // Auto-scroll
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [filteredOutput.length])

  const label = connection?.target.type === 'remote'
    ? `${connection.target.config.user}@${connection.target.config.host}`
    : connectionId

  return (
    <div className="ssh-pane">
      <div className="ssh-pane-header">
        <span
          className="ssh-pane-status-dot"
          style={{ backgroundColor: getStatusColor(connection?.status) }}
        />
        <span className="ssh-pane-label">{label}</span>
        {connection?.status && (
          <span className="ssh-pane-status-text">({connection.status})</span>
        )}
        {connection?.error && (
          <span className="ssh-pane-error">{connection.error}</span>
        )}
        <button
          className="ssh-pane-disconnect"
          onClick={() => disconnectRemote(connectionId)}
          title="Disconnect"
        >
          Disconnect
        </button>
      </div>
      <div className="ssh-pane-tabs">
        <button
          className={`ssh-pane-tab ${activeTab === 'daemon' ? 'active' : ''}`}
          onClick={() => setActiveTab('daemon')}
        >
          Daemon
        </button>
        <button
          className={`ssh-pane-tab ${activeTab === 'proxy' ? 'active' : ''}`}
          onClick={() => setActiveTab('proxy')}
        >
          Proxy
        </button>
      </div>
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
    </div>
  )
}
