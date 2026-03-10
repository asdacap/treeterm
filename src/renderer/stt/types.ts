export interface STTResult {
  text: string
  confidence?: number
}

export interface STTProvider {
  name: string
  isAvailable(): Promise<boolean>
  startListening(): Promise<void>
  stopListening(): Promise<STTResult>
  onInterimResult?: (callback: (text: string) => void) => void
  onError?: (callback: (error: Error) => void) => void
}
