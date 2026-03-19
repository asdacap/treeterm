import { useCallback, useMemo } from 'react'
import { useStore } from 'zustand'
import BaseTerminal, { type BaseTerminalConfig, type BaseTerminalState } from './BaseTerminal'
import PushToTalkButton from './PushToTalkButton'
import { ReviewCommentsButton } from './ReviewCommentsButton'
import { useTerminalApi } from '../contexts/TerminalApiContext'
import type { SandboxConfig } from '../types'
import type { StoreApi } from 'zustand'
import type { WorkspaceState } from '../store/createWorkspaceStore'

interface AiHarnessProps {
  cwd: string
  workspaceId: string
  tabId: string
  sandbox?: SandboxConfig
  isVisible?: boolean
  command: string
  backgroundColor: string
  disableScrollbar?: boolean
  stripScrollbackClear?: boolean
  workspaceStore: StoreApi<WorkspaceState>
}

export default function AiHarness({
  cwd,
  workspaceId,
  tabId,
  sandbox,
  isVisible,
  command,
  backgroundColor,
  disableScrollbar,
  stripScrollbackClear,
  workspaceStore
}: AiHarnessProps) {
  const terminalApi = useTerminalApi()
  const workspace = useStore(workspaceStore, (state) => state.workspaces[workspaceId])
  const appState = workspace?.appStates[tabId]
  const ptyId = (appState?.state as BaseTerminalState | undefined)?.ptyId

  const handlePushToTalkTranscript = useCallback((text: string) => {
    if (ptyId) {
      terminalApi.write(ptyId, text)
    }
  }, [ptyId, terminalApi])

  const handlePushToTalkSubmit = useCallback(() => {
    if (ptyId) {
      terminalApi.write(ptyId, '\r')
    }
  }, [ptyId, terminalApi])

  // Memoize config based on props to prevent unnecessary re-renders
  const config = useMemo<BaseTerminalConfig>(() => ({
    themeBackground: backgroundColor,
    promptPatterns: [/❯\s/], // Common AI tool prompt pattern
    startupCommand: command,
    logPrefix: 'AiHarness',
    disableScrollbar,
    stripScrollbackClear
  }), [backgroundColor, command, disableScrollbar, stripScrollbackClear])

  return (
    <div className="ai-harness-wrapper" style={{ position: 'relative', width: '100%', height: '100%' }}>
      <BaseTerminal
        cwd={cwd}
        workspaceId={workspaceId}
        tabId={tabId}
        sandbox={sandbox}
        isVisible={isVisible}
        config={config}
        workspaceStore={workspaceStore}
      />
      <PushToTalkButton
        onTranscript={handlePushToTalkTranscript}
        onSubmit={handlePushToTalkSubmit}
      />
      {ptyId && (
        <ReviewCommentsButton
          workspaceStore={workspaceStore}
          workspaceId={workspaceId}
          ptyId={ptyId}
        />
      )}
    </div>
  )
}
