import type { Application, Tab, Workspace, ActivityState } from '../types'
import Claude from '../components/Claude'
import { createElement } from 'react'
import { useActivityStateStore } from '../store/activityState'

export interface ClaudeState {
  ptyId: string | null
}

export const claudeApplication: Application<ClaudeState> = {
  id: 'claude',
  name: 'Claude',
  icon: '✦',

  createInitialState: () => ({
    ptyId: null
  }),

  cleanup: async (tab: Tab, _workspace: Workspace) => {
    const state = tab.state as ClaudeState
    if (state.ptyId) {
      window.electron.terminal.kill(state.ptyId)
    }
    // Remove activity state for this tab
    useActivityStateStore.getState().removeTabState(tab.id)
  },

  render: ({ tab, workspaceId, workspacePath, sandbox }) => {
    return createElement(Claude, {
      key: tab.id,
      cwd: workspacePath,
      workspaceId,
      tabId: tab.id,
      sandbox
    })
  },

  getActivityState: (tab: Tab): ActivityState => {
    return useActivityStateStore.getState().states[tab.id] || 'idle'
  },

  canClose: true,
  canHaveMultiple: true,
  showInNewTabMenu: true,
  keepAlive: true,
  displayStyle: 'block'
}
