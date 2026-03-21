import { useCallback, useMemo } from 'react'
import BaseTerminal, { type BaseTerminalConfig, type BaseTerminalState } from './BaseTerminal'
import PushToTalkButton from './PushToTalkButton'
import { ReviewCommentsButton } from './ReviewCommentsButton'
import { useTerminalApi } from '../contexts/TerminalApiContext'
import type { SandboxConfig, WorkspaceHandle } from '../types'

interface AiHarnessProps {
  cwd: string
  workspace: WorkspaceHandle
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
  const terminalApi = useTerminalApi()
  const wsData = workspace.data
  const appState = wsData?.appStates[tabId]
  const ptyHandle = (appState?.state as BaseTerminalState | undefined)?.ptyHandle

  const handlePushToTalkTranscript = useCallback((text: string) => {
    if (ptyHandle) {
      terminalApi.write(ptyHandle, text)
    }
  }, [ptyHandle, terminalApi])

  const handlePushToTalkSubmit = useCallback(() => {
    if (ptyHandle) {
      terminalApi.write(ptyHandle, '\r')
    }
  }, [ptyHandle, terminalApi])

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
        workspace={workspace}
        tabId={tabId}
        sandbox={sandbox}
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
