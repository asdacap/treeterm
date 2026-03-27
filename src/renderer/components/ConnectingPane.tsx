import { useEffect, useRef, useState } from 'react'
import { useAppStore } from '../store/app'
import { Loader2 } from 'lucide-react'

function getStatusColor(status: string | undefined): string {
  switch (status) {
    case 'connected': return '#4caf50'
    case 'connecting': return '#ff9800'
    case 'error': return '#f44336'
    default: return '#666'
  }
}

interface ConnectingPaneProps {
  connectionId: string
}

export default function ConnectingPane({ connectionId }: ConnectingPaneProps) {
  const connectingRemote = useAppStore(s => s.connectingRemote)
  const ssh = useAppStore(s => s.ssh)
  const [output, setOutput] = useState<string[]>([])
  const [status, setStatus] = useState<string>('connecting')
  const [error, setError] = useState<string | undefined>()
  const scrollRef = useRef<HTMLDivElement>(null)

  const config = connectingRemote?.config
  const label = config ? `${config.user}@${config.host}` : connectionId

  // Watch SSH output
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

  // Watch connection status
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

  // Auto-scroll
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [output.length])

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
      <div className="ssh-pane-output" ref={scrollRef}>
        {output.length === 0 && (
          <div className="ssh-pane-output-empty">
            Connecting to {label}...
          </div>
        )}
        {output.map((line, i) => (
          <div key={i} className="ssh-pane-output-line">{line}</div>
        ))}
      </div>
    </div>
  )
}
