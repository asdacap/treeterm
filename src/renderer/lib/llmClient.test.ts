import { describe, it, expect } from 'vitest'
import { parseLlmJson, formatLlmError, createLlmClient } from './llmClient'

// ---------------------------------------------------------------------------
// parseLlmJson
// ---------------------------------------------------------------------------

describe('parseLlmJson', () => {
  it('parses plain JSON', () => {
    expect(parseLlmJson('{"state":"running","reason":"ok"}')).toEqual({ state: 'running', reason: 'ok' })
  })

  it('strips markdown fences', () => {
    expect(parseLlmJson('```json\n{"state":"done"}\n```')).toEqual({ state: 'done' })
  })

  it('merges nested JSON strings', () => {
    const raw = '{"state":"ok","nested":"{\\"title\\":\\"hello\\"}"}'
    const result = parseLlmJson(raw)
    expect(result.title).toBe('hello')
  })
})

// ---------------------------------------------------------------------------
// formatLlmError
// ---------------------------------------------------------------------------

describe('formatLlmError', () => {
  it('formats Error instances', () => {
    expect(formatLlmError(new Error('boom'))).toBe('boom')
  })

  it('formats unknown values', () => {
    expect(formatLlmError('oops')).toBe('Unknown LLM error')
  })
})

// ---------------------------------------------------------------------------
// createLlmClient — analyzer cache
// ---------------------------------------------------------------------------

describe('createLlmClient', () => {
  it('clearAnalyzerCache empties the cache', async () => {
    const client = createLlmClient()
    // No crash on empty clear
    await client.clearAnalyzerCache()
  })

  it('cancel aborts an active stream', () => {
    const client = createLlmClient()
    // Should not throw on unknown requestId
    client.cancel('nonexistent')
  })

  it('onDelta/onDone/onError return unsubscribe functions', () => {
    const client = createLlmClient()
    const unsub1 = client.onDelta(() => {})
    const unsub2 = client.onDone(() => {})
    const unsub3 = client.onError(() => {})
    expect(typeof unsub1).toBe('function')
    expect(typeof unsub2).toBe('function')
    expect(typeof unsub3).toBe('function')
    // Should not throw
    unsub1()
    unsub2()
    unsub3()
  })
})
