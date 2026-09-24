/* eslint-disable custom/no-string-literal-comparison -- test fixtures */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createAnalyzerStore, TitleRefreshStatus } from './createAnalyzerStore'
import type { AnalyzerDeps } from './createAnalyzerStore'
import { ActivityState } from '../types'
import { ActivityTransitionKind } from './activityState'
import { ClassifierProvider } from '../../shared/types'
import { createLlmClient, parseLlmJson } from '../lib/llmClient'
import type { LlmApi, Settings } from '../types'
import type { Tty, TtyState } from './createTtyStore'
import { createStore } from 'zustand/vanilla'
import { Terminal } from '@xterm/xterm'
import { PtyEventType } from '../../shared/ipc-types'
import type { PtyEvent } from '../../shared/ipc-types'
import { defaultClassifierCriteria } from '../../shared/classifierSettings'

/** Creates a mock Tty with controllable event emission via openTtyStream callback */
function makeMockTty() {
  type PtyEvent = import('../../shared/ipc-types').PtyEvent
  let eventCallback: ((event: PtyEvent) => void) | null = null

  const ttyState: TtyState = {
    ptyId: 'pty-1',
    write: vi.fn<(data: string) => Promise<void>>().mockResolvedValue(undefined),
    resize: vi.fn(),
    kill: vi.fn(),
  }

  const dispose = vi.fn()
  const tty: Tty = Object.assign(createStore<TtyState>()(() => ttyState), { dispose })

  return {
    tty,
    ttyState,
    dispose,
    setEventCallback: (cb: (event: PtyEvent) => void) => { eventCallback = cb },
    emitData: (data: string) => eventCallback?.({ type: PtyEventType.Data, data: new TextEncoder().encode(data) }),
    emitResize: (cols: number, rows: number) => eventCallback?.({ type: PtyEventType.Resize, cols, rows }),
    emitExit: (code: number) => eventCallback?.({ type: PtyEventType.Exit, exitCode: code }),
  }
}

/**
 * A faithful `openTtyStream` double.
 *
 * The daemon replays existing scrollback as ordinary Data events once the attach lands
 * — it does not hand back a scrollback array. The previous mock returned
 * `{ tty, scrollback, exitCode }`, a shape the real `openTtyStream` stopped producing in
 * 10f1910. That made the mock strictly more capable than production and kept a dead
 * branch in the analyzer looking alive. Model the events, not a convenient return value.
 */
function makeTtyStreamMock(mock: ReturnType<typeof makeMockTty>, scrollback: string[] = []) {
  return vi.fn().mockImplementation((_ptyId: string, onEvent: (event: PtyEvent) => void) => {
    mock.setEventCallback(onEvent)
    for (const chunk of scrollback) {
      onEvent({ type: PtyEventType.Data, data: new TextEncoder().encode(chunk) })
    }
    return Promise.resolve(mock.tty)
  })
}

function makeDeps(overrides?: Partial<AnalyzerDeps>): AnalyzerDeps {
  return {
    getSettings: vi.fn().mockReturnValue({
      llm: { apiKey: 'test-key', baseUrl: 'http://localhost' },
      terminalAnalyzer: { provider: ClassifierProvider.ChatCompletions, titleModel: 'test-model',
        model: 'test-model',
        systemPrompt: 'test prompt',
        titleSystemPrompt: 'title prompt',
        reasoningEffort: 'low',
        safePaths: ['/tmp'],
        bufferLines: 10,
        idleDebounceMs: 500,
        idleDebounceUnreadMs: 15000,
        criteria: { ...defaultClassifierCriteria },
      },
    } as unknown as Settings),
    hasUnreadAttention: vi.fn().mockReturnValue(false),
    isIdleDetectorDisabled: vi.fn().mockReturnValue(false),
    llm: {
      analyzeTerminal: vi.fn().mockResolvedValue({ state: 'idle', reason: 'prompt visible' }),
      generateTitle: vi.fn().mockResolvedValue({ title: 'Test Title', description: 'Test Description', branchName: 'test-title' }),
    } as unknown as LlmApi,
    updateMetadata: vi.fn(),
    getDisplayName: vi.fn().mockReturnValue(undefined),
    getDescription: vi.fn().mockReturnValue(undefined),
    setActivityTabState: vi.fn(),
    openTtyStream: makeTtyStreamMock(makeMockTty()),
    cwd: '/test',
    renameBranch: vi.fn().mockResolvedValue(undefined),
    getGitBranch: vi.fn().mockReturnValue('old-branch'),
    getBranchIsUserDefined: vi.fn().mockReturnValue(false),
    getParentId: vi.fn().mockReturnValue('parent-1'),
    refreshGitInfo: vi.fn().mockResolvedValue(undefined),
    refreshGit: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  }
}

/**
 * Runs the silent first burst after start() (the scrollback replay) through to its
 * classification so the detector knows a resting state and later bursts publish Working.
 */
async function settle(deps: AnalyzerDeps, mock: ReturnType<typeof makeMockTty>): Promise<void> {
  mock.emitData('replay')
  await vi.advanceTimersByTimeAsync(1000)
  expect(deps.setActivityTabState).not.toHaveBeenCalledWith('tab-1', ActivityState.Working, expect.objectContaining({ kind: ActivityTransitionKind.Classification }))
  vi.mocked(deps.setActivityTabState).mockClear()
  vi.mocked(deps.llm.analyzeTerminal).mockClear()
}

