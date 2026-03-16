import { useState, useCallback, useRef, useEffect } from 'react'
import { useSettingsStore } from '../store/settings'
import { useElectron } from '../store/ElectronContext'
import { createSTTProvider } from '../stt'
import type { STTProvider } from '../stt/types'

interface UsePushToTalkOptions {
  onTranscript: (text: string) => void
  onError?: (error: Error) => void
}

interface UsePushToTalkReturn {
  isRecording: boolean
  isProcessing: boolean
  startRecording: () => Promise<void>
  stopRecording: () => Promise<void>
  interimText: string
}

export function usePushToTalk({
  onTranscript,
  onError
}: UsePushToTalkOptions): UsePushToTalkReturn {
  const [isRecording, setIsRecording] = useState(false)
  const [isProcessing, setIsProcessing] = useState(false)
  const [interimText, setInterimText] = useState('')
  const providerRef = useRef<STTProvider | null>(null)
  const { settings } = useSettingsStore()
  const { stt } = useElectron()

  useEffect(() => {
    // Initialize provider based on settings
    if (settings.stt.enabled) {
      providerRef.current = createSTTProvider(
        stt,
        settings.stt.provider,
        settings.stt.openaiApiKey,
        settings.stt.localWhisperModelPath,
        settings.stt.language
      )
    }
  }, [settings.stt])

  const startRecording = useCallback(async () => {
    if (!providerRef.current) {
      onError?.(new Error('STT provider not initialized'))
      return
    }

    if (!settings.stt.enabled) {
      onError?.(new Error('Push-to-talk is disabled in settings'))
      return
    }

    try {
      // Check if provider is available
      const available = await providerRef.current.isAvailable()
      if (!available) {
        onError?.(
          new Error(
            `${providerRef.current.name} is not available. Please check your settings.`
          )
        )
        return
      }

      setIsRecording(true)
      setInterimText('')

      // Set up interim results callback if supported
      if (providerRef.current.onInterimResult) {
        providerRef.current.onInterimResult(setInterimText)
      }

      // Set up error callback for errors during recording
      if (providerRef.current.onError && onError) {
        providerRef.current.onError((error) => {
          console.error('Error during recording:', error)
          setIsRecording(false)
          setIsProcessing(false)
          onError(error)
        })
      }

      await providerRef.current.startListening()
    } catch (error) {
      console.error('Failed to start recording:', error)
      onError?.(error as Error)
      setIsRecording(false)
    }
  }, [settings.stt.enabled, onError])

  const stopRecording = useCallback(async () => {
    console.log('stopRecording called, isRecording:', isRecording, 'hasProvider:', !!providerRef.current)

    if (!providerRef.current) {
      console.log('No provider, aborting stopRecording')
      return
    }

    if (!isRecording) {
      console.log('Not recording, aborting stopRecording')
      return
    }

    try {
      console.log('Setting isRecording to false')
      setIsRecording(false)
      setIsProcessing(true)

      console.log('Calling provider.stopListening()')
      const result = await providerRef.current.stopListening()
      console.log('Got result:', result)

      if (result.text) {
        console.log('Transcribed text:', result.text)
        onTranscript(result.text)
      } else {
        console.log('No text transcribed')
      }
    } catch (error) {
      console.error('Failed to stop recording:', error)
      onError?.(error as Error)
    } finally {
      console.log('Finally: setting isProcessing to false')
      setIsProcessing(false)
      setInterimText('')
    }
  }, [isRecording, onTranscript, onError])

  return { isRecording, isProcessing, startRecording, stopRecording, interimText }
}
