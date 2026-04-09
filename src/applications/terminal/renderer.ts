import type { Application, TerminalState, TerminalInstance, Tab, TerminalAppRef, WorkspaceStore } from '../../renderer/types'
import Terminal from '../../renderer/components/Terminal'
import { createElement } from 'react'
import { useActivityStateStore } from '../../renderer/store/activityState'
import type { Analyzer } from '../../renderer/store/createAnalyzerStore'

type TerminalDeps = { terminal: { kill: (connectionId: string, sessionId: string) => void } }

export interface TerminalRef extends TerminalAppRef {
  analyzer: Analyzer
}

function makeTerminalOnWorkspaceLoad(
  deps: TerminalDeps,
  startupCommand?: string
): (tab: Tab, workspaceStore: WorkspaceStore) => TerminalRef {
  return (tab: Tab, workspaceStore: WorkspaceStore): TerminalRef => {
    const ws = workspaceStore.getState()
    const state = tab.state as TerminalState
    const analyzer = ws.initAnalyzer(tab.id)

    if (state.ptyId) {
      analyzer.getState().start(state.ptyId)
    } else {
      void ws.createTty(ws.workspace.path, undefined, startupCommand).then((ptyId) => {
        workspaceStore.getState().updateTabState<TerminalState>(tab.id, (s) => ({
          ...s,
          ptyId,
          connectionId: ws.connectionId,
        }))
        analyzer.getState().start(ptyId)
      })
    }
    const ref: TerminalRef = {
      analyzer,
      cachedTerminal: null,
      disposeCachedTerminal() {
        if (this.cachedTerminal) {
          this.cachedTerminal.mountedHandler = null
          this.cachedTerminal.unsubscribeEvents()
          this.cachedTerminal.terminal.dispose()
          this.cachedTerminal = null
        }
      },
      close: () => {
        const current = workspaceStore.getState().workspace.appStates[tab.id]?.state as TerminalState | undefined
        const ptyId = current?.ptyId ?? state.ptyId
        if (ptyId) {
          deps.terminal.kill(current?.connectionId ?? ws.connectionId, ptyId)
        }
      },
      dispose: () => {
        ref.disposeCachedTerminal()
        analyzer.getState().stop()
        useActivityStateStore.getState().removeTabState(tab.id)
      },
    }
    return ref
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
      ptyHandle: null,
      keepOnExit: false
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
      ptyHandle: null,
      keepOnExit: false
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

    showInNewTabMenu: true,
    displayStyle: 'flex',
    isDefault: instance.isDefault
  }
}
