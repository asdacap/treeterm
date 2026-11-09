// @vitest-environment jsdom
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { FsReadFileBytesResult, WorkspaceFilesystemApi } from '../types'
import { createMockFilesystemApi } from '../../shared/mockApis'
import { FileDownloadButton } from './FileDownloadButton'

const download = vi.hoisted(() => ({ downloadBytes: vi.fn() }))
vi.mock('../utils/downloadFile', () => download)

/** A read the test settles by hand, to observe the in-flight state. */
function deferredRead(filesystem: WorkspaceFilesystemApi): {
  resolve: (result: FsReadFileBytesResult) => void
  reject: (err: unknown) => void
} {
  let settle!: { resolve: (result: FsReadFileBytesResult) => void; reject: (err: unknown) => void }
  // The executor runs synchronously, so `settle` is set before this returns.
  vi.mocked(filesystem.readFileBytes).mockReturnValue(new Promise<FsReadFileBytesResult>((resolve, reject) => {
    settle = { resolve, reject }
  }))
  return settle
}

describe('FileDownloadButton', () => {
  let filesystem: WorkspaceFilesystemApi

  beforeEach(() => {
    vi.clearAllMocks()
    filesystem = createMockFilesystemApi()
  })

  it('reads the exact bytes under the given budget and saves them under the file name', async () => {
    const read = deferredRead(filesystem)
    render(<FileDownloadButton filesystem={filesystem} filePath="assets/fonts/icon.woff2" maxBytes={2048} />)

    fireEvent.click(screen.getByText('Download'))

    const pending = screen.getByText('Downloading…').closest('button')!
    expect(pending.disabled).toBe(true)
    expect(filesystem.readFileBytes).toHaveBeenCalledWith('assets/fonts/icon.woff2', 2048)
    expect(download.downloadBytes).not.toHaveBeenCalled()

    read.resolve({ success: true, bytes: new Uint8Array([0x77, 0x4f, 0x46, 0x32]) })

    expect(await screen.findByText('Download')).toBeTruthy()
    expect(download.downloadBytes).toHaveBeenCalledWith('icon.woff2', new Uint8Array([0x77, 0x4f, 0x46, 0x32]))
  })

  it('shows a failed read and lets the user retry', async () => {
    vi.mocked(filesystem.readFileBytes).mockResolvedValueOnce({
      success: false, error: 'File too large to open (2.0MB, limit 1.0MB)', tooLarge: true, size: 2 * 1024 * 1024,
    })
    render(<FileDownloadButton filesystem={filesystem} filePath="big.log" maxBytes={1024} />)

    fireEvent.click(screen.getByText('Download'))

    expect(await screen.findByText('Download failed: File too large to open (2.0MB, limit 1.0MB)')).toBeTruthy()
    expect(download.downloadBytes).not.toHaveBeenCalled()

    vi.mocked(filesystem.readFileBytes).mockResolvedValueOnce({ success: true, bytes: new Uint8Array([1]) })
    fireEvent.click(screen.getByText('Retry download'))

    await waitFor(() => { expect(download.downloadBytes).toHaveBeenCalledWith('big.log', new Uint8Array([1])) })
    expect(screen.queryByText(/Download failed/)).toBeNull()
  })

  it('shows a read that throws', async () => {
    const read = deferredRead(filesystem)
    render(<FileDownloadButton filesystem={filesystem} filePath="a.bin" maxBytes={1024} />)

    fireEvent.click(screen.getByText('Download'))
    read.reject(new Error('Not connected to daemon'))

    expect(await screen.findByText('Download failed: Not connected to daemon')).toBeTruthy()
  })

  it('shows a non-Error rejection as text', async () => {
    const read = deferredRead(filesystem)
    render(<FileDownloadButton filesystem={filesystem} filePath="a.bin" maxBytes={1024} />)

    fireEvent.click(screen.getByText('Download'))
    read.reject('socket closed')

    expect(await screen.findByText('Download failed: socket closed')).toBeTruthy()
  })

  it('shows a save that throws, such as a blob the browser refuses', async () => {
    vi.mocked(filesystem.readFileBytes).mockResolvedValue({ success: true, bytes: new Uint8Array([1]) })
    download.downloadBytes.mockImplementationOnce(() => { throw new Error('Blob too large') })
    render(<FileDownloadButton filesystem={filesystem} filePath="a.bin" maxBytes={1024} />)

    fireEvent.click(screen.getByText('Download'))

    expect(await screen.findByText('Download failed: Blob too large')).toBeTruthy()
  })
})
