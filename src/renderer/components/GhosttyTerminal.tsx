import { useState } from 'react'
import { useStore } from 'zustand'
import BaseTerminal, { type BaseTerminalConfig, type BaseTerminalState } from './BaseTerminal'
import { createGhosttyEngine } from '../terminal/ghosttyEngine'
import type { WorkspaceStore } from '../types'

interface GhosttyTerminalProps {
  workspace: WorkspaceStore
  tabId: string
}

/**
 * The terminal application, backed by ghostty-web instead of xterm.js.
 *
 * Everything but the engine is shared with `Terminal` — the terminal cache, scroll badge,
 * pin-to-bottom, activity detection and exit handling all live in `BaseTerminal`.
 */
export default function GhosttyTerminal({ workspace, tabId }: GhosttyTerminalProps) {
  const wsData = useStore(workspace, s => s.workspace)
  const appState = wsData.appStates[tabId]
  const existingPtyId = (appState?.state as BaseTerminalState | undefined)?.ptyId

  // Stable config — useState initializer runs once, so BaseTerminal never re-renders from config changes
  const [terminalConfig] = useState<BaseTerminalConfig>(() => ({
    createEngine: createGhosttyEngine,
    themeBackground: '#1e1e1e',
    logPrefix: 'Ghostty',
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
