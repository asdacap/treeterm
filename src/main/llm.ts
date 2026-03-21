import OpenAI from 'openai'
import { BrowserWindow } from 'electron'

interface LlmSettings {
  baseUrl: string
  apiKey: string
  model: string
  reasoning: boolean
}

interface ChatMessage {
  role: 'user' | 'assistant' | 'system'
  content: string
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
        ...(settings.reasoning && { reasoning_effort: 'medium' as const })
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
    const message = error instanceof Error ? error.message : 'Unknown LLM error'
    sender.send('llm:chat:error', requestId, message)
  } finally {
    activeStreams.delete(requestId)
  }
}

export function cancelChatStream(requestId: string): void {
  const controller = activeStreams.get(requestId)
  if (controller) {
    controller.abort()
    activeStreams.delete(requestId)
  }
}
