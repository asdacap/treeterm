/* eslint-disable custom/no-string-literal-comparison -- TODO: migrate existing string-literal comparisons to enums */
import { createStore } from 'zustand/vanilla'
import type { StoreApi } from 'zustand'
import { Terminal } from '@xterm/xterm'
import { ActivityState } from '../types'
import type { LlmApi, Settings, PtyEvent } from '../types'
import { PtyEventType } from '../../shared/ipc-types'
import { DisposableStore, thenRegisterOrDispose } from '../../shared/lifecycle'
import type { Tty } from './createTtyStore'

export interface AnalyzerDeps {
  getSettings: () => Settings
  llm: LlmApi
  updateMetadata: (key: string, value: string, reason: string) => void
  getDisplayName: () => string | undefined
  getDescription: () => string | undefined
  setActivityTabState: (tabId: string, state: ActivityState) => void
  openTtyStream: (ptyId: string, onEvent: (event: PtyEvent) => void) => Promise<Tty>
  cwd: string
  renameBranch: (oldName: string, newName: string) => Promise<void>
  getGitBranch: () => string | undefined
  getBranchIsUserDefined: () => boolean
  getParentId: () => string | undefined
  refreshGitInfo: () => Promise<void>
  refreshGit: () => Promise<void>
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

export enum TitleRefreshStatus {
  Success = 'success',
  Failure = 'failure',
}

/** Outcome of a manual LLM re-label. Failures carry a message fit to show the user. */
export type TitleRefreshResult =
  | { status: TitleRefreshStatus.Success }
  | { status: TitleRefreshStatus.Failure; error: string }

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

  // On-demand regeneration (triggered manually, e.g. from a context menu).
  // Force-overwrite the title/description, ignoring whether they are already set.
  refreshTitleAndDescription(): Promise<TitleRefreshResult>
  // Force-rename the git branch from the LLM suggestion and mark it user-defined.
  refreshBranchName(): Promise<TitleRefreshResult>

  // Auto-approve control
  setAutoApprove(value: boolean): void

  // Debug support
  getBufferText(): string | null
  getHistory(): AnalyzerHistoryEntry[]
}

export type Analyzer = StoreApi<AnalyzerState>

type AnalyzerResult = { state: string; reason: string }
type TitleQueryResult =
  | { status: TitleRefreshStatus.Success; title: string; description: string; branchName: string }
  | { status: TitleRefreshStatus.Failure; error: string }
type BufferCheckResult =
  | { action: 'skip' }
  | { action: 'reuse'; result: AnalyzerResult }
  | { action: 'analyze' }

function isValidBranchName(name: string): boolean {
  return /^[a-z0-9]+(-[a-z0-9]+)*$/.test(name)
}

// Delay after the user presses Enter before capturing the terminal screen for the
// LLM. Gives the command time to run and render its output so the title/branch
// suggestion reflects what actually happened, not the bare prompt.
const TITLE_GENERATION_DELAY_MS = 1000

