import type { Application, Tab, Workspace, ActivityState, SandboxConfig, TerminalState } from '../types'
import Claude from '../components/Claude'
import { createElement } from 'react'
import { useActivityStateStore } from '../store/activityState'
import { useSettingsStore } from '../store/settings'

export interface ClaudeState extends TerminalState {
  sandbox: SandboxConfig
}

export const claudeApplication: Application<ClaudeState> = {
  id: 'claude',
  name: 'Claude',
  icon: '✦',

  createInitialState: () => {
    const { settings } = useSettingsStore.getState()
    return {
      ptyId: null,
      sandbox: {
        enabled: settings.sandbox.enabledByDefault,
        allowNetwork: settings.sandbox.allowNetworkByDefault,
        allowedPaths: []
      }
    }
  },

  cleanup: async (tab: Tab, _workspace: Workspace) => {
    const state = tab.state as ClaudeState
    if (state.ptyId) {
      window.electron.terminal.kill(state.ptyId)
    }
    // Remove activity state for this tab
    useActivityStateStore.getState().removeTabState(tab.id)
  },

  render: ({ tab, workspaceId, workspacePath, isVisible }) => {
    const state = tab.state as ClaudeState
    return createElement(Claude, {
      key: tab.id,
      cwd: workspacePath,
      workspaceId,
      tabId: tab.id,
      sandbox: state.sandbox,
      isVisible
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
