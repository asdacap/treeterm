import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createSTTProvider } from './index'
import { OpenAIWhisperProvider } from './openaiWhisperProvider'
import { LocalWhisperProvider } from './localWhisperProvider'
import type { STTApi } from '../types/index'

function createMockSTTApi(): STTApi {
  return {
    transcribeOpenAI: vi.fn().mockResolvedValue({ text: 'hello world' }),
    transcribeLocal: vi.fn().mockResolvedValue({ text: 'local result' }),
    checkMicPermission: vi.fn().mockResolvedValue(true)
  }
}

// Set up browser API mocks
function setupBrowserMocks(): void {
  Object.defineProperty(globalThis, 'window', {
    value: { MediaRecorder: class {} },
    writable: true,
    configurable: true
  })
  Object.defineProperty(globalThis, 'navigator', {
    value: {
      mediaDevices: {
        getUserMedia: vi.fn().mockResolvedValue({
          getTracks: () => [{ stop: vi.fn() }]
        })
      }
    },
    writable: true,
    configurable: true
  })
}

function clearBrowserMocks(): void {
  Object.defineProperty(globalThis, 'window', {
    value: undefined,
    writable: true,
    configurable: true
  })
  Object.defineProperty(globalThis, 'navigator', {
    value: undefined,
    writable: true,
    configurable: true
  })
}

describe('createSTTProvider', () => {
  it('returns OpenAIWhisperProvider for openaiWhisper', () => {
    const api = createMockSTTApi()
    const provider = createSTTProvider(api, 'openaiWhisper', 'key-123')
    expect(provider).toBeInstanceOf(OpenAIWhisperProvider)
  })

  it('returns LocalWhisperProvider for localWhisper', () => {
    const api = createMockSTTApi()
    const provider = createSTTProvider(api, 'localWhisper', undefined, '/model/path')
    expect(provider).toBeInstanceOf(LocalWhisperProvider)
  })

  it('defaults to OpenAIWhisperProvider for unknown provider', () => {
    const api = createMockSTTApi()
    const provider = createSTTProvider(api, 'unknown' as any, 'key')
    expect(provider).toBeInstanceOf(OpenAIWhisperProvider)
  })
})

describe('OpenAIWhisperProvider', () => {
  let api: STTApi

  beforeEach(() => {
    api = createMockSTTApi()
    clearBrowserMocks()
  })

  describe('isAvailable', () => {
    it('returns false when no apiKey', async () => {
      setupBrowserMocks()
      const provider = new OpenAIWhisperProvider(api, '')
      expect(await provider.isAvailable()).toBe(false)
    })

    it('returns false when MediaRecorder not in window', async () => {
      Object.defineProperty(globalThis, 'window', {
        value: {},
        writable: true,
        configurable: true
      })
      Object.defineProperty(globalThis, 'navigator', {
        value: { mediaDevices: { getUserMedia: vi.fn() } },
        writable: true,
        configurable: true
      })
      const provider = new OpenAIWhisperProvider(api, 'key-123')
      expect(await provider.isAvailable()).toBe(false)
    })

    it('returns true when apiKey present and browser APIs available', async () => {
      setupBrowserMocks()
      const provider = new OpenAIWhisperProvider(api, 'key-123')
      expect(await provider.isAvailable()).toBe(true)
    })
  })

  describe('startListening', () => {
    it('calls getUserMedia and creates MediaRecorder', async () => {
      const mockStream = { getTracks: () => [{ stop: vi.fn() }] }
      const getUserMedia = vi.fn().mockResolvedValue(mockStream)
      Object.defineProperty(globalThis, 'navigator', {
        value: { mediaDevices: { getUserMedia } },
        writable: true,
        configurable: true
      })

      const mockStart = vi.fn()
      class MockMediaRecorder {
        start = mockStart
        ondataavailable: any = null
        constructor() {}
      }
      Object.defineProperty(globalThis, 'MediaRecorder', {
        value: MockMediaRecorder,
        writable: true,
        configurable: true
      })

      const provider = new OpenAIWhisperProvider(api, 'key-123')
      await provider.startListening()

      expect(getUserMedia).toHaveBeenCalledWith({ audio: true })
      expect(mockStart).toHaveBeenCalled()
    })

    it('throws on getUserMedia failure', async () => {
      const getUserMedia = vi.fn().mockRejectedValue(new Error('Permission denied'))
      Object.defineProperty(globalThis, 'navigator', {
        value: { mediaDevices: { getUserMedia } },
        writable: true,
        configurable: true
      })

      const provider = new OpenAIWhisperProvider(api, 'key-123')
      await expect(provider.startListening()).rejects.toThrow('Permission denied')
    })
  })

  describe('stopListening', () => {
    it('returns empty text when no recorder', async () => {
      const provider = new OpenAIWhisperProvider(api, 'key-123')
      const result = await provider.stopListening()
      expect(result).toEqual({ text: '' })
    })
  })

  describe('onInterimResult and onError', () => {
    it('onInterimResult does not throw', () => {
      const provider = new OpenAIWhisperProvider(api, 'key-123')
      expect(() => provider.onInterimResult(vi.fn())).not.toThrow()
    })

    it('onError does not throw', () => {
      const provider = new OpenAIWhisperProvider(api, 'key-123')
      expect(() => provider.onError(vi.fn())).not.toThrow()
    })
  })
})

describe('LocalWhisperProvider', () => {
  let api: STTApi

  beforeEach(() => {
    api = createMockSTTApi()
    clearBrowserMocks()
  })

  describe('isAvailable', () => {
    it('returns false when no modelPath', async () => {
      setupBrowserMocks()
      const provider = new LocalWhisperProvider(api, '')
      expect(await provider.isAvailable()).toBe(false)
    })

    it('returns true when modelPath present and browser APIs available', async () => {
      setupBrowserMocks()
      const provider = new LocalWhisperProvider(api, '/path/to/model')
      expect(await provider.isAvailable()).toBe(true)
    })
  })

  describe('startListening', () => {
    it('calls getUserMedia and creates MediaRecorder', async () => {
      const mockStream = { getTracks: () => [{ stop: vi.fn() }] }
      const getUserMedia = vi.fn().mockResolvedValue(mockStream)
      Object.defineProperty(globalThis, 'navigator', {
        value: { mediaDevices: { getUserMedia } },
        writable: true,
        configurable: true
      })

      const mockStart = vi.fn()
      class MockMediaRecorder {
        start = mockStart
        ondataavailable: any = null
        constructor() {}
      }
      Object.defineProperty(globalThis, 'MediaRecorder', {
        value: MockMediaRecorder,
        writable: true,
        configurable: true
      })

      const provider = new LocalWhisperProvider(api, '/path/to/model')
      await provider.startListening()

      expect(getUserMedia).toHaveBeenCalledWith({ audio: true })
      expect(mockStart).toHaveBeenCalled()
    })
  })

  describe('stopListening', () => {
    it('returns empty text when no recorder', async () => {
      const provider = new LocalWhisperProvider(api, '/path/to/model')
      const result = await provider.stopListening()
      expect(result).toEqual({ text: '' })
    })
  })
})
