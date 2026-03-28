import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createAnalyzerStore } from './createAnalyzerStore'
import type { AnalyzerDeps } from './createAnalyzerStore'
import type { Settings } from '../types'
import type { TtyState } from './createTtyStore'
import { createStore } from 'zustand/vanilla'

/** Creates a mock Tty with controllable onEvent callback */
function makeMockTty() {
  type PtyEvent = import('../../shared/ipc-types').PtyEvent
  let eventCallback: ((event: PtyEvent) => void) | null = null

  const ttyState: TtyState = {
    ptyId: 'pty-1',
    write: vi.fn(),
    resize: vi.fn(),
    kill: vi.fn(),
    isAlive: vi.fn().mockResolvedValue(true),
    onEvent: vi.fn((cb) => {
      eventCallback = cb
      return () => { eventCallback = null }
    }),
  }

  const tty = createStore<TtyState>()(() => ttyState)

  return {
    tty,
    ttyState,
    emitData: (data: string) => eventCallback?.({ type: 'data', data: new TextEncoder().encode(data) }),
    emitExit: (code: number) => eventCallback?.({ type: 'exit', exitCode: code }),
  }
}

function makeDeps(overrides?: Partial<AnalyzerDeps>): AnalyzerDeps {
  return {
    getSettings: vi.fn().mockReturnValue({
      llm: { apiKey: 'test-key', baseUrl: 'http://localhost' },
      terminalAnalyzer: {
        model: 'test-model',
        systemPrompt: 'test prompt',
        titleSystemPrompt: 'title prompt',
        reasoningEffort: 'low',
        safePaths: ['/tmp'],
        bufferLines: 10,
      },
    } as unknown as Settings),
    llm: {
      analyzeTerminal: vi.fn().mockResolvedValue({ state: 'idle', reason: 'prompt visible' }),
      generateTitle: vi.fn().mockResolvedValue({ title: 'Test Title', description: 'Test Description', branchName: 'test-title' }),
    },
    updateMetadata: vi.fn(),
    getDisplayName: vi.fn().mockReturnValue(undefined),
    getDescription: vi.fn().mockReturnValue(undefined),
    setActivityTabState: vi.fn(),
    openTtyStream: vi.fn().mockResolvedValue({ tty: makeMockTty().tty, scrollback: [], exitCode: undefined }),
    cwd: '/test',
    renameBranch: vi.fn().mockResolvedValue(undefined),
    getGitBranch: vi.fn().mockReturnValue('old-branch'),
    getBranchIsUserDefined: vi.fn().mockReturnValue(false),
    getParentId: vi.fn().mockReturnValue('parent-1'),
    refreshGitInfo: vi.fn().mockResolvedValue(undefined),
    refreshDiffStatus: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  }
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
    expect(state.aiState).toBe('idle')
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
      openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: ['$ echo hello\r\nhello\r\n$ '], exitCode: undefined }),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.waitFor(() => {
      expect(store.getState().getBufferText()).not.toBeNull()
    })

    store.getState().stop()
    expect(store.getState().getBufferText()).toBeNull()
  })

  it('start opens TTY stream and starts polling when settings are configured', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: [], exitCode: undefined }),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0) // resolve openTtyStream

    // Simulate data arrival
    mock.emitData('$ echo hello\r\nhello\r\n$ ')
    vi.advanceTimersByTime(500) // poll interval

    expect(store.getState().aiState).toBe('working')

    store.getState().stop()
    vi.useRealTimers()
  })

  it('start polls but analyze resets to idle when settings are missing', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      getSettings: vi.fn().mockReturnValue({
        llm: { apiKey: '', baseUrl: '' },
        terminalAnalyzer: { model: '', systemPrompt: '', titleSystemPrompt: '', reasoningEffort: 'off', safePaths: [], bufferLines: 10 },
      } as unknown as Settings),
      openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: [], exitCode: undefined }),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)

    mock.emitData('$ hello')
    await vi.advanceTimersByTimeAsync(500)
    // Poll fires and sets 'working', then schedules analyze
    expect(store.getState().aiState).toBe('working')
    await vi.advanceTimersByTimeAsync(500)
    // analyze() sees missing settings and resets to idle
    expect(store.getState().aiState).toBe('idle')

    store.getState().stop()
    vi.useRealTimers()
  })

  it('analyze calls llm.analyzeTerminal and updates state', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: [], exitCode: undefined }),
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
    expect(store.getState().aiState).toBe('idle')
    expect(store.getState().reason).toBe('prompt visible')
    expect(store.getState().analyzing).toBe(false)
    expect(deps.setActivityTabState).toHaveBeenCalledWith('tab-1', 'idle')

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
      },
      openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: [], exitCode: undefined }),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)

    mock.emitData('$ echo hello\r\nhello\r\n$ ')
    vi.advanceTimersByTime(1000)
    await vi.advanceTimersByTimeAsync(0)

    expect(store.getState().aiState).toBe('error')
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
      },
      openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: [], exitCode: undefined }),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)

    mock.emitData('$ echo hello\r\nhello\r\n$ ')
    vi.advanceTimersByTime(1000)
    await vi.advanceTimersByTimeAsync(0)

    expect(store.getState().aiState).toBe('error')

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
      },
      openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: [], exitCode: undefined }),
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
    calls[0]({ state: 'idle', reason: 'prompt visible' })
    await vi.advanceTimersByTimeAsync(0)

    // Now the pending analyze should have fired
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(2)

    // Resolve second
    calls[1]({ state: 'idle', reason: 'tests passed' })
    await vi.advanceTimersByTimeAsync(0)

    store.getState().stop()
    vi.useRealTimers()
  })

  it('dedup skips analysis when same buffer is in-flight', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()

    let resolveAnalysis!: (value: any) => void
    deps = makeDeps({
      llm: {
        analyzeTerminal: vi.fn().mockImplementation(() => new Promise(r => { resolveAnalysis = r })),
        generateTitle: vi.fn().mockResolvedValue({ title: '', description: '', branchName: '' }),
      },
      openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: [], exitCode: undefined }),
    })

    const store = createAnalyzerStore('tab-1', deps)
    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)

    // First analysis
    mock.emitData('$ echo hello\r\nhello\r\n$ ')
    vi.advanceTimersByTime(1000)

    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    // Second tick with dataVersion change but same buffer content
    mock.emitData('') // empty data just bumps dataVersion
    vi.advanceTimersByTime(1000)

    // Should skip because same buffer is in-flight
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    resolveAnalysis({ state: 'idle', reason: '' })
    await vi.advanceTimersByTimeAsync(0)

    store.getState().stop()
    vi.useRealTimers()
  })

  it('dedup reuses cached result for unchanged buffer', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: [], exitCode: undefined }),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)

    // First analysis
    mock.emitData('$ echo hello\r\nhello\r\n$ ')
    vi.advanceTimersByTime(1000)
    await vi.advanceTimersByTimeAsync(0)

    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    // Second analysis, same buffer (empty data just bumps version)
    mock.emitData('')
    vi.advanceTimersByTime(1000)
    await vi.advanceTimersByTimeAsync(0)

    // Should reuse cached result, not call LLM again
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    store.getState().stop()
    vi.useRealTimers()
  })

  it('restores scrollback into headless terminal on start', async () => {
    const mock = makeMockTty()
    deps = makeDeps({
      openTtyStream: vi.fn().mockResolvedValue({
        tty: mock.tty,
        scrollback: ['$ echo hello\r\nhello\r\n$ '],
        exitCode: undefined,
      }),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.waitFor(() => {
      expect(store.getState().getBufferText()).not.toBeNull()
    })

    expect(store.getState().getBufferText()).toContain('hello')

    store.getState().stop()
  })

  describe('onUserInput', () => {
    it('triggers title generation on first Enter key', async () => {
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: ['$ '], exitCode: undefined }),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.waitFor(() => {
        expect(store.getState().getBufferText()).not.toBeNull()
      })

      store.getState().onUserInput('hello\r')

      await vi.waitFor(() => {
        expect(deps.llm.generateTitle).toHaveBeenCalled()
      })
      await vi.waitFor(() => {
        expect(deps.updateMetadata).toHaveBeenCalledWith('displayName', 'Test Title')
      })
      await vi.waitFor(() => {
        expect(deps.updateMetadata).toHaveBeenCalledWith('description', 'Test Description')
      })

      store.getState().stop()
    })

    it('does not trigger title generation on non-Enter input', async () => {
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: ['$ '], exitCode: undefined }),
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
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: ['$ '], exitCode: undefined }),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.waitFor(() => {
        expect(store.getState().getBufferText()).not.toBeNull()
      })

      store.getState().onUserInput('\r')

      await vi.waitFor(() => {
        expect(deps.llm.generateTitle).toHaveBeenCalledTimes(1)
      })

      store.getState().onUserInput('\r')
      // Still only called once
      expect(deps.llm.generateTitle).toHaveBeenCalledTimes(1)

      store.getState().stop()
    })

    it('does not generate title when displayName and description already exist', async () => {
      const mock = makeMockTty()
      deps = makeDeps({
        getDisplayName: vi.fn().mockReturnValue('Existing Title'),
        getDescription: vi.fn().mockReturnValue('Existing Description'),
        openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: ['$ '], exitCode: undefined }),
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
      const mock = makeMockTty()
      deps = makeDeps({
        getDisplayName: vi.fn().mockReturnValue('Existing Title'),
        getDescription: vi.fn().mockReturnValue(undefined),
        openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: ['$ '], exitCode: undefined }),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.waitFor(() => {
        expect(store.getState().getBufferText()).not.toBeNull()
      })

      store.getState().onUserInput('\r')

      await vi.waitFor(() => {
        expect(deps.llm.generateTitle).toHaveBeenCalled()
      })
      await vi.waitFor(() => {
        expect(deps.updateMetadata).toHaveBeenCalledWith('description', 'Test Description')
      })
      // Should not overwrite existing displayName
      expect(deps.updateMetadata).not.toHaveBeenCalledWith('displayName', expect.anything())

      store.getState().stop()
    })
  })

  describe('auto-approve', () => {
    it('auto-approves safe permission requests via own TTY', async () => {
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: [], exitCode: undefined }),
      })

      const store = createAnalyzerStore('tab-1', deps)
      store.getState().start('pty-1')
      await vi.waitFor(() => {
        expect(deps.openTtyStream).toHaveBeenCalled()
      })
      // Wait for stream to be connected
      await vi.advanceTimersByTimeAsync?.(0).catch(() => {})
      await new Promise(r => setTimeout(r, 0))

      store.getState().setAutoApprove(true)

      // Simulate state change to safe_permission_requested
      store.setState({ aiState: 'safe_permission_requested' })

      expect(mock.ttyState.write).toHaveBeenCalledWith('\r')

      store.getState().stop()
    })

    it('does not auto-approve when autoApprove is false', async () => {
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: [], exitCode: undefined }),
      })

      const store = createAnalyzerStore('tab-1', deps)
      store.getState().start('pty-1')
      await new Promise(r => setTimeout(r, 0))

      store.setState({ aiState: 'safe_permission_requested' })

      expect(mock.ttyState.write).not.toHaveBeenCalled()
      store.getState().stop()
    })

    it('does not auto-approve for non-safe states', async () => {
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: [], exitCode: undefined }),
      })

      const store = createAnalyzerStore('tab-1', deps)
      store.getState().start('pty-1')
      await new Promise(r => setTimeout(r, 0))

      store.getState().setAutoApprove(true)
      store.setState({ aiState: 'permission_request' })

      expect(mock.ttyState.write).not.toHaveBeenCalled()
      store.getState().stop()
    })
  })

  it('activity state sync calls setActivityTabState during analysis', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: [], exitCode: undefined }),
    })
    const store = createAnalyzerStore('tab-1', deps)

    store.getState().start('pty-1')
    await vi.advanceTimersByTimeAsync(0)

    mock.emitData('$ echo hello\r\nhello\r\n$ ')
    vi.advanceTimersByTime(500) // poll detects change
    // 'working' state set via updateAiState
    expect(deps.setActivityTabState).toHaveBeenCalledWith('tab-1', 'working')

    vi.advanceTimersByTime(500) // debounce fires analyze
    await vi.advanceTimersByTimeAsync(0) // resolve async
    // 'idle' state set after analysis completes
    expect(deps.setActivityTabState).toHaveBeenCalledWith('tab-1', 'idle')

    store.getState().stop()
    vi.useRealTimers()
  })

  describe('history logging', () => {
    it('logs successful analysis to history with response', async () => {
      vi.useFakeTimers()
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: [], exitCode: undefined }),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.advanceTimersByTimeAsync(0)

      mock.emitData('$ echo hello\r\nhello\r\n$ ')
      vi.advanceTimersByTime(1000)
      await vi.advanceTimersByTimeAsync(0)

      const history = store.getState().getHistory()
      expect(history).toHaveLength(1)
      expect(history[0].kind).toBe('analyzer')
      expect(history[0].error).toBeUndefined()
      expect(history[0].model).toBe('test-model')
      expect(history[0].response).toBe(JSON.stringify({ state: 'idle', reason: 'prompt visible' }))

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
        },
        openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: [], exitCode: undefined }),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.advanceTimersByTimeAsync(0)

      mock.emitData('$ echo hello\r\nhello\r\n$ ')
      vi.advanceTimersByTime(1000)
      await vi.advanceTimersByTimeAsync(0)

      const history = store.getState().getHistory()
      expect(history).toHaveLength(1)
      expect(history[0].kind).toBe('analyzer')
      expect(history[0].error).toBe('API error')
      expect(history[0].response).toBe(JSON.stringify({ error: 'API error' }))

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
        },
        openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: [], exitCode: undefined }),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.advanceTimersByTimeAsync(0)

      mock.emitData('$ echo hello\r\nhello\r\n$ ')
      vi.advanceTimersByTime(1000)
      await vi.advanceTimersByTimeAsync(0)

      const history = store.getState().getHistory()
      expect(history).toHaveLength(1)
      expect(history[0].kind).toBe('analyzer')
      expect(history[0].error).toBe('Network error')
      expect(history[0].response).toBe('')

      store.getState().stop()
      vi.useRealTimers()
    })

    it('logs unexpected response (no state) to history', async () => {
      vi.useFakeTimers()
      const mock = makeMockTty()
      deps = makeDeps({
        llm: {
          analyzeTerminal: vi.fn().mockResolvedValue({ something: 'unexpected' }),
          generateTitle: vi.fn().mockResolvedValue({ title: '', description: '', branchName: '' }),
        },
        openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: [], exitCode: undefined }),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.advanceTimersByTimeAsync(0)

      mock.emitData('$ echo hello\r\nhello\r\n$ ')
      vi.advanceTimersByTime(1000)
      await vi.advanceTimersByTimeAsync(0)

      const history = store.getState().getHistory()
      expect(history).toHaveLength(1)
      expect(history[0].kind).toBe('analyzer')
      expect(history[0].error).toBe('[unexpected] no state in result')
      expect(history[0].response).toBe(JSON.stringify({ something: 'unexpected' }))

      store.getState().stop()
      vi.useRealTimers()
    })

    it('logs title generation to history', async () => {
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: ['$ '], exitCode: undefined }),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.waitFor(() => {
        expect(store.getState().getBufferText()).not.toBeNull()
      })

      store.getState().onUserInput('hello\r')

      await vi.waitFor(() => {
        expect(deps.llm.generateTitle).toHaveBeenCalled()
      })

      await vi.waitFor(() => {
        const history = store.getState().getHistory()
        expect(history.some(h => h.kind === 'title')).toBe(true)
      })

      const history = store.getState().getHistory()
      const titleEntry = history.find(h => h.kind === 'title')!
      expect(titleEntry.error).toBeUndefined()
      expect(titleEntry.response).toBe(JSON.stringify({ title: 'Test Title', description: 'Test Description', branchName: 'test-title' }))

      store.getState().stop()
    })

    it('skips branch rename when branch is user-defined', async () => {
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: ['$ '], exitCode: undefined }),
        getBranchIsUserDefined: vi.fn().mockReturnValue(true),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.waitFor(() => {
        expect(store.getState().getBufferText()).not.toBeNull()
      })

      store.getState().onUserInput('hello\r')

      await vi.waitFor(() => {
        expect(deps.llm.generateTitle).toHaveBeenCalled()
      })

      await vi.waitFor(() => {
        expect(deps.updateMetadata).toHaveBeenCalledWith('displayName', 'Test Title')
      })

      expect(deps.renameBranch).not.toHaveBeenCalled()

      store.getState().stop()
    })

    it('skips branch rename when workspace has no parent', async () => {
      const mock = makeMockTty()
      deps = makeDeps({
        openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: ['$ '], exitCode: undefined }),
        getParentId: vi.fn().mockReturnValue(null),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.waitFor(() => {
        expect(store.getState().getBufferText()).not.toBeNull()
      })

      store.getState().onUserInput('hello\r')

      await vi.waitFor(() => {
        expect(deps.llm.generateTitle).toHaveBeenCalled()
      })

      await vi.waitFor(() => {
        expect(deps.updateMetadata).toHaveBeenCalledWith('displayName', 'Test Title')
      })

      expect(deps.renameBranch).not.toHaveBeenCalled()

      store.getState().stop()
    })

    it('logs title generation failure to history', async () => {
      const mock = makeMockTty()
      deps = makeDeps({
        llm: {
          analyzeTerminal: vi.fn().mockResolvedValue({ state: 'idle', reason: '' }),
          generateTitle: vi.fn().mockRejectedValue(new Error('Title API error')),
        },
        openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: ['$ '], exitCode: undefined }),
      })
      const store = createAnalyzerStore('tab-1', deps)

      store.getState().start('pty-1')
      await vi.waitFor(() => {
        expect(store.getState().getBufferText()).not.toBeNull()
      })

      store.getState().onUserInput('hello\r')

      await vi.waitFor(() => {
        expect(deps.llm.generateTitle).toHaveBeenCalled()
      })

      await vi.waitFor(() => {
        const history = store.getState().getHistory()
        expect(history.some(h => h.kind === 'title' && h.error)).toBe(true)
      })

      const history = store.getState().getHistory()
      const titleEntry = history.find(h => h.kind === 'title' && h.error)!
      expect(titleEntry.error).toBe('Title API error')
      expect(titleEntry.response).toBe('')

      store.getState().stop()
    })
  })

  it('stops polling on TTY exit', async () => {
    vi.useFakeTimers()
    const mock = makeMockTty()
    deps = makeDeps({
      openTtyStream: vi.fn().mockResolvedValue({ tty: mock.tty, scrollback: [], exitCode: undefined }),
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
})
