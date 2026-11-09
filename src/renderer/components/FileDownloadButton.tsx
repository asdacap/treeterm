import React, { useState } from 'react'
import { Download } from 'lucide-react'
import type { WorkspaceFilesystemApi } from '../types'
import { downloadBytes } from '../utils/downloadFile'

enum DownloadStatus {
  Idle = 'idle',
  Downloading = 'downloading',
  Failed = 'failed',
}

type DownloadState =
  | { status: DownloadStatus.Idle }
  | { status: DownloadStatus.Downloading }
  | { status: DownloadStatus.Failed; message: string }

interface FileDownloadButtonProps {
  filesystem: WorkspaceFilesystemApi
  filePath: string
  /** The transfer budget the viewer opened the file with. */
  maxBytes: number
}

function renderDownloadState(state: DownloadState, onDownload: () => void): React.ReactNode {
  const button = (label: string, disabled: boolean): React.JSX.Element => (
    <button
      className="file-viewer-open-btn"
      onClick={onDownload}
      disabled={disabled}
      title="Save a copy of this file to your computer"
    >
      <Download size={11} /> {label}
    </button>
  )

  const views: Record<DownloadStatus, () => React.ReactNode> = {
    [DownloadStatus.Idle]: () => button('Download', false),
    [DownloadStatus.Downloading]: () => button('Downloading…', true),
    [DownloadStatus.Failed]: () => state.status === DownloadStatus.Failed && (
      <>
        <span className="file-viewer-download-error" title={state.message}>
          Download failed: {state.message}
        </span>
        {button('Retry download', false)}
      </>
    ),
  }
  return views[state.status]()
}

/**
 * Saves a copy of the file to the user's machine. It reads the file's exact bytes afresh
 * rather than reusing the viewer's content, which was decoded for display: a UTF-8 decode
 * does not survive a round trip for binary files or text in another encoding.
 */
export function FileDownloadButton({ filesystem, filePath, maxBytes }: FileDownloadButtonProps): React.JSX.Element {
  const [state, setState] = useState<DownloadState>({ status: DownloadStatus.Idle })

  const download = async (): Promise<void> => {
    setState({ status: DownloadStatus.Downloading })
    try {
      const result = await filesystem.readFileBytes(filePath, maxBytes)
      if (!result.success) {
        setState({ status: DownloadStatus.Failed, message: result.error })
        return
      }
      downloadBytes(filePath.split('/').pop() || filePath, result.bytes)
      setState({ status: DownloadStatus.Idle })
    } catch (err) {
      setState({ status: DownloadStatus.Failed, message: err instanceof Error ? err.message : String(err) })
    }
  }

  return <>{renderDownloadState(state, () => { void download() })}</>
}
