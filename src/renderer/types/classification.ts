import { z } from 'zod'
import { ClassifierProvider, type ReasoningEffort } from '../../shared/types'
import { ActivityState } from '../types'

export const classificationSchema = z.object({
  state: z.enum(ActivityState),
  reason: z.string(),
})

export type Classification = z.infer<typeof classificationSchema>

interface ClassifierConnection {
  baseUrl: string
  apiKey: string
  model: string
}

/** Only the chat adapter supports reasoning effort. */
export type ClassifierSettings = ClassifierConnection & (
  | { provider: ClassifierProvider.ChatCompletions; reasoningEffort: ReasoningEffort }
  | { provider: ClassifierProvider.Jev }
)

export interface ClassificationInput {
  buffer: string
  cwd: string
  safePaths: string[]
  systemPrompt: string
}

/** Provider-specific transport and validation stay behind this renderer interface. */
export interface ClassificationProvider {
  classify: (input: ClassificationInput) => Promise<Classification>
}