export function createAnalyzerStore(tabId: string, deps: AnalyzerDeps): Analyzer {
  // Internal closure state — not part of Zustand state
  let terminal: Terminal | null = null
  let ownTty: Tty | null = null
  let dataVersion = 0
  let lastVersion = 0
  let pollInterval: ReturnType<typeof setInterval> | null = null
  let debounceTimer: ReturnType<typeof setTimeout> | null = null
  let titleTimer: ReturnType<typeof setTimeout> | null = null
  /** Owns the TTY attachment for the current start()/stop() cycle. */
  let streamOwner = new DisposableStore()
  let running = false
  let titleGenerated = false

  // Dedup buffer (inlined from TerminalAnalyzerBuffer)
  let inFlightBuffer: string | null = null
  const CACHE_SIZE = 10
  const cache: { buffer: string; result: AnalyzerResult }[] = []
  let requestInFlight = false
  let pendingAnalyze = false
  let modelErrorShown = false

  // History log
  const history: AnalyzerHistoryEntry[] = []
  const MAX_HISTORY = 1000

  function checkBuffer(buffer: string): BufferCheckResult {
    if (buffer === inFlightBuffer) {
      return { action: 'skip' }
    }
    const idx = cache.findIndex((entry) => entry.buffer === buffer)
    if (idx !== -1) {
      // eslint-disable-next-line @typescript-eslint/no-non-null-assertion -- idx from findIndex, guaranteed valid
      const hit = cache[idx]!
      cache.splice(idx, 1)
      cache.push(hit)
      return { action: 'reuse', result: hit.result }
    }
    return { action: 'analyze' }
  }

  function extractBuffer(): string | null {
    if (!terminal) return null
    const xtermBuffer = terminal.buffer.normal
    const startLine = xtermBuffer.baseY
    const endLine = xtermBuffer.baseY + terminal.rows
    const lines: string[] = []
    for (let i = startLine; i < endLine; i++) {
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

    if (!settings.terminalAnalyzer.model) {
      if (!modelErrorShown) {
        modelErrorShown = true
        store.setState({ analyzing: false })
        updateAiState(ActivityState.Error, 'Terminal analyzer model not configured')
      }
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

      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition
      if (!running) return

      if (dataVersion !== requestVersion) {
        console.debug('[terminal-analyzer] discarding stale response')
        history.push({ timestamp: Date.now(), kind: 'analyzer', model: settings.terminalAnalyzer.model, bufferText: buffer, response: JSON.stringify(result), error: '[discarded]', systemPrompt, durationMs })
        if (history.length > MAX_HISTORY) history.shift()
        inFlightBuffer = null
        store.setState({ analyzing: false })
        if (pendingAnalyze) {
          pendingAnalyze = false
          void analyze()
        }
        return
      }

      if ('state' in result) {
        console.debug('[terminal-analyzer] state set:', result.state, 'reason:', result.reason)
        cache.push({ buffer, result: { state: result.state, reason: result.reason } })
        if (cache.length > CACHE_SIZE) {
          cache.shift()
        }
        inFlightBuffer = null
        store.setState({ analyzing: false })
        updateAiState(result.state as ActivityState, result.reason)
        history.push({ timestamp: Date.now(), kind: 'analyzer', model: settings.terminalAnalyzer.model, bufferText: buffer, response: JSON.stringify(result), cached: result.cached, systemPrompt, durationMs })
        if (history.length > MAX_HISTORY) history.shift()
      } else {
        console.error('[terminal-analyzer] error:', result.error)
        inFlightBuffer = null
        store.setState({ analyzing: false })
        updateAiState(ActivityState.Error)
        history.push({ timestamp: Date.now(), kind: 'analyzer', model: settings.terminalAnalyzer.model, bufferText: buffer, response: JSON.stringify(result), error: result.error, systemPrompt, durationMs })
        if (history.length > MAX_HISTORY) history.shift()
      }

      if (pendingAnalyze) {
        pendingAnalyze = false
        void analyze()
      }
    } catch (err) {
      requestInFlight = false
      inFlightBuffer = null
      console.error('[terminal-analyzer] LLM call failed:', err)
      store.setState({ analyzing: false })
      updateAiState(ActivityState.Error)
      history.push({ timestamp: Date.now(), kind: 'analyzer', model: settings.terminalAnalyzer.model, bufferText: buffer, response: '', error: err instanceof Error ? err.message : String(err) })
      if (history.length > MAX_HISTORY) history.shift()
      if (pendingAnalyze) {
        pendingAnalyze = false
        void analyze()
      }
    }
  }

  function startPolling(): void {
    if (pollInterval) return
    running = true

    pollInterval = setInterval(() => {
      if (dataVersion === lastVersion) return

      lastVersion = dataVersion
      updateAiState(ActivityState.Working)

      if (debounceTimer) clearTimeout(debounceTimer)
      debounceTimer = setTimeout(() => { void analyze(); }, 500)
    }, 500)
  }

  function stopPolling(): void {
    running = false
    // Releases the TTY event subscription. Before ownership moved onto the Tty this was
    // a `() => void` the dependency type had silently erased, so it never ran.
    streamOwner.dispose()
    if (pollInterval) {
      clearInterval(pollInterval)
      pollInterval = null
    }
    if (debounceTimer) {
      clearTimeout(debounceTimer)
      debounceTimer = null
    }
    if (titleTimer) {
      clearTimeout(titleTimer)
      titleTimer = null
    }
    if (terminal) {
      terminal.dispose()
      terminal = null
    }
    ownTty = null
    // Reset dedup state
    inFlightBuffer = null
    cache.length = 0
    dataVersion = 0
    lastVersion = 0
    requestInFlight = false
    pendingAnalyze = false
    modelErrorShown = false
  }

  // Runs the LLM title/description/branch generation against the current terminal
  // buffer and records the call in history. Applying the result (which fields to
  // write, with which guards) is left to the caller.
  async function requestTitleResult(): Promise<TitleQueryResult> {
    const settings = deps.getSettings()
    if (!settings.terminalAnalyzer.model) {
      return { status: TitleRefreshStatus.Failure, error: 'Terminal analyzer model not configured' }
    }

    const buffer = extractBuffer()
    if (!buffer) {
      return { status: TitleRefreshStatus.Failure, error: 'Terminal is empty — nothing for the labeller to read' }
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
      const error = 'error' in result ? result.error : undefined
      history.push({ timestamp: Date.now(), kind: 'title', model: settings.terminalAnalyzer.model, bufferText: buffer, response: JSON.stringify(result), error, systemPrompt, durationMs })
      if (history.length > MAX_HISTORY) history.shift()
      if ('title' in result && result.title) {
        return { status: TitleRefreshStatus.Success, title: result.title, description: result.description, branchName: result.branchName }
      }
      return { status: TitleRefreshStatus.Failure, error: error ?? 'LLM returned no title' }
    } catch (err) {
      const error = err instanceof Error ? err.message : String(err)
      console.error('[analyzer] title generation failed:', err)
      history.push({ timestamp: Date.now(), kind: 'title', model: settings.terminalAnalyzer.model, bufferText: buffer, response: '', error })
      if (history.length > MAX_HISTORY) history.shift()
      return { status: TitleRefreshStatus.Failure, error }
    }
  }

  // Renames the git branch to the LLM-suggested name when it is valid and the
  // workspace is a worktree with a current branch.
  async function applyBranchName(branchName: string): Promise<TitleRefreshResult> {
    // `branchName` is typed but LLM-sourced: an absent key arrives as undefined at
    // runtime, and "undefined" happens to satisfy isValidBranchName.
    if (!branchName || !isValidBranchName(branchName)) {
      return { status: TitleRefreshStatus.Failure, error: `LLM suggested an invalid branch name: "${branchName}"` }
    }
    const currentBranch = deps.getGitBranch()
    if (!currentBranch || !deps.getParentId()) {
      return { status: TitleRefreshStatus.Failure, error: 'Workspace has no branch to rename' }
    }
    try {
      await deps.renameBranch(currentBranch, branchName)
      return { status: TitleRefreshStatus.Success }
    } catch (err) {
      const error = err instanceof Error ? err.message : String(err)
      console.error('[analyzer] branch rename failed:', err)
      return { status: TitleRefreshStatus.Failure, error }
    }
  }

  // Automatic generation: triggered on first user input. Only fills in fields that
  // are not already set, and only renames the branch if the user hasn't defined it.
  async function generateTitle(): Promise<void> {
    if (!deps.getParentId()) return
    if (deps.getDisplayName() && deps.getDescription()) return

    if (!extractBuffer()) {
      titleGenerated = false
      return
    }

    const result = await requestTitleResult()
    if (result.status === TitleRefreshStatus.Failure) {
      console.warn('[analyzer] automatic title generation failed:', result.error)
      return
    }

    if (!deps.getDisplayName()) {
      deps.updateMetadata('displayName', result.title, 'analyzerSetDisplayName')
    }
    if (!deps.getDescription() && result.description) {
      deps.updateMetadata('description', result.description, 'analyzerSetDescription')
      deps.updateMetadata('descriptionPrompted', 'true', 'analyzerSetDescriptionPrompted')
    }
    if (!deps.getBranchIsUserDefined()) {
      const renamed = await applyBranchName(result.branchName)
      if (renamed.status === TitleRefreshStatus.Failure) {
        console.warn('[analyzer] automatic branch rename skipped:', renamed.error)
      }
    }
  }

  function handleAutoApprove(): void {
    const state = store.getState()
    if (state.aiState !== ActivityState.SafePermissionRequested || !state.autoApprove) return
    if (!ownTty) return
    ownTty.getState().write('\r').catch((err: unknown) => {
      console.error('[analyzer] auto-approve write failed:', err)
    })
  }

  const store = createStore<AnalyzerState>()((set) => ({
    tabId,
    aiState: ActivityState.Idle,
    analyzing: false,
    reason: '',
    autoApprove: false,

    start: (ptyId: string): void => {
      if (running) return

      // Create headless xterm (no DOM attachment needed)
      terminal = new Terminal()
      streamOwner = new DisposableStore()

      // The daemon replays scrollback as Data events after attach, and an
      // already-exited PTY arrives as an Exit event — both land in onEvent below.
      void thenRegisterOrDispose(deps.openTtyStream(ptyId, (event) => {
        switch (event.type) {
          case PtyEventType.Data:
            terminal?.write(event.data)
            dataVersion++
            break
          case PtyEventType.Exit:
            store.getState().stop()
            break
          case PtyEventType.Resize:
            terminal?.resize(event.cols, event.rows)
            break
        }
      }), streamOwner).then((tty) => {
        // stop() ran while attach was in flight; `tty` is already disposed.
        if (streamOwner.isDisposed) return
        ownTty = tty
      }).catch((err: unknown) => {
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
        // Wait for the command to run and render before capturing the screen.
        titleTimer = setTimeout(() => {
          titleTimer = null
          void generateTitle()
        }, TITLE_GENERATION_DELAY_MS)
      }
    },

    refreshTitleAndDescription: async (): Promise<TitleRefreshResult> => {
      const result = await requestTitleResult()
      if (result.status === TitleRefreshStatus.Failure) return result

      deps.updateMetadata('displayName', result.title, 'manualRefreshTitle')
      if (result.description) {
        deps.updateMetadata('description', result.description, 'manualRefreshDescription')
        deps.updateMetadata('descriptionPrompted', 'true', 'manualRefreshDescriptionPrompted')
      }
      return { status: TitleRefreshStatus.Success }
    },

    refreshBranchName: async (): Promise<TitleRefreshResult> => {
      const result = await requestTitleResult()
      if (result.status === TitleRefreshStatus.Failure) return result

      const renamed = await applyBranchName(result.branchName)
      if (renamed.status === TitleRefreshStatus.Failure) return renamed

      deps.updateMetadata('branchIsUserDefined', 'true', 'manualRefreshBranch')
      return { status: TitleRefreshStatus.Success }
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

  // Subscribe to own state changes for auto-approve and git refresh
  let prevAiState: ActivityState = ActivityState.Idle
  store.subscribe((state) => {
    if (state.aiState !== prevAiState) {
      prevAiState = state.aiState
      handleAutoApprove()
      if (state.aiState === ActivityState.Idle || state.aiState === ActivityState.Completed) {
        void deps.refreshGitInfo().catch(() => {})
        void deps.refreshGit().catch(() => {})
      }
    }
  })

  return store
}
