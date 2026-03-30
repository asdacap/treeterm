import { useState, useEffect } from 'react'
import { useAppStore } from '../store/app'
import type { SSHConnectionConfig, PortForwardSpec } from '../types'

interface ConnectionPickerProps {
  isOpen: boolean
  onClose: () => void
}

export default function ConnectionPicker({ isOpen, onClose }: ConnectionPickerProps) {
  const ssh = useAppStore(s => s.ssh)
  const addRemoteSession = useAppStore(s => s.addRemoteSession)
  const startRemoteConnect = useAppStore(s => s.startRemoteConnect)
  const setSessionError = useAppStore(s => s.setSessionError)
  const [savedConnections, setSavedConnections] = useState<SSHConnectionConfig[]>([])
  const [host, setHost] = useState('')
  const [user, setUser] = useState('')
  const [port, setPort] = useState('22')
  const [identityFile, setIdentityFile] = useState('')
  const [label, setLabel] = useState('')
  const [portForwards, setPortForwards] = useState<PortForwardSpec[]>([])
  const [pfLocalPort, setPfLocalPort] = useState('')
  const [pfRemoteHost, setPfRemoteHost] = useState('localhost')
  const [pfRemotePort, setPfRemotePort] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [refreshDaemon, setRefreshDaemon] = useState(false)

  useEffect(() => {
    if (isOpen) {
      ssh.getSavedConnections().then(setSavedConnections).catch(console.error)
    }
  }, [isOpen, ssh])

  if (!isOpen) return null

  const connectAndCreateSession = (config: SSHConnectionConfig) => {
    console.log(`[renderer:ConnectionPicker] Initiating SSH connection to ${config.host}:${config.port} (id=${config.id})`)

    // 1. Immediately navigate to connecting view and close modal
    startRemoteConnect(config)
    onClose()

    // 2. Fire SSH connection in the background
    ssh.connect(config, { refreshDaemon }).then(({ info, session }) => {
      console.log(`[renderer:ConnectionPicker] ssh.connect returned: status=${info.status}, session=${session ? session.id : 'undefined'}`)

      if (info.status !== 'connected' || !session) {
        const errorMsg = info.status === 'error' ? info.error : 'No session returned'
        console.error(`[renderer:ConnectionPicker] SSH connection failed: ${errorMsg}`)
        setSessionError(config.id, errorMsg)
        return
      }

      // 3. Add real session (replaces connecting entry)
      console.log(`[renderer:ConnectionPicker] Adding remote session to store: session=${session.id}`)
      addRemoteSession(session, info)
    }).catch((err) => {
      console.error(`[renderer:ConnectionPicker] SSH connection error:`, err)
      setSessionError(config.id, err instanceof Error ? err.message : String(err))
    })
  }

  const handleConnect = () => {
    if (!host || !user) {
      setError('Host and user are required')
      return
    }

    const config: SSHConnectionConfig = {
      id: `ssh-${host}-${Date.now()}`,
      host,
      user,
      port: parseInt(port, 10) || 22,
      identityFile: identityFile || undefined,
      label: label || `${user}@${host}`,
      portForwards,
    }

    setError(null)
    connectAndCreateSession(config)
    // Save connection for future use
    ssh.saveConnection(config).catch(console.error)
    // Reset form
    setHost('')
    setUser('')
    setPort('22')
    setIdentityFile('')
    setLabel('')
    setPortForwards([])
  }

  const handleConnectSaved = (config: SSHConnectionConfig) => {
    setError(null)
    connectAndCreateSession(config)
  }

  const handleRemoveSaved = async (id: string) => {
    await ssh.removeSavedConnection(id)
    setSavedConnections(await ssh.getSavedConnections())
  }

  const handleAddPortForward = () => {
    const lp = parseInt(pfLocalPort, 10)
    const rp = parseInt(pfRemotePort, 10)
    if (!lp || lp < 1 || lp > 65535 || !rp || rp < 1 || rp > 65535 || !pfRemoteHost) return
    setPortForwards([...portForwards, { localPort: lp, remoteHost: pfRemoteHost, remotePort: rp }])
    setPfLocalPort('')
    setPfRemoteHost('localhost')
    setPfRemotePort('')
  }

  const handleRemovePortForward = (index: number) => {
    setPortForwards(portForwards.filter((_, i) => i !== index))
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') onClose()
    if (e.key === 'Enter') handleConnect()
  }

  return (
    <div
      style={{
        position: 'fixed', top: 0, left: 0, right: 0, bottom: 0,
        backgroundColor: 'rgba(0,0,0,0.6)', display: 'flex',
        alignItems: 'center', justifyContent: 'center', zIndex: 10000
      }}
      onClick={(e) => { if (e.target === e.currentTarget) onClose() }}
      onKeyDown={handleKeyDown}
    >
      <div style={{
        backgroundColor: '#1e1e2e', borderRadius: 8, padding: 24,
        width: 500, maxHeight: '80vh', overflow: 'auto',
        border: '1px solid #333', color: '#cdd6f4'
      }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
          <h2 style={{ margin: 0, fontSize: 18 }}>SSH Connections</h2>
          <button onClick={onClose} style={closeButtonStyle}>x</button>
        </div>

        {/* Saved connections */}
        {savedConnections.length > 0 && (
          <div style={{ marginBottom: 16 }}>
            <h3 style={{ fontSize: 14, color: '#a6adc8', marginBottom: 8 }}>Saved Connections</h3>
            {savedConnections.map(config => (
              <div key={config.id} style={{
                display: 'flex', alignItems: 'center', justifyContent: 'space-between',
                padding: '6px 12px', backgroundColor: '#313244', borderRadius: 4, marginBottom: 4
              }}>
                <div>
                  <span>{config.label || `${config.user}@${config.host}`}</span>
                  {config.portForwards.length > 0 && (
                    <span style={{ fontSize: 11, color: '#a6adc8', marginLeft: 6 }}>
                      ({config.portForwards.length} port fwd{config.portForwards.length > 1 ? 's' : ''})
                    </span>
                  )}
                </div>
                <div style={{ display: 'flex', gap: 4 }}>
                  <button
                    onClick={() => {
                      setHost(config.host)
                      setUser(config.user)
                      setPort(String(config.port))
                      setIdentityFile(config.identityFile || '')
                      setLabel(config.label || '')
                      setPortForwards(config.portForwards)
                    }}
                    style={smallButtonStyle}
                  >Fill</button>
                  <button
                    onClick={() => handleConnectSaved(config)}
                    style={smallButtonStyle}
                  >Connect</button>
                  <button
                    onClick={() => handleRemoveSaved(config.id)}
                    style={{ ...smallButtonStyle, color: '#f44336' }}
                  >Remove</button>
                </div>
              </div>
            ))}
          </div>
        )}

        {/* New connection form */}
        <h3 style={{ fontSize: 14, color: '#a6adc8', marginBottom: 8 }}>New Connection</h3>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8, marginBottom: 12 }}>
          <div>
            <label style={labelStyle}>User</label>
            <input
              value={user} onChange={e => setUser(e.target.value)}
              placeholder="username" style={inputStyle} autoFocus
            />
          </div>
          <div>
            <label style={labelStyle}>Host</label>
            <input
              value={host} onChange={e => setHost(e.target.value)}
              placeholder="hostname or IP" style={inputStyle}
            />
          </div>
          <div>
            <label style={labelStyle}>Port</label>
            <input
              value={port} onChange={e => setPort(e.target.value)}
              placeholder="22" style={inputStyle}
            />
          </div>
          <div>
            <label style={labelStyle}>Label (optional)</label>
            <input
              value={label} onChange={e => setLabel(e.target.value)}
              placeholder="display name" style={inputStyle}
            />
          </div>
          <div style={{ gridColumn: '1 / -1' }}>
            <label style={labelStyle}>Identity File (optional)</label>
            <input
              value={identityFile} onChange={e => setIdentityFile(e.target.value)}
              placeholder="~/.ssh/id_rsa" style={inputStyle}
            />
          </div>
        </div>

        {/* Port forwards editor */}
        <div style={{ marginBottom: 12 }}>
          <label style={labelStyle}>Port Forwards</label>
          {portForwards.map((pf, i) => (
            <div key={i} style={{
              display: 'flex', alignItems: 'center', gap: 6, marginBottom: 4,
              fontSize: 12, color: '#cdd6f4'
            }}>
              <span style={{ flex: 1 }}>
                localhost:{pf.localPort} → {pf.remoteHost}:{pf.remotePort}
              </span>
              <button
                onClick={() => handleRemovePortForward(i)}
                style={{ ...smallButtonStyle, color: '#f44336', padding: '0 6px' }}
              >x</button>
            </div>
          ))}
          <div style={{ display: 'flex', gap: 4, alignItems: 'center' }}>
            <input
              value={pfLocalPort} onChange={e => setPfLocalPort(e.target.value)}
              placeholder="local port" style={{ ...inputStyle, width: 80 }}
            />
            <span style={{ fontSize: 12, color: '#a6adc8' }}>→</span>
            <input
              value={pfRemoteHost} onChange={e => setPfRemoteHost(e.target.value)}
              placeholder="remote host" style={{ ...inputStyle, width: 120 }}
            />
            <span style={{ fontSize: 12, color: '#a6adc8' }}>:</span>
            <input
              value={pfRemotePort} onChange={e => setPfRemotePort(e.target.value)}
              placeholder="remote port" style={{ ...inputStyle, width: 80 }}
            />
            <button onClick={handleAddPortForward} style={smallButtonStyle}>Add</button>
          </div>
        </div>

        <label style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 13, color: '#a6adc8', marginBottom: 8 }}>
          <input
            type="checkbox"
            checked={refreshDaemon}
            onChange={e => setRefreshDaemon(e.target.checked)}
          />
          Refresh remote daemon
        </label>

        {error && (
          <div style={{ color: '#f44336', fontSize: 13, marginBottom: 8 }}>{error}</div>
        )}

        <button
          onClick={handleConnect}
          disabled={!host || !user}
          style={{
            ...connectButtonStyle,
            opacity: !host || !user ? 0.5 : 1
          }}
        >
          Connect
        </button>
      </div>
    </div>
  )
}

const inputStyle: React.CSSProperties = {
  width: '100%', padding: '6px 8px', backgroundColor: '#313244',
  border: '1px solid #45475a', borderRadius: 4, color: '#cdd6f4',
  fontSize: 13, outline: 'none', boxSizing: 'border-box'
}

const labelStyle: React.CSSProperties = {
  display: 'block', fontSize: 12, color: '#a6adc8', marginBottom: 4
}

const smallButtonStyle: React.CSSProperties = {
  padding: '2px 8px', backgroundColor: 'transparent', border: '1px solid #45475a',
  borderRadius: 4, color: '#cdd6f4', fontSize: 12, cursor: 'pointer'
}

const closeButtonStyle: React.CSSProperties = {
  background: 'none', border: 'none', color: '#666', fontSize: 18, cursor: 'pointer', padding: '0 4px'
}

const connectButtonStyle: React.CSSProperties = {
  width: '100%', padding: '8px 16px', backgroundColor: '#89b4fa',
  border: 'none', borderRadius: 4, color: '#1e1e2e',
  fontSize: 14, fontWeight: 600, cursor: 'pointer'
}
