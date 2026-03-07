import type { Application, TerminalState, Tab, Workspace } from '../types'
import Terminal from '../components/Terminal'
import { createElement } from 'react'

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
  },

  render: ({ tab, workspaceId, workspacePath, isVisible }) => {
    return createElement(Terminal, {
      key: tab.id,
      cwd: workspacePath,
      workspaceId,
      tabId: tab.id,
      config: tab.config,
      isVisible
    })
  },

  canClose: true,
  canHaveMultiple: true,
  showInNewTabMenu: true,
  keepAlive: true,
  displayStyle: 'block'
}
