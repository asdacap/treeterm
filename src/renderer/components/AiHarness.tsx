import { useMemo } from 'react'
import BaseTerminal, { type BaseTerminalConfig } from './BaseTerminal'
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
  // Memoize config based on props to prevent unnecessary re-renders
  const config = useMemo<BaseTerminalConfig>(() => ({
    themeBackground: backgroundColor,
    promptPatterns: [/❯\s/], // Common AI tool prompt pattern
    startupCommand: command,
    logPrefix: 'AiHarness',
    showPushToTalk: true,
    showPromptDescription: true,
    disableScrollbar,
    stripScrollbackClear
  }), [backgroundColor, command, disableScrollbar, stripScrollbackClear])

  return (
    <BaseTerminal
      cwd={cwd}
      workspaceId={workspaceId}
      tabId={tabId}
      sandbox={sandbox}
      isVisible={isVisible}
      config={config}
      workspaceStore={workspaceStore}
    />
  )
}
