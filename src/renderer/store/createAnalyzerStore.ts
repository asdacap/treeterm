import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import type { Terminal as XTerm } from '@xterm/xterm'
import type { ActivityState, LlmApi, Settings } from '../types'
import type { Tty } from './createTtyStore'

export interface AnalyzerDeps {
  getSettings: () => Settings
  llm: Pick<LlmApi, 'analyzeTerminal' | 'generateTitle'>
  updateMetadata: (key: string, value: string) => void
  getDisplayName: () => string | undefined
  setActivityTabState: (tabId: string, state: ActivityState) => void
  getTty: (ptyId: string) => Tty | null
  getPtyId: () => string | null
  cwd: string
}

export interface AnalyzerState {
  tabId: string
  aiState: ActivityState
  analyzing: boolean
  reason: string
  autoApprove: boolean

  // Lifecycle
  attach(terminal: XTerm, dataVersionRef: { current: number }): void
  detach(): void

  // Called by component when user types in the terminal
  onUserInput(data: string): void

  // Auto-approve control
  setAutoApprove(value: boolean): void

  // Debug support
  getBufferText(): string | null
}

export type Analyzer = StoreApi<AnalyzerState>

type AnalyzerResult = { state: string; reason: string }
type BufferCheckResult =
  | { action: 'skip' }
  | { action: 'reuse'; result: AnalyzerResult }
  | { action: 'analyze' }

