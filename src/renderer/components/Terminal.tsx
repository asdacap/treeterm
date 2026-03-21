import { useCallback, useMemo } from 'react'
import { useStore } from 'zustand'
import BaseTerminal, { type BaseTerminalConfig, type BaseTerminalState } from './BaseTerminal'
import { useTtyCreation } from '../hooks/useTtyConnection'
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
  const { workspace: wsData, updateTabState } = useStore(workspace)
  const appState = wsData?.appStates[tabId]
  const existingPtyId = (appState?.state as BaseTerminalState | undefined)?.ptyId

  const onCreated = useCallback((ptyId: string) => {
    updateTabState<BaseTerminalState>(tabId, (state) => ({
      ...state,
      ptyId,
    }))
  }, [tabId, updateTabState])

  const { loading, error } = useTtyCreation(existingPtyId, cwd, sandbox, startupCommand, onCreated)

  const isSandboxed = sandbox?.enabled ?? false
  const terminalConfig = useMemo<BaseTerminalConfig>(() => ({
    themeBackground: isSandboxed ? '#1a1a2e' : '#1e1e1e',
    logPrefix: 'Terminal'
  }), [isSandboxed])

  if (loading) {
    return <div style={{ padding: 16, color: '#888' }}>Creating terminal...</div>
  }

  if (error) {
    return <div style={{ padding: 16, color: '#f14c4c' }}>Failed to create terminal: {error.message}</div>
  }

  return (
    <BaseTerminal
      workspace={workspace}
      tabId={tabId}
      isVisible={isVisible}
      config={terminalConfig}
    />
  )
}
