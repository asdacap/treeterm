// @vitest-environment jsdom
import { render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PdfViewer } from './PdfViewer'

// jsdom's Blob has no arrayBuffer(); FileReader reads it back.
function readBlob(blob: Blob): Promise<ArrayBuffer> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => { resolve(reader.result as ArrayBuffer) }
    reader.onerror = () => { reject(reader.error ?? new Error('read failed')) }
    reader.readAsArrayBuffer(blob)
  })
}

describe('PdfViewer', () => {
  // jsdom has no blob URLs; record what the viewer creates and releases instead.
  let created: Blob[]
  const createObjectURL = vi.fn<(blob: Blob) => string>()
  const revokeObjectURL = vi.fn<(url: string) => void>()

  beforeEach(() => {
    created = []
    createObjectURL.mockImplementation((blob) => {
      created.push(blob)
      return `blob:treeterm/${String(created.length)}`
    })
    Object.assign(URL, { createObjectURL, revokeObjectURL })
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  it('frames a PDF blob of the decoded bytes, titled with the file name', async () => {
    render(<PdfViewer base64="JVBERgD/" fileName="manual.pdf" />)

    const frame = screen.getByTitle('manual.pdf')
    expect(frame.getAttribute('src')).toBe('blob:treeterm/1')
    expect(created).toHaveLength(1)
    expect(created[0]!.type).toBe('application/pdf')
    expect(new Uint8Array(await readBlob(created[0]!))).toEqual(new Uint8Array([0x25, 0x50, 0x44, 0x46, 0x00, 0xff]))
    expect(revokeObjectURL).not.toHaveBeenCalled()
  })

  it('swaps in a new blob for new content and releases the old one', () => {
    const { rerender } = render(<PdfViewer base64="JVBERg==" fileName="manual.pdf" />)

    rerender(<PdfViewer base64="JVBERi0=" fileName="manual.pdf" />)

    expect(revokeObjectURL).toHaveBeenCalledWith('blob:treeterm/1')
    expect(screen.getByTitle('manual.pdf').getAttribute('src')).toBe('blob:treeterm/2')
  })

  it('releases the blob on unmount', () => {
    const { unmount } = render(<PdfViewer base64="JVBERg==" fileName="manual.pdf" />)

    unmount()

    expect(revokeObjectURL).toHaveBeenCalledWith('blob:treeterm/1')
  })
})
