import { useStore } from 'zustand'
import { ToggleSwitch } from './ToggleSwitch'
import { useSettingsStore } from '../store/settings'
import { isWidthLimitDisabled } from '../types'
import type { TerminalState, WorkspaceStore } from '../types'

interface WidthLimitToggleProps {
  workspace: WorkspaceStore
  tabId: string
}

/**
 * Per-tab switch for the width cap set by terminal.maxCols. Persisted in the tab state so it
 * survives restarts and follows the tab into other windows.
 */
export function WidthLimitToggle({ workspace, tabId }: WidthLimitToggleProps) {
  const state = useStore(workspace, s => s.workspace.appStates[tabId]?.state)
  const maxCols = useSettingsStore(s => s.settings.terminal.maxCols)
  return (
    <ToggleSwitch
      checked={!isWidthLimitDisabled(state)}
      label={`${String(maxCols)} col limit`}
      onChange={(enabled) => {
        workspace.getState().updateTabState<TerminalState>(tabId, s => ({ ...s, widthLimitDisabled: !enabled }))
      }}
    />
  )
}
