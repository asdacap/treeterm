import { useCallback, useState, useEffect, useRef } from 'react'
import { useSettingsStore } from '../store/settings'
import { useSTTApi } from '../contexts/STTApiContext'
import { createSTTProvider } from '../stt'
import type { STTProvider } from '../stt/types'

interface PushToTalkButtonProps {
  onTranscript: (text: string) => void
  onSubmit: () => void
}

export default function PushToTalkButton({ onTranscript, onSubmit }: PushToTalkButtonProps) {
  const [isRecording, setIsRecording] = useState(false)
  const [isProcessing, setIsProcessing] = useState(false)
  const [interimText, setInterimText] = useState('')
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const providerRef = useRef<STTProvider | null>(null)
  const { settings } = useSettingsStore()
  const stt = useSTTApi()

  useEffect(() => {
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

  const handleTranscript = useCallback(
    (text: string) => {
      onTranscript(text)
      setTimeout(() => onSubmit(), 50)
    },
    [onTranscript, onSubmit]
  )

  const handleError = useCallback((error: Error) => {
    console.error('Push-to-talk error:', error)
    setErrorMessage(error.message)
    setTimeout(() => setErrorMessage(null), 5000)
  }, [])

  const startRecording = useCallback(async () => {
    if (!providerRef.current) {
      handleError(new Error('STT provider not initialized'))
      return
    }

    if (!settings.stt.enabled) {
      handleError(new Error('Push-to-talk is disabled in settings'))
      return
    }

    try {
      const available = await providerRef.current.isAvailable()
      if (!available) {
        handleError(
          new Error(
            `${providerRef.current.name} is not available. Please check your settings.`
          )
        )
        return
      }

      setIsRecording(true)
      setInterimText('')

      if (providerRef.current.onInterimResult) {
        providerRef.current.onInterimResult(setInterimText)
      }

      if (providerRef.current.onError) {
        providerRef.current.onError((error) => {
          console.error('Error during recording:', error)
          setIsRecording(false)
          setIsProcessing(false)
          handleError(error)
        })
      }

      await providerRef.current.startListening()
    } catch (error) {
      console.error('Failed to start recording:', error)
      handleError(error as Error)
      setIsRecording(false)
    }
  }, [settings.stt.enabled, handleError])

  const stopRecording = useCallback(async () => {
    if (!providerRef.current || !isRecording) return

    try {
      setIsRecording(false)
      setIsProcessing(true)

      const result = await providerRef.current.stopListening()

      if (result.text) {
        handleTranscript(result.text)
      }
    } catch (error) {
      console.error('Failed to stop recording:', error)
      handleError(error as Error)
    } finally {
      setIsProcessing(false)
      setInterimText('')
    }
  }, [isRecording, handleTranscript, handleError])

  return (
    <>
      <button
        className={`push-to-talk-btn ${isRecording ? 'recording' : ''} ${isProcessing ? 'processing' : ''}`}
        onMouseDown={(e) => {
          e.preventDefault()
          if (!isRecording) startRecording()
        }}
        onMouseUp={(e) => {
          e.preventDefault()
          if (isRecording) stopRecording()
        }}
        onClick={(e) => {
          e.preventDefault()
          if (isRecording) stopRecording()
        }}
        onMouseLeave={() => {
          if (isRecording) stopRecording()
        }}
        title={isRecording ? 'Recording... (release to stop)' : 'Push to talk (click and hold)'}
      >
        {isProcessing ? '⋯' : isRecording ? '🎤' : '🎙️'}
      </button>
      {isRecording && interimText && (
        <div className="push-to-talk-interim" title={interimText}>
          {interimText}
        </div>
      )}
      {errorMessage && (
        <div className="push-to-talk-error">
          <strong>⚠️ Speech Recognition Error</strong>
          <p>{errorMessage}</p>
          <button onClick={() => setErrorMessage(null)}>×</button>
        </div>
      )}
    </>
  )
}
