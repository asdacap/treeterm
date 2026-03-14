import { describe, it, expect, vi, beforeEach } from 'vitest'

const mockServer = {
  onSttTranscribeOpenai: vi.fn(),
  onSttTranscribeLocal: vi.fn(),
  onSttCheckMicPermission: vi.fn()
}

describe('stt', () => {
  beforeEach(() => {
    vi.clearAllMocks()
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
  })
})