describe('createAnalyzerStore', () => {
  let deps: AnalyzerDeps

  beforeEach(() => {
    vi.clearAllMocks()
    deps = makeDeps()
  })

  it('creates store with initial state', () => {
    const store = createAnalyzerStore('tab-1', deps)
    const state = store.getState()

    expect(state.tabId).toBe('tab-1')
    expect(state.aiState).toBe(ActivityState.Idle)
    expect(state.analyzing).toBe(false)
    expect(state.reason).toBe('')
    expect(state.autoApprove).toBe(false)
  })

  it('setAutoApprove updates autoApprove state', () => {
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().setAutoApprove(true)
    expect(store.getState().autoApprove).toBe(true)

    store.getState().setAutoApprove(false)
    expect(store.getState().autoApprove).toBe(false)
  })

  it('getBufferText returns null when not started', () => {
    const store = createAnalyzerStore('tab-1', deps)
    expect(store.getState().getBufferText()).toBeNull()
  })

  it('stop resets terminal reference', async () => {
    const mock = makeMockTty()
    deps = makeDeps({
      openTtyStream: makeTtyStreamMock(mock, ['$ echo hello\r\nhello\r\n$ ']),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.waitFor(() => {
      expect(store.getState().getBufferText()).not.toBeNull()
    })

    store.getState().stop()
    expect(store.getState().getBufferText()).toBeNull()
  })

  it('reads the alternate buffer while a full-screen app holds the screen', async () => {
    const mock = makeMockTty()
    deps = makeDeps({ openTtyStream: makeTtyStreamMock(mock, []) })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.waitFor(() => {
      expect(deps.openTtyStream).toHaveBeenCalled()
    })

    mock.emitData('$ vim notes.txt\r\n')
    await vi.waitFor(() => {
      expect(store.getState().getBufferText()).toContain('vim notes.txt')
    })

    // Enter alt screen (DECSET 1049) and paint the app's own content.
    mock.emitData('\x1b[?1049h\x1b[H~ editing notes.txt')
    await vi.waitFor(() => {
      expect(store.getState().getBufferText()).toContain('editing notes.txt')
    })
    // The pre-launch prompt lives on the normal buffer and must not leak through.
    expect(store.getState().getBufferText()).not.toContain('vim notes.txt')

    // Leaving alt screen (DECRST 1049) restores the normal buffer.
    mock.emitData('\x1b[?1049l')
    await vi.waitFor(() => {
      expect(store.getState().getBufferText()).toContain('vim notes.txt')
    })
    expect(store.getState().getBufferText()).not.toContain('editing notes.txt')

    store.getState().stop()
  })

  it('start opens TTY stream and starts polling when settings are configured', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      openTtyStream: makeTtyStreamMock(mock, []),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0) // resolve openTtyStream
    await settle(deps, mock)

    // Simulate data arrival
    mock.emitData('$ echo hello\r\nhello\r\n$ ')
    vi.advanceTimersByTime(500) // poll interval

    expect(store.getState().aiState).toBe(ActivityState.Working)

    store.getState().stop()
    vi.useRealTimers()
  })

  it('start polls but analyze shows error once when model is missing', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      getSettings: vi.fn().mockReturnValue({
        llm: { apiKey: '', baseUrl: '' },
        terminalAnalyzer: { provider: ClassifierProvider.ChatCompletions, titleModel: 'test-model', model: '', systemPrompt: '', titleSystemPrompt: '', reasoningEffort: 'off', safePaths: [], bufferLines: 10, idleDebounceMs: 500, idleDebounceUnreadMs: 15000, criteria: defaultClassifierCriteria },
      } as unknown as Settings),
      openTtyStream: makeTtyStreamMock(mock, []),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)

    mock.emitData('$ hello')
    await vi.advanceTimersByTimeAsync(0)
    // The first burst after start is the replay: no Working, only the idle debounce schedules analyze
    expect(store.getState().aiState).toBe(ActivityState.Idle)
    expect(deps.setActivityTabState).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(500)
    // analyze() sees missing model and sets error once
    expect(store.getState().aiState).toBe(ActivityState.Error)
    expect(store.getState().reason).toBe('Terminal analyzer model not configured')

    // Second poll should not call setActivityTabState again (error shown once)
    const callCount = vi.mocked(deps.setActivityTabState).mock.calls.length
    mock.emitData('$ more data')
    await vi.advanceTimersByTimeAsync(1000)
    expect(vi.mocked(deps.setActivityTabState).mock.calls.length).toBe(callCount + 1) // only the Working state from the detector

    store.getState().stop()
    vi.useRealTimers()
  })

  it('analyzes with empty apiKey (valid for self-hosted like Ollama)', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      getSettings: vi.fn().mockReturnValue({
        llm: { apiKey: '', baseUrl: 'http://localhost:11434/v1' },
        terminalAnalyzer: { provider: ClassifierProvider.ChatCompletions, titleModel: 'test-model', model: 'llama3', systemPrompt: 'test prompt', titleSystemPrompt: 'title prompt', reasoningEffort: 'off', safePaths: [], bufferLines: 10, idleDebounceMs: 500, idleDebounceUnreadMs: 15000, criteria: defaultClassifierCriteria },
      } as unknown as Settings),
      openTtyStream: makeTtyStreamMock(mock, []),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)

    mock.emitData('$ echo hello\r\nhello\r\n$ ')
    vi.advanceTimersByTime(1000)
    await vi.advanceTimersByTimeAsync(0)

    expect(deps.llm.analyzeTerminal).toHaveBeenCalled()
    expect(store.getState().aiState).toBe(ActivityState.Idle)

    store.getState().stop()
    vi.useRealTimers()
  })

  it('analyze calls llm.analyzeTerminal and updates state', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      openTtyStream: makeTtyStreamMock(mock, []),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)

    mock.emitData('$ echo hello\r\nhello\r\n$ ')
    vi.advanceTimersByTime(500) // poll fires
    vi.advanceTimersByTime(500) // debounce fires

    // Let the async analyze() complete
    await vi.advanceTimersByTimeAsync(0)

    expect(deps.llm.analyzeTerminal).toHaveBeenCalled()
    expect(store.getState().aiState).toBe(ActivityState.Idle)
    expect(store.getState().reason).toBe('prompt visible')
    expect(store.getState().analyzing).toBe(false)
    expect(deps.setActivityTabState).toHaveBeenCalledWith('tab-1', ActivityState.Idle, expect.objectContaining({ kind: ActivityTransitionKind.Classification }))

    store.getState().stop()
    vi.useRealTimers()
  })

  it('analyze handles error result', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      llm: {
        analyzeTerminal: vi.fn().mockResolvedValue({ error: 'API error' }),
        generateTitle: vi.fn().mockResolvedValue({ title: '', description: '', branchName: '' }),
      } as unknown as LlmApi,
      openTtyStream: makeTtyStreamMock(mock, []),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)

    mock.emitData('$ echo hello\r\nhello\r\n$ ')
    vi.advanceTimersByTime(1000)
    await vi.advanceTimersByTimeAsync(0)

    expect(store.getState().aiState).toBe(ActivityState.Error)
    expect(store.getState().analyzing).toBe(false)

    store.getState().stop()
    vi.useRealTimers()
  })

  it('analyze handles LLM call failure', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      llm: {
        analyzeTerminal: vi.fn().mockRejectedValue(new Error('Network error')),
        generateTitle: vi.fn().mockResolvedValue({ title: '', description: '', branchName: '' }),
      } as unknown as LlmApi,
      openTtyStream: makeTtyStreamMock(mock, []),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)

    mock.emitData('$ echo hello\r\nhello\r\n$ ')
    vi.advanceTimersByTime(1000)
    await vi.advanceTimersByTimeAsync(0)

    expect(store.getState().aiState).toBe(ActivityState.Error)

    store.getState().stop()
    vi.useRealTimers()
  })

  it('queues pending analyze when request is in-flight and drains after completion', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()

    const calls: Array<(value: any) => void> = []
    deps = makeDeps({
      llm: {
        analyzeTerminal: vi.fn().mockImplementation(() => new Promise(r => { calls.push(r) })),
        generateTitle: vi.fn().mockResolvedValue({ title: '', description: '', branchName: '' }),
      } as unknown as LlmApi,
      openTtyStream: makeTtyStreamMock(mock, []),
    })

    const store = createAnalyzerStore('tab-1', deps)
    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)

    // First analysis triggers
    mock.emitData('$ echo hello\r\nhello\r\n$ ')
    vi.advanceTimersByTime(1000)
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    // Change buffer while request is in-flight
    mock.emitData('$ npm test\r\nPASS all tests\r\n$ ')
    vi.advanceTimersByTime(1000)

    // Should NOT start a second request — it should be queued
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    // Resolve the first request — should drain pending
    calls[0]!({ state: 'idle', reason: 'prompt visible' })
    await vi.advanceTimersByTimeAsync(0)

    // The real parsed change rejects the first response and drains pending work.
    expect(store.getState().getHistory()[0]?.error).toBe('[discarded]')
    expect(store.getState().aiState).toBe(ActivityState.Working)
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(2)

    // Resolve second
    calls[1]!({ state: 'idle', reason: 'tests passed' })
    await vi.advanceTimersByTimeAsync(0)

    store.getState().stop()
    vi.useRealTimers()
  })

  it('accepts an in-flight classification despite continuous identical viewport repaints', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()

    let resolveAnalysis!: (value: any) => void
    deps = makeDeps({
      llm: {
        analyzeTerminal: vi.fn().mockImplementation(() => new Promise(r => { resolveAnalysis = r })),
        generateTitle: vi.fn().mockResolvedValue({ title: '', description: '', branchName: '' }),
      } as unknown as LlmApi,
      openTtyStream: makeTtyStreamMock(mock, []),
    })

    const store = createAnalyzerStore('tab-1', deps)
    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)

    // First analysis
    mock.emitData('$ echo hello\r\nhello\r\n$ ')
    vi.advanceTimersByTime(1000)

    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    for (let i = 0; i < 10; i++) {
      mock.emitData('\x1b[H$ echo hello\r\nhello\r\n$ ')
      await vi.advanceTimersByTimeAsync(100)
    }
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    resolveAnalysis({ state: ActivityState.Idle, reason: 'prompt visible' })
    await vi.advanceTimersByTimeAsync(0)
    expect(store.getState().aiState).toBe(ActivityState.Idle)
    expect(store.getState().reason).toBe('prompt visible')
    expect(store.getState().getHistory()[0]?.error).toBeUndefined()

    mock.emitData('\x1b[H$ echo hello\r\nhello\r\n$ ')
    await vi.advanceTimersByTimeAsync(1000)
    expect(store.getState().aiState).toBe(ActivityState.Idle)
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    store.getState().stop()
    vi.useRealTimers()
  })

  it('ignores empty data after classification', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      openTtyStream: makeTtyStreamMock(mock, []),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)

    // First analysis
    mock.emitData('$ echo hello\r\nhello\r\n$ ')
    vi.advanceTimersByTime(1000)
    await vi.advanceTimersByTimeAsync(0)

    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    // Empty data does not change the parsed viewport.
    mock.emitData('')
    vi.advanceTimersByTime(1000)
    await vi.advanceTimersByTimeAsync(0)

    // No new classification is needed.
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    store.getState().stop()
    vi.useRealTimers()
  })

  it('waits for asynchronous parsing before scheduling classification', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({ openTtyStream: makeTtyStreamMock(mock) })
    const store = createAnalyzerStore('tab-1', deps)
    // eslint-disable-next-line @typescript-eslint/unbound-method -- invoked with its original receiver below
    const originalWrite = Terminal.prototype.write
    const writes: Array<() => void> = []
    const write = vi.spyOn(Terminal.prototype, 'write').mockImplementation(function (
      this: Terminal, data: string | Uint8Array, callback?: () => void,
    ): void {
      writes.push(() => { originalWrite.call(this, data, callback) })
    })

    store.getState().start('pty-1')
    mock.emitData('$ parsed later')
    await vi.advanceTimersByTimeAsync(1500)
    expect(store.getState().getBufferText()).toBeNull()
    expect(store.getState().aiState).toBe(ActivityState.Idle)
    expect(deps.llm.analyzeTerminal).not.toHaveBeenCalled()

    writes[0]!()
    await vi.advanceTimersByTimeAsync(1000)
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)
    expect(vi.mocked(deps.llm.analyzeTerminal).mock.calls[0]?.[0]).toContain('$ parsed later')

    store.getState().stop()
    write.mockRestore()
    vi.useRealTimers()
  })

  it('classifies changed viewport text after resize but ignores identical resize', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({ openTtyStream: makeTtyStreamMock(mock, ['abcdefghij']) })
    const store = createAnalyzerStore('tab-1', deps)
    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(1000)
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    mock.emitResize(5, 24)
    await vi.advanceTimersByTimeAsync(1000)
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(2)
    expect(vi.mocked(deps.llm.analyzeTerminal).mock.calls[1]?.[0]).toBe('abcde' + '\n'.repeat(23))

    mock.emitResize(5, 24)
    await vi.advanceTimersByTimeAsync(1000)
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(2)
    expect(store.getState().aiState).toBe(ActivityState.Idle)
    store.getState().stop()
    vi.useRealTimers()
  })

  it('ignores old stream events and parsed-write callbacks after stop and restart', async () => {
    vi.useFakeTimers()
    const oldMock = makeMockTty()
    const newMock = makeMockTty()
    deps = makeDeps({
      openTtyStream: vi.fn()
        .mockImplementationOnce(makeTtyStreamMock(oldMock))
        .mockImplementationOnce(makeTtyStreamMock(newMock)),
    })
    const callbacks: Array<() => void> = []
    const buffers: Terminal['buffer'][] = []
    const write = vi.spyOn(Terminal.prototype, 'write').mockImplementation(function (
      this: Terminal, _data: string | Uint8Array, callback?: () => void,
    ): void {
      buffers.push(this.buffer)
      if (callback) callbacks.push(callback)
    })
    const store = createAnalyzerStore('tab-1', deps)
    store.getState().start('pty-1')
    oldMock.emitData('old screen')
    store.getState().stop()
    callbacks[0]!()
    store.getState().start('pty-2')

    newMock.emitData('new screen')
    // Read spying lets an old callback incorrectly reading the new terminal be observed.
    const getBuffer = vi.spyOn(buffers[1]!, 'active', 'get')
    callbacks[0]!()
    oldMock.emitData('late old output')
    oldMock.emitResize(5, 3)
    oldMock.emitExit(0)
    expect(getBuffer).not.toHaveBeenCalled()
    expect(write).toHaveBeenCalledTimes(2)
    await vi.advanceTimersByTimeAsync(1000)
    expect(newMock.dispose).not.toHaveBeenCalled()
    expect(deps.llm.analyzeTerminal).not.toHaveBeenCalled()
    expect(store.getState().aiState).toBe(ActivityState.Idle)

    store.getState().stop()
    getBuffer.mockRestore()
    write.mockRestore()
    vi.useRealTimers()
  })

  it('returns to idle when the viewport is cleared while classification is pending', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    let resolveResult!: (value: Awaited<ReturnType<LlmApi['analyzeTerminal']>>) => void
    const result = new Promise<Awaited<ReturnType<LlmApi['analyzeTerminal']>>>((resolve) => { resolveResult = resolve })
    deps = makeDeps({ openTtyStream: makeTtyStreamMock(mock, ['working']) })
    vi.mocked(deps.llm.analyzeTerminal).mockReturnValue(result)
    const store = createAnalyzerStore('tab-1', deps)
    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(1000)
    expect(store.getState().analyzing).toBe(true)

    mock.emitData('\x1b[2J\x1b[H')
    await vi.advanceTimersByTimeAsync(1000)
    expect(store.getState().aiState).toBe(ActivityState.Idle)
    expect(store.getState().analyzing).toBe(false)
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    resolveResult({ state: ActivityState.Working, reason: 'old output' })
    await vi.advanceTimersByTimeAsync(0)
    expect(store.getState().aiState).toBe(ActivityState.Idle)
    expect(store.getState().getHistory()[0]?.error).toBe('[discarded]')
    store.getState().stop()
    vi.useRealTimers()
  })

  it('marks an exited PTY completed and ignores its pending classification', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    let resolveResult!: (value: Awaited<ReturnType<LlmApi['analyzeTerminal']>>) => void
    const result = new Promise<Awaited<ReturnType<LlmApi['analyzeTerminal']>>>((resolve) => { resolveResult = resolve })
    deps = makeDeps({ openTtyStream: makeTtyStreamMock(mock, ['working']) })
    vi.mocked(deps.llm.analyzeTerminal).mockReturnValue(result)
    const store = createAnalyzerStore('tab-1', deps)
    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(1000)
    expect(store.getState().analyzing).toBe(true)

    mock.emitExit(0)
    expect(store.getState().aiState).toBe(ActivityState.Completed)
    expect(store.getState().analyzing).toBe(false)
    resolveResult({ state: ActivityState.Working, reason: 'old output' })
    await vi.advanceTimersByTimeAsync(1000)
    expect(store.getState().aiState).toBe(ActivityState.Completed)
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)
    vi.useRealTimers()
  })

  it('does not restart polling after an already-exited PTY is replayed during attach', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      openTtyStream: vi.fn().mockImplementation((_ptyId: string, onEvent: (event: PtyEvent) => void) => {
        onEvent({ type: PtyEventType.Exit, exitCode: 0 })
        return Promise.resolve(mock.tty)
      }),
    })
    const store = createAnalyzerStore('tab-1', deps)
    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(1000)
    expect(store.getState().aiState).toBe(ActivityState.Completed)
    expect(mock.dispose).toHaveBeenCalledTimes(1)
    expect(vi.getTimerCount()).toBe(0)
    expect(deps.llm.analyzeTerminal).not.toHaveBeenCalled()
    vi.useRealTimers()
  })

  it('dedup LRU cache retains previous buffers across changes', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()

    let callCount = 0
    deps = makeDeps({
      llm: {
        analyzeTerminal: vi.fn().mockImplementation(() => {
          callCount++
          return Promise.resolve({ state: 'idle', reason: `call-${String(callCount)}` })
        }),
        generateTitle: vi.fn().mockResolvedValue({ title: '', description: '', branchName: '' }),
      } as unknown as LlmApi,
      openTtyStream: makeTtyStreamMock(mock, []),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)

    // Analyze buffer A
    mock.emitData('$ echo hello\r\nhello\r\n$ ')
    vi.advanceTimersByTime(1000)
    await vi.advanceTimersByTimeAsync(0)
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    // Analyze buffer B (different content via terminal reset + new data)
    mock.emitData('\x1bc$ npm test\r\nPASS\r\n$ ')
    vi.advanceTimersByTime(1000)
    await vi.advanceTimersByTimeAsync(0)
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(2)

    // Go back to buffer A by resetting terminal and writing same content
    mock.emitData('\x1bc$ echo hello\r\nhello\r\n$ ')
    vi.advanceTimersByTime(1000)
    await vi.advanceTimersByTimeAsync(0)

    // Should reuse cached result for buffer A — no third LLM call
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(2)

    store.getState().stop()
    vi.useRealTimers()
  })

  it('feeds the daemon-replayed scrollback into the headless terminal on start', async () => {
    const mock = makeMockTty()
    deps = makeDeps({
      openTtyStream: makeTtyStreamMock(mock, ['$ echo hello\r\nhello\r\n$ ']),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.waitFor(() => {
      expect(store.getState().getBufferText()).not.toBeNull()
    })

    expect(store.getState().getBufferText()).toContain('hello')

    store.getState().stop()
  })

  // The regression this whole change exists to prevent: `stopPolling` read an
  // `unsubscribeEvents` that nothing ever assigned, because the dependency type had
  // erased it. Ownership now lives on the Tty, so stop() cannot forget to release it.
  it('disposes the TTY attachment on stop', async () => {
    const mock = makeMockTty()
    deps = makeDeps({ openTtyStream: makeTtyStreamMock(mock) })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.waitFor(() => { expect(mock.dispose).not.toHaveBeenCalled() })

    store.getState().stop()

    expect(mock.dispose).toHaveBeenCalledTimes(1)
  })

  it('disposes a TTY that lands after stop() already ran', async () => {
    const mock = makeMockTty()
    let resolveAttach: (tty: Tty) => void = () => {}
    deps = makeDeps({
      openTtyStream: vi.fn().mockImplementation((_ptyId: string, onEvent: (event: PtyEvent) => void) => {
        mock.setEventCallback(onEvent)
        return new Promise<Tty>((resolve) => { resolveAttach = resolve })
      }),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    store.getState().stop()          // stop wins the race
    resolveAttach(mock.tty)          // attach lands afterwards
    await vi.waitFor(() => { expect(mock.dispose).toHaveBeenCalledTimes(1) })
  })

  // Restart race: the attach continuation must guard the owner it was born with, not
  // the mutable `streamOwner` (which the second start() has already reassigned). A late
  // attach from cycle #1 otherwise sees cycle #2's live owner and clobbers the current
  // TTY with its own already-disposed handle for the wrong pty.
  it('keeps the current TTY when a previous cycle\'s attach resolves late after restart', async () => {
    const mock1 = makeMockTty()
    const mock2 = makeMockTty()
    const mocks = [mock1, mock2]
    const resolvers: Array<(tty: Tty) => void> = []
    let call = 0
    deps = makeDeps({
      openTtyStream: vi.fn().mockImplementation((_ptyId: string, onEvent: (event: PtyEvent) => void) => {
        const mock = mocks[call++]!
        mock.setEventCallback(onEvent)
        return new Promise<Tty>((resolve) => { resolvers.push(resolve) })
      }),
    })
    const store = createAnalyzerStore('tab-1', deps)

    // Cycle #1: attach P1 in flight, then stop() disposes owner #1.
    store.getState().start('pty-1')
    store.getState().stop()

    // Cycle #2: attach P2 in flight and resolves first, becoming the active TTY.
    store.getState().start('pty-1')
    resolvers[1]!(mock2.tty)
    // Let P2's continuation assign the active TTY before P1 lands.
    await vi.waitFor(() => { expect(mock2.dispose).not.toHaveBeenCalled() })
    await Promise.resolve()

    // P1 (previous cycle) resolves late: it must be disposed into the dead owner #1,
    // and must NOT become the active TTY.
    resolvers[0]!(mock1.tty)
    await vi.waitFor(() => { expect(mock1.dispose).toHaveBeenCalledTimes(1) })

    // Auto-approve must reach tty2 (current), never the disposed tty1.
    store.getState().setAutoApprove(true)
    store.setState({ aiState: ActivityState.SafePermissionRequested })

    // eslint-disable-next-line @typescript-eslint/unbound-method
    expect(mock2.ttyState.write).toHaveBeenCalledWith('\r')
    // eslint-disable-next-line @typescript-eslint/unbound-method
    expect(mock1.ttyState.write).not.toHaveBeenCalled()

    store.getState().stop()
  })

  describe('onUserInput', () => {
    it('triggers title generation on first Enter key', async () => {
      vi.useFakeTimers()
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: makeTtyStreamMock(mock, ['$ ']),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.advanceTimersByTimeAsync(0)
      expect(store.getState().getBufferText()).not.toBeNull()

      store.getState().onUserInput('hello\r')

      // Screen is not captured until the delay elapses.
      await vi.advanceTimersByTimeAsync(999)
      expect(deps.llm.generateTitle).not.toHaveBeenCalled()

      await vi.advanceTimersByTimeAsync(1)
      expect(deps.llm.generateTitle).toHaveBeenCalled()
      expect(deps.updateMetadata).toHaveBeenCalledWith('displayName', 'Test Title', 'analyzerSetDisplayName')
      expect(deps.updateMetadata).toHaveBeenCalledWith('description', 'Test Description', 'analyzerSetDescription')

      store.getState().stop()
      vi.useRealTimers()
    })

    it('does not trigger title generation on non-Enter input', async () => {
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: makeTtyStreamMock(mock, ['$ ']),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.waitFor(() => {
        expect(store.getState().getBufferText()).not.toBeNull()
      })

      store.getState().onUserInput('hello')

      expect(deps.llm.generateTitle).not.toHaveBeenCalled()
      store.getState().stop()
    })

    it('does not trigger title generation twice', async () => {
      vi.useFakeTimers()
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: makeTtyStreamMock(mock, ['$ ']),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.advanceTimersByTimeAsync(0)
      expect(store.getState().getBufferText()).not.toBeNull()

      store.getState().onUserInput('\r')
      await vi.advanceTimersByTimeAsync(1000)
      expect(deps.llm.generateTitle).toHaveBeenCalledTimes(1)

      store.getState().onUserInput('\r')
      await vi.advanceTimersByTimeAsync(1000)
      // Still only called once
      expect(deps.llm.generateTitle).toHaveBeenCalledTimes(1)

      store.getState().stop()
      vi.useRealTimers()
    })

    it('cancels the pending title capture when stopped before the delay elapses', async () => {
      vi.useFakeTimers()
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: makeTtyStreamMock(mock, ['$ ']),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.advanceTimersByTimeAsync(0)
      expect(store.getState().getBufferText()).not.toBeNull()

      store.getState().onUserInput('hello\r')
      // Stop before the 1s delay fires.
      await vi.advanceTimersByTimeAsync(500)
      store.getState().stop()

      await vi.advanceTimersByTimeAsync(1000)
      expect(deps.llm.generateTitle).not.toHaveBeenCalled()

      vi.useRealTimers()
    })

    it('does not generate title when displayName and description already exist', async () => {
      const mock = makeMockTty()
      deps = makeDeps({
        getDisplayName: vi.fn().mockReturnValue('Existing Title'),
        getDescription: vi.fn().mockReturnValue('Existing Description'),
        openTtyStream: makeTtyStreamMock(mock, ['$ ']),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.waitFor(() => {
        expect(store.getState().getBufferText()).not.toBeNull()
      })

      store.getState().onUserInput('\r')

      expect(deps.llm.generateTitle).not.toHaveBeenCalled()
      store.getState().stop()
    })

    it('generates description even when displayName already exists', async () => {
      vi.useFakeTimers()
      const mock = makeMockTty()
      deps = makeDeps({
        getDisplayName: vi.fn().mockReturnValue('Existing Title'),
        getDescription: vi.fn().mockReturnValue(undefined),
        openTtyStream: makeTtyStreamMock(mock, ['$ ']),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.advanceTimersByTimeAsync(0)
      expect(store.getState().getBufferText()).not.toBeNull()

      store.getState().onUserInput('\r')
      await vi.advanceTimersByTimeAsync(1000)

      expect(deps.llm.generateTitle).toHaveBeenCalled()
      expect(deps.updateMetadata).toHaveBeenCalledWith('description', 'Test Description', 'analyzerSetDescription')
      // Should not overwrite existing displayName
      expect(deps.updateMetadata).not.toHaveBeenCalledWith('displayName', expect.anything(), expect.anything())

      store.getState().stop()
      vi.useRealTimers()
    })
  })

  describe('manual refresh', () => {
    async function startedStore(overrides?: Partial<AnalyzerDeps>) {
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: makeTtyStreamMock(mock, ['$ echo hi\r\nhi\r\n$ ']),
        ...overrides,
      })
      const store = createAnalyzerStore('tab-1', deps)
      store.getState().start('pty-1')
      await vi.waitFor(() => {
        expect(store.getState().getBufferText()).not.toBeNull()
      })
      return store
    }

    it('refreshTitleAndDescription overwrites display name and description even when already set', async () => {
      const store = await startedStore({
        getDisplayName: vi.fn().mockReturnValue('Existing Title'),
        getDescription: vi.fn().mockReturnValue('Existing Description'),
      })

      const result = await store.getState().refreshTitleAndDescription()

      expect(result.status).toBe(TitleRefreshStatus.Success)
      expect(deps.llm.generateTitle).toHaveBeenCalled()
      expect(deps.updateMetadata).toHaveBeenCalledWith('displayName', 'Test Title', 'manualRefreshTitle')
      expect(deps.updateMetadata).toHaveBeenCalledWith('description', 'Test Description', 'manualRefreshDescription')
      store.getState().stop()
    })

    it('refreshTitleAndDescription reports failure when there is no buffer', async () => {
      deps = makeDeps()
      const store = createAnalyzerStore('tab-1', deps)

      const result = await store.getState().refreshTitleAndDescription()

      expect(result).toEqual({ status: TitleRefreshStatus.Failure, error: 'Terminal is empty — nothing for the labeller to read' })
      expect(deps.llm.generateTitle).not.toHaveBeenCalled()
      expect(deps.updateMetadata).not.toHaveBeenCalled()
    })

    it('refreshTitleAndDescription reports failure when no model is configured', async () => {
      deps = makeDeps({
        getSettings: vi.fn().mockReturnValue({
          llm: { apiKey: 'test-key', baseUrl: 'http://localhost' },
          terminalAnalyzer: { provider: ClassifierProvider.ChatCompletions, titleModel: '', model: '', titleSystemPrompt: 'title prompt', reasoningEffort: 'low' },
        } as unknown as Settings),
      })
      const store = createAnalyzerStore('tab-1', deps)

      const result = await store.getState().refreshTitleAndDescription()

      expect(result).toEqual({ status: TitleRefreshStatus.Failure, error: 'Title model not configured' })
      expect(deps.llm.generateTitle).not.toHaveBeenCalled()
    })

    it('refreshTitleAndDescription surfaces the LLM error message and writes no metadata', async () => {
      const store = await startedStore({
        llm: {
          analyzeTerminal: vi.fn().mockResolvedValue({ state: 'idle', reason: '' }),
          generateTitle: vi.fn().mockResolvedValue({ error: 'rate limited' }),
        } as unknown as LlmApi,
      })

      const result = await store.getState().refreshTitleAndDescription()

      expect(result).toEqual({ status: TitleRefreshStatus.Failure, error: 'rate limited' })
      expect(deps.updateMetadata).not.toHaveBeenCalled()
      store.getState().stop()
    })

    it('refreshTitleAndDescription surfaces a thrown LLM error', async () => {
      const store = await startedStore({
        llm: {
          analyzeTerminal: vi.fn().mockResolvedValue({ state: 'idle', reason: '' }),
          generateTitle: vi.fn().mockRejectedValue(new Error('network down')),
        } as unknown as LlmApi,
      })

      const result = await store.getState().refreshTitleAndDescription()

      expect(result).toEqual({ status: TitleRefreshStatus.Failure, error: 'network down' })
      store.getState().stop()
    })

    it('refreshBranchName renames the branch and marks it user-defined even when already user-defined', async () => {
      const store = await startedStore({
        getBranchIsUserDefined: vi.fn().mockReturnValue(true),
      })

      const result = await store.getState().refreshBranchName()

      expect(result.status).toBe(TitleRefreshStatus.Success)
      expect(deps.renameBranch).toHaveBeenCalledWith('old-branch', 'test-title')
      expect(deps.updateMetadata).toHaveBeenCalledWith('branchIsUserDefined', 'true', 'manualRefreshBranch')
      // Branch-only refresh must not touch the display name / description
      expect(deps.updateMetadata).not.toHaveBeenCalledWith('displayName', expect.anything(), expect.anything())
      store.getState().stop()
    })

    it('refreshBranchName does not rename when the suggested branch name is invalid', async () => {
      const store = await startedStore({
        llm: {
          analyzeTerminal: vi.fn().mockResolvedValue({ state: 'idle', reason: '' }),
          generateTitle: vi.fn().mockResolvedValue({ title: 'Test Title', description: 'd', branchName: 'Invalid Name!' }),
        } as unknown as LlmApi,
      })

      const result = await store.getState().refreshBranchName()

      expect(result).toEqual({ status: TitleRefreshStatus.Failure, error: 'LLM suggested an invalid branch name: "Invalid Name!"' })
      expect(deps.renameBranch).not.toHaveBeenCalled()
      expect(deps.updateMetadata).not.toHaveBeenCalledWith('branchIsUserDefined', 'true', 'manualRefreshBranch')
      store.getState().stop()
    })

    // A missing `branchName` key arrives as undefined, and the string "undefined" would
    // otherwise satisfy the kebab-case branch name regex.
    it('refreshBranchName does not rename when the LLM omits the branch name', async () => {
      const store = await startedStore({
        llm: {
          analyzeTerminal: vi.fn().mockResolvedValue({ state: 'idle', reason: '' }),
          generateTitle: vi.fn().mockResolvedValue({ title: 'Test Title', description: 'd' }),
        } as unknown as LlmApi,
      })

      const result = await store.getState().refreshBranchName()

      expect(result.status).toBe(TitleRefreshStatus.Failure)
      expect(deps.renameBranch).not.toHaveBeenCalled()
      store.getState().stop()
    })

    it('refreshBranchName surfaces a rename failure', async () => {
      const store = await startedStore({
        renameBranch: vi.fn().mockRejectedValue(new Error('branch already exists')),
      })

      const result = await store.getState().refreshBranchName()

      expect(result).toEqual({ status: TitleRefreshStatus.Failure, error: 'branch already exists' })
      expect(deps.updateMetadata).not.toHaveBeenCalledWith('branchIsUserDefined', 'true', 'manualRefreshBranch')
      store.getState().stop()
    })

    it('refreshBranchName reports failure when the workspace has no branch', async () => {
      const store = await startedStore({
        getGitBranch: vi.fn().mockReturnValue(undefined),
      })

      const result = await store.getState().refreshBranchName()

      expect(result).toEqual({ status: TitleRefreshStatus.Failure, error: 'Workspace has no branch to rename' })
      expect(deps.renameBranch).not.toHaveBeenCalled()
      store.getState().stop()
    })
  })

  describe('auto-approve', () => {
    it('auto-approves safe permission requests via own TTY', async () => {
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: makeTtyStreamMock(mock, []),
      })

      const store = createAnalyzerStore('tab-1', deps)
      store.getState().start('pty-1')
      await vi.waitFor(() => {
        expect(deps.openTtyStream).toHaveBeenCalled()
      })
      // Wait for stream to be connected
      await vi.advanceTimersByTimeAsync(0).catch(() => {})
      await new Promise(r => setTimeout(r, 0))

      store.getState().setAutoApprove(true)

      // Simulate state change to safe_permission_requested
      store.setState({ aiState: ActivityState.SafePermissionRequested })

      // eslint-disable-next-line @typescript-eslint/unbound-method
      expect(mock.ttyState.write).toHaveBeenCalledWith('\r')

      store.getState().stop()
    })

    it('does not auto-approve when autoApprove is false', async () => {
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: makeTtyStreamMock(mock, []),
      })

      const store = createAnalyzerStore('tab-1', deps)
      store.getState().start('pty-1')
      await new Promise(r => setTimeout(r, 0))

      store.setState({ aiState: ActivityState.SafePermissionRequested })

      // eslint-disable-next-line @typescript-eslint/unbound-method
      expect(mock.ttyState.write).not.toHaveBeenCalled()
      store.getState().stop()
    })

    it('does not auto-approve for non-safe states', async () => {
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: makeTtyStreamMock(mock, []),
      })

      const store = createAnalyzerStore('tab-1', deps)
      store.getState().start('pty-1')
      await new Promise(r => setTimeout(r, 0))

      store.getState().setAutoApprove(true)
      store.setState({ aiState: ActivityState.PermissionRequest })

      // eslint-disable-next-line @typescript-eslint/unbound-method
      expect(mock.ttyState.write).not.toHaveBeenCalled()
      store.getState().stop()
    })
  })

  it('activity state sync calls setActivityTabState during analysis', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      openTtyStream: makeTtyStreamMock(mock, []),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)
    await settle(deps, mock)

    mock.emitData('$ echo hello\r\nhello\r\n$ ')
    vi.advanceTimersByTime(500) // poll detects change
    // 'working' state set via updateAiState; the detector edge carries no reason, only the screen
    expect(deps.setActivityTabState).toHaveBeenCalledWith('tab-1', ActivityState.Working, {
      kind: ActivityTransitionKind.Classification, reason: '', snapshot: expect.stringContaining('hello') as string,
    })

    vi.advanceTimersByTime(500) // debounce fires analyze
    await vi.advanceTimersByTimeAsync(0) // resolve async
    // 'idle' state set after analysis completes, with the classifier's reason
    expect(deps.setActivityTabState).toHaveBeenCalledWith('tab-1', ActivityState.Idle, {
      kind: ActivityTransitionKind.Classification, reason: 'prompt visible', snapshot: expect.stringContaining('hello') as string,
    })

    store.getState().stop()
    vi.useRealTimers()
  })

  it('does not publish Working for the replay after start, only its classification', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      openTtyStream: makeTtyStreamMock(mock, ['$ replayed scrollback\r\n$ ']),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)
    vi.advanceTimersByTime(500)
    expect(deps.setActivityTabState).not.toHaveBeenCalled()

    vi.advanceTimersByTime(500)
    await vi.advanceTimersByTimeAsync(0)
    expect(deps.setActivityTabState).toHaveBeenCalledExactlyOnceWith('tab-1', ActivityState.Idle, expect.objectContaining({ kind: ActivityTransitionKind.Classification }))

    mock.emitData('$ npm test\r\n')
    vi.advanceTimersByTime(500)
    expect(deps.setActivityTabState).toHaveBeenLastCalledWith('tab-1', ActivityState.Working, expect.objectContaining({ kind: ActivityTransitionKind.Classification }))

    store.getState().stop()
    vi.useRealTimers()
  })

  it('publishes Working when the process was already producing output at start', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      openTtyStream: makeTtyStreamMock(mock, ['$ npm test\r\n']),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)
    // A spinner keeps the viewport changing; the replay would have gone quiet by now.
    for (let t = 0; t < 500; t += 100) {
      await vi.advanceTimersByTimeAsync(100)
      mock.emitData(`spinner ${String(t)}\r\n`)
    }
    await vi.advanceTimersByTimeAsync(0)
    expect(deps.setActivityTabState).toHaveBeenCalledExactlyOnceWith('tab-1', ActivityState.Working, expect.objectContaining({ kind: ActivityTransitionKind.Classification }))
    expect(deps.llm.analyzeTerminal).not.toHaveBeenCalled()

    vi.advanceTimersByTime(500)
    await vi.advanceTimersByTimeAsync(0)
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)
    expect(deps.setActivityTabState).toHaveBeenLastCalledWith('tab-1', ActivityState.Idle, expect.objectContaining({ kind: ActivityTransitionKind.Classification }))

    store.getState().stop()
    vi.useRealTimers()
  })

  it('skips the Working edge but still classifies on idle while the idle detector is disabled', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      openTtyStream: makeTtyStreamMock(mock, []),
      isIdleDetectorDisabled: vi.fn().mockReturnValue(true),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)
    await settle(deps, mock)

    mock.emitData('$ echo hello\r\nhello\r\n$ ')
    vi.advanceTimersByTime(500)
    expect(deps.setActivityTabState).not.toHaveBeenCalledWith('tab-1', ActivityState.Working, expect.objectContaining({ kind: ActivityTransitionKind.Classification }))

    vi.advanceTimersByTime(500)
    await vi.advanceTimersByTimeAsync(0)
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)
    expect(deps.setActivityTabState).toHaveBeenCalledWith('tab-1', ActivityState.Idle, expect.objectContaining({ kind: ActivityTransitionKind.Classification }))

    store.getState().stop()
    vi.useRealTimers()
  })

  describe('history logging', () => {
    it('logs successful analysis to history with response', async () => {
      vi.useFakeTimers()
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: makeTtyStreamMock(mock, []),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.advanceTimersByTimeAsync(0)

      mock.emitData('$ echo hello\r\nhello\r\n$ ')
      vi.advanceTimersByTime(1000)
      await vi.advanceTimersByTimeAsync(0)

      const history = store.getState().getHistory()
      expect(history).toHaveLength(1)
      expect(history[0]!.kind).toBe('analyzer')
      expect(history[0]!.error).toBeUndefined()
      expect(history[0]!.model).toBe('test-model')
      expect(history[0]!.response).toBe(JSON.stringify({ state: 'idle', reason: 'prompt visible' }))

      store.getState().stop()
      vi.useRealTimers()
    })

    it('logs error result to history with response', async () => {
      vi.useFakeTimers()
      const mock = makeMockTty()
      deps = makeDeps({
        llm: {
          analyzeTerminal: vi.fn().mockResolvedValue({ error: 'API error' }),
          generateTitle: vi.fn().mockResolvedValue({ title: '', description: '', branchName: '' }),
        } as unknown as LlmApi,
        openTtyStream: makeTtyStreamMock(mock, []),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.advanceTimersByTimeAsync(0)

      mock.emitData('$ echo hello\r\nhello\r\n$ ')
      vi.advanceTimersByTime(1000)
      await vi.advanceTimersByTimeAsync(0)

      const history = store.getState().getHistory()
      expect(history).toHaveLength(1)
      expect(history[0]!.kind).toBe('analyzer')
      expect(history[0]!.error).toBe('API error')
      expect(history[0]!.response).toBe(JSON.stringify({ error: 'API error' }))

      store.getState().stop()
      vi.useRealTimers()
    })

    it('logs exception to history', async () => {
      vi.useFakeTimers()
      const mock = makeMockTty()
      deps = makeDeps({
        llm: {
          analyzeTerminal: vi.fn().mockRejectedValue(new Error('Network error')),
          generateTitle: vi.fn().mockResolvedValue({ title: '', description: '', branchName: '' }),
        } as unknown as LlmApi,
        openTtyStream: makeTtyStreamMock(mock, []),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.advanceTimersByTimeAsync(0)

      mock.emitData('$ echo hello\r\nhello\r\n$ ')
      vi.advanceTimersByTime(1000)
      await vi.advanceTimersByTimeAsync(0)

      const history = store.getState().getHistory()
      expect(history).toHaveLength(1)
      expect(history[0]!.kind).toBe('analyzer')
      expect(history[0]!.error).toBe('Network error')
      expect(history[0]!.response).toBe('')

      store.getState().stop()
      vi.useRealTimers()
    })

    it('logs title generation to history', async () => {
      vi.useFakeTimers()
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: makeTtyStreamMock(mock, ['$ ']),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.advanceTimersByTimeAsync(0)
      expect(store.getState().getBufferText()).not.toBeNull()

      store.getState().onUserInput('hello\r')
      await vi.advanceTimersByTimeAsync(1000)

      expect(deps.llm.generateTitle).toHaveBeenCalled()

      const history = store.getState().getHistory()
      const titleEntry = history.find(h => h.kind === 'title')
      expect(titleEntry).toBeDefined()
      expect(titleEntry?.error).toBeUndefined()
      expect(titleEntry?.response).toBe(JSON.stringify({ title: 'Test Title', description: 'Test Description', branchName: 'test-title' }))

      store.getState().stop()
      vi.useRealTimers()
    })

    it('skips branch rename when branch is user-defined', async () => {
      vi.useFakeTimers()
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: makeTtyStreamMock(mock, ['$ ']),
        getBranchIsUserDefined: vi.fn().mockReturnValue(true),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.advanceTimersByTimeAsync(0)
      expect(store.getState().getBufferText()).not.toBeNull()

      store.getState().onUserInput('hello\r')
      await vi.advanceTimersByTimeAsync(1000)

      expect(deps.llm.generateTitle).toHaveBeenCalled()
      expect(deps.updateMetadata).toHaveBeenCalledWith('displayName', 'Test Title', 'analyzerSetDisplayName')
      expect(deps.renameBranch).not.toHaveBeenCalled()

      store.getState().stop()
      vi.useRealTimers()
    })

    it('skips title, description and branch rename when workspace has no parent', async () => {
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: makeTtyStreamMock(mock, ['$ ']),
        getParentId: vi.fn().mockReturnValue(null),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.waitFor(() => {
        expect(store.getState().getBufferText()).not.toBeNull()
      })

      store.getState().onUserInput('hello\r')

      await new Promise((resolve) => setTimeout(resolve, 50))

      expect(deps.llm.generateTitle).not.toHaveBeenCalled()
      expect(deps.updateMetadata).not.toHaveBeenCalledWith('displayName', expect.anything(), expect.anything())
      expect(deps.updateMetadata).not.toHaveBeenCalledWith('description', expect.anything(), expect.anything())
      expect(deps.renameBranch).not.toHaveBeenCalled()

      store.getState().stop()
    })

    it('logs title generation failure to history', async () => {
      vi.useFakeTimers()
      const mock = makeMockTty()
      deps = makeDeps({
        llm: {
          analyzeTerminal: vi.fn().mockResolvedValue({ state: 'idle', reason: '' }),
          generateTitle: vi.fn().mockRejectedValue(new Error('Title API error')),
        } as unknown as LlmApi,
        openTtyStream: makeTtyStreamMock(mock, ['$ ']),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.advanceTimersByTimeAsync(0)
      expect(store.getState().getBufferText()).not.toBeNull()

      store.getState().onUserInput('hello\r')
      await vi.advanceTimersByTimeAsync(1000)

      expect(deps.llm.generateTitle).toHaveBeenCalled()

      const history = store.getState().getHistory()
      const titleEntry = history.find(h => h.kind === 'title' && h.error)
      expect(titleEntry).toBeDefined()
      expect(titleEntry?.error).toBe('Title API error')
      expect(titleEntry?.response).toBe('')

      store.getState().stop()
      vi.useRealTimers()
    })
  })

  describe('git refresh on AI state change', () => {
    it('triggers refreshGitInfo and refreshGit when state becomes Idle', async () => {
      const store = createAnalyzerStore('tab-1', deps)
      store.setState({ aiState: ActivityState.Working })
      store.setState({ aiState: ActivityState.Idle })
      await new Promise(r => setTimeout(r, 0))
      expect(deps.refreshGitInfo).toHaveBeenCalled()
      expect(deps.refreshGit).toHaveBeenCalled()
    })

    it('triggers refreshGitInfo and refreshGit when state becomes Completed', async () => {
      const store = createAnalyzerStore('tab-1', deps)
      store.setState({ aiState: ActivityState.Working })
      store.setState({ aiState: ActivityState.Completed })
      await new Promise(r => setTimeout(r, 0))
      expect(deps.refreshGitInfo).toHaveBeenCalled()
      expect(deps.refreshGit).toHaveBeenCalled()
    })

    it('does not trigger git refresh when state becomes Error', async () => {
      const store = createAnalyzerStore('tab-1', deps)
      store.setState({ aiState: ActivityState.Working })
      store.setState({ aiState: ActivityState.Error })
      await new Promise(r => setTimeout(r, 0))
      expect(deps.refreshGitInfo).not.toHaveBeenCalled()
      expect(deps.refreshGit).not.toHaveBeenCalled()
    })

    it('does not trigger git refresh when state becomes UserInputRequired', async () => {
      const store = createAnalyzerStore('tab-1', deps)
      store.setState({ aiState: ActivityState.Working })
      store.setState({ aiState: ActivityState.UserInputRequired })
      await new Promise(r => setTimeout(r, 0))
      expect(deps.refreshGitInfo).not.toHaveBeenCalled()
      expect(deps.refreshGit).not.toHaveBeenCalled()
    })

    it('does not trigger git refresh when state becomes Working', async () => {
      const store = createAnalyzerStore('tab-1', deps)
      store.setState({ aiState: ActivityState.Idle })
      store.setState({ aiState: ActivityState.Working })
      await new Promise(r => setTimeout(r, 0))
      expect(deps.refreshGitInfo).not.toHaveBeenCalled()
      expect(deps.refreshGit).not.toHaveBeenCalled()
    })
  })

  it('stops polling on TTY exit', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      openTtyStream: makeTtyStreamMock(mock, []),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)

    // Simulate PTY exit
    mock.emitExit(0)

    // Data should not trigger analysis after exit
    mock.emitData('some data')
    vi.advanceTimersByTime(1000)
    await vi.advanceTimersByTimeAsync(0)

    expect(deps.llm.analyzeTerminal).not.toHaveBeenCalled()

    vi.useRealTimers()
  })
  it('reclassifies an unchanged buffer when provider, safe paths or criteria change, without changing the title model', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    const settings = makeDeps().getSettings()
    const deps = makeDeps({ getSettings: () => settings, openTtyStream: makeTtyStreamMock(mock) })
    const store = createAnalyzerStore('tab-1', deps)
    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)
    mock.emitData('$ ')
    await vi.advanceTimersByTimeAsync(1000)
    expect(deps.llm.analyzeTerminal).toHaveBeenLastCalledWith(expect.any(String), '/test', expect.objectContaining({ provider: ClassifierProvider.ChatCompletions, model: 'test-model' }))

    settings.terminalAnalyzer.provider = ClassifierProvider.Classifier
    await vi.advanceTimersByTimeAsync(1000)
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(2)
    expect(deps.llm.analyzeTerminal).toHaveBeenLastCalledWith(expect.any(String), '/test', expect.objectContaining({ provider: ClassifierProvider.Classifier, model: 'test-model' }))
    const call = vi.mocked(deps.llm.analyzeTerminal).mock.calls[1]!
    expect(call[2]).not.toHaveProperty('reasoningEffort')

    settings.terminalAnalyzer.safePaths = ['/different']
    await vi.advanceTimersByTimeAsync(1000)
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(3)

    settings.terminalAnalyzer.criteria = { ...settings.terminalAnalyzer.criteria, idle: 'Edited idle' }
    await vi.advanceTimersByTimeAsync(1000)
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(4)
    expect(deps.llm.analyzeTerminal).toHaveBeenLastCalledWith(expect.any(String), '/test', expect.objectContaining({ criteria: expect.objectContaining({ idle: 'Edited idle' }) as unknown }))
    await store.getState().refreshTitleAndDescription()
    expect(deps.llm.generateTitle).toHaveBeenCalledWith(expect.any(String), expect.objectContaining({ model: 'test-model' }))
    expect(store.getState().getHistory().at(-1)).toMatchObject({ kind: 'title', model: 'test-model' })
    store.getState().stop()
    vi.useRealTimers()
  })

  it('waits for the unread idle debounce while the workspace carries an unread marker', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    const hasUnreadAttention = vi.fn().mockReturnValue(false)
    const deps = makeDeps({ hasUnreadAttention, openTtyStream: makeTtyStreamMock(mock) })
    const store = createAnalyzerStore('tab-1', deps)
    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)
    await settle(deps, mock)
    hasUnreadAttention.mockReturnValue(true)
    const settledState = store.getState().aiState
    mock.emitData('$ ')
    await vi.advanceTimersByTimeAsync(14999)
    expect(store.getState().aiState).toBe(settledState)
    expect(deps.llm.analyzeTerminal).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(1)
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    // Once acknowledged, the next burst uses the short debounce again.
    hasUnreadAttention.mockReturnValue(false)
    mock.emitData('$ ls')
    await vi.advanceTimersByTimeAsync(500)
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(2)
    store.getState().stop()
    vi.useRealTimers()
  })

  it('neither marks Working nor reclassifies for a blip that restores the screen', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    const deps = makeDeps({ openTtyStream: makeTtyStreamMock(mock) })
    const store = createAnalyzerStore('tab-1', deps)
    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)
    await settle(deps, mock)
    mock.emitData('x')
    await vi.advanceTimersByTimeAsync(100)
    mock.emitData('\b\x1b[K')
    await vi.advanceTimersByTimeAsync(5000)
    expect(deps.setActivityTabState).not.toHaveBeenCalled()
    expect(deps.llm.analyzeTerminal).not.toHaveBeenCalled()
    store.getState().stop()
    vi.useRealTimers()
  })

  it('applies an in-flight response when a blip restores the requested screen before it lands', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    let resolve: (value: { state: ActivityState; reason: string }) => void = () => { throw new Error('request not started') }
    const analyzeTerminal = vi.fn().mockImplementationOnce(() => new Promise(r => { resolve = r }))
    const deps = makeDeps({ openTtyStream: makeTtyStreamMock(mock), llm: { ...makeDeps().llm, analyzeTerminal } })
    const store = createAnalyzerStore('tab-1', deps)
    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)
    mock.emitData('Continue?')
    await vi.advanceTimersByTimeAsync(1000)
    expect(store.getState().analyzing).toBe(true)
    mock.emitData('x')
    await vi.advanceTimersByTimeAsync(100)
    mock.emitData('\b\x1b[K')
    await vi.advanceTimersByTimeAsync(0)
    resolve({ state: ActivityState.UserInputRequired, reason: 'asks to continue' })
    await vi.advanceTimersByTimeAsync(5000)
    expect(analyzeTerminal).toHaveBeenCalledTimes(1)
    expect(store.getState().aiState).toBe(ActivityState.UserInputRequired)
    expect(store.getState().analyzing).toBe(false)
    store.getState().stop()
    vi.useRealTimers()
  })

  it('discards a safe-permission response after a provider switch before the next poll', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    const settings = makeDeps().getSettings()
    let resolve: (value: { state: ActivityState; reason: string }) => void = () => { throw new Error('request not started') }
    const analyzeTerminal = vi.fn().mockImplementationOnce(() => new Promise(r => { resolve = r }))
      .mockResolvedValue({ state: ActivityState.PermissionRequest, reason: 'Classifier decision: permission_request' })
    const deps = makeDeps({ getSettings: () => settings, openTtyStream: makeTtyStreamMock(mock), llm: { ...makeDeps().llm, analyzeTerminal } })
    const store = createAnalyzerStore('tab-1', deps)
    store.getState().setAutoApprove(true)
    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)
    mock.emitData('Allow mutation?')
    await vi.advanceTimersByTimeAsync(1000)
    expect(store.getState().analyzing).toBe(true)
    settings.terminalAnalyzer.provider = ClassifierProvider.Classifier
    resolve({ state: ActivityState.SafePermissionRequested, reason: 'stale' })
    await vi.advanceTimersByTimeAsync(0)
    expect(analyzeTerminal).toHaveBeenCalledTimes(2)
    expect(store.getState().aiState).toBe(ActivityState.PermissionRequest)
    expect(store.getState().reason).toBe('Classifier decision: permission_request')
    expect(store.getState().analyzing).toBe(false)
    // eslint-disable-next-line @typescript-eslint/unbound-method -- mocked TTY method
    expect(mock.ttyState.write).not.toHaveBeenCalled()
    expect(deps.setActivityTabState).not.toHaveBeenCalledWith('tab-1', ActivityState.SafePermissionRequested, expect.objectContaining({ kind: ActivityTransitionKind.Classification }))
    expect(store.getState().getHistory()[0]).toMatchObject({ error: '[discarded]' })
    store.getState().stop()
    vi.useRealTimers()
  })

  it.each(['malformed', 'safe_permission_requested_typo', 'permission_request'])('real Jev adapter handles %s without auto-approval', async (choice) => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    const settings = makeDeps().getSettings()
    settings.terminalAnalyzer.provider = ClassifierProvider.Classifier
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValue(new Response(JSON.stringify({ answers: { activity_state: { type: 'choice', choice } } }), { status: 200 }))
    const llm = createLlmClient({ fetch: fetchMock, completeChat: vi.fn(), parseChatJson: parseLlmJson })
    await llm.clearAnalyzerCache()
    const deps = makeDeps({ getSettings: () => settings, openTtyStream: makeTtyStreamMock(mock), llm })
    const store = createAnalyzerStore('tab-1', deps)
    store.getState().setAutoApprove(true)
    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)
    mock.emitData('Allow action?')
    await vi.advanceTimersByTimeAsync(1000)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(store.getState().aiState).toBe(choice === 'permission_request' ? ActivityState.PermissionRequest : ActivityState.Error)
    if (choice === 'permission_request') expect(store.getState().reason).toBe('Classifier decision: permission_request')
    expect(store.getState().reason).not.toBe('')
    expect(store.getState().analyzing).toBe(false)
    // eslint-disable-next-line @typescript-eslint/unbound-method -- mocked TTY method
    expect(mock.ttyState.write).not.toHaveBeenCalled()
    store.getState().stop()
    vi.useRealTimers()
  })

})
