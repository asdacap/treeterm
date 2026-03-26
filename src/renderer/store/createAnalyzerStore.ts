import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import { Terminal } from '@xterm/xterm'
import type { ActivityState, LlmApi, Settings } from '../types'
import type { Tty } from './createTtyStore'

export interface AnalyzerDeps {
  getSettings: () => Settings
  llm: Pick<LlmApi, 'analyzeTerminal' | 'generateTitle'>
  updateMetadata: (key: string, value: string) => void
  getDisplayName: () => string | undefined
  getDescription: () => string | undefined
  setActivityTabState: (tabId: string, state: ActivityState) => void
  openTtyStream: (ptyId: string) => Promise<{ tty: Tty; scrollback?: string[]; exitCode?: number }>
  cwd: string
  renameBranch: (oldName: string, newName: string) => Promise<void>
  getGitBranch: () => string | null
  getBranchIsUserDefined: () => boolean
  getParentId: () => string | null
  refreshGitInfo: () => Promise<void>
}

export interface AnalyzerHistoryEntry {
  timestamp: number
  kind: 'analyzer' | 'title'
  model: string
  bufferText: string
  response: string
  cached?: boolean
  error?: string
  systemPrompt?: string
  durationMs?: number
}

export interface AnalyzerState {
  tabId: string
  aiState: ActivityState
  analyzing: boolean
  reason: string
  autoApprove: boolean

  // Lifecycle
  start(ptyId: string): void
  stop(): void

  // Called by component when user types in the terminal
  onUserInput(data: string): void

  // Auto-approve control
  setAutoApprove(value: boolean): void

  // Debug support
  getBufferText(): string | null
  getHistory(): AnalyzerHistoryEntry[]
}

export type Analyzer = StoreApi<AnalyzerState>

type AnalyzerResult = { state: string; reason: string }
type BufferCheckResult =
  | { action: 'skip' }
  | { action: 'reuse'; result: AnalyzerResult }
  | { action: 'analyze' }

function isValidBranchName(name: string): boolean {
  return /^[a-z0-9]+(-[a-z0-9]+)*$/.test(name)
}

