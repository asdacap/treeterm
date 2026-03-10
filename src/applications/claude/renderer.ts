import type { Application, Tab, Workspace, ActivityState, SandboxConfig, ClaudeState } from '../../renderer/types'
import { isClaudeState } from '../../renderer/types'
import Claude from '../../renderer/components/Claude'
import { createElement } from 'react'
import { useActivityStateStore } from '../../renderer/store/activityState'
import { useSettingsStore } from '../../renderer/store/settings'

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
    if (isClaudeState(tab.state) && tab.state.ptyId) {
      window.electron.terminal.kill(tab.state.ptyId)
    }
    // Remove activity state for this tab
    useActivityStateStore.getState().removeTabState(tab.id)
  },

  render: ({ tab, workspaceId, workspacePath, isVisible }) => {
    if (!isClaudeState(tab.state)) {
      return null
    }
    return createElement(Claude, {
      key: tab.id,
      cwd: workspacePath,
      workspaceId,
      tabId: tab.id,
      sandbox: tab.state.sandbox,
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
  displayStyle: 'flex',
  isDefault: false
}
