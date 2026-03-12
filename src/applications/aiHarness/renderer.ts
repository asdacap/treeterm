import type { Application, Tab, Workspace, ActivityState, AiHarnessState, AiHarnessInstance } from '../../renderer/types'
import { isAiHarnessState } from '../../renderer/types'
import AiHarness from '../../renderer/components/AiHarness'
import { createElement } from 'react'
import { useActivityStateStore } from '../../renderer/store/activityState'

// Factory function to create AI Harness variant applications
export function createAiHarnessVariant(instance: AiHarnessInstance): Application<AiHarnessState> {
  return {
    id: `aiharness-${instance.id}`,
    name: instance.name,
    icon: instance.icon,

    createInitialState: () => {
      return {
        ptyId: null,
        sandbox: {
          enabled: instance.enableSandbox,
          allowNetwork: instance.allowNetwork,
          allowedPaths: []
        }
      }
    },

    cleanup: async (tab: Tab, _workspace: Workspace) => {
      if (isAiHarnessState(tab.state) && tab.state.ptyId) {
        window.electron.terminal.kill(tab.state.ptyId)
      }
      useActivityStateStore.getState().removeTabState(tab.id)
    },

    render: ({ tab, workspaceId, workspacePath, isVisible }) => {
      if (!isAiHarnessState(tab.state)) {
        return null
      }
      return createElement(AiHarness, {
        key: tab.id,
        cwd: workspacePath,
        workspaceId,
        tabId: tab.id,
        sandbox: tab.state.sandbox,
        isVisible,
        command: instance.command,
        backgroundColor: instance.backgroundColor,
        disableScrollbar: instance.disableScrollbar
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
    isDefault: instance.isDefault
  }
}
