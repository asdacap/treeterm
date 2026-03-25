import type { Application, TerminalState, TerminalInstance, Tab, AppRef, WorkspaceStore } from '../../renderer/types'
import Terminal from '../../renderer/components/Terminal'
import { createElement } from 'react'
import { useActivityStateStore } from '../../renderer/store/activityState'

type TerminalDeps = { terminal: { kill: (connectionId: string, sessionId: string) => void } }

function makeTerminalOnWorkspaceLoad(
  deps: TerminalDeps,
  startupCommand?: string
): (tab: Tab, workspaceStore: WorkspaceStore) => AppRef {
  return (tab: Tab, workspaceStore: WorkspaceStore): AppRef => {
    const ws = workspaceStore.getState()
    const state = tab.state as TerminalState
    if (!state.ptyId) {
      ws.createTty(ws.workspace.path, undefined, startupCommand).then((ptyId) => {
        workspaceStore.getState().updateTabState<TerminalState>(tab.id, (s) => ({
          ...s,
          ptyId,
          connectionId: ws.connectionId,
        }))
      })
    }
    return {
      dispose: () => {
        const current = workspaceStore.getState().workspace?.appStates?.[tab.id]?.state as TerminalState | undefined
        const ptyId = current?.ptyId ?? state.ptyId
        if (ptyId) {
          deps.terminal.kill(current?.connectionId ?? ws.connectionId, ptyId)
        }
        useActivityStateStore.getState().removeTabState(tab.id)
      },
    }
  }
}

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

    onWorkspaceLoad: makeTerminalOnWorkspaceLoad(deps),

    render: ({ tab, workspace, isVisible }) => createElement(Terminal, {
      key: tab.id,
      cwd: workspace.getState().workspace.path,
      workspace,
      tabId: tab.id,
      isVisible,
    }),

    canClose: true,
    canHaveMultiple: true,
    showInNewTabMenu: true,
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

    onWorkspaceLoad: makeTerminalOnWorkspaceLoad(deps, instance.startupCommand),

    render: ({ tab, workspace, isVisible }) => createElement(Terminal, {
      key: tab.id,
      cwd: workspace.getState().workspace.path,
      workspace,
      tabId: tab.id,
      isVisible,
      startupCommand: instance.startupCommand,
    }),

    canClose: true,
    canHaveMultiple: true,
    showInNewTabMenu: true,
    displayStyle: 'flex',
    isDefault: instance.isDefault
  }
}
