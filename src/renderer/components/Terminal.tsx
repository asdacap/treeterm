import { useState } from 'react'
import { useStore } from 'zustand'
import BaseTerminal, { type BaseTerminalConfig, type BaseTerminalState } from './BaseTerminal'
import type { SandboxConfig, WorkspaceStore } from '../types'

interface TerminalProps {
  cwd: string
  workspace: WorkspaceStore
  tabId: string
  startupCommand?: string
  sandbox?: SandboxConfig
  isVisible?: boolean
}

export default function Terminal({ workspace, tabId, sandbox }: TerminalProps) {
  const { workspace: wsData } = useStore(workspace)
  const appState = wsData.appStates[tabId]
  const existingPtyId = (appState?.state as BaseTerminalState | undefined)?.ptyId

  const isSandboxed = sandbox?.enabled ?? false
  // Stable config — useState initializer runs once, so BaseTerminal never re-renders from config changes
  const [terminalConfig] = useState<BaseTerminalConfig>(() => ({
    themeBackground: isSandboxed ? '#1a1a2e' : '#1e1e1e',
    logPrefix: 'Terminal',
    disableActivityDetector: true,
  }))

  if (!existingPtyId) {
    return <div style={{ padding: 16, color: '#888' }}>Creating terminal...</div>
  }

  return (
    <BaseTerminal
      workspace={workspace}
      tabId={tabId}
      config={terminalConfig}
    />
  )
}
