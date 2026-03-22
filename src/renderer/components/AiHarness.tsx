import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useStore } from 'zustand'
import type { Terminal as XTerm } from '@xterm/xterm'
import BaseTerminal, { type BaseTerminalConfig, type BaseTerminalState } from './BaseTerminal'
import PushToTalkButton from './PushToTalkButton'
import { ReviewCommentsButton } from './ReviewCommentsButton'
import { useSessionApi } from '../contexts/SessionStoreContext'
import { useTtyCreation } from '../hooks/useTtyConnection'
import { useTerminalAnalyzer } from '../hooks/useTerminalAnalyzer'
import { useActivityStateStore } from '../store/activityState'
import { useSettingsStore } from '../store/settings'
import type { ActivityState, AiHarnessState, SandboxConfig, WorkspaceStore } from '../types'
import { clampContextMenuPosition } from '../utils/contextMenuPosition'

const STATE_COLORS: Record<ActivityState, string> = {
  idle: '#666',
  working: '#2472c8',
  user_input_required: '#e5e510',
  permission_request: '#cd6600',
  safe_permission_requested: '#0dbc79',
  completed: '#23d18b',
  error: '#f44747'
}

const STATE_LABELS: Record<ActivityState, string> = {
  idle: 'idle',
  working: 'working',
  user_input_required: 'input required',
  permission_request: 'permission request',
  safe_permission_requested: 'safe permission',
  completed: 'completed',
  error: 'error'
}

interface AiHarnessProps {
  cwd: string
  workspace: WorkspaceStore
  tabId: string
  sandbox?: SandboxConfig
  isVisible?: boolean
  command: string
  backgroundColor: string
  disableScrollbar?: boolean
  stripScrollbackClear?: boolean
}

