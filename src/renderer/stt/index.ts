import type { STTProvider as STTProviderType } from '../types'
import type { STTProvider } from './types'
import { OpenAIWhisperProvider } from './openaiWhisperProvider'
import { LocalWhisperProvider } from './localWhisperProvider'

export * from './types'

export function createSTTProvider(
  provider: STTProviderType,
  apiKey?: string,
  modelPath?: string
): STTProvider {
  switch (provider) {
    case 'openaiWhisper':
      return new OpenAIWhisperProvider(apiKey || '')
    case 'localWhisper':
      return new LocalWhisperProvider(modelPath || '')
    default:
      return new OpenAIWhisperProvider(apiKey || '')
  }
}
