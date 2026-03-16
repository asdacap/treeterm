import type { STTProvider as STTProviderType, STTApi } from '../types'
import type { STTProvider } from './types'
import { OpenAIWhisperProvider } from './openaiWhisperProvider'
import { LocalWhisperProvider } from './localWhisperProvider'

export * from './types'

export function createSTTProvider(
  sttApi: STTApi,
  provider: STTProviderType,
  apiKey?: string,
  modelPath?: string,
  language?: string
): STTProvider {
  switch (provider) {
    case 'openaiWhisper':
      return new OpenAIWhisperProvider(sttApi, apiKey || '', language)
    case 'localWhisper':
      return new LocalWhisperProvider(sttApi, modelPath || '')
    default:
      return new OpenAIWhisperProvider(sttApi, apiKey || '', language)
  }
}
