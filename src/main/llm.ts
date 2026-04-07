import OpenAI, { APIError } from 'openai'

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

/** Strip markdown fences and parse JSON. If a field value is itself a markdown-wrapped JSON string, re-parse and merge. */
export function parseLlmJson(raw: string): Record<string, unknown> {
  const stripped = raw.replace(/```json\n?/g, '').replace(/```\n?/g, '').trim()
  const parsed: unknown = JSON.parse(stripped)
  if (typeof parsed !== 'object' || parsed === null) return parsed as Record<string, unknown>

  const result = parsed as Record<string, unknown>

  for (const [, value] of Object.entries(result)) {
    if (typeof value !== 'string') continue
    const inner = value.replace(/```json\n?/g, '').replace(/```\n?/g, '').trim()
    if (inner.startsWith('{')) {
      try {
        const innerParsed: unknown = JSON.parse(inner)
        if (typeof innerParsed === 'object' && innerParsed !== null) {
          // Merge inner fields, preferring non-empty inner values
          for (const [ik, iv] of Object.entries(innerParsed as Record<string, unknown>)) {
            if (iv !== '' && iv !== null && iv !== undefined) result[ik] = iv
          }
        }
      } catch { /* not JSON, leave as-is */ }
    }
  }

  return result
}

export function cancelChatStream(requestId: string): void {
  const controller = activeStreams.get(requestId)
  if (controller) {
    controller.abort()
    activeStreams.delete(requestId)
  }
}
