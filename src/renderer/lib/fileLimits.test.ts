import { describe, it, expect } from 'vitest'
import { MAX_READ_FILE_BYTES, MAX_READ_MODEL_BYTES, MAX_READ_PDF_BYTES, maxReadBytesFor } from './fileLimits'

describe('maxReadBytesFor', () => {
  it('gives 3D models and PDFs their larger budgets and every other file the default', () => {
    expect(maxReadBytesFor('/repo/parts/bracket.stl')).toBe(MAX_READ_MODEL_BYTES)
    expect(maxReadBytesFor('/repo/docs/Manual.PDF')).toBe(MAX_READ_PDF_BYTES)
    expect(maxReadBytesFor('/repo/src/main.ts')).toBe(MAX_READ_FILE_BYTES)
    expect(maxReadBytesFor('/repo/logo.png')).toBe(MAX_READ_FILE_BYTES)
  })
})
