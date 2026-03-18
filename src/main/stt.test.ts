import { describe, it, expect, vi, beforeEach } from 'vitest'

const { mockCreate } = vi.hoisted(() => ({
  mockCreate: vi.fn()
}))

vi.mock('openai', () => {
  return {
    default: class MockOpenAI {
      audio = {
        transcriptions: {
          create: mockCreate
        }
      }
    }
  }
})

const mockServer = {
  onSttTranscribeOpenai: vi.fn(),
  onSttTranscribeLocal: vi.fn(),
  onSttCheckMicPermission: vi.fn()
}

describe('stt', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.resetModules()
  })

  describe('registerSTTHandlers', () => {
    it('registers all STT handlers', async () => {
      const { registerSTTHandlers } = await import('./stt')

      registerSTTHandlers(mockServer as any)

      expect(mockServer.onSttTranscribeOpenai).toHaveBeenCalled()
      expect(mockServer.onSttTranscribeLocal).toHaveBeenCalled()
      expect(mockServer.onSttCheckMicPermission).toHaveBeenCalled()
    })

    it('registers OpenAI handler that handles transcription', async () => {
      const { registerSTTHandlers } = await import('./stt')

      registerSTTHandlers(mockServer as any)

      const openaiHandler = mockServer.onSttTranscribeOpenai.mock.calls[0][0]

      // Handler should be an async function
      expect(typeof openaiHandler).toBe('function')
    })

    it('registers local handler that throws not implemented error', async () => {
      const { registerSTTHandlers } = await import('./stt')

      registerSTTHandlers(mockServer as any)

      const localHandler = mockServer.onSttTranscribeLocal.mock.calls[0][0]

      await expect(localHandler(Buffer.from(''), '')).rejects.toThrow('Local Whisper is not yet implemented')
    })

    it('registers mic permission handler that returns true', async () => {
      const { registerSTTHandlers } = await import('./stt')

      registerSTTHandlers(mockServer as any)

      const permissionHandler = mockServer.onSttCheckMicPermission.mock.calls[0][0]

      const result = await permissionHandler()
      expect(result).toBe(true)
    })

    it('OpenAI handler transcribes audio with language', async () => {
      mockCreate.mockResolvedValue({ text: 'hello world' })

      const { registerSTTHandlers } = await import('./stt')
      registerSTTHandlers(mockServer as any)

      const handler = mockServer.onSttTranscribeOpenai.mock.calls[0][0]
      const buffer = new ArrayBuffer(8)
      const result = await handler(buffer, 'sk-test-key', 'en')

      expect(result).toEqual({ text: 'hello world' })
      expect(mockCreate).toHaveBeenCalledWith(
        expect.objectContaining({
          model: 'whisper-1',
          language: 'en'
        })
      )
    })

    it('OpenAI handler transcribes audio without language', async () => {
      mockCreate.mockResolvedValue({ text: 'bonjour' })

      const { registerSTTHandlers } = await import('./stt')
      registerSTTHandlers(mockServer as any)

      const handler = mockServer.onSttTranscribeOpenai.mock.calls[0][0]
      const buffer = new ArrayBuffer(8)
      const result = await handler(buffer, 'sk-test-key', '')

      expect(result).toEqual({ text: 'bonjour' })
      // language should not be included when empty
      const createArgs = mockCreate.mock.calls[0][0]
      expect(createArgs.language).toBeUndefined()
    })

    it('OpenAI handler propagates API errors', async () => {
      mockCreate.mockRejectedValue(new Error('API rate limit exceeded'))

      const { registerSTTHandlers } = await import('./stt')
      registerSTTHandlers(mockServer as any)

      const handler = mockServer.onSttTranscribeOpenai.mock.calls[0][0]
      const buffer = new ArrayBuffer(8)

      await expect(handler(buffer, 'sk-test-key', 'en')).rejects.toThrow('API rate limit exceeded')
    })
  })
})
