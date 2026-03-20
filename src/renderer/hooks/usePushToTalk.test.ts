import { describe, it, expect, vi, beforeEach } from 'vitest'

const { mockProvider, mockCreateSTTProvider, effectCallbacks } = vi.hoisted(() => {
  const mockProvider = {
    name: 'TestProvider',
    isAvailable: vi.fn(),
    startListening: vi.fn(),
    stopListening: vi.fn(),
    onInterimResult: vi.fn(),
    onError: vi.fn()
  }
  return {
    mockProvider,
    mockCreateSTTProvider: vi.fn(() => mockProvider),
    effectCallbacks: [] as Array<() => void>
  }
})

vi.mock('../store/settings', () => ({
  useSettingsStore: vi.fn(() => ({
    settings: {
      stt: {
        enabled: true,
        provider: 'openaiWhisper',
        openaiApiKey: 'key',
        localWhisperModelPath: '',
        language: 'en'
      }
    }
  }))
}))

vi.mock('../contexts/STTApiContext', () => ({
  useSTTApi: vi.fn(() => ({ startRecording: vi.fn() }))
}))

vi.mock('../stt', () => ({
  createSTTProvider: mockCreateSTTProvider
}))

vi.mock('react', () => ({
  useState: vi.fn((initial: unknown) => {
    const setter = vi.fn()
    return [initial, setter]
  }),
  useCallback: vi.fn((fn: unknown) => fn),
  useRef: vi.fn((initial: unknown) => ({ current: initial })),
  useEffect: vi.fn((fn: () => void) => {
    effectCallbacks.push(fn)
  })
}))

import { usePushToTalk } from './usePushToTalk'
import { useRef, useEffect } from 'react'

describe('usePushToTalk', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    effectCallbacks.length = 0
    mockProvider.isAvailable.mockResolvedValue(true)
    mockProvider.startListening.mockResolvedValue(undefined)
    mockProvider.stopListening.mockResolvedValue({ text: 'hello world' })
  })

  it('returns expected shape', () => {
    const onTranscript = vi.fn()
    const result = usePushToTalk({ onTranscript })

    expect(result).toHaveProperty('isRecording')
    expect(result).toHaveProperty('isProcessing')
    expect(result).toHaveProperty('startRecording')
    expect(result).toHaveProperty('stopRecording')
    expect(result).toHaveProperty('interimText')
    expect(typeof result.startRecording).toBe('function')
    expect(typeof result.stopRecording).toBe('function')
  })

  it('initializes provider via useEffect', () => {
    const onTranscript = vi.fn()
    usePushToTalk({ onTranscript })

    expect(useEffect).toHaveBeenCalled()
    if (effectCallbacks.length > 0) {
      effectCallbacks[0]()
    }
    expect(mockCreateSTTProvider).toHaveBeenCalled()
  })

  it('calls onError when provider is not initialized and startRecording is called', async () => {
    vi.mocked(useRef).mockReturnValueOnce({ current: null })

    const onTranscript = vi.fn()
    const onError = vi.fn()
    const { startRecording } = usePushToTalk({ onTranscript, onError })

    await startRecording()
    expect(onError).toHaveBeenCalledWith(
      expect.objectContaining({ message: 'STT provider not initialized' })
    )
  })

  it('calls onError when provider is not available', async () => {
    mockProvider.isAvailable.mockResolvedValue(false)
    vi.mocked(useRef).mockReturnValueOnce({ current: mockProvider })

    const onTranscript = vi.fn()
    const onError = vi.fn()
    const { startRecording } = usePushToTalk({ onTranscript, onError })

    await startRecording()
    expect(onError).toHaveBeenCalledWith(
      expect.objectContaining({ message: expect.stringContaining('not available') })
    )
  })

  it('starts listening on the provider when available', async () => {
    vi.mocked(useRef).mockReturnValueOnce({ current: mockProvider })

    const onTranscript = vi.fn()
    const { startRecording } = usePushToTalk({ onTranscript })

    await startRecording()
    expect(mockProvider.isAvailable).toHaveBeenCalled()
    expect(mockProvider.startListening).toHaveBeenCalled()
  })

  it('stopRecording is a no-op when not recording', async () => {
    vi.mocked(useRef).mockReturnValueOnce({ current: mockProvider })

    const onTranscript = vi.fn()
    const { stopRecording } = usePushToTalk({ onTranscript })

    await stopRecording()
    expect(mockProvider.stopListening).not.toHaveBeenCalled()
  })

  it('handles error during startListening', async () => {
    const error = new Error('mic error')
    mockProvider.isAvailable.mockResolvedValue(true)
    mockProvider.startListening.mockRejectedValue(error)
    vi.mocked(useRef).mockReturnValueOnce({ current: mockProvider })

    const onTranscript = vi.fn()
    const onError = vi.fn()
    const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    const { startRecording } = usePushToTalk({ onTranscript, onError })
    await startRecording()

    expect(onError).toHaveBeenCalledWith(error)
    consoleSpy.mockRestore()
  })
})
