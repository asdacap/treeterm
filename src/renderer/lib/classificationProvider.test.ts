import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defaultClassifierCriteria } from '../../shared/classifierSettings'
import { ClassifierProvider, ReasoningEffort } from '../../shared/types'
import { ActivityState } from '../types'
import type { ClassifierSettings } from '../types/classification'
import { classificationIdentity, createClassificationProvider, decisionsUrl, type ClassifierTransports } from './classificationProvider'
import { createLlmClient, parseLlmJson } from './llmClient'

const criteria = { ...defaultClassifierCriteria, [ActivityState.Idle]: 'Prompt at {{cwd}}', [ActivityState.SafePermissionRequested]: 'Only mutates {{safe_paths}}' }
const input = { buffer: 'Allow write? y/n', cwd: '/workspace', safePaths: ['/tmp'], systemPrompt: 'Custom policy at {{cwd}}; safe: {{safe_paths}}', criteria }
const jev: ClassifierSettings = { provider: ClassifierProvider.Classifier, baseUrl: 'https://openrouter.ai/api/v1', apiKey: 'test-secret', model: 'typesafe/jev-1.13' }
const chat: ClassifierSettings = { ...jev, provider: ClassifierProvider.ChatCompletions, reasoningEffort: ReasoningEffort.Off }
function transports(): ClassifierTransports {
  return { fetch: vi.fn<typeof fetch>().mockResolvedValue(new Response(JSON.stringify({ answers: { activity_state: { type: 'choice', choice: 'idle' } } }))), completeChat: vi.fn().mockResolvedValue('{"state":"idle","reason":"Waiting"}'), parseChatJson: parseLlmJson }
}

beforeEach(async () => { await createLlmClient().clearAnalyzerCache() })

describe('OpenRouter Decisions contract', () => {
  it('uses the configured origin and exact Decisions request, preserving custom guidance/context', async () => {
    const deps = transports()
    await createClassificationProvider(jev, deps).classify(input)
    expect(deps.fetch).toHaveBeenCalledWith('https://openrouter.ai/api/alpha/decisions', {
      method: 'POST', headers: { Authorization: 'Bearer test-secret', 'Content-Type': 'application/json' },
      body: JSON.stringify({ model: jev.model, state: { terminal: input.buffer, cwd: input.cwd, safe_paths: ['/tmp', '/workspace'] }, questions: { activity_state: {
        type: 'choice', instructions: { guidance: 'Custom policy at /workspace; safe: /tmp, /workspace', output: 'Choose the activity state using these criteria. Return the typed choice, not generated JSON or an explanation.' },
        criteria: { ...defaultClassifierCriteria, idle: 'Prompt at /workspace', safe_permission_requested: 'Only mutates /tmp, /workspace' },
      } } }),
    })
    expect(deps.completeChat).not.toHaveBeenCalled()
    expect(decisionsUrl('https://example.test/proxy/api/v1/')).toBe('https://example.test/api/alpha/decisions')
    expect(decisionsUrl('https://openrouter.ai/api/v1/')).toBe('https://openrouter.ai/api/alpha/decisions')
  })

  it.each([ActivityState.Working, ActivityState.Idle, ActivityState.Completed, ActivityState.UserInputRequired, ActivityState.PermissionRequest, ActivityState.SafePermissionRequested, ActivityState.ApplicationError])('normalizes %s without requiring confidence', async (state) => {
    const deps = transports()
    vi.mocked(deps.fetch).mockResolvedValue(new Response(JSON.stringify({ answers: { activity_state: { type: 'choice', choice: state } } })))
    await expect(createClassificationProvider(jev, deps).classify(input)).resolves.toEqual({ state, reason: `Classifier decision: ${state}` })
  })

  it('sends any configured classifier model rather than hardcoding Jev', async () => {
    const deps = transports()
    await createClassificationProvider({ ...jev, model: 'other/classifier' }, deps).classify(input)
    const request = vi.mocked(deps.fetch).mock.calls[0]![1]!
    expect(JSON.parse(request.body as string)).toHaveProperty('model', 'other/classifier')
    expect(deps.completeChat).not.toHaveBeenCalled()
  })

  it('allows optional response metadata', async () => {
    const deps = transports()
    vi.mocked(deps.fetch).mockResolvedValue(new Response(JSON.stringify({ model: 'resolved-model', usage: {}, answers: { activity_state: { type: 'choice', choice: 'idle', confidence: 0.9, probabilities: { idle: 1 } } } })))
    await expect(createClassificationProvider(jev, deps).classify(input)).resolves.toHaveProperty('state', 'idle')
  })

  it.each(['invalid json', '{}', '{"answers":{}}', '{"answers":{"activity_state":{"type":"score","choice":"idle"}}}', '{"answers":{"activity_state":{"type":"choice","choice":"unknown"}}}', '{"answers":{"activity_state":{"type":"choice","choice":"error"}}}'])('rejects invalid response %s', async (body) => {
    const deps = transports()
    vi.mocked(deps.fetch).mockResolvedValue(new Response(body))
    await expect(createClassificationProvider(jev, deps).classify(input)).rejects.toThrow()
  })

  it.each([
    [401, '{"error":{"code":401,"message":"Unauthorized"}}', 'Unauthorized'],
    [429, '<html>Rate limited</html>', 'Too Many Requests'],
    [500, '{"unexpected":"body"}', 'Internal Server Error'],
  ])('surfaces HTTP %s without falling back', async (status, body, message) => {
    const deps = transports()
    vi.mocked(deps.fetch).mockResolvedValue(new Response(body, { status, statusText: message }))
    await expect(createClassificationProvider(jev, deps).classify(input)).rejects.toThrow(`OpenRouter Decisions HTTP ${String(status)}: ${message}`)
    expect(deps.completeChat).not.toHaveBeenCalled()
  })

  it('returns transport errors through the facade and does not cache failures', async () => {
    const deps = transports()
    vi.mocked(deps.fetch).mockRejectedValue(new Error('Network unavailable'))
    const client = createLlmClient(deps)
    for (let i = 0; i < 2; i++) {
      await expect(client.analyzeTerminal(input.buffer, input.cwd, { ...jev, ...input })).resolves.toMatchObject({ error: 'Network unavailable', systemPrompt: 'Custom policy at /workspace; safe: /tmp, /workspace' })
    }
    expect(deps.fetch).toHaveBeenCalledTimes(2)
  })
})

