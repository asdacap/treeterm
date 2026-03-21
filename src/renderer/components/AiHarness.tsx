import { useCallback, useMemo } from 'react'
import { useStore } from 'zustand'
import BaseTerminal, { type BaseTerminalConfig, type BaseTerminalState } from './BaseTerminal'
import PushToTalkButton from './PushToTalkButton'
import { ReviewCommentsButton } from './ReviewCommentsButton'
import { useSessionApi } from '../contexts/SessionStoreContext'
import { useTtyCreation } from '../hooks/useTtyConnection'
import type { SandboxConfig, WorkspaceStore } from '../types'

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

  // Memoize config based on props to prevent unnecessary re-renders
  const config = useMemo<BaseTerminalConfig>(() => ({
    themeBackground: backgroundColor,
    promptPatterns: [/❯\s/], // Common AI tool prompt pattern
    logPrefix: 'AiHarness',
    disableScrollbar,
    stripScrollbackClear
  }), [backgroundColor, disableScrollbar, stripScrollbackClear])

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
