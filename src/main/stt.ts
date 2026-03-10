import { ipcMain } from 'electron'

export function registerSTTHandlers(): void {
  // OpenAI Whisper transcription
  ipcMain.handle(
    'stt:transcribe-openai',
    async (_event, audioBuffer: ArrayBuffer, apiKey: string, language?: string) => {
      try {
        // Try to import OpenAI SDK dynamically
        let OpenAI: typeof import('openai').default
        try {
          OpenAI = await import('openai').then((m) => m.default)
        } catch (importError) {
          throw new Error(
            'OpenAI package not installed. Run: npm install openai'
          )
        }

        const openai = new OpenAI({ apiKey })

        // Convert ArrayBuffer to File
        // The OpenAI SDK expects a specific File-like interface
        const audioBlob = new Blob([audioBuffer], { type: 'audio/webm' })
        const file = new File([audioBlob], 'audio.webm', { type: 'audio/webm' })

        // Transcribe using Whisper API
        const transcription = await openai.audio.transcriptions.create({
          file,
          model: 'whisper-1',
          ...(language && { language }) // Include language if provided
        })

        return { text: transcription.text }
      } catch (error) {
        console.error('OpenAI Whisper transcription error:', error)
        throw error
      }
    }
  )

  // Local Whisper transcription (stub for now)
  ipcMain.handle(
    'stt:transcribe-local',
    async (_event, audioBuffer: ArrayBuffer, modelPath: string) => {
      try {
        // TODO: Implement local Whisper using whisper.cpp or whisper-node
        // For now, return an error
        throw new Error(
          'Local Whisper is not yet implemented. Please use Web Speech API or OpenAI Whisper.'
        )
      } catch (error) {
        console.error('Local Whisper transcription error:', error)
        throw error
      }
    }
  )

  // Check microphone permission status
  ipcMain.handle('stt:check-mic-permission', async () => {
    // Microphone permissions are handled by the browser/Chromium in Electron
    // Always return true as Electron will prompt the user when getUserMedia is called
    return true
  })
}
