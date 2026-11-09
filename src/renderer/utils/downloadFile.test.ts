// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { downloadBytes } from './downloadFile'

// jsdom's Blob has no arrayBuffer(); FileReader reads it back.
function readBlob(blob: Blob): Promise<ArrayBuffer> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => { resolve(reader.result as ArrayBuffer) }
    reader.onerror = () => { reject(reader.error ?? new Error('read failed')) }
    reader.readAsArrayBuffer(blob)
  })
}

describe('downloadBytes', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('clicks a download link to a blob of the bytes, then releases the blob URL', async () => {
    // jsdom has no blob URLs; record what the helper hands over instead.
    const createObjectURL = vi.fn<(blob: Blob) => string>(() => 'blob:treeterm/1')
    const revokeObjectURL = vi.fn()
    Object.assign(URL, { createObjectURL, revokeObjectURL })
    const clicked: HTMLAnchorElement[] = []
    vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
      clicked.push(this)
    })

    downloadBytes('part.stl', new Uint8Array([0, 1, 255]))

    expect(clicked).toHaveLength(1)
    expect(clicked[0]!.download).toBe('part.stl')
    expect(clicked[0]!.href).toBe('blob:treeterm/1')
    const blob = createObjectURL.mock.calls[0]![0]
    expect(blob.type).toBe('application/octet-stream')
    expect(Array.from(new Uint8Array(await readBlob(blob)))).toEqual([0, 1, 255])
    expect(revokeObjectURL).toHaveBeenCalledWith('blob:treeterm/1')
  })
})
