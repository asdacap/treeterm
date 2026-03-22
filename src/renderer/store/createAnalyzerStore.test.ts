import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createAnalyzerStore } from './createAnalyzerStore'
import type { AnalyzerDeps } from './createAnalyzerStore'
import type { Settings } from '../types'

function makeMockTerminal() {
  const lines = ['$ echo hello', 'hello', '$ ']
  return {
    buffer: {
      normal: {
        baseY: 0,
        cursorY: lines.length - 1,
        getLine: (i: number) => lines[i] ? { translateToString: () => lines[i] } : null,
      },
    },
  } as any
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
      generateTitle: vi.fn().mockResolvedValue({ title: 'Test Title', description: 'Test Description' }),
    },
    updateMetadata: vi.fn(),
    getDisplayName: vi.fn().mockReturnValue(undefined),
    getDescription: vi.fn().mockReturnValue(undefined),
    setActivityTabState: vi.fn(),
    getTty: vi.fn().mockReturnValue(null),
    getPtyId: vi.fn().mockReturnValue(null),
    cwd: '/test',
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

  it('getBufferText returns null when not attached', () => {
    const store = createAnalyzerStore('tab-1', deps)
    expect(store.getState().getBufferText()).toBeNull()
  })

  it('getBufferText returns buffer text when attached', () => {
    const store = createAnalyzerStore('tab-1', deps)
    const terminal = makeMockTerminal()

    store.getState().attach(terminal, { current: 0 })
    const text = store.getState().getBufferText()

    expect(text).toBe('$ echo hello\nhello\n$ ')
  })

  it('getBufferText returns null for empty buffer', () => {
    const store = createAnalyzerStore('tab-1', deps)
    const terminal = {
      buffer: {
        normal: {
          baseY: 0,
          cursorY: 0,
          getLine: () => ({ translateToString: () => '' }),
        },
      },
    } as any

    store.getState().attach(terminal, { current: 0 })
    expect(store.getState().getBufferText()).toBeNull()
  })

  it('detach resets terminal reference', () => {
    const store = createAnalyzerStore('tab-1', deps)
    const terminal = makeMockTerminal()

    store.getState().attach(terminal, { current: 0 })
    expect(store.getState().getBufferText()).not.toBeNull()

    store.getState().detach()
    expect(store.getState().getBufferText()).toBeNull()
  })

  it('attach starts polling when settings are configured', () => {
    vi.useFakeTimers()
    const store = createAnalyzerStore('tab-1', deps)
    const terminal = makeMockTerminal()
    const dvRef = { current: 0 }

    store.getState().attach(terminal, dvRef)

    // Simulate data version change
    dvRef.current = 1
    vi.advanceTimersByTime(200) // poll interval

    expect(store.getState().aiState).toBe('working')

    store.getState().detach()
    vi.useRealTimers()
  })

  it('attach does not poll when settings are missing', () => {
    vi.useFakeTimers()
    deps = makeDeps({
      getSettings: vi.fn().mockReturnValue({
        llm: { apiKey: '', baseUrl: '' },
        terminalAnalyzer: { model: '', systemPrompt: '', titleSystemPrompt: '', reasoningEffort: 'off', safePaths: [], bufferLines: 10 },
      } as unknown as Settings),
    })
    const store = createAnalyzerStore('tab-1', deps)
    const terminal = makeMockTerminal()
    const dvRef = { current: 0 }

    store.getState().attach(terminal, dvRef)
    dvRef.current = 1
    vi.advanceTimersByTime(200)

    expect(store.getState().aiState).toBe('idle')

    store.getState().detach()
    vi.useRealTimers()
  })

  it('analyze calls llm.analyzeTerminal and updates state', async () => {
    vi.useFakeTimers()
    const store = createAnalyzerStore('tab-1', deps)
    const terminal = makeMockTerminal()
    const dvRef = { current: 0 }

    store.getState().attach(terminal, dvRef)

    dvRef.current = 1
    vi.advanceTimersByTime(200) // poll fires
    vi.advanceTimersByTime(500) // debounce fires

    // Let the async analyze() complete
    await vi.advanceTimersByTimeAsync(0)

    expect(deps.llm.analyzeTerminal).toHaveBeenCalled()
    expect(store.getState().aiState).toBe('idle')
    expect(store.getState().reason).toBe('prompt visible')
    expect(store.getState().analyzing).toBe(false)
    expect(deps.setActivityTabState).toHaveBeenCalledWith('tab-1', 'idle')

    store.getState().detach()
    vi.useRealTimers()
  })

  it('analyze handles error result', async () => {
    vi.useFakeTimers()
    deps = makeDeps({
      llm: {
        analyzeTerminal: vi.fn().mockResolvedValue({ error: 'API error' }),
        generateTitle: vi.fn().mockResolvedValue({ title: '', description: '' }),
      },
    })
    const store = createAnalyzerStore('tab-1', deps)
    const terminal = makeMockTerminal()
    const dvRef = { current: 0 }

    store.getState().attach(terminal, dvRef)
    dvRef.current = 1
    vi.advanceTimersByTime(700)
    await vi.advanceTimersByTimeAsync(0)

    expect(store.getState().aiState).toBe('error')
    expect(store.getState().analyzing).toBe(false)

    store.getState().detach()
    vi.useRealTimers()
  })

  it('analyze handles LLM call failure', async () => {
    vi.useFakeTimers()
    deps = makeDeps({
      llm: {
        analyzeTerminal: vi.fn().mockRejectedValue(new Error('Network error')),
        generateTitle: vi.fn().mockResolvedValue({ title: '', description: '' }),
      },
    })
    const store = createAnalyzerStore('tab-1', deps)
    const terminal = makeMockTerminal()
    const dvRef = { current: 0 }

    store.getState().attach(terminal, dvRef)
    dvRef.current = 1
    vi.advanceTimersByTime(700)
    await vi.advanceTimersByTimeAsync(0)

    expect(store.getState().aiState).toBe('error')

    store.getState().detach()
    vi.useRealTimers()
  })

  it('dedup skips analysis when same buffer is in-flight', async () => {
    vi.useFakeTimers()
    const store = createAnalyzerStore('tab-1', deps)
    const terminal = makeMockTerminal()
    const dvRef = { current: 0 }

    // Make analyzeTerminal slow
    let resolveAnalysis!: (value: any) => void
    ;(deps.llm.analyzeTerminal as any).mockImplementation(() => new Promise(r => { resolveAnalysis = r }))

    store.getState().attach(terminal, dvRef)

    // First analysis
    dvRef.current = 1
    vi.advanceTimersByTime(700)

    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    // Second tick, same buffer (buffer content hasn't changed)
    dvRef.current = 2
    vi.advanceTimersByTime(700)

    // Should skip because same buffer is in-flight
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    resolveAnalysis({ state: 'idle', reason: '' })
    await vi.advanceTimersByTimeAsync(0)

    store.getState().detach()
    vi.useRealTimers()
  })

  it('dedup reuses cached result for unchanged buffer', async () => {
    vi.useFakeTimers()
    const store = createAnalyzerStore('tab-1', deps)
    const terminal = makeMockTerminal()
    const dvRef = { current: 0 }

    store.getState().attach(terminal, dvRef)

    // First analysis
    dvRef.current = 1
    vi.advanceTimersByTime(700)
    await vi.advanceTimersByTimeAsync(0)

    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    // Second analysis, same buffer
    dvRef.current = 2
    vi.advanceTimersByTime(700)
    await vi.advanceTimersByTimeAsync(0)

    // Should reuse cached result, not call LLM again
    expect(deps.llm.analyzeTerminal).toHaveBeenCalledTimes(1)

    store.getState().detach()
    vi.useRealTimers()
  })

  describe('onUserInput', () => {
    it('triggers title generation on first Enter key', async () => {
      const store = createAnalyzerStore('tab-1', deps)
      const terminal = makeMockTerminal()

      store.getState().attach(terminal, { current: 0 })
      store.getState().onUserInput('hello\r')

      // Wait for async title generation
      await vi.waitFor(() => {
        expect(deps.llm.generateTitle).toHaveBeenCalled()
      })
      await vi.waitFor(() => {
        expect(deps.updateMetadata).toHaveBeenCalledWith('displayName', 'Test Title')
      })
      await vi.waitFor(() => {
        expect(deps.updateMetadata).toHaveBeenCalledWith('description', 'Test Description')
      })

      store.getState().detach()
    })

    it('does not trigger title generation on non-Enter input', () => {
      const store = createAnalyzerStore('tab-1', deps)
      const terminal = makeMockTerminal()

      store.getState().attach(terminal, { current: 0 })
      store.getState().onUserInput('hello')

      expect(deps.llm.generateTitle).not.toHaveBeenCalled()
    })

    it('does not trigger title generation twice', async () => {
      const store = createAnalyzerStore('tab-1', deps)
      const terminal = makeMockTerminal()

      store.getState().attach(terminal, { current: 0 })
      store.getState().onUserInput('\r')

      await vi.waitFor(() => {
        expect(deps.llm.generateTitle).toHaveBeenCalledTimes(1)
      })

      store.getState().onUserInput('\r')
      // Still only called once
      expect(deps.llm.generateTitle).toHaveBeenCalledTimes(1)

      store.getState().detach()
    })

    it('does not generate title when displayName and description already exist', () => {
      deps = makeDeps({
        getDisplayName: vi.fn().mockReturnValue('Existing Title'),
        getDescription: vi.fn().mockReturnValue('Existing Description'),
      })
      const store = createAnalyzerStore('tab-1', deps)
      const terminal = makeMockTerminal()

      store.getState().attach(terminal, { current: 0 })
      store.getState().onUserInput('\r')

      expect(deps.llm.generateTitle).not.toHaveBeenCalled()

      store.getState().detach()
    })

    it('generates description even when displayName already exists', async () => {
      deps = makeDeps({
        getDisplayName: vi.fn().mockReturnValue('Existing Title'),
        getDescription: vi.fn().mockReturnValue(undefined),
      })
      const store = createAnalyzerStore('tab-1', deps)
      const terminal = makeMockTerminal()

      store.getState().attach(terminal, { current: 0 })
      store.getState().onUserInput('\r')

      await vi.waitFor(() => {
        expect(deps.llm.generateTitle).toHaveBeenCalled()
      })
      await vi.waitFor(() => {
        expect(deps.updateMetadata).toHaveBeenCalledWith('description', 'Test Description')
      })
      // Should not overwrite existing displayName
      expect(deps.updateMetadata).not.toHaveBeenCalledWith('displayName', expect.anything())

      store.getState().detach()
    })
  })

  describe('auto-approve', () => {
    it('auto-approves safe permission requests', () => {
      const mockTty = {
        getState: vi.fn().mockReturnValue({ write: vi.fn() }),
      }
      deps = makeDeps({
        getTty: vi.fn().mockReturnValue(mockTty),
        getPtyId: vi.fn().mockReturnValue('pty-1'),
      })

      const store = createAnalyzerStore('tab-1', deps)
      store.getState().setAutoApprove(true)

      // Simulate state change to safe_permission_requested
      store.setState({ aiState: 'safe_permission_requested' })

      expect(mockTty.getState().write).toHaveBeenCalledWith('\r')
    })

    it('does not auto-approve when autoApprove is false', () => {
      const mockTty = {
        getState: vi.fn().mockReturnValue({ write: vi.fn() }),
      }
      deps = makeDeps({
        getTty: vi.fn().mockReturnValue(mockTty),
        getPtyId: vi.fn().mockReturnValue('pty-1'),
      })

      const store = createAnalyzerStore('tab-1', deps)
      // autoApprove defaults to false

      store.setState({ aiState: 'safe_permission_requested' })

      expect(mockTty.getState().write).not.toHaveBeenCalled()
    })

    it('does not auto-approve for non-safe states', () => {
      const mockTty = {
        getState: vi.fn().mockReturnValue({ write: vi.fn() }),
      }
      deps = makeDeps({
        getTty: vi.fn().mockReturnValue(mockTty),
        getPtyId: vi.fn().mockReturnValue('pty-1'),
      })

      const store = createAnalyzerStore('tab-1', deps)
      store.getState().setAutoApprove(true)

      store.setState({ aiState: 'permission_request' })

      expect(mockTty.getState().write).not.toHaveBeenCalled()
    })
  })

  it('activity state sync calls setActivityTabState during analysis', async () => {
    vi.useFakeTimers()
    const store = createAnalyzerStore('tab-1', deps)
    const terminal = makeMockTerminal()
    const dvRef = { current: 0 }

    store.getState().attach(terminal, dvRef)

    dvRef.current = 1
    vi.advanceTimersByTime(200) // poll detects change
    // 'working' state set via updateAiState
    expect(deps.setActivityTabState).toHaveBeenCalledWith('tab-1', 'working')

    vi.advanceTimersByTime(500) // debounce fires analyze
    await vi.advanceTimersByTimeAsync(0) // resolve async
    // 'idle' state set after analysis completes
    expect(deps.setActivityTabState).toHaveBeenCalledWith('tab-1', 'idle')

    store.getState().detach()
    vi.useRealTimers()
  })
})