describe('unified chat adapter and identity', () => {
  it('preserves chat messages, expanded instructions and reasoning', async () => {
    const deps = transports()
    await expect(createClassificationProvider(chat, deps).classify(input)).resolves.toEqual({ state: 'idle', reason: 'Waiting' })
    expect(deps.completeChat).toHaveBeenCalledWith([
      { role: 'system', content: expect.any(String) as string }, { role: 'user', content: input.buffer },
    ], { baseUrl: chat.baseUrl, apiKey: chat.apiKey, model: chat.model, reasoning: ReasoningEffort.Off })
    expect(deps.fetch).not.toHaveBeenCalled()
  })

  it('generates the output format and state list from the configured criteria', async () => {
    const deps = transports()
    await createClassificationProvider(chat, deps).classify(input)
    const system = vi.mocked(deps.completeChat).mock.calls[0]![0][0]!.content
    expect(system.startsWith('Custom policy at /workspace; safe: /tmp, /workspace\n\nRespond with ONLY a JSON object: {"state": "<state>", "reason": "<reason>"}')).toBe(true)
    expect(system).toContain(' - "idle": Prompt at /workspace')
    expect(system).toContain(' - "safe_permission_requested": Only mutates /tmp, /workspace')
    expect(system).toContain(`- "working": ${defaultClassifierCriteria.working}`)
    const order = ['working', 'safe_permission_requested', 'permission_request', 'completed', 'user_input_required', 'application_error', 'idle'].map((state) => system.indexOf(`- "${state}": `))
    expect(order).toEqual([...order].sort((x, y) => x - y))
    expect(system).not.toContain('"error"')
    expect(system.endsWith('The reason field should give the reason for the verdict, in no more than 10 words.')).toBe(true)
  })

  it('reports the prompt each provider actually receives', async () => {
    const client = createLlmClient(transports())
    const chatResult = await client.analyzeTerminal(input.buffer, input.cwd, { ...chat, ...input })
    expect(chatResult.systemPrompt).toContain(' - "idle": Prompt at /workspace')
    await client.clearAnalyzerCache()
    const decisionsResult = await client.analyzeTerminal(input.buffer, input.cwd, { ...jev, ...input })
    expect(decisionsResult.systemPrompt).toBe('Custom policy at /workspace; safe: /tmp, /workspace')
  })

  it.each(['{"state":"unknown","reason":"oops"}', '{"state":"idle"}', '{"state":"idle","reason":3}', 'null'])('rejects invalid chat classification %s', async (raw) => {
    const deps = transports()
    vi.mocked(deps.completeChat).mockResolvedValue(raw)
    await expect(createClassificationProvider(chat, deps).classify(input)).rejects.toThrow()
  })

  it('isolates global cache by provider, endpoint, model, instructions, criteria, cwd, paths and reasoning', async () => {
    const deps = transports()
    const client = createLlmClient(deps)
    const settings = { ...chat, systemPrompt: input.systemPrompt, criteria: input.criteria, safePaths: input.safePaths }
    await client.analyzeTerminal(input.buffer, input.cwd, settings)
    await client.analyzeTerminal(input.buffer, input.cwd, { ...settings, model: 'other' })
    await client.analyzeTerminal(input.buffer, input.cwd, { ...settings, baseUrl: 'https://other.test/v1' })
    await client.analyzeTerminal(input.buffer, input.cwd, { ...settings, systemPrompt: 'other' })
    await client.analyzeTerminal(input.buffer, input.cwd, { ...settings, criteria: { ...criteria, [ActivityState.Completed]: 'other' } })
    await client.analyzeTerminal(input.buffer, '/different', settings)
    await client.analyzeTerminal(input.buffer, input.cwd, { ...settings, safePaths: ['/different'] })
    await client.analyzeTerminal(input.buffer, input.cwd, { ...settings, reasoningEffort: ReasoningEffort.High })
    await client.analyzeTerminal(input.buffer, input.cwd, { ...settings, ...jev })
    expect(deps.completeChat).toHaveBeenCalledTimes(8)
    expect(deps.fetch).toHaveBeenCalledTimes(1)
    await expect(client.analyzeTerminal(input.buffer, input.cwd, settings)).resolves.toHaveProperty('cached', true)
    expect(classificationIdentity(input, chat)).not.toContain(chat.apiKey)
  })

  it('reports an invalid endpoint through the facade', async () => {
    await expect(createLlmClient(transports()).analyzeTerminal(input.buffer, input.cwd, { ...jev, ...input, baseUrl: 'not a url' })).resolves.toHaveProperty('error')
  })
})
