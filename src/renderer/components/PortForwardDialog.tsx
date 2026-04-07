import { useState } from 'react'
import type { PortForwardConfig, PortForwardInfo } from '../types'

interface Props {
  connectionId: string
  onClose: () => void
  onCreated: (info: PortForwardInfo) => void
  addPortForward: (config: PortForwardConfig) => Promise<PortForwardInfo>
}

export default function PortForwardDialog({ connectionId, onClose, onCreated, addPortForward }: Props) {
  const [localPort, setLocalPort] = useState('')
  const [remoteHost, setRemoteHost] = useState('localhost')
  const [remotePort, setRemotePort] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const handleSubmit = async () => {
    const localPortNum = parseInt(localPort, 10)
    const remotePortNum = parseInt(remotePort, 10)

    if (!Number.isInteger(localPortNum) || localPortNum < 1 || localPortNum > 65535) {
      setError('Local port must be a number between 1 and 65535')
      return
    }
    if (!remoteHost.trim()) {
      setError('Remote host is required')
      return
    }
    if (!Number.isInteger(remotePortNum) || remotePortNum < 1 || remotePortNum > 65535) {
      setError('Remote port must be a number between 1 and 65535')
      return
    }

    const config: PortForwardConfig = {
      id: crypto.randomUUID(),
      connectionId,
      localPort: localPortNum,
      remoteHost: remoteHost.trim(),
      remotePort: remotePortNum,
    }

    setLoading(true)
    setError(null)
    try {
      const info = await addPortForward(config)
      onCreated(info)
      onClose()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') void handleSubmit()
    if (e.key === 'Escape') onClose()
  }

  return (
    <div className="dialog-overlay" onClick={onClose}>
      <div className="port-forward-dialog" onClick={(e) => { e.stopPropagation(); }} onKeyDown={handleKeyDown}>
        <div className="port-forward-dialog-header">
          <h2>Add Port Forward</h2>
          <button className="dialog-close" onClick={onClose}>✕</button>
        </div>
        <div className="port-forward-dialog-content">
          <div className="port-forward-dialog-row">
            <label>Local Port</label>
            <input
              type="number"
              placeholder="8080"
              value={localPort}
              onChange={(e) => { setLocalPort(e.target.value); }}
              autoFocus
              min={1}
              max={65535}
            />
          </div>
          <div className="port-forward-dialog-row">
            <label>Remote Host</label>
            <input
              type="text"
              placeholder="localhost"
              value={remoteHost}
              onChange={(e) => { setRemoteHost(e.target.value); }}
            />
          </div>
          <div className="port-forward-dialog-row">
            <label>Remote Port</label>
            <input
              type="number"
              placeholder="3000"
              value={remotePort}
              onChange={(e) => { setRemotePort(e.target.value); }}
              min={1}
              max={65535}
            />
          </div>
          <p className="port-forward-dialog-hint">
            Forwards <strong>localhost:{localPort || '?'}</strong> on this machine to{' '}
            <strong>{remoteHost || '?'}:{remotePort || '?'}</strong> via the server.
          </p>
          {error && (
            <div className="port-forward-dialog-error">{error}</div>
          )}
        </div>
        <div className="port-forward-dialog-actions">
          <button className="dialog-btn cancel" onClick={onClose} disabled={loading}>
            Cancel
          </button>
          <button className="dialog-btn create" onClick={() => { void handleSubmit() }} disabled={loading}>
            {loading ? 'Starting...' : 'Start Forward'}
          </button>
        </div>
      </div>
    </div>
  )
}
