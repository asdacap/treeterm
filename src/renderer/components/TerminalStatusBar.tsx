import { IdleDetectorToggle } from './IdleDetectorToggle'
import { ActivityStateBadge } from './ActivityStateBadge'
import { useActivityStateStore } from '../store/activityState'
import { snapshotViewport } from '../terminal/engine'
import { ActivityState } from '../types'
import type { TerminalAppRef, WorkspaceStore } from '../types'

interface TerminalStatusBarProps {
  workspace: WorkspaceStore
  tabId: string
}

/** Status bar under a plain terminal: activity badge (with its debug menu) and the idle detector switch. */
export function TerminalStatusBar({ workspace, tabId }: TerminalStatusBarProps) {
  const activityState = useActivityStateStore((s) => s.states[tabId] ?? ActivityState.Idle)

  const getBufferText = (): string => {
    const ref = workspace.getState().getTabRef(tabId) as TerminalAppRef | null
    return ref?.cachedTerminal ? snapshotViewport(ref.cachedTerminal.engine) : ''
  }

  return (
    <div className="terminal-status-bar">
      <ActivityStateBadge workspace={workspace} tabId={tabId} state={activityState} getBufferText={getBufferText} />
      <IdleDetectorToggle workspace={workspace} tabId={tabId} />
    </div>
  )
}
