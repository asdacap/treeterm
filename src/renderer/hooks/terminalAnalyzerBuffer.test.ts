import { describe, it, expect, beforeEach } from 'vitest'
import { TerminalAnalyzerBuffer } from './terminalAnalyzerBuffer'
import type { AnalyzerResult } from './terminalAnalyzerBuffer'

describe('TerminalAnalyzerBuffer', () => {
  let buf: TerminalAnalyzerBuffer
  const result: AnalyzerResult = { state: 'idle', reason: 'prompt visible' }

  beforeEach(() => {
    buf = new TerminalAnalyzerBuffer()
  })

  it('returns analyze for a fresh instance', () => {
    expect(buf.check('hello')).toEqual({ action: 'analyze' })
  })

  it('returns skip when same buffer is in-flight', () => {
    buf.setInFlight('hello')
    expect(buf.check('hello')).toEqual({ action: 'skip' })
  })

  it('returns analyze when a different buffer is in-flight', () => {
    buf.setInFlight('hello')
    expect(buf.check('world')).toEqual({ action: 'analyze' })
  })

  it('returns reuse when same buffer was already analyzed', () => {
    buf.setResult('hello', result)
    expect(buf.check('hello')).toEqual({ action: 'reuse', result })
  })

  it('returns analyze when a different buffer than last analyzed', () => {
    buf.setResult('hello', result)
    expect(buf.check('world')).toEqual({ action: 'analyze' })
  })

  it('clears in-flight when result is set', () => {
    buf.setInFlight('hello')
    buf.setResult('hello', result)
    // in-flight cleared, so checking same buffer returns reuse not skip
    expect(buf.check('hello')).toEqual({ action: 'reuse', result })
  })

  it('clearInFlight allows re-analyze of same buffer', () => {
    buf.setInFlight('hello')
    buf.clearInFlight()
    // no result stored yet, so it should analyze
    expect(buf.check('hello')).toEqual({ action: 'analyze' })
  })

  it('reset clears all state', () => {
    buf.setResult('hello', result)
    buf.reset()
    expect(buf.check('hello')).toEqual({ action: 'analyze' })
  })

  it('in-flight takes priority over last result for same buffer', () => {
    buf.setResult('hello', result)
    buf.setInFlight('hello')
    expect(buf.check('hello')).toEqual({ action: 'skip' })
  })
})
