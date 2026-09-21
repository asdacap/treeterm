// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { ClassifierProvider, ReasoningEffort } from '../../shared/types'
import { ActivityState, Platform, type ApplicationRenderProps, type Application, type SandboxApi, type Settings } from '../types'
import { defaultSettings, useSettingsStore } from '../store/settings'
import { ActivityTransitionKind, useActivityStateStore } from '../store/activityState'

const llm = vi.hoisted(() => ({
  analyzeTerminal: vi.fn(),
  generateTitle: vi.fn(),
  clearAnalyzerCache: vi.fn(),
}))
vi.mock('../store/app', () => ({ useAppStore: (selector: (state: { applications: Map<string, Application> }) => unknown): unknown => selector({ applications: new Map() }) }))
vi.mock('../lib/llmClient', () => ({ createLlmClient: () => llm }))
import SettingsDialog from './SettingsDialog'
import TerminalAnalyzerDebugger from './TerminalAnalyzerDebugger'
import SystemPromptDebugger from './SystemPromptDebugger'

const props = { tab: { id: 'debug', state: { bufferText: 'terminal output' } }, isVisible: true } as ApplicationRenderProps

beforeEach(() => {
  vi.resetAllMocks()
  useSettingsStore.setState({ settings: {
    ...defaultSettings,
    terminalAnalyzer: {
      ...defaultSettings.terminalAnalyzer,
      provider: ClassifierProvider.Classifier,
      model: 'typesafe/jev-1.13',
      titleModel: 'title-model',
      reasoningEffort: ReasoningEffort.Low,
    },
  } })
  llm.analyzeTerminal.mockResolvedValue({ state: ActivityState.Idle, reason: 'Jev summary' })
  llm.generateTitle.mockResolvedValue({ title: 'Generated title' })
})
afterEach(cleanup)

it('logs the debugger run as activity transitions carrying the tested buffer', async () => {
  useActivityStateStore.setState({ states: {}, transitions: {} })
  const { unmount } = render(<SystemPromptDebugger {...props} />)
  fireEvent.click(screen.getByRole('button', { name: 'Test' }))
  await waitFor(() => { expect(useActivityStateStore.getState().states['debug']).toBe(ActivityState.Idle) })

  expect(useActivityStateStore.getState().transitions['debug']?.map((t) => [t.to, t.detail])).toEqual([
    [ActivityState.Working, { kind: ActivityTransitionKind.Debugger, snapshot: 'terminal output' }],
    [ActivityState.Idle, { kind: ActivityTransitionKind.Debugger, snapshot: 'terminal output' }],
  ])
  unmount()
  expect(useActivityStateStore.getState().transitions['debug']).toBeUndefined()
})

for (const Component of [TerminalAnalyzerDebugger, SystemPromptDebugger]) {
  describe(Component.name, () => {
    it('uses the Jev model without reasoning and displays the summary', async () => {
      render(<Component {...props} />)
      expect((screen.getByLabelText<HTMLInputElement>('Model')).value).toBe('typesafe/jev-1.13')
      expect((screen.getByLabelText<HTMLSelectElement>('Reasoning')).disabled).toBe(true)
      fireEvent.click(screen.getByRole('button', { name: 'Test' }))
      await waitFor(() => { expect(llm.analyzeTerminal).toHaveBeenCalled(); })
      const request = llm.analyzeTerminal.mock.calls[0]![2] as Record<string, unknown>
      expect(request).toMatchObject({ provider: ClassifierProvider.Classifier, model: 'typesafe/jev-1.13' })
      expect(request).not.toHaveProperty('reasoningEffort')
      await screen.findByText(/Jev summary/)
    })

    it('shows loading during cache clearing and surfaces failures', async () => {
      let rejectCache: (error: Error) => void = () => { throw new Error('Cache promise not initialized') }
      llm.clearAnalyzerCache.mockImplementation(() => new Promise<void>((_resolve, reject) => { rejectCache = reject }))
      render(<Component {...props} />)
      fireEvent.click(screen.getByRole('button', { name: 'Test' }))
      expect((screen.getByRole<HTMLButtonElement>('button', { name: /Analyzing|Testing/ })).disabled).toBe(true)
      rejectCache(new Error('Cache unavailable'))
      await screen.findByText('Cache unavailable')
      expect(llm.analyzeTerminal).not.toHaveBeenCalled()
    })

    it('uses the same analyzer model and reasoning when chat is selected', async () => {
      useSettingsStore.setState((state) => ({ settings: {
        ...state.settings,
        terminalAnalyzer: { ...state.settings.terminalAnalyzer, provider: ClassifierProvider.ChatCompletions },
      } }))
      render(<Component {...props} />)
      fireEvent.click(screen.getByRole('button', { name: 'Test' }))
      await waitFor(() => { expect(llm.analyzeTerminal).toHaveBeenCalledWith('terminal output', '', expect.objectContaining({
        provider: ClassifierProvider.ChatCompletions, model: 'typesafe/jev-1.13', reasoningEffort: ReasoningEffort.Low,
      })); })
    })
  })
}

