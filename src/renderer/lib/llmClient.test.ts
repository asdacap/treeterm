import { describe, it, expect, vi, beforeEach } from 'vitest'
import { ReasoningEffort } from '../../shared/types'

const { mockCreate } = vi.hoisted(() => ({
  mockCreate: vi.fn<(...args: unknown[]) => unknown>(),
}))

vi.mock('openai', () => {
  class MockAPIError extends Error {
    status: number | undefined
    error: unknown
    headers: unknown
    constructor(status: number | undefined, error: unknown, message: string | undefined, headers?: unknown) {
      super(message ?? '')
      this.name = 'APIError'
      this.status = status
      this.error = error
      this.headers = headers
    }
  }

  class MockOpenAI {
    chat = {
      completions: {
        create: mockCreate,
      },
    }
  }

  return {
    default: MockOpenAI,
    APIError: MockAPIError,
  }
})

import { APIError } from 'openai'
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

  it('throws on invalid JSON', () => {
    expect(() => parseLlmJson('not json')).toThrow()
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

  it('formats APIError with error body', () => {
    const err = new APIError(429, { message: 'too many requests' }, 'rate limit', undefined)
    const result = formatLlmError(err)
    expect(result).toContain('rate limit')
    expect(result).toContain('too many requests')
  })

  it('formats APIError without error body', () => {
    const err = new APIError(400, undefined, 'bad request', undefined)
    const result = formatLlmError(err)
    expect(result).toBe('bad request')
  })
})

// ---------------------------------------------------------------------------
// createLlmClient — send (streaming)
// ---------------------------------------------------------------------------

const mockSettings = {
  baseUrl: 'https://api.openai.com/v1',
  apiKey: 'test-key',
  model: 'gpt-4',
  reasoning: ReasoningEffort.Off,
}

describe('createLlmClient send', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('streams deltas to listeners and fires done', async () => {
    const chunks = [
      { choices: [{ delta: { content: 'Hello' } }] },
      { choices: [{ delta: { content: ' world' } }] },
    ]
    mockCreate.mockResolvedValue({
      // eslint-disable-next-line @typescript-eslint/require-await
      [Symbol.asyncIterator]: async function* () {
        for (const chunk of chunks) yield chunk
      },
    })

    const client = createLlmClient()
    const deltas: [string, string][] = []
    const doneIds: string[] = []
    client.onDelta((rid, text) => { deltas.push([rid, text]) })
    client.onDone((rid) => { doneIds.push(rid) })

    await client.send('req-1', [{ role: 'user', content: 'hi' }], mockSettings)

    expect(deltas).toEqual([['req-1', 'Hello'], ['req-1', ' world']])
    expect(doneIds).toEqual(['req-1'])
  })

  it('skips chunks without content', async () => {
    mockCreate.mockResolvedValue({
      // eslint-disable-next-line @typescript-eslint/require-await
      [Symbol.asyncIterator]: async function* () {
        yield { choices: [{ delta: {} }] }
        yield { choices: [{ delta: { content: 'data' } }] }
      },
    })

    const client = createLlmClient()
    const deltas: string[] = []
    client.onDelta((_rid, text) => { deltas.push(text) })

    await client.send('req-2', [], mockSettings)

    expect(deltas).toEqual(['data'])
  })

  it('sends error on exception', async () => {
    mockCreate.mockRejectedValue(new Error('network error'))

    const client = createLlmClient()
    const errors: [string, string][] = []
    client.onError((rid, msg) => { errors.push([rid, msg]) })

    await client.send('req-3', [], mockSettings)

    expect(errors).toEqual([['req-3', 'network error']])
  })

  it('passes reasoning_effort when reasoning is enabled', async () => {
    mockCreate.mockResolvedValue({
      [Symbol.asyncIterator]: async function* () {
        // empty stream
      },
    })

    const client = createLlmClient()
    await client.send('req-5', [], { ...mockSettings, reasoning: ReasoningEffort.Medium })

    expect(mockCreate).toHaveBeenCalledWith(
      expect.objectContaining({ reasoning_effort: ReasoningEffort.Medium }),
      expect.any(Object)
    )
  })
})

// ---------------------------------------------------------------------------
// createLlmClient — completeChatCall (tested via analyzeTerminal / generateTitle)
// ---------------------------------------------------------------------------

const analyzerSettings = {
  baseUrl: 'https://api.openai.com/v1',
  apiKey: 'test-key',
  model: 'gpt-4',
  systemPrompt: 'You are analyzing a terminal at {{cwd}}. Safe paths: {{safe_paths}}',
  reasoningEffort: ReasoningEffort.Off,
  safePaths: ['/usr/bin'],
}

