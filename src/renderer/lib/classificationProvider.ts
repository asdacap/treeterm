import { z } from 'zod'
import { classifiedStates } from '../../shared/classifierSettings'
import { ClassifierProvider, type ClassifierCriteria, type ReasoningEffort } from '../../shared/types'
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

const choiceSchema = z.enum(classifiedStates)
const decisionsResponseSchema = z.object({
  answers: z.object({ activity_state: z.object({ type: z.literal('choice'), choice: choiceSchema }) }),
})
const errorSchema = z.object({ error: z.object({ message: z.string() }) })

function fillTemplate(text: string, cwd: string, safePaths: string[]): string {
  return text.replace(/\{\{cwd\}\}/g, cwd).replace(/\{\{safe_paths\}\}/g, safePaths.join(', '))
}

export function prepareClassificationInput(input: ClassificationInput): ClassificationInput {
  const safePaths = Array.from(new Set([...input.safePaths, input.cwd]))
  const criteria = Object.fromEntries(
    classifiedStates.map((state) => [state, fillTemplate(input.criteria[state], input.cwd, safePaths)]),
  ) as ClassifierCriteria
  return {
    ...input,
    safePaths,
    systemPrompt: fillTemplate(input.systemPrompt, input.cwd, safePaths),
    criteria,
  }
}

/** The configured prompt followed by the output format and state list generated from the criteria. */
export function chatSystemPrompt(prepared: ClassificationInput): string {
  const states = classifiedStates.map((state) => ` - "${state}": ${prepared.criteria[state]}`).join('\n')
  return `${prepared.systemPrompt}\n\nRespond with ONLY a JSON object: {"state": "<state>", "reason": "<reason>"} where state is the one of the following that best represents the terminal:\n${states}\nThe reason field should give the reason for the verdict, in no more than 10 words.`
}

/** The prompt text the selected provider actually receives, for history and debugging. */
export function effectivePrompt(prepared: ClassificationInput, settings: ClassifierSettings): string {
  return settings.provider === ClassifierProvider.ChatCompletions ? chatSystemPrompt(prepared) : prepared.systemPrompt
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
    endpoint: settings.provider === ClassifierProvider.Classifier ? decisionsUrl(settings.baseUrl) : settings.baseUrl,
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
            { role: 'system', content: chatSystemPrompt(prepared) },
            { role: 'user', content: prepared.buffer },
          ], { baseUrl: settings.baseUrl, apiKey: settings.apiKey, model: settings.model, reasoning: settings.reasoningEffort })
          return classificationSchema.parse(transports.parseChatJson(raw))
        }
        case ClassifierProvider.Classifier: {
          const response = await transports.fetch(decisionsUrl(settings.baseUrl), {
            method: 'POST',
            headers: { Authorization: `Bearer ${settings.apiKey}`, 'Content-Type': 'application/json' },
            body: JSON.stringify({
              model: settings.model,
              state: { terminal: prepared.buffer, cwd: prepared.cwd, safe_paths: prepared.safePaths },
              questions: { activity_state: {
                type: 'choice',
                instructions: { guidance: prepared.systemPrompt, output: 'Choose the activity state using these criteria. Return the typed choice, not generated JSON or an explanation.' },
                criteria: prepared.criteria,
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
            throw new Error(`OpenRouter Decisions HTTP ${String(response.status)}: ${message}`)
          }
          const answer = decisionsResponseSchema.parse(JSON.parse(text)).answers.activity_state
          return { state: answer.choice, reason: `Classifier decision: ${answer.choice}` }
        }
      }
    },
  }
}
