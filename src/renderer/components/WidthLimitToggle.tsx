import { useStore } from 'zustand'
import { ToggleSwitch } from './ToggleSwitch'
import { isWidthLimitDisabled } from '../types'
import type { TerminalState, WorkspaceStore } from '../types'

interface WidthLimitToggleProps {
  workspace: WorkspaceStore
  tabId: string
}

/**
 * Per-tab switch for the default 80-column width cap. Persisted in the tab state so it survives
 * restarts and follows the tab into other windows.
 */
export function WidthLimitToggle({ workspace, tabId }: WidthLimitToggleProps) {
  const state = useStore(workspace, s => s.workspace.appStates[tabId]?.state)
  return (
    <ToggleSwitch
      checked={!isWidthLimitDisabled(state)}
      label="80 col limit"
      onChange={(enabled) => {
        workspace.getState().updateTabState<TerminalState>(tabId, s => ({ ...s, widthLimitDisabled: !enabled }))
      }}
    />
  )
}
