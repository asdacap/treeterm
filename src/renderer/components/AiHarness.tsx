import { useMemo } from 'react'
import BaseTerminal, { type BaseTerminalConfig } from './BaseTerminal'
import type { SandboxConfig } from '../types'

interface AiHarnessProps {
  cwd: string
  workspaceId: string
  tabId: string
  sandbox?: SandboxConfig
  isVisible?: boolean
  command: string
  backgroundColor: string
  disableScrollbar?: boolean
}

export default function AiHarness({
  cwd,
  workspaceId,
  tabId,
  sandbox,
  isVisible,
  command,
  backgroundColor,
  disableScrollbar
}: AiHarnessProps) {
  // Memoize config based on props to prevent unnecessary re-renders
  const config = useMemo<BaseTerminalConfig>(() => ({
    themeBackground: backgroundColor,
    promptPatterns: [/❯\s/], // Common AI tool prompt pattern
    startupCommand: command,
    logPrefix: 'AiHarness',
    showPushToTalk: true,
    disableScrollbar
  }), [backgroundColor, command, disableScrollbar])

  return (
    <BaseTerminal
      cwd={cwd}
      workspaceId={workspaceId}
      tabId={tabId}
      sandbox={sandbox}
      isVisible={isVisible}
      config={config}
    />
  )
}