describe('createLlmClient analyzeTerminal (completeChatCall)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('returns content from completion', async () => {
    mockCreate.mockResolvedValue({
      choices: [{ message: { content: '{"state":"idle","reason":"waiting"}' } }],
    })

    const client = createLlmClient()
    const result = await client.analyzeTerminal('$ ls', '/home/user', analyzerSettings)

    expect(result).toEqual(expect.objectContaining({ state: 'idle', reason: 'waiting' }))
  })

  it('returns error when no content', async () => {
    mockCreate.mockResolvedValue({
      choices: [{ message: { content: null } }],
    })

    const client = createLlmClient()
    const result = await client.analyzeTerminal('$ ls', '/home/user', analyzerSettings)

    // Empty string from completeChatCall → JSON.parse('') throws → error path
    // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
    expect(result).toEqual(expect.objectContaining({ error: expect.any(String) }))
  })

  it('returns error when no choices', async () => {
    mockCreate.mockResolvedValue({ choices: [] })

    const client = createLlmClient()
    const result = await client.analyzeTerminal('$ ls', '/home/user', analyzerSettings)

    // Empty string → JSON.parse('') throws → error path
    // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment
    expect(result).toEqual(expect.objectContaining({ error: expect.any(String) }))
  })

  it('passes reasoning_effort when reasoning enabled', async () => {
    mockCreate.mockResolvedValue({
      choices: [{ message: { content: '{"state":"ok","reason":"test"}' } }],
    })

    const client = createLlmClient()
    await client.analyzeTerminal('$ ls', '/home/user', {
      ...analyzerSettings,
      reasoningEffort: ReasoningEffort.High,
    })

    expect(mockCreate).toHaveBeenCalledWith(
      expect.objectContaining({ reasoning_effort: ReasoningEffort.High, stream: false })
    )
  })
})

// ---------------------------------------------------------------------------
// createLlmClient — cancel
// ---------------------------------------------------------------------------

describe('createLlmClient cancel', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('does nothing for unknown request id', () => {
    const client = createLlmClient()
    // Should not throw on unknown requestId
    client.cancel('nonexistent')
  })

  it('aborts an active stream', async () => {
    // Create a stream that yields one chunk then hangs until aborted
    let resolveHang: (() => void) | undefined
    const hangPromise = new Promise<void>((resolve) => { resolveHang = resolve })

    mockCreate.mockResolvedValue({
      [Symbol.asyncIterator]: async function* () {
        yield { choices: [{ delta: { content: 'start' } }] }
        // Hang here until the controller is aborted
        await hangPromise
        yield { choices: [{ delta: { content: 'should not arrive' } }] }
      },
    })

    const client = createLlmClient()
    const deltas: string[] = []
    const doneIds: string[] = []
    client.onDelta((_rid, text) => { deltas.push(text) })
    client.onDone((rid) => { doneIds.push(rid) })

    // Start the stream but don't await it yet
    const sendPromise = client.send('cancel-me', [], mockSettings)

    // Wait a tick so the stream starts iterating
    await new Promise<void>((r) => { setTimeout(r, 10) })

    // Cancel the stream
    client.cancel('cancel-me')

    // Resolve the hang so the generator can finish
    resolveHang!()

    await sendPromise

    // The first delta should have been received, but done should NOT fire (aborted)
    expect(deltas).toContain('start')
    expect(doneIds).toEqual([])
  })
})

// ---------------------------------------------------------------------------
// createLlmClient — clearAnalyzerCache
// ---------------------------------------------------------------------------

describe('createLlmClient clearAnalyzerCache', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('clearAnalyzerCache empties the cache', async () => {
    const client = createLlmClient()
    // No crash on empty clear
    await client.clearAnalyzerCache()
  })
})

// ---------------------------------------------------------------------------
// createLlmClient — onDelta/onDone/onError
// ---------------------------------------------------------------------------

describe('createLlmClient event subscriptions', () => {
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

// ---------------------------------------------------------------------------
// createLlmClient — analyzeTerminal cache
// ---------------------------------------------------------------------------

describe('createLlmClient analyzeTerminal cache', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('cache hit returns cached result with cached: true', async () => {
    mockCreate.mockResolvedValue({
      choices: [{ message: { content: '{"state":"idle","reason":"waiting"}' } }],
    })

    const client = createLlmClient()

    // First call — cache miss
    const result1 = await client.analyzeTerminal('$ ls', '/home/user', analyzerSettings)
    expect(result1).toEqual(expect.objectContaining({ state: 'idle', reason: 'waiting' }))
    expect(result1).not.toHaveProperty('cached')

    // Second call with same buffer — cache hit
    const result2 = await client.analyzeTerminal('$ ls', '/home/user', analyzerSettings)
    expect(result2).toEqual(expect.objectContaining({ state: 'idle', reason: 'waiting', cached: true }))

    // Only one API call should have been made
    expect(mockCreate).toHaveBeenCalledTimes(1)
  })

  it('cache evicts oldest entry when full (LRU, size 10)', async () => {
    let callCount = 0
    mockCreate.mockImplementation(() => {
      callCount++
      return Promise.resolve({
        choices: [{ message: { content: `{"state":"s${String(callCount)}","reason":"r${String(callCount)}"}` } }],
      })
    })

    const client = createLlmClient()

    // Fill the cache with 10 entries
    for (let i = 0; i < 10; i++) {
      await client.analyzeTerminal(`buffer-${String(i)}`, '/home/user', analyzerSettings)
    }
    expect(callCount).toBe(10)

    // Add one more to trigger eviction (oldest = buffer-0)
    await client.analyzeTerminal('buffer-10', '/home/user', analyzerSettings)
    expect(callCount).toBe(11)

    // buffer-0 should have been evicted — requesting it again should cause a new API call
    await client.analyzeTerminal('buffer-0', '/home/user', analyzerSettings)
    expect(callCount).toBe(12)

    // buffer-2 should still be cached (only buffer-0 and buffer-1 were evicted)
    await client.analyzeTerminal('buffer-2', '/home/user', analyzerSettings)
    expect(callCount).toBe(12) // no new call
  })
})
