import { useMemo } from 'react'
import BaseTerminal, { type BaseTerminalConfig } from './BaseTerminal'
import type { SandboxConfig } from '../types'
import type { StoreApi } from 'zustand'
import type { WorkspaceState } from '../store/createWorkspaceStore'

interface TerminalProps {
  cwd: string
  workspaceId: string
  tabId: string
  startupCommand?: string
  sandbox?: SandboxConfig
  isVisible?: boolean
  workspaceStore: StoreApi<WorkspaceState>
}

export default function Terminal({ cwd, workspaceId, tabId, startupCommand, sandbox, isVisible, workspaceStore }: TerminalProps) {
  // Build terminal config with optional startup command from props
  const terminalConfig = useMemo<BaseTerminalConfig>(() => ({
    themeBackground: '#1e1e1e',
    startupCommand,
    logPrefix: 'Terminal'
  }), [startupCommand])

  return (
    <BaseTerminal
      cwd={cwd}
      workspaceId={workspaceId}
      tabId={tabId}
      sandbox={sandbox}
      isVisible={isVisible}
      config={terminalConfig}
      workspaceStore={workspaceStore}
    />
  )
}
