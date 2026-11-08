import { describe, it, expect } from 'vitest'
import { formatMegabytes } from './formatBytes'

describe('formatMegabytes', () => {
  it('formats bytes as megabytes to one decimal place', () => {
    expect(formatMegabytes(1024 * 1024)).toBe('1.0MB')
    expect(formatMegabytes(1.4 * 1024 * 1024)).toBe('1.4MB')
    expect(formatMegabytes(0)).toBe('0.0MB')
  })
})
