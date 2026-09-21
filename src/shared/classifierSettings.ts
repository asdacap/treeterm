import { z } from 'zod'
import { ClassifierProvider } from './types'

/** Validate new classifier settings at the persistence boundary; empty models are UI configuration errors. */
export const classifierSettingsSchema = z.object({
  provider: z.enum(ClassifierProvider),
  jevModel: z.string(),
  titleModel: z.string(),
})