export default function AiHarness({
  cwd,
  workspace,
  tabId,
  sandbox,
  isVisible,
  command,
  backgroundColor,
  disableScrollbar,
  stripScrollbackClear,
}: AiHarnessProps) {
  const sessionStore = useSessionApi()
  const { workspace: wsData, updateTabState } = useStore(workspace)
  const appState = wsData?.appStates[tabId]
  const state = appState?.state as AiHarnessState | undefined
  const ptyId = state?.ptyId ?? null
  const aiState = state?.aiState ?? 'idle'
  const analyzing = state?.analyzing ?? false
  const reason = state?.reason ?? ''
  const autoApprove = state?.autoApprove ?? false

  const [terminal, setTerminal] = useState<XTerm | null>(null)
  const dataVersionRefHolder = useRef<React.MutableRefObject<number> | null>(null)
  const [dataVersionReady, setDataVersionReady] = useState(false)

  const onCreated = useCallback((newPtyId: string) => {
    const connId = sessionStore.getState().connection?.id ?? 'local'
    updateTabState<BaseTerminalState>(tabId, (state) => ({
      ...state,
      ptyId: newPtyId,
      connectionId: connId,
    }))
  }, [tabId, updateTabState, sessionStore])

  const { loading, error } = useTtyCreation(ptyId, cwd, sandbox, command, onCreated)

  const handlePushToTalkTranscript = useCallback((text: string) => {
    if (ptyId) {
      const tty = sessionStore.getState().getTty(ptyId)
      if (tty) tty.getState().write(text)
    }
  }, [ptyId, sessionStore])

  const handlePushToTalkSubmit = useCallback(() => {
    if (ptyId) {
      const tty = sessionStore.getState().getTty(ptyId)
      if (tty) tty.getState().write('\r')
    }
  }, [ptyId, sessionStore])

  const handleTerminalReady = useCallback((term: XTerm, dvRef: React.MutableRefObject<number>) => {
    setTerminal(term)
    dataVersionRefHolder.current = dvRef
    setDataVersionReady(true)
  }, [])

  useTerminalAnalyzer(
    terminal,
    dataVersionReady ? dataVersionRefHolder.current : null,
    cwd,
    updateTabState,
    tabId
  )

  // Sync LLM analyzer state to activity state store so TabActivityIndicator matches
  const setActivityTabState = useActivityStateStore((s) => s.setTabState)
  useEffect(() => {
    setActivityTabState(tabId, aiState)
  }, [tabId, aiState, setActivityTabState])

  // Auto-generate workspace title on first successful analyzer return
  const titleGeneratedRef = useRef(false)
  useEffect(() => {
    if (titleGeneratedRef.current) return
    if (aiState === 'idle' || aiState === 'error' || !aiState) return
    if (!terminal) return
    const ws = workspace.getState().workspace
    if (ws.metadata?.displayName) return

    const s = useSettingsStore.getState().settings
    if (!s.llm.apiKey || !s.terminalAnalyzer.model) return

    titleGeneratedRef.current = true

    const numLines = s.terminalAnalyzer.bufferLines || 10
    const xtermBuffer = terminal.buffer.normal
    const contentEnd = xtermBuffer.baseY + xtermBuffer.cursorY + 1
    const startLine = Math.max(0, contentEnd - numLines)
    const lines: string[] = []
    for (let i = startLine; i < contentEnd; i++) {
      const line = xtermBuffer.getLine(i)
      if (line) lines.push(line.translateToString(true))
    }
    const buffer = lines.join('\n')
    if (!buffer.trim()) { titleGeneratedRef.current = false; return }

    window.electron.llm.generateTitle(buffer, {
      baseUrl: s.llm.baseUrl,
      apiKey: s.llm.apiKey,
      model: s.terminalAnalyzer.model,
      titleSystemPrompt: s.terminalAnalyzer.titleSystemPrompt,
      reasoningEffort: s.terminalAnalyzer.reasoningEffort
    }).then((result) => {
      if ('title' in result && result.title) {
        workspace.getState().updateMetadata('displayName', result.title)
      }
    }).catch((err) => {
      console.error('[ai-harness] title generation failed:', err)
    })
  }, [aiState, terminal, workspace])

  const settings = useSettingsStore((s) => s.settings)
  const setAutoApprove = useCallback((value: boolean) => {
    updateTabState<AiHarnessState>(tabId, (state) => ({ ...state, autoApprove: value }))
  }, [tabId, updateTabState])
  const autoApproveSentRef = useRef(false)
  const [badgeContextMenu, setBadgeContextMenu] = useState<{ x: number; y: number } | null>(null)

  // Auto-approve safe permission requests
  useEffect(() => {
    if (aiState === 'safe_permission_requested' && autoApprove && !autoApproveSentRef.current) {
      autoApproveSentRef.current = true
      if (ptyId) {
        const tty = sessionStore.getState().getTty(ptyId)
        if (tty) tty.getState().write('\r')
      }
    }
    if (aiState !== 'safe_permission_requested') {
      autoApproveSentRef.current = false
    }
  }, [aiState, autoApprove, ptyId, sessionStore])

  const handleBadgeContextMenu = (e: React.MouseEvent) => {
    e.preventDefault()
    e.stopPropagation()
    setBadgeContextMenu(clampContextMenuPosition(e.clientX, e.clientY))
  }

  // Close badge context menu on outside click
  useEffect(() => {
    if (!badgeContextMenu) return
    const handleClick = (e: MouseEvent) => {
      const target = e.target as HTMLElement
      if (target.closest('.context-menu')) return
      setBadgeContextMenu(null)
    }
    document.addEventListener('click', handleClick, true)
    return () => document.removeEventListener('click', handleClick, true)
  }, [badgeContextMenu])

  const handleDebugAnalyzer = () => {
    setBadgeContextMenu(null)
    if (!terminal) return

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

    workspace.getState().addTab<{ bufferText: string }>('system-prompt-debugger', { bufferText: buffer })
  }

  // Memoize config based on props to prevent unnecessary re-renders
  const config = useMemo<BaseTerminalConfig>(() => ({
    themeBackground: backgroundColor,
    promptPatterns: [/❯\s/], // Common AI tool prompt pattern
    logPrefix: 'AiHarness',
    disableScrollbar,
    stripScrollbackClear,
    disableActivityDetector: true,
    onTerminalReady: handleTerminalReady
  }), [backgroundColor, disableScrollbar, stripScrollbackClear, handleTerminalReady])

  if (loading) {
    return <div style={{ padding: 16, color: '#888' }}>Starting AI harness...</div>
  }

  if (error) {
    return <div style={{ padding: 16, color: '#f14c4c' }}>Failed to start AI harness: {error.message}</div>
  }

  return (
    <div className="ai-harness-wrapper">
      <div className="ai-harness-terminal">
        <BaseTerminal
          workspace={workspace}
          tabId={tabId}
          isVisible={isVisible}
          config={config}
        />
      </div>
      <div className="ai-harness-status-bar">
        <div
          className="ai-state-badge"
          style={{ background: STATE_COLORS[aiState] }}
          title={reason}
          onContextMenu={handleBadgeContextMenu}
        >
          {analyzing && <span className="ai-state-badge-spinner" />}
          {STATE_LABELS[aiState]}
        </div>
        <label className="ai-harness-toggle">
          <input
            type="checkbox"
            checked={autoApprove}
            onChange={(e) => setAutoApprove(e.target.checked)}
          />
          <span className="ai-harness-toggle-slider" />
          <span className="ai-harness-toggle-label">Auto-approve safe</span>
        </label>
      </div>
      {badgeContextMenu && (
        <div
          className="context-menu"
          style={{ top: badgeContextMenu.y, left: badgeContextMenu.x }}
        >
          <div className="context-menu-item" onClick={handleDebugAnalyzer}>
            Debug System Prompt
          </div>
        </div>
      )}
      <PushToTalkButton
        onTranscript={handlePushToTalkTranscript}
        onSubmit={handlePushToTalkSubmit}
      />
      <ReviewCommentsButton
        workspace={workspace}
      />
    </div>
  )
}
