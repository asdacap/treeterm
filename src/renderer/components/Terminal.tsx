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

export default function Terminal({ cwd: _cwd, workspace, tabId, startupCommand: _startupCommand, sandbox, isVisible: _isVisible }: TerminalProps) {
  const { workspace: wsData } = useStore(workspace)
  const appState = wsData?.appStates[tabId]
  const existingPtyId = (appState?.state as BaseTerminalState | undefined)?.ptyId

  const isSandboxed = sandbox?.enabled ?? false
  const terminalConfig: BaseTerminalConfig = {
    themeBackground: isSandboxed ? '#1a1a2e' : '#1e1e1e',
    logPrefix: 'Terminal'
  }

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
