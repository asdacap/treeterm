import { describe, expect, it } from 'vitest'
import { classificationSchema } from '../types/classification'
import { ActivityState } from '../types'
import { ClassifierProvider } from '../../shared/types'
import { classifierSettingsSchema } from '../../shared/classifierSettings'

describe('classification contract', () => {
  it.each(Object.values(ActivityState))('accepts existing activity state %s', (state) => {
    expect(classificationSchema.parse({ state, reason: 'classification reason' })).toEqual({ state, reason: 'classification reason' })
  })

  it.each([
    null,
    {},
    { state: 'unknown', reason: '' },
    { state: ActivityState.Idle },
    { state: ActivityState.Idle, reason: 42 },
  ])('rejects invalid classifier output %j', (value) => {
    expect(classificationSchema.safeParse(value).success).toBe(false)
  })

  it.each(Object.values(ClassifierProvider))('accepts configured provider %s and preserves empty models', (provider) => {
    expect(classifierSettingsSchema.parse({ provider, jevModel: '', titleModel: '' })).toEqual({ provider, jevModel: '', titleModel: '' })
  })

  it.each([
    { provider: 'unknown', jevModel: '', titleModel: '' },
    { provider: null, jevModel: '', titleModel: '' },
    { provider: ClassifierProvider.Jev, jevModel: 42, titleModel: '' },
    { provider: ClassifierProvider.Jev, jevModel: '', titleModel: null },
  ])('rejects invalid persisted classifier configuration %j', (value) => {
    expect(classifierSettingsSchema.safeParse(value).success).toBe(false)
  })
})
