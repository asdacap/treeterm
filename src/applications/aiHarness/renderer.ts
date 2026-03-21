import type { Application, Tab, Workspace, AiHarnessState, AiHarnessInstance, TerminalApi } from '../../renderer/types'
import { isAiHarnessState } from '../../renderer/types'
import AiHarness from '../../renderer/components/AiHarness'
import { createElement } from 'react'

type TerminalDeps = { terminal: Pick<TerminalApi, 'kill'> }

// Factory function to create AI Harness variant applications
export function createAiHarnessVariant(instance: AiHarnessInstance, deps: TerminalDeps): Application<AiHarnessState> {
  return {
    id: `aiharness-${instance.id}`,
    name: instance.name,
    icon: instance.icon,

    createInitialState: () => {
      return {
        ptyId: null,
        ptyHandle: null,
        sandbox: {
          enabled: instance.enableSandbox,
          allowNetwork: instance.allowNetwork,
          allowedPaths: []
        },
        aiState: 'idle',
        analyzing: false,
        reason: ''
      }
    },

    cleanup: async (tab: Tab, _workspace: Workspace) => {
      if (isAiHarnessState(tab.state) && tab.state.ptyId) {
        deps.terminal.kill(tab.state.ptyId)
      }
    },

    render: ({ tab, workspace, isVisible }) => {
      if (!isAiHarnessState(tab.state)) {
        return null
      }
      return createElement(AiHarness, {
        key: tab.id,
        cwd: workspace.getState().workspace.path,
        workspace,
        tabId: tab.id,
        sandbox: tab.state.sandbox,
        isVisible,
        command: instance.command,
        backgroundColor: instance.backgroundColor,
        disableScrollbar: instance.disableScrollbar,
        stripScrollbackClear: instance.stripScrollbackClear,
      })
    },

    getActivityState: (tab: Tab) => {
      if (!isAiHarnessState(tab.state)) return 'idle'
      return tab.state.aiState
    },

    canClose: true,
    canHaveMultiple: true,
    showInNewTabMenu: true,
    keepAlive: true,
    displayStyle: 'flex',
    isDefault: instance.isDefault
  }
}
