import { useEffect, useState } from 'react'
import { Loader2 } from 'lucide-react'
import { useAppStore } from '../store/app'
import RemoteDirectoryBrowser from './RemoteDirectoryBrowser'
import { ConnectionStatus, ConnectionTargetType } from '../../shared/types'
import type { ConnectionInfo } from '../../shared/types'

enum UploadStatus {
  Idle = 'idle',
  Uploading = 'uploading',
  Success = 'success',
  Error = 'error',
}

type UploadState =
  | { status: UploadStatus.Idle }
  | { status: UploadStatus.Uploading }
  | { status: UploadStatus.Success; remotePath: string }
  | { status: UploadStatus.Error; error: string }

/** Last path segment, handling both POSIX and Windows separators (local file may be on either). */
function basename(p: string): string {
  const parts = p.split(/[\\/]/).filter(Boolean)
  return parts[parts.length - 1] ?? p
}

function UploadStatusBanner({ upload }: { upload: UploadState }) {
  switch (upload.status) {
    case UploadStatus.Idle:
      return null
    case UploadStatus.Uploading:
      return <div className="ssh-upload-status"><Loader2 size={14} className="spinning" /> Uploading...</div>
    case UploadStatus.Success:
      return <div className="ssh-upload-status" style={{ color: '#4caf50' }}>Uploaded to {upload.remotePath}</div>
    case UploadStatus.Error:
      return <div className="ssh-upload-status" style={{ color: '#f44336' }}>Upload failed: {upload.error}</div>
  }
}

interface SshUploadPaneProps {
  connectionId: string
}

export default function SshUploadPane({ connectionId }: SshUploadPaneProps) {
  const ssh = useAppStore(s => s.ssh)
  const selectFile = useAppStore(s => s.selectFile)
  const filesystem = useAppStore(s => s.filesystem)

  const [connection, setConnection] = useState<ConnectionInfo | null>(null)
  const [localPath, setLocalPath] = useState<string | null>(null)
  const [remoteDir, setRemoteDir] = useState('')
  const [showBrowser, setShowBrowser] = useState(false)
  const [upload, setUpload] = useState<UploadState>({ status: UploadStatus.Idle })

  // Subscribe to live connection status — gives us host details and the connected gate.
  useEffect(() => {
    let unsubscribe: (() => void) | undefined
    const applyInfo = (info: ConnectionInfo) => {
      setConnection(info)
      if (info.target.type === ConnectionTargetType.Remote) {
        const user = info.target.config.user
        setRemoteDir(prev => prev || `/home/${user}`)
      }
    }
    void ssh.watchConnectionStatus(connectionId, applyInfo).then(({ initial, unsubscribe: unsub }) => {
      applyInfo(initial)
      unsubscribe = unsub
    }).catch((e: unknown) => { console.error(e) })
    return () => { unsubscribe?.() }
  }, [connectionId, ssh])

  const handleChooseFile = () => {
    void selectFile().then((p) => {
      if (p) {
        setLocalPath(p)
        setUpload({ status: UploadStatus.Idle })
      }
    }).catch((e: unknown) => { console.error(e) })
  }

  const handleUpload = () => {
    const dir = remoteDir.trim()
    if (!localPath || !dir) return
    const dest = `${dir.replace(/\/+$/, '')}/${basename(localPath)}`
    setUpload({ status: UploadStatus.Uploading })
    void ssh.uploadFile(connectionId, localPath, dest).then((result) => {
      if (result.success) {
        setUpload({ status: UploadStatus.Success, remotePath: dest })
      } else {
        setUpload({ status: UploadStatus.Error, error: result.error })
      }
    }).catch((e: unknown) => {
      setUpload({ status: UploadStatus.Error, error: e instanceof Error ? e.message : String(e) })
    })
  }

  // Loading until the first connection snapshot arrives.
  if (!connection) {
    return (
      <div className="ssh-pane-output">
        <div className="ssh-pane-output-line"><Loader2 size={14} className="spinning" /> Loading connection...</div>
      </div>
    )
  }

  // Upload only makes sense for remote SSH sessions.
  if (connection.target.type !== ConnectionTargetType.Remote) {
    return (
      <div className="ssh-pane-output">
        <div className="ssh-pane-output-empty">File upload is only available for SSH sessions.</div>
      </div>
    )
  }

  const config = connection.target.config
  const isConnected = connection.status === ConnectionStatus.Connected
  const uploading = upload.status === UploadStatus.Uploading
  const canUpload = !!localPath && remoteDir.trim().length > 0 && isConnected && !uploading

  return (
    <div className="ssh-pane-output ssh-upload-pane">
      <div className="ssh-upload-target">
        Upload to {config.label || `${config.user}@${config.host}`}
      </div>

      {!isConnected && (
        <div className="ssh-pane-output-empty">
          Connection is {connection.status} — uploads are disabled until connected.
        </div>
      )}

      {/* Local source file */}
      <div className="ssh-upload-field">
        <label className="ssh-upload-label">Local file</label>
        <div className="ssh-upload-row">
          <input
            className="remote-dir-path-input"
            value={localPath ?? ''}
            readOnly
            placeholder="No file selected"
          />
          <button className="dialog-btn" onClick={handleChooseFile} disabled={uploading}>
            Choose file...
          </button>
        </div>
      </div>

      {/* Remote destination directory */}
      <div className="ssh-upload-field">
        <label className="ssh-upload-label">Remote directory (absolute)</label>
        <div className="ssh-upload-row">
          <input
            className="remote-dir-path-input"
            value={remoteDir}
            onChange={(e) => { setRemoteDir(e.target.value); }}
            placeholder="/path/on/remote"
            disabled={uploading}
          />
          <button className="dialog-btn" onClick={() => { setShowBrowser(true); }} disabled={uploading}>
            Browse...
          </button>
        </div>
      </div>

      {showBrowser && (
        <RemoteDirectoryBrowser
          readDirectory={(dirPath: string) => filesystem.readDirectory(connectionId, '/', dirPath)}
          initialPath={remoteDir.trim() || `/home/${config.user}`}
          onSelect={(path) => { setRemoteDir(path); setShowBrowser(false); }}
          onCancel={() => { setShowBrowser(false); }}
        />
      )}

      <div className="ssh-upload-actions">
        <button className="dialog-btn create" onClick={handleUpload} disabled={!canUpload}>
          Upload
        </button>
      </div>

      <UploadStatusBanner upload={upload} />
    </div>
  )
}
