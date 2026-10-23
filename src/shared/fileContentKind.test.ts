import { describe, it, expect } from 'vitest'
import { imageMimeType, isBinaryContent, isImagePath } from './fileContentKind'

describe('fileContentKind', () => {
  it('recognises image extensions case-insensitively', () => {
    expect(isImagePath('a/logo.PNG')).toBe(true)
    expect(isImagePath('a/icon.svg')).toBe(true)
    expect(isImagePath('a/app.ts')).toBe(false)
    expect(isImagePath('Makefile')).toBe(false)
  })

  it('maps image extensions to mime types', () => {
    expect(imageMimeType('photo.jpg')).toBe('image/jpeg')
    expect(imageMimeType('icon.svg')).toBe('image/svg+xml')
    expect(imageMimeType('data.bin')).toBe('application/octet-stream')
  })

  it('detects binary content by a leading NUL byte', () => {
    expect(isBinaryContent('hello\nworld')).toBe(false)
    expect(isBinaryContent('')).toBe(false)
    expect(isBinaryContent('\x89PNG\r\n\x1a\n\0\0')).toBe(true)
    expect(isBinaryContent(`${'x'.repeat(8000)}\0`)).toBe(false)
  })
})
