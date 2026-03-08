import type { Application, TerminalState, TerminalInstance, Tab, Workspace } from '../types'
import Terminal from '../components/Terminal'
import { createElement } from 'react'
import { useActivityStateStore } from '../store/activityState'

export const terminalApplication: Application<TerminalState> = {
  id: 'terminal',
  name: 'Terminal',
  icon: '>',

  createInitialState: () => ({
    ptyId: null
  }),

  cleanup: async (tab: Tab, _workspace: Workspace) => {
    const state = tab.state as TerminalState
    if (state.ptyId) {
      window.electron.terminal.kill(state.ptyId)
    }
    // Remove activity state for this tab
    useActivityStateStore.getState().removeTabState(tab.id)
  },

  render: ({ tab, workspaceId, workspacePath, isVisible }) => {
    return createElement(Terminal, {
      key: tab.id,
      cwd: workspacePath,
      workspaceId,
      tabId: tab.id,
      isVisible
    })
  },

  canClose: true,
  canHaveMultiple: true,
  showInNewTabMenu: true,
  keepAlive: true,
  displayStyle: 'block',
  isDefault: true
}

// Factory function to create terminal variant applications
export function createTerminalVariant(instance: TerminalInstance): Application<TerminalState> {
  return {
    id: `terminal-${instance.id}`,
    name: instance.name,
    icon: instance.icon,

    createInitialState: () => ({
      ptyId: null
    }),

    cleanup: async (tab: Tab, _workspace: Workspace) => {
      const state = tab.state as TerminalState
      if (state.ptyId) {
        window.electron.terminal.kill(state.ptyId)
      }
      useActivityStateStore.getState().removeTabState(tab.id)
    },

    render: ({ tab, workspaceId, workspacePath, isVisible }) => {
      return createElement(Terminal, {
        key: tab.id,
        cwd: workspacePath,
        workspaceId,
        tabId: tab.id,
        isVisible,
        startupCommand: instance.startupCommand
      })
    },

    canClose: true,
    canHaveMultiple: true,
    showInNewTabMenu: true,
    keepAlive: true,
    displayStyle: 'block',
    isDefault: instance.isDefault
  }
}
