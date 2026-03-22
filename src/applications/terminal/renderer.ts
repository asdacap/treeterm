import type { Application, TerminalState, TerminalApi, TerminalInstance, Tab, Workspace } from '../../renderer/types'
import { isTerminalState } from '../../renderer/types'
import Terminal from '../../renderer/components/Terminal'
import { createElement } from 'react'
import { useActivityStateStore } from '../../renderer/store/activityState'

type TerminalDeps = { terminal: { kill: (connectionId: string, sessionId: string) => void } }

// Factory function to create the base terminal application
export function createTerminalApplication(deps: TerminalDeps): Application<TerminalState> {
  return {
    id: 'terminal',
    name: 'Terminal',
    icon: '>',

    createInitialState: () => ({
      ptyId: null,
      ptyHandle: null
    }),

    cleanup: async (tab: Tab, _workspace: Workspace) => {
      if (isTerminalState(tab.state) && tab.state.ptyId) {
        deps.terminal.kill(tab.state.connectionId ?? 'local', tab.state.ptyId)
      }
      // Remove activity state for this tab
      useActivityStateStore.getState().removeTabState(tab.id)
    },

    render: ({ tab, workspace, isVisible }) => {
      return createElement(Terminal, {
        key: tab.id,
        cwd: workspace.getState().workspace.path,
        workspace,
        tabId: tab.id,
        isVisible,
      })
    },

    canClose: true,
    canHaveMultiple: true,
    showInNewTabMenu: true,
    keepAlive: true,
    displayStyle: 'flex',
    isDefault: true
  }
}

// Factory function to create terminal variant applications
export function createTerminalVariant(instance: TerminalInstance, deps: TerminalDeps): Application<TerminalState> {
  return {
    id: `terminal-${instance.id}`,
    name: instance.name,
    icon: instance.icon,

    createInitialState: () => ({
      ptyId: null,
      ptyHandle: null
    }),

    cleanup: async (tab: Tab, _workspace: Workspace) => {
      if (isTerminalState(tab.state) && tab.state.ptyId) {
        deps.terminal.kill(tab.state.connectionId ?? 'local', tab.state.ptyId)
      }
      useActivityStateStore.getState().removeTabState(tab.id)
    },

    render: ({ tab, workspace, isVisible }) => {
      return createElement(Terminal, {
        key: tab.id,
        cwd: workspace.getState().workspace.path,
        workspace,
        tabId: tab.id,
        isVisible,
        startupCommand: instance.startupCommand,
      })
    },

    canClose: true,
    canHaveMultiple: true,
    showInNewTabMenu: true,
    keepAlive: true,
    displayStyle: 'flex',
    isDefault: instance.isDefault
  }
}
