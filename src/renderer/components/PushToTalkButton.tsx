import { useCallback, useState, useEffect } from 'react'
import { usePushToTalk } from '../hooks/usePushToTalk'
import { useCapsLockHold } from '../hooks/useCapsLockHold'
import { useSettingsStore } from '../store/settings'

interface PushToTalkButtonProps {
  onTranscript: (text: string) => void
  onSubmit: () => void
}

export default function PushToTalkButton({ onTranscript, onSubmit }: PushToTalkButtonProps) {
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const { settings } = useSettingsStore()
  const pushToTalkKey = settings.stt?.pushToTalkKey || 'Shift+Space'

  const handleTranscript = useCallback(
    (text: string) => {
      onTranscript(text)
      // Small delay then submit
      setTimeout(() => onSubmit(), 50)
    },
    [onTranscript, onSubmit]
  )

  const handleError = useCallback((error: Error) => {
    console.error('Push-to-talk error:', error)
    setErrorMessage(error.message)
    // Auto-hide after 5 seconds
    setTimeout(() => setErrorMessage(null), 5000)
  }, [])

  // Clear error when starting a new recording
  useEffect(() => {
    if (errorMessage) {
      const timer = setTimeout(() => setErrorMessage(null), 5000)
      return () => clearTimeout(timer)
    }
  }, [errorMessage])

  const { isRecording, isProcessing, startRecording, stopRecording, interimText } = usePushToTalk({
    onTranscript: handleTranscript,
    onError: handleError
  })

  // Handle Caps Lock hold
  useCapsLockHold({
    onHoldStart: startRecording,
    onHoldEnd: stopRecording
  })

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
          // If recording, clicking should stop it
          if (isRecording) stopRecording()
        }}
        onMouseLeave={() => {
          // Stop recording if mouse leaves while recording
          if (isRecording) stopRecording()
        }}
        title={isRecording ? 'Recording... (release to stop)' : `Push to talk (hold ${pushToTalkKey} or click)`}
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
