import { useStore } from 'zustand'
import { ToggleSwitch } from './ToggleSwitch'
import { isIdleDetectorDisabled } from '../types'
import type { TerminalState, WorkspaceStore } from '../types'

interface IdleDetectorToggleProps {
  workspace: WorkspaceStore
  tabId: string
}

/**
 * Per-tab switch for the viewport idle detector. Persisted in the tab state so it survives
 * restarts and follows the tab into other windows.
 */
export function IdleDetectorToggle({ workspace, tabId }: IdleDetectorToggleProps) {
  const state = useStore(workspace, s => s.workspace.appStates[tabId]?.state)
  return (
    <ToggleSwitch
      checked={!isIdleDetectorDisabled(state)}
      label="Idle detector"
      onChange={(enabled) => {
        workspace.getState().updateTabState<TerminalState>(tabId, s => ({ ...s, idleDetectorDisabled: !enabled }))
      }}
    />
  )
}
