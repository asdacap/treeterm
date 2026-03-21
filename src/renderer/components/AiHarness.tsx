import { useCallback, useMemo, useRef, useState } from 'react'
import { useStore } from 'zustand'
import type { Terminal as XTerm } from '@xterm/xterm'
import BaseTerminal, { type BaseTerminalConfig, type BaseTerminalState } from './BaseTerminal'
import PushToTalkButton from './PushToTalkButton'
import { ReviewCommentsButton } from './ReviewCommentsButton'
import { useSessionApi } from '../contexts/SessionStoreContext'
import { useTtyCreation } from '../hooks/useTtyConnection'
import { useTerminalAnalyzer, type TerminalAiState } from '../hooks/useTerminalAnalyzer'
import type { SandboxConfig, WorkspaceStore } from '../types'

const STATE_COLORS: Record<TerminalAiState, string> = {
  idle: '#666',
  working: '#2472c8',
  user_input_required: '#e5e510',
  permission_request: '#cd6600',
  safe_permission_requested: '#0dbc79',
  completed: '#23d18b'
}

const STATE_LABELS: Record<TerminalAiState, string> = {
  idle: 'idle',
  working: 'working',
  user_input_required: 'input required',
  permission_request: 'permission request',
  safe_permission_requested: 'safe permission',
  completed: 'completed'
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
  const ptyId = (appState?.state as BaseTerminalState | undefined)?.ptyId

  const [terminal, setTerminal] = useState<XTerm | null>(null)
  const dataVersionRefHolder = useRef<React.MutableRefObject<number> | null>(null)
  const [dataVersionReady, setDataVersionReady] = useState(false)

  const onCreated = useCallback((newPtyId: string) => {
    updateTabState<BaseTerminalState>(tabId, (state) => ({
      ...state,
      ptyId: newPtyId,
    }))
  }, [tabId, updateTabState])

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

  const aiState = useTerminalAnalyzer(
    terminal,
    dataVersionReady ? dataVersionRefHolder.current : null,
    cwd
  )

  // Memoize config based on props to prevent unnecessary re-renders
  const config = useMemo<BaseTerminalConfig>(() => ({
    themeBackground: backgroundColor,
    promptPatterns: [/❯\s/], // Common AI tool prompt pattern
    logPrefix: 'AiHarness',
    disableScrollbar,
    stripScrollbackClear,
    onTerminalReady: handleTerminalReady
  }), [backgroundColor, disableScrollbar, stripScrollbackClear, handleTerminalReady])

  if (loading) {
    return <div style={{ padding: 16, color: '#888' }}>Starting AI harness...</div>
  }

  if (error) {
    return <div style={{ padding: 16, color: '#f14c4c' }}>Failed to start AI harness: {error.message}</div>
  }

  return (
    <div className="ai-harness-wrapper" style={{ position: 'relative', width: '100%', height: '100%' }}>
      <BaseTerminal
        workspace={workspace}
        tabId={tabId}
        isVisible={isVisible}
        config={config}
      />
      {aiState && (
        <div
          className="ai-state-badge"
          style={{
            background: STATE_COLORS[aiState] ?? '#666',
          }}
        >
          {STATE_LABELS[aiState] ?? aiState}
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
