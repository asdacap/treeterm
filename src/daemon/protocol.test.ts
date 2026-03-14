import { describe, it, expect } from 'vitest'
import { isTypedResponse } from './protocol'
import type { DaemonResponse } from './protocol'

describe('isTypedResponse', () => {
  it('returns true for success type', () => {
    const response: DaemonResponse = { type: 'success' }
    expect(isTypedResponse(response)).toBe(true)
  })

  it('returns true for error type', () => {
    const response: DaemonResponse = { type: 'error', error: 'something failed' }
    expect(isTypedResponse(response)).toBe(true)
  })

  it('returns true for data type', () => {
    const response: DaemonResponse = { type: 'data', sessionId: 'abc', payload: 'some data' }
    expect(isTypedResponse(response)).toBe(true)
  })

  it('returns true for scrollback type', () => {
    const response: DaemonResponse = { type: 'scrollback', sessionId: 'abc', payload: [] }
    expect(isTypedResponse(response)).toBe(true)
  })

  it('returns true for exit type', () => {
    const response: DaemonResponse = { type: 'exit', sessionId: 'abc', payload: { exitCode: 0 } }
    expect(isTypedResponse(response)).toBe(true)
  })

  it('returns false for unknown type', () => {
    const response = { type: 'unknown' } as unknown as DaemonResponse
    expect(isTypedResponse(response)).toBe(false)
  })

  it('returns false for empty string type', () => {
    const response = { type: '' } as unknown as DaemonResponse
    expect(isTypedResponse(response)).toBe(false)
  })
})