it('keeps title generation on its separate chat model and retains analyzer edits across modes', async () => {
  render(<SystemPromptDebugger {...props} />)
  fireEvent.change(screen.getByLabelText<HTMLInputElement>('Model'), { target: { value: 'edited-jev' } })
  fireEvent.change(screen.getByLabelText('Mode'), { target: { value: 'title' } })
  expect((screen.getByLabelText<HTMLInputElement>('Model')).value).toBe('title-model')
  expect((screen.getByLabelText<HTMLSelectElement>('Reasoning')).disabled).toBe(false)
  screen.getByText('Provider: Chat Completions')
  fireEvent.click(screen.getByRole('button', { name: 'Test' }))
  await waitFor(() => { expect(llm.generateTitle).toHaveBeenCalledWith('terminal output', expect.objectContaining({
    model: 'title-model', reasoningEffort: ReasoningEffort.Low,
  })); })
  expect(llm.analyzeTerminal).not.toHaveBeenCalled()
  fireEvent.change(screen.getByLabelText('Mode'), { target: { value: 'analyzer' } })
  expect((screen.getByLabelText<HTMLInputElement>('Model')).value).toBe('edited-jev')
})

it('retains a single analyzer model across provider switches and an independent title model', async () => {
  const saveSettings = vi.fn<(settings: Settings) => Promise<void>>().mockResolvedValue(undefined)
  useSettingsStore.setState({ saveSettings })
  const sandbox = { isAvailable: vi.fn().mockResolvedValue(true) } as unknown as SandboxApi
  render(<SettingsDialog isOpen onClose={vi.fn()} sandbox={sandbox} platform={Platform.Darwin} />)
  fireEvent.click(screen.getByRole('button', { name: 'LLM' }))
  fireEvent.change(screen.getByLabelText('Classifier Model'), { target: { value: 'new-jev' } })
  fireEvent.change(screen.getByLabelText('Title Model'), { target: { value: 'new-title' } })
  fireEvent.change(screen.getByLabelText('Classifier Provider'), { target: { value: ClassifierProvider.ChatCompletions } })
  expect(screen.getByLabelText<HTMLInputElement>('Classifier Model').value).toBe('new-jev')
  fireEvent.change(screen.getByLabelText('Classifier Model'), { target: { value: 'new-chat' } })
  fireEvent.change(screen.getByLabelText('Classifier Provider'), { target: { value: ClassifierProvider.Classifier } })
  expect(screen.getByLabelText<HTMLInputElement>('Classifier Model').value).toBe('new-chat')
  expect(screen.getByLabelText<HTMLInputElement>('Title Model').value).toBe('new-title')
  fireEvent.click(screen.getByRole('button', { name: 'Save' }))
  await waitFor(() => { expect(saveSettings).toHaveBeenCalled() })
  expect(saveSettings.mock.calls[0]![0].terminalAnalyzer).toMatchObject({
    provider: ClassifierProvider.Classifier, model: 'new-chat', titleModel: 'new-title',
  })
})
