import { useCallback, useMemo, useRef } from 'react'
import { useStore } from 'zustand'
import type { Terminal as XTerm } from '@xterm/xterm'
import BaseTerminal, { type BaseTerminalConfig, type BaseTerminalState } from './BaseTerminal'
import PushToTalkButton from './PushToTalkButton'
import { ReviewCommentsButton } from './ReviewCommentsButton'
import { useSessionApi } from '../contexts/SessionStoreContext'
import { useTtyCreation } from '../hooks/useTtyConnection'
import type { ActivityState, AiHarnessState, SandboxConfig, WorkspaceStore } from '../types'
import type { Analyzer } from '../store/createAnalyzerStore'
import { useContextMenuStore } from '../store/contextMenu'
import ContextMenu from './ContextMenu'

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

  // Create or get the analyzer store for this tab
  const analyzerRef = useRef<Analyzer | null>(null)
  if (!analyzerRef.current) {
    analyzerRef.current = workspace.getState().getOrCreateAnalyzer(tabId)
  }
  const analyzer = analyzerRef.current

  const { aiState, analyzing, reason, autoApprove } = useStore(analyzer)

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
      const writer = sessionStore.getState().getWriter(ptyId)
      if (writer) writer.write(text)
    }
  }, [ptyId, sessionStore])

  const handlePushToTalkSubmit = useCallback(() => {
    if (ptyId) {
      const writer = sessionStore.getState().getWriter(ptyId)
      if (writer) writer.write('\r')
    }
  }, [ptyId, sessionStore])

  const handleTerminalReady = useCallback((term: XTerm) => {
    // Intercept user input for title generation
    term.onData((data) => {
      analyzer.getState().onUserInput(data)
    })
  }, [analyzer])

  const openContextMenu = useContextMenuStore((s) => s.open)
  const closeContextMenu = useContextMenuStore((s) => s.close)
  const badgeMenuId = `ai-badge-${tabId}`

  const handleBadgeContextMenu = (e: React.MouseEvent) => {
    e.preventDefault()
    e.stopPropagation()
    openContextMenu(badgeMenuId, e.clientX, e.clientY)
  }

  const handleDebugAnalyzer = () => {
    closeContextMenu()
    const bufferText = analyzer.getState().getBufferText()
    if (!bufferText) return
    workspace.getState().addTab<{ bufferText: string }>('system-prompt-debugger', { bufferText })
  }

  const handleViewHistory = () => {
    closeContextMenu()
    workspace.getState().addTab<{ sourceTabId: string }>('analyzer-history', { sourceTabId: tabId })
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
          config={config}
          extraButtons={
            <>
              <ReviewCommentsButton workspace={workspace} />
              <PushToTalkButton
                onTranscript={handlePushToTalkTranscript}
                onSubmit={handlePushToTalkSubmit}
              />
            </>
          }
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
            onChange={(e) => analyzer.getState().setAutoApprove(e.target.checked)}
          />
          <span className="ai-harness-toggle-slider" />
          <span className="ai-harness-toggle-label">Auto-approve safe</span>
        </label>
      </div>
      <ContextMenu menuId={badgeMenuId}>
        <div className="context-menu-item" onClick={handleDebugAnalyzer}>
          Debug System Prompt
        </div>
        <div className="context-menu-item" onClick={handleViewHistory}>
          History
        </div>
      </ContextMenu>
    </div>
  )
}