export function createAnalyzerStore(tabId: string, deps: AnalyzerDeps): Analyzer {
  // Internal closure state — not part of Zustand state
  let terminal: Terminal | null = null
  let ownTty: Tty | null = null
  let dataVersion = 0
  let lastVersion = 0
  let pollInterval: ReturnType<typeof setInterval> | null = null
  let debounceTimer: ReturnType<typeof setTimeout> | null = null
  let unsubscribeEvents: (() => void) | null = null
  let running = false
  let titleGenerated = false

  // Dedup buffer (inlined from TerminalAnalyzerBuffer)
  let inFlightBuffer: string | null = null
  let lastAnalyzedBuffer: string | null = null
  let lastResult: AnalyzerResult | null = null
  let requestInFlight = false
  let pendingAnalyze = false

  // History log
  const history: AnalyzerHistoryEntry[] = []
  const MAX_HISTORY = 1000

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
    if (!running) return

    // Only one request at a time — buffer pending work
    if (requestInFlight) {
      pendingAnalyze = true
      return
    }

    const requestVersion = dataVersion
    const settings = deps.getSettings()

    if (!settings.llm.apiKey || !settings.terminalAnalyzer.model) {
      store.setState({ analyzing: false })
      updateAiState('idle')
      return
    }

    const buffer = extractBuffer()
    if (!buffer) return

    try {

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
      requestInFlight = true
      store.setState({ analyzing: true })

      const startTime = Date.now()
      const result = await deps.llm.analyzeTerminal(buffer, deps.cwd, {
        baseUrl: settings.llm.baseUrl,
        apiKey: settings.llm.apiKey,
        model: settings.terminalAnalyzer.model,
        systemPrompt: settings.terminalAnalyzer.systemPrompt,
        reasoningEffort: settings.terminalAnalyzer.reasoningEffort,
        safePaths: settings.terminalAnalyzer.safePaths,
      })
      const durationMs = Date.now() - startTime
      const systemPrompt = 'systemPrompt' in result ? result.systemPrompt : undefined

      requestInFlight = false

      if (!running) return

      if (dataVersion !== requestVersion) {
        console.debug('[terminal-analyzer] discarding stale response')
        history.push({ timestamp: Date.now(), kind: 'analyzer', model: settings.terminalAnalyzer.model, bufferText: buffer, response: JSON.stringify(result), error: '[discarded]', systemPrompt, durationMs })
        if (history.length > MAX_HISTORY) history.shift()
        inFlightBuffer = null
        store.setState({ analyzing: false })
        if (pendingAnalyze) {
          pendingAnalyze = false
          analyze()
        }
        return
      }

      if ('state' in result) {
        console.debug('[terminal-analyzer] state set:', result.state, 'reason:', result.reason)
        lastAnalyzedBuffer = buffer
        lastResult = { state: result.state, reason: result.reason }
        inFlightBuffer = null
        store.setState({ analyzing: false })
        updateAiState(result.state as ActivityState, result.reason)
        history.push({ timestamp: Date.now(), kind: 'analyzer', model: settings.terminalAnalyzer.model, bufferText: buffer, response: JSON.stringify(result), cached: result.cached, systemPrompt, durationMs })
        if (history.length > MAX_HISTORY) history.shift()
      } else if ('error' in result) {
        console.error('[terminal-analyzer] error:', result.error)
        inFlightBuffer = null
        store.setState({ analyzing: false })
        updateAiState('error')
        history.push({ timestamp: Date.now(), kind: 'analyzer', model: settings.terminalAnalyzer.model, bufferText: buffer, response: JSON.stringify(result), error: result.error, systemPrompt, durationMs })
        if (history.length > MAX_HISTORY) history.shift()
      } else {
        console.debug('[terminal-analyzer] ignored (no state in result)')
        inFlightBuffer = null
        store.setState({ analyzing: false })
        history.push({ timestamp: Date.now(), kind: 'analyzer', model: settings.terminalAnalyzer.model, bufferText: buffer, response: JSON.stringify(result), error: '[unexpected] no state in result', systemPrompt, durationMs })
        if (history.length > MAX_HISTORY) history.shift()
      }

      if (pendingAnalyze) {
        pendingAnalyze = false
        analyze()
      }
    } catch (err) {
      requestInFlight = false
      console.error('[terminal-analyzer] LLM call failed:', err)
      store.setState({ analyzing: false })
      updateAiState('error')
      history.push({ timestamp: Date.now(), kind: 'analyzer', model: settings.terminalAnalyzer.model, bufferText: buffer, response: '', error: err instanceof Error ? err.message : String(err) })
      if (history.length > MAX_HISTORY) history.shift()
      if (pendingAnalyze) {
        pendingAnalyze = false
        analyze()
      }
    }
  }

  function startPolling(): void {
    if (pollInterval) return
    running = true

    pollInterval = setInterval(() => {
      if (dataVersion === lastVersion) return

      lastVersion = dataVersion
      updateAiState('working')

      if (debounceTimer) clearTimeout(debounceTimer)
      debounceTimer = setTimeout(analyze, 500)
    }, 500)
  }

  function stopPolling(): void {
    running = false
    if (unsubscribeEvents) {
      unsubscribeEvents()
      unsubscribeEvents = null
    }
    if (pollInterval) {
      clearInterval(pollInterval)
      pollInterval = null
    }
    if (debounceTimer) {
      clearTimeout(debounceTimer)
      debounceTimer = null
    }
    if (terminal) {
      terminal.dispose()
      terminal = null
    }
    ownTty = null
    // Reset dedup state
    inFlightBuffer = null
    lastAnalyzedBuffer = null
    lastResult = null
    dataVersion = 0
    lastVersion = 0
    requestInFlight = false
    pendingAnalyze = false
  }

  async function generateTitle(): Promise<void> {
    const settings = deps.getSettings()
    if (!settings.llm.apiKey || !settings.terminalAnalyzer.model) return
    if (deps.getDisplayName() && deps.getDescription()) return

    const buffer = extractBuffer()
    if (!buffer) {
      titleGenerated = false
      return
    }

    try {
      const startTime = Date.now()
      const result = await deps.llm.generateTitle(buffer, {
        baseUrl: settings.llm.baseUrl,
        apiKey: settings.llm.apiKey,
        model: settings.terminalAnalyzer.model,
        titleSystemPrompt: settings.terminalAnalyzer.titleSystemPrompt,
        reasoningEffort: settings.terminalAnalyzer.reasoningEffort,
      })
      const durationMs = Date.now() - startTime
      const systemPrompt = 'systemPrompt' in result ? result.systemPrompt : undefined

      if ('title' in result && result.title) {
        if (!deps.getDisplayName()) {
          deps.updateMetadata('displayName', result.title)
        }
        if (!deps.getDescription() && result.description) {
          deps.updateMetadata('description', result.description)
          deps.updateMetadata('descriptionPrompted', 'true')
        }
        if (result.branchName && isValidBranchName(result.branchName) && deps.getGitBranch() && !deps.getBranchIsUserDefined() && deps.getParentId()) {
          try {
            await deps.renameBranch(deps.getGitBranch()!, result.branchName)
          } catch (err) {
            console.error('[analyzer] branch rename failed:', err)
          }
        }
      }
      history.push({ timestamp: Date.now(), kind: 'title', model: settings.terminalAnalyzer.model, bufferText: buffer, response: JSON.stringify(result), systemPrompt, durationMs })
      if (history.length > MAX_HISTORY) history.shift()
    } catch (err) {
      console.error('[analyzer] title generation failed:', err)
      history.push({ timestamp: Date.now(), kind: 'title', model: settings.terminalAnalyzer.model, bufferText: buffer, response: '', error: err instanceof Error ? err.message : String(err) })
      if (history.length > MAX_HISTORY) history.shift()
    }
  }

  function handleAutoApprove(): void {
    const state = store.getState()
    if (state.aiState !== 'safe_permission_requested' || !state.autoApprove) return
    if (!ownTty) return
    ownTty.getState().write('\r')
  }

  const store = createStore<AnalyzerState>()((set, get) => ({
    tabId,
    aiState: 'idle',
    analyzing: false,
    reason: '',
    autoApprove: false,

    start: (ptyId: string): void => {
      if (running) return

      // Create headless xterm (no DOM attachment needed)
      terminal = new Terminal()

      deps.openTtyStream(ptyId).then(({ tty, scrollback, exitCode }) => {
        if (!running && !terminal) return // stopped before stream opened

        ownTty = tty

        // Restore scrollback into headless terminal
        if (scrollback) {
          for (const chunk of scrollback) {
            terminal!.write(chunk)
          }
          dataVersion++ // trigger initial analysis from scrollback
        }

        // If already exited, don't start polling
        if (exitCode !== undefined) return

        // Subscribe to live events
        unsubscribeEvents = tty.getState().onEvent((event) => {
          switch (event.type) {
            case 'data':
              terminal?.write(event.data)
              dataVersion++
              break
            case 'exit':
              store.getState().stop()
              break
            case 'resize':
              terminal?.resize(event.cols, event.rows)
              break
          }
        })
      }).catch((err) => {
        console.error('[analyzer] failed to open TTY stream:', err)
      })

      startPolling()
    },

    stop: (): void => {
      stopPolling()
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

    getHistory: (): AnalyzerHistoryEntry[] => {
      return [...history]
    },
  }))

  // Subscribe to own state changes for auto-approve
  let prevAiState: ActivityState = 'idle'
  store.subscribe((state) => {
    if (state.aiState !== prevAiState) {
      prevAiState = state.aiState
      handleAutoApprove()
      deps.refreshGitInfo().catch(() => {})
    }
  })

  return store
}
