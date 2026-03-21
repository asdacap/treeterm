import { useEffect, useRef, useState } from 'react'
import { useAppStore } from '../store/app'

interface SSHOutputPanelProps {
  connectionId: string
}

export default function SSHOutputPanel({ connectionId }: SSHOutputPanelProps) {
  const [output, setOutput] = useState<string[]>([])
  const ssh = useAppStore(s => s.ssh)
  const activeSessionStore = useAppStore(s => {
    const { activeSessionId, sessionStores } = s
    if (!activeSessionId) return null
    return sessionStores[activeSessionId] || null
  })
  const scrollRef = useRef<HTMLDivElement>(null)

  const connection = activeSessionStore?.getState().connection
  const isMatch = connection?.id === connectionId
  const matchedConnection = isMatch ? connection : undefined
  const label = matchedConnection?.target.type === 'remote'
    ? `${matchedConnection.target.config.user}@${matchedConnection.target.config.host}`
    : connectionId

  // Auto-scroll to bottom
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [output.length])

  // Watch output for this connection
  useEffect(() => {
    let unsubscribe: (() => void) | undefined

    ssh.watchOutput(connectionId, (line) => {
      setOutput(prev => [...prev, line])
    }).then(({ scrollback, unsubscribe: unsub }) => {
      setOutput(scrollback)
      unsubscribe = unsub
    }).catch(console.error)

    return () => {
      unsubscribe?.()
    }
  }, [connectionId, ssh])

  const getStatusColor = (status: string | undefined) => {
    switch (status) {
      case 'connected': return '#4caf50'
      case 'connecting': return '#ff9800'
      case 'error': return '#f44336'
      default: return '#666'
    }
  }

  return (
    <div style={{
      display: 'flex', flexDirection: 'column', height: '100%',
      backgroundColor: '#11111b', color: '#cdd6f4'
    }}>
      <div style={{
        display: 'flex', alignItems: 'center', gap: 8,
        padding: '8px 12px', borderBottom: '1px solid #333',
        backgroundColor: '#1e1e2e', fontSize: 13
      }}>
        <span style={{
          width: 8, height: 8, borderRadius: '50%',
          backgroundColor: getStatusColor(matchedConnection?.status),
          display: 'inline-block'
        }} />
        <span style={{ fontWeight: 600 }}>SSH: {label}</span>
        {matchedConnection?.status && (
          <span style={{ color: '#666', fontSize: 12 }}>({matchedConnection.status})</span>
        )}
        {matchedConnection?.error && (
          <span style={{ color: '#f44336', fontSize: 12 }}>{matchedConnection.error}</span>
        )}
      </div>
      <div
        ref={scrollRef}
        style={{
          flex: 1, overflow: 'auto', padding: 8,
          fontFamily: 'monospace', fontSize: 12, lineHeight: 1.5
        }}
      >
        {output.length === 0 && (
          <div style={{ color: '#666' }}>Waiting for SSH output...</div>
        )}
        {output.map((line, i) => (
          <div key={i} style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>{line}</div>
        ))}
      </div>
    </div>
  )
}
