import OpenAI, { APIError } from 'openai'
import { BrowserWindow } from 'electron'

import type { ReasoningEffort } from '../shared/types'

interface LlmSettings {
  baseUrl: string
  apiKey: string
  model: string
  reasoning: ReasoningEffort
}

interface ChatMessage {
  role: 'user' | 'assistant' | 'system'
  content: string
}

export function formatLlmError(error: unknown): string {
  if (error instanceof APIError) {
    return `${error.message}${error.error ? '\n' + JSON.stringify(error.error) : ''}`
  }
  return error instanceof Error ? error.message : 'Unknown LLM error'
}

// Track active streams for cancellation
const activeStreams = new Map<string, AbortController>()

export async function startChatStream(
  requestId: string,
  messages: ChatMessage[],
  settings: LlmSettings,
  sender: Electron.WebContents
): Promise<void> {
  const controller = new AbortController()
  activeStreams.set(requestId, controller)

  try {
    const client = new OpenAI({
      baseURL: settings.baseUrl,
      apiKey: settings.apiKey
    })

    const stream = await client.chat.completions.create(
      {
        model: settings.model,
        messages,
        stream: true,
        ...(settings.reasoning !== 'off' && { reasoning_effort: settings.reasoning })
      },
      { signal: controller.signal }
    )

    for await (const chunk of stream) {
      if (controller.signal.aborted) break
      const delta = chunk.choices[0]?.delta?.content
      if (delta) {
        sender.send('llm:chat:delta', requestId, delta)
      }
    }

    if (!controller.signal.aborted) {
      sender.send('llm:chat:done', requestId)
    }
  } catch (error: unknown) {
    if (controller.signal.aborted) return
    sender.send('llm:chat:error', requestId, formatLlmError(error))
  } finally {
    activeStreams.delete(requestId)
  }
}

export async function completeChatCall(
  messages: ChatMessage[],
  settings: LlmSettings
): Promise<string> {
  const client = new OpenAI({
    baseURL: settings.baseUrl,
    apiKey: settings.apiKey
  })

  const response = await client.chat.completions.create({
    model: settings.model,
    messages,
    stream: false,
    ...(settings.reasoning !== 'off' ? { reasoning_effort: settings.reasoning } : {})
  })

  return response.choices[0]?.message?.content ?? ''
}

export function cancelChatStream(requestId: string): void {
  const controller = activeStreams.get(requestId)
  if (controller) {
    controller.abort()
    activeStreams.delete(requestId)
  }
}
