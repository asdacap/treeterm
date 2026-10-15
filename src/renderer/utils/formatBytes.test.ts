import { describe, it, expect } from 'vitest'
import { formatBytes, formatRate } from './formatBytes'

describe('formatBytes', () => {
  it('formats bytes', () => {
    expect(formatBytes(512)).toBe('512 B')
  })

  it('formats kilobytes', () => {
    expect(formatBytes(2048)).toBe('2.0 KB')
  })

  it('formats megabytes', () => {
    expect(formatBytes(1048576)).toBe('1.0 MB')
  })

  it('formats gigabytes', () => {
    expect(formatBytes(2147483648)).toBe('2.0 GB')
  })

  it('formats zero', () => {
    expect(formatBytes(0)).toBe('0 B')
  })
})

describe('formatRate', () => {
  it('rounds fractional bytes', () => {
    expect(formatRate(333.4)).toBe('333 B/s')
  })

  it('scales like formatBytes', () => {
    expect(formatRate(2048)).toBe('2.0 KB/s')
  })
})
