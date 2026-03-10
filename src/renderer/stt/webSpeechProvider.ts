import type { STTProvider, STTResult } from './types'

export class WebSpeechProvider implements STTProvider {
  name = 'Web Speech API'
  private recognition: SpeechRecognition | null = null
  private interimCallback: ((text: string) => void) | null = null
  private errorCallback: ((error: Error) => void) | null = null
  private finalText = ''
  private resolveStop: ((result: STTResult) => void) | null = null
  private rejectStop: ((error: Error) => void) | null = null
  private rejectStart: ((error: Error) => void) | null = null
  private isListening = false

  async isAvailable(): Promise<boolean> {
    // Check if Web Speech API is available
    const hasAPI = 'webkitSpeechRecognition' in window || 'SpeechRecognition' in window

    if (!hasAPI) {
      return false
    }

    // Also check if microphone is accessible
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
      // Close the stream immediately after checking
      stream.getTracks().forEach((track) => track.stop())
      console.log('Microphone access granted')
      return true
    } catch (error) {
      console.error('Microphone access denied or not available:', error)
      return false
    }
  }

  async startListening(): Promise<void> {
    if (this.isListening) {
      console.warn('Already listening, ignoring start request')
      return
    }

    const SpeechRecognitionClass =
      window.SpeechRecognition || window.webkitSpeechRecognition
    if (!SpeechRecognitionClass) {
      throw new Error('Speech Recognition not available in this browser')
    }

    return new Promise((resolve, reject) => {
      this.recognition = new SpeechRecognitionClass()
      this.recognition.continuous = true
      this.recognition.interimResults = true
      this.recognition.lang = 'en-US'
      this.recognition.maxAlternatives = 1
      this.finalText = ''
      this.rejectStart = reject

      console.log('Starting Web Speech API recognition...')

    this.recognition.onresult = (event: SpeechRecognitionEvent) => {
      let interimTranscript = ''
      let finalTranscript = ''

      for (let i = event.resultIndex; i < event.results.length; i++) {
        const transcript = event.results[i][0].transcript
        if (event.results[i].isFinal) {
          finalTranscript += transcript + ' '
        } else {
          interimTranscript += transcript
        }
      }

      if (finalTranscript) {
        this.finalText += finalTranscript
      }

      // Call interim callback with either final or interim text
      if (this.interimCallback) {
        this.interimCallback(this.finalText + interimTranscript)
      }
    }

    this.recognition.onerror = (event: SpeechRecognitionErrorEvent) => {
      const errorType = event.error
      const errorMessage = event.message || ''

      console.log(`Speech recognition error type: "${errorType}", message: "${errorMessage}"`)

      // Don't throw error for "no-speech" - it's normal when user doesn't speak
      if (errorType === 'no-speech') {
        console.log('No speech detected - this is normal')
        return
      }
      // Aborted is expected when we stop manually
      if (errorType === 'aborted') {
        console.log('Recognition aborted - this is normal')
        return
      }

      // Create user-friendly error messages
      let userMessage = ''
      switch (errorType) {
        case 'not-allowed':
          userMessage = 'Microphone access denied. Please allow microphone permissions in your browser.'
          break
        case 'network':
          userMessage = 'Network error. Speech recognition requires an internet connection.'
          break
        case 'audio-capture':
          userMessage = 'No microphone detected. Please check your audio input devices.'
          break
        case 'service-not-allowed':
          userMessage = 'Speech recognition service not available.'
          break
        default:
          userMessage = `Speech recognition error: ${errorType}`
      }

      console.error(`Speech recognition error: ${errorType}`)

      // Reject the appropriate promise or call error callback
      const error = new Error(userMessage)

      if (this.rejectStart) {
        this.rejectStart(error)
        this.rejectStart = null
        this.isListening = false
      } else if (this.rejectStop) {
        this.rejectStop(error)
        this.rejectStop = null
        this.resolveStop = null
        this.isListening = false
      } else if (this.errorCallback) {
        // Error happened during recording (not during start/stop)
        this.errorCallback(error)
        this.isListening = false
        // Auto-stop the recognition
        this.recognition?.stop()
      }
    }

    this.recognition.onstart = () => {
      console.log('Speech recognition started successfully')
      this.isListening = true
      if (this.rejectStart) {
        this.rejectStart = null
      }
      resolve()
    }

    this.recognition.onend = () => {
      // Recognition ended, resolve if waiting
      this.isListening = false
      if (this.resolveStop) {
        this.resolveStop({ text: this.finalText.trim() })
        this.resolveStop = null
      }
    }

    try {
      this.recognition.start()
    } catch (error) {
      this.isListening = false
      reject(error)
    }
    })
  }

  async stopListening(): Promise<STTResult> {
    if (!this.recognition || !this.isListening) {
      this.isListening = false
      return { text: this.finalText.trim() }
    }

    return new Promise((resolve, reject) => {
      this.resolveStop = resolve
      this.rejectStop = reject
      this.recognition?.stop()
    })
  }

  onInterimResult(callback: (text: string) => void): void {
    this.interimCallback = callback
  }

  onError(callback: (error: Error) => void): void {
    this.errorCallback = callback
  }
}
