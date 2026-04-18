/* eslint-disable custom/no-string-literal-comparison -- TODO: migrate existing string-literal comparisons to enums */
/**
 * LLM Client for Renderer Process
 *
 * Migrated from main/llm.ts and main/index.ts — all LLM operations now run
 * directly in the renderer using the OpenAI SDK instead of going through IPC.
 */

import OpenAI, { APIError } from 'openai'
import { ReasoningEffort } from '../../shared/types'
import type { LlmApi } from '../types'

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

async function completeChatCall(
  messages: ChatMessage[],
  settings: LlmSettings
): Promise<string> {
  const client = new OpenAI({
    baseURL: settings.baseUrl,
    apiKey: settings.apiKey,
    dangerouslyAllowBrowser: true
  })

  const response = await client.chat.completions.create({
    model: settings.model,
    messages,
    stream: false,
    ...(settings.reasoning !== ReasoningEffort.Off ? { reasoning_effort: settings.reasoning } : {})
  })

  return response.choices[0]?.message.content ?? ''
}

const ANALYZER_CACHE_SIZE = 10

export function createLlmClient(): LlmApi {
  // Analyzer cache
  const analyzerCache: { buffer: string; result: { state: string; reason: string } }[] = []

  // Active streams for cancellation
  const activeStreams = new Map<string, AbortController>()

  // Event listeners (per-instance, replaces IPC events)
  const deltaListeners: Array<(requestId: string, text: string) => void> = []
  const doneListeners: Array<(requestId: string) => void> = []
  const errorListeners: Array<(requestId: string, error: string) => void> = []

  return {
    send: async (requestId, messages, settings) => {
      const controller = new AbortController()
      activeStreams.set(requestId, controller)

      try {
        const client = new OpenAI({
          baseURL: settings.baseUrl,
          apiKey: settings.apiKey,
          dangerouslyAllowBrowser: true
        })

        const stream = await client.chat.completions.create(
          {
            model: settings.model,
            messages,
            stream: true,
            ...(settings.reasoning !== ReasoningEffort.Off && { reasoning_effort: settings.reasoning })
          },
          { signal: controller.signal }
        )

        for await (const chunk of stream) {
          if (controller.signal.aborted) break
          const delta = chunk.choices[0]?.delta.content
          if (delta) {
            deltaListeners.forEach(cb => { cb(requestId, delta) })
          }
        }

        if (!controller.signal.aborted) {
          doneListeners.forEach(cb => { cb(requestId) })
        }
      } catch (error: unknown) {
        if (controller.signal.aborted) return
        errorListeners.forEach(cb => { cb(requestId, formatLlmError(error)) })
      } finally {
        activeStreams.delete(requestId)
      }
    },

    analyzeTerminal: async (buffer, cwd, settings) => {
      const cached = analyzerCache.find((entry) => entry.buffer === buffer)
      if (cached) {
        return { ...cached.result, cached: true }
      }

      const allSafePaths = Array.from(new Set([...settings.safePaths, cwd]))
      const systemPrompt = settings.systemPrompt
        .replace(/\{\{cwd\}\}/g, cwd)
        .replace(/\{\{safe_paths\}\}/g, allSafePaths.join(', '))
      const messages: ChatMessage[] = [
        { role: 'system', content: systemPrompt },
        { role: 'user', content: buffer }
      ]
      try {
        const response = await completeChatCall(messages, {
          baseUrl: settings.baseUrl,
          apiKey: settings.apiKey,
          model: settings.model,
          reasoning: settings.reasoningEffort
        })
        const parsed = parseLlmJson(response)
        const result = { state: parsed.state as string, reason: parsed.reason as string }
        analyzerCache.push({ buffer, result })
        if (analyzerCache.length > ANALYZER_CACHE_SIZE) {
          analyzerCache.shift()
        }
        return { ...result, systemPrompt }
      } catch (error) {
        return { error: formatLlmError(error), systemPrompt }
      }
    },

    // eslint-disable-next-line @typescript-eslint/require-await
    clearAnalyzerCache: async () => {
      analyzerCache.length = 0
    },

    generateTitle: async (buffer, settings) => {
      const messages: ChatMessage[] = [
        { role: 'system', content: settings.titleSystemPrompt },
        { role: 'user', content: buffer }
      ]
      try {
        const response = await completeChatCall(messages, {
          baseUrl: settings.baseUrl,
          apiKey: settings.apiKey,
          model: settings.model,
          reasoning: settings.reasoningEffort
        })
        const parsed = parseLlmJson(response)
        return { title: (parsed.title as string) || '', description: (parsed.description as string) || '', branchName: (parsed.branchName as string) || '', systemPrompt: settings.titleSystemPrompt }
      } catch (error) {
        return { error: formatLlmError(error), systemPrompt: settings.titleSystemPrompt }
      }
    },

    cancel: (requestId) => {
      const controller = activeStreams.get(requestId)
      if (controller) {
        controller.abort()
        activeStreams.delete(requestId)
      }
    },

    onDelta: (callback) => {
      deltaListeners.push(callback)
      return () => {
        const index = deltaListeners.indexOf(callback)
        if (index > -1) deltaListeners.splice(index, 1)
      }
    },

    onDone: (callback) => {
      doneListeners.push(callback)
      return () => {
        const index = doneListeners.indexOf(callback)
        if (index > -1) doneListeners.splice(index, 1)
      }
    },

    onError: (callback) => {
      errorListeners.push(callback)
      return () => {
        const index = errorListeners.indexOf(callback)
        if (index > -1) errorListeners.splice(index, 1)
      }
    }
  }
}