export function createAnalyzerStore(tabId: string, deps: AnalyzerDeps): Analyzer {
  // Internal closure state — not part of Zustand state
  let terminal: XTerm | null = null
  let dataVersionRef: { current: number } | null = null
  let lastVersion = 0
  let pollInterval: ReturnType<typeof setInterval> | null = null
  let debounceTimer: ReturnType<typeof setTimeout> | null = null
  let running = false
  let titleGenerated = false

  // Dedup buffer (inlined from TerminalAnalyzerBuffer)
  let inFlightBuffer: string | null = null
  let lastAnalyzedBuffer: string | null = null
  let lastResult: AnalyzerResult | null = null

  function checkBuffer(buffer: string): BufferCheckResult {
    if (buffer === inFlightBuffer) {
      return { action: 'skip' }
    }
    if (buffer === lastAnalyzedBuffer && lastResult !== null) {
      return { action: 'reuse', result: lastResult }
    }
    return { action: 'analyze' }
  }

  function extractBuffer(): string | null {
    if (!terminal) return null
    const settings = deps.getSettings()
    const numLines = settings.terminalAnalyzer.bufferLines || 10
    const xtermBuffer = terminal.buffer.normal
    const contentEnd = xtermBuffer.baseY + xtermBuffer.cursorY + 1
    const startLine = Math.max(0, contentEnd - numLines)
    const lines: string[] = []
    for (let i = startLine; i < contentEnd; i++) {
      const line = xtermBuffer.getLine(i)
      if (line) lines.push(line.translateToString(true))
    }
    const buffer = lines.join('\n')
    return buffer.trim() ? buffer : null
  }

  function updateAiState(aiState: ActivityState, reason?: string): void {
    store.setState((s) => ({
      ...s,
      aiState,
      ...(reason !== undefined ? { reason } : {}),
    }))
    deps.setActivityTabState(tabId, aiState)
  }

  async function analyze(): Promise<void> {
    if (!running || !dataVersionRef) return

    const requestVersion = dataVersionRef.current
    const settings = deps.getSettings()

    if (!settings.llm.apiKey || !settings.terminalAnalyzer.model) return

    try {
      const buffer = extractBuffer()
      if (!buffer) return

      const checkResult = checkBuffer(buffer)
      if (checkResult.action === 'skip') {
        console.debug('[terminal-analyzer] skipping, same buffer in-flight')
        return
      }
      if (checkResult.action === 'reuse') {
        console.debug('[terminal-analyzer] reusing cached result for unchanged buffer')
        store.setState({ analyzing: false })
        updateAiState(checkResult.result.state as ActivityState, checkResult.result.reason)
        return
      }

      console.debug('[terminal-analyzer] buffer:', buffer)
      inFlightBuffer = buffer
      store.setState({ analyzing: true })

      const result = await deps.llm.analyzeTerminal(buffer, deps.cwd, {
        baseUrl: settings.llm.baseUrl,
        apiKey: settings.llm.apiKey,
        model: settings.terminalAnalyzer.model,
        systemPrompt: settings.terminalAnalyzer.systemPrompt,
        reasoningEffort: settings.terminalAnalyzer.reasoningEffort,
        safePaths: settings.terminalAnalyzer.safePaths,
      })

      if (!running) return

      if (dataVersionRef.current !== requestVersion) {
        console.debug('[terminal-analyzer] discarding stale response')
        inFlightBuffer = null
        store.setState({ analyzing: false })
        return
      }

      if ('state' in result) {
        console.debug('[terminal-analyzer] state set:', result.state, 'reason:', result.reason)
        lastAnalyzedBuffer = buffer
        lastResult = { state: result.state, reason: result.reason }
        inFlightBuffer = null
        store.setState({ analyzing: false })
        updateAiState(result.state as ActivityState, result.reason)
      } else if ('error' in result) {
        console.error('[terminal-analyzer] error:', result.error)
        inFlightBuffer = null
        store.setState({ analyzing: false })
        updateAiState('error')
      } else {
        console.debug('[terminal-analyzer] ignored (no state in result)')
        inFlightBuffer = null
        store.setState({ analyzing: false })
      }
    } catch (err) {
      console.error('[terminal-analyzer] LLM call failed:', err)
      store.setState({ analyzing: false })
      updateAiState('error')
    }
  }

  function startPolling(): void {
    if (pollInterval) return
    running = true
    pollInterval = setInterval(() => {
      if (!dataVersionRef || dataVersionRef.current === lastVersion) return

      lastVersion = dataVersionRef.current
      updateAiState('working')

      if (debounceTimer) clearTimeout(debounceTimer)
      debounceTimer = setTimeout(analyze, 500)
    }, 200)
  }

  function stopPolling(): void {
    running = false
    if (pollInterval) {
      clearInterval(pollInterval)
      pollInterval = null
    }
    if (debounceTimer) {
      clearTimeout(debounceTimer)
      debounceTimer = null
    }
    // Reset dedup state
    inFlightBuffer = null
    lastAnalyzedBuffer = null
    lastResult = null
    lastVersion = 0
  }

  async function generateTitle(): Promise<void> {
    const settings = deps.getSettings()
    if (!settings.llm.apiKey || !settings.terminalAnalyzer.model) return
    if (deps.getDisplayName()) return

    const buffer = extractBuffer()
    if (!buffer) {
      titleGenerated = false
      return
    }

    try {
      const result = await deps.llm.generateTitle(buffer, {
        baseUrl: settings.llm.baseUrl,
        apiKey: settings.llm.apiKey,
        model: settings.terminalAnalyzer.model,
        titleSystemPrompt: settings.terminalAnalyzer.titleSystemPrompt,
        reasoningEffort: settings.terminalAnalyzer.reasoningEffort,
      })

      if ('title' in result && result.title) {
        deps.updateMetadata('displayName', result.title)
      }
    } catch (err) {
      console.error('[analyzer] title generation failed:', err)
    }
  }

  function handleAutoApprove(): void {
    const state = store.getState()
    if (state.aiState !== 'safe_permission_requested' || !state.autoApprove) return

    const ptyId = deps.getPtyId()
    if (!ptyId) return
    const tty = deps.getTty(ptyId)
    if (!tty) return
    tty.getState().write('\r')
  }

  const store = createStore<AnalyzerState>()((set, get) => ({
    tabId,
    aiState: 'idle',
    analyzing: false,
    reason: '',
    autoApprove: false,

    attach: (term: XTerm, dvRef: { current: number }): void => {
      terminal = term
      dataVersionRef = dvRef
      const settings = deps.getSettings()
      if (settings.llm.apiKey && settings.terminalAnalyzer.model) {
        startPolling()
      }
    },

    detach: (): void => {
      stopPolling()
      terminal = null
      dataVersionRef = null
      titleGenerated = false
    },

    onUserInput: (data: string): void => {
      if (titleGenerated) return
      if (data.includes('\r')) {
        titleGenerated = true
        generateTitle()
      }
    },

    setAutoApprove: (value: boolean): void => {
      set({ autoApprove: value })
    },

    getBufferText: (): string | null => {
      return extractBuffer()
    },
  }))

  // Subscribe to own state changes for auto-approve
  let prevAiState: ActivityState = 'idle'
  store.subscribe((state) => {
    if (state.aiState !== prevAiState) {
      prevAiState = state.aiState
      handleAutoApprove()
    }
  })

  return store
}
