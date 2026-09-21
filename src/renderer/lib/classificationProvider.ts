import { z } from 'zod'
import { ClassifierProvider, type ReasoningEffort } from '../../shared/types'
import { ActivityState } from '../types'
import { classificationSchema, type ClassificationInput, type ClassificationProvider, type ClassifierSettings } from '../types/classification'

export interface ChatMessage {
  role: 'user' | 'assistant' | 'system'
  content: string
}

export interface ChatCompletionSettings {
  baseUrl: string
  apiKey: string
  model: string
  reasoning: ReasoningEffort
}

export interface ClassifierTransports {
  completeChat: (messages: ChatMessage[], settings: ChatCompletionSettings) => Promise<string>
  parseChatJson: (raw: string) => unknown
  fetch: typeof fetch
}

const criteria = {
  [ActivityState.Idle]: 'Shell prompt visible, waiting for a command, or user input is incomplete.',
  [ActivityState.Completed]: 'The previous user request has been satisfied.',
  [ActivityState.UserInputRequired]: 'Program asks for text input, a design choice, or plan confirmation.',
  [ActivityState.PermissionRequest]: 'Program asks for y/n or similar permission, but the action does not meet the safe permission criteria.',
  [ActivityState.SafePermissionRequested]: 'Program asks permission for a safe action: git operations unless changing another worktree; build, test, or dependency installation; or mutations only within the safe paths. Reading outside safe paths is allowed, but wide-ranging searches using find or recursive grep outside safe paths are not allowed.',
}

const choiceSchema = z.enum([
  ActivityState.Idle, ActivityState.Completed, ActivityState.UserInputRequired,
  ActivityState.PermissionRequest, ActivityState.SafePermissionRequested,
])
const decisionsResponseSchema = z.object({
  answers: z.object({ activity_state: z.object({ type: z.literal('choice'), choice: choiceSchema }) }),
})
const errorSchema = z.object({ error: z.object({ message: z.string() }) })

export function prepareClassificationInput(input: ClassificationInput): ClassificationInput {
  const safePaths = Array.from(new Set([...input.safePaths, input.cwd]))
  return {
    ...input,
    safePaths,
    systemPrompt: input.systemPrompt.replace(/\{\{cwd\}\}/g, input.cwd).replace(/\{\{safe_paths\}\}/g, safePaths.join(', ')),
  }
}

export function decisionsUrl(baseUrl: string): string {
  return new URL('/api/alpha/decisions', baseUrl).href
}

/** Contains only request semantics, never API credentials. Shared by both cache layers. */
export function classificationIdentity(input: ClassificationInput, settings: ClassifierSettings): string {
  const prepared = prepareClassificationInput(input)
  return JSON.stringify({
    ...prepared,
    provider: settings.provider,
    endpoint: settings.provider === ClassifierProvider.Jev ? decisionsUrl(settings.baseUrl) : settings.baseUrl,
    model: settings.model,
    ...(settings.provider === ClassifierProvider.ChatCompletions ? { reasoning: settings.reasoningEffort } : {}),
  })
}

export function createClassificationProvider(settings: ClassifierSettings, transports: ClassifierTransports): ClassificationProvider {
  return {
    classify: async (input) => {
      const prepared = prepareClassificationInput(input)
      switch (settings.provider) {
        case ClassifierProvider.ChatCompletions: {
          const raw = await transports.completeChat([
            { role: 'system', content: prepared.systemPrompt },
            { role: 'user', content: prepared.buffer },
          ], { baseUrl: settings.baseUrl, apiKey: settings.apiKey, model: settings.model, reasoning: settings.reasoningEffort })
          return classificationSchema.parse(transports.parseChatJson(raw))
        }
        case ClassifierProvider.Jev: {
          const response = await transports.fetch(decisionsUrl(settings.baseUrl), {
            method: 'POST',
            headers: { Authorization: `Bearer ${settings.apiKey}`, 'Content-Type': 'application/json' },
            body: JSON.stringify({
              model: settings.model,
              state: { terminal: prepared.buffer, cwd: prepared.cwd, safe_paths: prepared.safePaths },
              questions: { activity_state: {
                type: 'choice',
                instructions: { guidance: prepared.systemPrompt, output: 'Choose the activity state using these criteria. Return the typed choice, not generated JSON or an explanation.' },
                criteria,
              } },
            }),
          })
          const text = await response.text()
          if (!response.ok) {
            let message = response.statusText
            try {
              const error = errorSchema.safeParse(JSON.parse(text))
              if (error.success) message = error.data.error.message
            } catch {
              // Non-JSON HTTP errors still surface their status; do not expose arbitrary response bodies.
            }
            throw new Error(`Jev Decisions HTTP ${String(response.status)}: ${message}`)
          }
          const answer = decisionsResponseSchema.parse(JSON.parse(text)).answers.activity_state
          return { state: answer.choice, reason: `Jev decision: ${answer.choice}` }
        }
      }
    },
  }
}
