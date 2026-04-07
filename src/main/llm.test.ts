import { describe, it, expect, vi, beforeEach } from 'vitest'

const { mockCreate } = vi.hoisted(() => ({
  mockCreate: vi.fn<(...args: any[]) => any>(),
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
import { formatLlmError, startChatStream, completeChatCall, cancelChatStream, parseLlmJson } from './llm'

const mockSettings = {
  baseUrl: 'https://api.openai.com/v1',
  apiKey: 'test-key',
  model: 'gpt-4',
  reasoning: 'off' as const,
}

function makeMockSender() {
  return { send: vi.fn<(...args: any[]) => void>() } as unknown as Electron.WebContents
}

describe('formatLlmError', () => {
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

  it('formats regular Error', () => {
    const result = formatLlmError(new Error('something went wrong'))
    expect(result).toBe('something went wrong')
  })

  it('formats unknown errors', () => {
    expect(formatLlmError('string error')).toBe('Unknown LLM error')
    expect(formatLlmError(42)).toBe('Unknown LLM error')
    expect(formatLlmError(null)).toBe('Unknown LLM error')
  })
})

describe('startChatStream', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('streams deltas to sender and sends done', async () => {
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

    const sender = makeMockSender()
    await startChatStream('req-1', [{ role: 'user', content: 'hi' }], mockSettings, sender)

    // eslint-disable-next-line @typescript-eslint/unbound-method
    expect(sender.send).toHaveBeenCalledWith('llm:chat:delta', 'req-1', 'Hello')
    // eslint-disable-next-line @typescript-eslint/unbound-method
    expect(sender.send).toHaveBeenCalledWith('llm:chat:delta', 'req-1', ' world')
    // eslint-disable-next-line @typescript-eslint/unbound-method
    expect(sender.send).toHaveBeenCalledWith('llm:chat:done', 'req-1')
  })

  it('skips chunks without content', async () => {
    mockCreate.mockResolvedValue({
      // eslint-disable-next-line @typescript-eslint/require-await
      [Symbol.asyncIterator]: async function* () {
        yield { choices: [{ delta: {} }] }
        yield { choices: [{ delta: { content: 'data' } }] }
      },
    })

    const sender = makeMockSender()
    await startChatStream('req-2', [], mockSettings, sender)

    // eslint-disable-next-line @typescript-eslint/unbound-method
    expect(sender.send).toHaveBeenCalledTimes(2) // one delta + done
  })

  it('sends error on exception', async () => {
    mockCreate.mockRejectedValue(new Error('network error'))

    const sender = makeMockSender()
    await startChatStream('req-3', [], mockSettings, sender)

    // eslint-disable-next-line @typescript-eslint/unbound-method
    expect(sender.send).toHaveBeenCalledWith('llm:chat:error', 'req-3', 'network error')
  })

  it('passes reasoning_effort when reasoning is enabled', async () => {
    mockCreate.mockResolvedValue({
       
      [Symbol.asyncIterator]: async function* () {
        // empty stream
      },
    })

    const sender = makeMockSender()
    await startChatStream('req-5', [], { ...mockSettings, reasoning: 'medium' }, sender)

    expect(mockCreate).toHaveBeenCalledWith(
      expect.objectContaining({ reasoning_effort: 'medium' }),
      expect.any(Object)
    )
  })
})

describe('completeChatCall', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('returns content from completion', async () => {
    mockCreate.mockResolvedValue({
      choices: [{ message: { content: 'response text' } }],
    })

    const result = await completeChatCall(
      [{ role: 'user', content: 'hello' }],
      mockSettings
    )
    expect(result).toBe('response text')
  })

  it('returns empty string when no content', async () => {
    mockCreate.mockResolvedValue({
      choices: [{ message: { content: null } }],
    })

    const result = await completeChatCall([], mockSettings)
    expect(result).toBe('')
  })

  it('returns empty string when no choices', async () => {
    mockCreate.mockResolvedValue({ choices: [] })

    const result = await completeChatCall([], mockSettings)
    expect(result).toBe('')
  })

  it('passes reasoning_effort when reasoning enabled', async () => {
    mockCreate.mockResolvedValue({ choices: [] })

    await completeChatCall([], { ...mockSettings, reasoning: 'high' })
    expect(mockCreate).toHaveBeenCalledWith(
      expect.objectContaining({ reasoning_effort: 'high', stream: false })
    )
  })
})

describe('cancelChatStream', () => {
  it('does nothing for unknown request id', () => {
    cancelChatStream('nonexistent')
  })

  it('aborts an active stream', async () => {
    mockCreate.mockResolvedValue({
      // eslint-disable-next-line @typescript-eslint/require-await
      [Symbol.asyncIterator]: async function* () {
        yield { choices: [{ delta: { content: 'hi' } }] }
      },
    })

    const sender = makeMockSender()
    // Start and complete the stream to register the request
    await startChatStream('cancel-me', [], mockSettings, sender)
    // Stream is done, activeStreams is cleaned up in finally — but we can test the abort path
    // by starting a new stream and immediately canceling
    let abortCalled = false
    // eslint-disable-next-line @typescript-eslint/unbound-method
    const realAbort: (reason?: any) => void = AbortController.prototype.abort
    AbortController.prototype.abort = function(this: AbortController, reason?: any) { abortCalled = true; realAbort.call(this, reason) }

    mockCreate.mockResolvedValue({
      // eslint-disable-next-line @typescript-eslint/require-await
      [Symbol.asyncIterator]: async function* () {
        yield { choices: [{ delta: { content: 'data' } }] }
      },
    })
    const promise = startChatStream('cancel-2', [], mockSettings, sender)
    cancelChatStream('cancel-2')
    await promise
    expect(abortCalled).toBe(true)
    AbortController.prototype.abort = realAbort
  })
})

describe('parseLlmJson', () => {
  it.each([
    ['plain JSON', '{"state":"idle","reason":"waiting"}', { state: 'idle', reason: 'waiting' }],
    ['markdown-fenced JSON', '```json\n{"state":"idle","reason":"waiting"}\n```', { state: 'idle', reason: 'waiting' }],
    [
      'nested markdown JSON in field value (title case)',
      '{"title":"```json\\n{\\"title\\": \\"TrieNodeCache Partition Removal\\", \\"description\\": \\"Convert partitioned cache\\"}\\n```","description":""}',
      { title: 'TrieNodeCache Partition Removal', description: 'Convert partitioned cache' },
    ],
    [
      'nested markdown JSON in field value (state case)',
      '{"state":"```json\\n{\\"state\\": \\"safe\\", \\"reason\\": \\"just ls\\"}\\n```","reason":""}',
      { state: 'safe', reason: 'just ls' },
    ],
    [
      'field value that looks like JSON but is not markdown-wrapped',
      '{"title":"My Title","description":"A description"}',
      { title: 'My Title', description: 'A description' },
    ],
  ])('handles %s', (_name, input, expected) => {
    expect(parseLlmJson(input)).toEqual(expected)
  })

  it('throws on invalid JSON', () => {
    expect(() => parseLlmJson('not json')).toThrow()
  })
})
