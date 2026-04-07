import type { Application, TerminalState, CustomRunnerInstance, Tab, AppRef, WorkspaceStore } from '../../renderer/types'
import Terminal from '../../renderer/components/Terminal'
import { createElement } from 'react'
import { useActivityStateStore } from '../../renderer/store/activityState'

type TerminalDeps = { terminal: { kill: (connectionId: string, sessionId: string) => void } }

export function resolveTemplate(template: string, workspacePath: string): string {
  return template.replace(/\{\{workspace_path\}\}/g, workspacePath)
}

export function createCustomRunnerVariant(instance: CustomRunnerInstance, deps: TerminalDeps): Application<TerminalState> {
  return {
    id: `customrunner-${instance.id}`,
    name: instance.name,
    icon: instance.icon,

    createInitialState: () => ({
      ptyId: null,
      ptyHandle: null,
      keepOnExit: false
    }),

    onWorkspaceLoad: (tab: Tab, workspaceStore: WorkspaceStore): AppRef => {
      const ws = workspaceStore.getState()
      const state = tab.state as TerminalState
      if (!state.ptyId) {
        const resolvedCommand = resolveTemplate(instance.commandTemplate, ws.workspace.path)
        void ws.createTty(ws.workspace.path, undefined, resolvedCommand).then((ptyId) => {
          workspaceStore.getState().updateTabState<TerminalState>(tab.id, (s) => ({
            ...s,
            ptyId,
            connectionId: ws.connectionId,
          }))
        })
      }
      return {
        dispose: () => {
          const current = workspaceStore.getState().workspace.appStates[tab.id]?.state as TerminalState | undefined
          const ptyId = current?.ptyId ?? state.ptyId
          if (ptyId) {
            deps.terminal.kill(current?.connectionId ?? ws.connectionId, ptyId)
          }
          useActivityStateStore.getState().removeTabState(tab.id)
        },
      }
    },

    render: ({ tab, workspace, isVisible }) => createElement(Terminal, {
      key: tab.id,
      cwd: workspace.getState().workspace.path,
      workspace,
      tabId: tab.id,
      isVisible,
    }),

    canClose: true,
    showInNewTabMenu: true,
    displayStyle: 'flex',
    isDefault: instance.isDefault
  }
}
