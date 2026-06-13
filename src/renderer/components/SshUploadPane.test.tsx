// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import SshUploadPane from './SshUploadPane'
import { ConnectionStatus, ConnectionTargetType } from '../../shared/types'
import type { ConnectionInfo } from '../../shared/types'

interface MockState {
  ssh: {
    watchConnectionStatus: ReturnType<typeof vi.fn>
    uploadFile: ReturnType<typeof vi.fn>
  }
  selectFile: ReturnType<typeof vi.fn>
  filesystem: { readDirectory: ReturnType<typeof vi.fn> }
}

const mockState: MockState = {
  ssh: {
    watchConnectionStatus: vi.fn(),
    uploadFile: vi.fn(),
  },
  selectFile: vi.fn(),
  filesystem: { readDirectory: vi.fn() },
}

vi.mock('../store/app', () => ({
  useAppStore: <T,>(selector: (s: MockState) => T): T => selector(mockState),
}))

function remoteInfo(status: ConnectionStatus): ConnectionInfo {
  return {
    id: 'conn-1',
    target: { type: ConnectionTargetType.Remote, config: { id: 'conn-1', host: 'example.com', user: 'testuser', port: 22, portForwards: [] } },
    status,
  } as ConnectionInfo
}

function localInfo(): ConnectionInfo {
  return { id: 'local', target: { type: ConnectionTargetType.Local }, status: ConnectionStatus.Connected }
}

function resolveWatch(info: ConnectionInfo): void {
  mockState.ssh.watchConnectionStatus.mockResolvedValue({ initial: info, unsubscribe: vi.fn() })
}

describe('SshUploadPane', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('shows loading until the connection snapshot arrives', () => {
    mockState.ssh.watchConnectionStatus.mockReturnValue(new Promise(() => {}))
    render(<SshUploadPane connectionId="conn-1" />)
    expect(screen.getByText('Loading connection...')).toBeDefined()
  })

  it('rejects non-remote sessions', async () => {
    resolveWatch(localInfo())
    render(<SshUploadPane connectionId="local" />)
    await waitFor(() => {
      expect(screen.getByText('File upload is only available for SSH sessions.')).toBeDefined()
    })
  })

  it('disables upload until a local file is chosen', async () => {
    resolveWatch(remoteInfo(ConnectionStatus.Connected))
    render(<SshUploadPane connectionId="conn-1" />)
    const uploadBtn = await screen.findByRole('button', { name: 'Upload' })
    // Remote dir is pre-filled from home, but no local file yet → disabled.
    expect((uploadBtn as HTMLButtonElement).disabled).toBe(true)
  })

  it('uploads the chosen file to the remote directory on success', async () => {
    resolveWatch(remoteInfo(ConnectionStatus.Connected))
    mockState.selectFile.mockResolvedValue('/local/file.txt')
    mockState.ssh.uploadFile.mockResolvedValue({ success: true })

    render(<SshUploadPane connectionId="conn-1" />)
    fireEvent.click(await screen.findByRole('button', { name: 'Choose file...' }))

    const uploadBtn = await screen.findByRole('button', { name: 'Upload' })
    await waitFor(() => { expect((uploadBtn as HTMLButtonElement).disabled).toBe(false) })
    fireEvent.click(uploadBtn)

    await waitFor(() => {
      expect(mockState.ssh.uploadFile).toHaveBeenCalledWith('conn-1', '/local/file.txt', '/home/testuser/file.txt')
      expect(screen.getByText('Uploaded to /home/testuser/file.txt')).toBeDefined()
    })
  })

  it('shows an error banner when the upload fails', async () => {
    resolveWatch(remoteInfo(ConnectionStatus.Connected))
    mockState.selectFile.mockResolvedValue('/local/file.txt')
    mockState.ssh.uploadFile.mockResolvedValue({ success: false, error: 'Permission denied' })

    render(<SshUploadPane connectionId="conn-1" />)
    fireEvent.click(await screen.findByRole('button', { name: 'Choose file...' }))
    const uploadBtn = await screen.findByRole('button', { name: 'Upload' })
    await waitFor(() => { expect((uploadBtn as HTMLButtonElement).disabled).toBe(false) })
    fireEvent.click(uploadBtn)

    await waitFor(() => {
      expect(screen.getByText('Upload failed: Permission denied')).toBeDefined()
    })
  })

  it('disables upload while the connection is not connected', async () => {
    resolveWatch(remoteInfo(ConnectionStatus.Reconnecting))
    mockState.selectFile.mockResolvedValue('/local/file.txt')

    render(<SshUploadPane connectionId="conn-1" />)
    fireEvent.click(await screen.findByRole('button', { name: 'Choose file...' }))
    const uploadBtn = await screen.findByRole('button', { name: 'Upload' })
    // Local file chosen + remote dir set, but status is Reconnecting → still disabled.
    await waitFor(() => { expect(screen.getByDisplayValue('/local/file.txt')).toBeDefined() })
    expect((uploadBtn as HTMLButtonElement).disabled).toBe(true)
  })
})
