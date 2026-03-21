import { useMemo } from 'react'
import BaseTerminal, { type BaseTerminalConfig } from './BaseTerminal'
import type { SandboxConfig, WorkspaceStore } from '../types'

interface TerminalProps {
  cwd: string
  workspace: WorkspaceStore
  tabId: string
  startupCommand?: string
  sandbox?: SandboxConfig
  isVisible?: boolean
}

export default function Terminal({ cwd, workspace, tabId, startupCommand, sandbox, isVisible }: TerminalProps) {
  // Build terminal config with optional startup command from props
  const terminalConfig = useMemo<BaseTerminalConfig>(() => ({
    themeBackground: '#1e1e1e',
    startupCommand,
    logPrefix: 'Terminal'
  }), [startupCommand])

  return (
    <BaseTerminal
      cwd={cwd}
      workspace={workspace}
      tabId={tabId}
      sandbox={sandbox}
      isVisible={isVisible}
      config={terminalConfig}
    />
  )
}
