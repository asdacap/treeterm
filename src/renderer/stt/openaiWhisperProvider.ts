import type { STTApi } from '../types'
import type { STTProvider, STTResult } from './types'

export class OpenAIWhisperProvider implements STTProvider {
  name = 'OpenAI Whisper'
  private mediaRecorder: MediaRecorder | null = null
  private audioChunks: Blob[] = []
  private stream: MediaStream | null = null
  private sttApi: STTApi
  private apiKey: string
  private language?: string

  constructor(sttApi: STTApi, apiKey: string, language?: string) {
    this.sttApi = sttApi
    this.apiKey = apiKey
    this.language = language
  }

  async isAvailable(): Promise<boolean> {
    if (!this.apiKey) return false
    return 'MediaRecorder' in window && 'getUserMedia' in navigator.mediaDevices
  }

  async startListening(): Promise<void> {
    try {
      this.stream = await navigator.mediaDevices.getUserMedia({ audio: true })
      this.audioChunks = []

      this.mediaRecorder = new MediaRecorder(this.stream, {
        mimeType: 'audio/webm;codecs=opus'
      })

      this.mediaRecorder.ondataavailable = (event) => {
        if (event.data.size > 0) {
          this.audioChunks.push(event.data)
        }
      }

      this.mediaRecorder.start()
    } catch (error) {
      console.error('Failed to start recording:', error)
      throw error
    }
  }

  async stopListening(): Promise<STTResult> {
    if (!this.mediaRecorder || !this.stream) {
      return { text: '' }
    }

    return new Promise((resolve, reject) => {
      this.mediaRecorder!.onstop = async () => {
        try {
          // Stop all tracks
          this.stream!.getTracks().forEach((track) => track.stop())

          // Create audio blob
          const audioBlob = new Blob(this.audioChunks, { type: 'audio/webm' })

          // Convert blob to array buffer
          const arrayBuffer = await audioBlob.arrayBuffer()

          // Send to main process for transcription
          const result = await this.sttApi.transcribeOpenAI(
            arrayBuffer,
            this.apiKey,
            this.language
          )

          resolve({ text: result.text })
        } catch (error) {
          console.error('Failed to transcribe audio:', error)
          reject(error)
        }
      }

      this.mediaRecorder!.stop()
    })
  }

  onInterimResult(_callback: (text: string) => void): void {
    // OpenAI Whisper doesn't support interim results
    // Could potentially use Web Speech API for interim and Whisper for final
  }

  onError(_callback: (error: Error) => void): void {
    // Errors are propagated via promise rejection
  }
}
