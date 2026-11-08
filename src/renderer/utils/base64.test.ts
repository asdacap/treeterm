import { describe, it, expect } from 'vitest'
import { base64ToBytes, decodeBase64Utf8 } from './base64'

describe('base64ToBytes', () => {
  it('decodes to the exact bytes, including NUL and high bytes', () => {
    expect(Array.from(base64ToBytes(Buffer.from([0, 255, 128, 1]).toString('base64')))).toEqual([0, 255, 128, 1])
    expect(base64ToBytes('')).toHaveLength(0)
  })

  it('throws on malformed input', () => {
    expect(() => base64ToBytes('not base64!')).toThrow()
  })
})

describe('decodeBase64Utf8', () => {
  it('matches what a UTF-8 read of the same bytes returns', () => {
    const bytes = Buffer.concat([
      Buffer.from([0xef, 0xbb, 0xbf]), // BOM, kept
      Buffer.from('héllo ', 'utf-8'),
      Buffer.from([0xff]), // invalid, becomes U+FFFD
    ])
    expect(decodeBase64Utf8(bytes.toString('base64'))).toBe(bytes.toString('utf-8'))
  })
})
