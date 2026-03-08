import { useMemo } from 'react'
import BaseTerminal, { type BaseTerminalConfig } from './BaseTerminal'
import type { SandboxConfig } from '../types'

interface TerminalProps {
  cwd: string
  workspaceId: string
  tabId: string
  config?: Record<string, unknown>
  sandbox?: SandboxConfig
  isVisible?: boolean
}

export default function Terminal({ cwd, workspaceId, tabId, config, sandbox, isVisible }: TerminalProps) {
  // Build terminal config with optional startup command from props
  const terminalConfig = useMemo<BaseTerminalConfig>(() => ({
    themeBackground: '#1e1e1e',
    startupCommand: (config?.command as string) || undefined,
    logPrefix: 'Terminal'
  }), [config?.command])

  return (
    <BaseTerminal
      cwd={cwd}
      workspaceId={workspaceId}
      tabId={tabId}
      sandbox={sandbox}
      isVisible={isVisible}
      config={terminalConfig}
    />
  )
}
