import type { Application, TerminalState, TerminalInstance, Tab, TerminalAppRef, WorkspaceStore } from '../../renderer/types'
import Terminal from '../../renderer/components/Terminal'
import { createElement } from 'react'
import { useActivityStateStore } from '../../renderer/store/activityState'

export type TerminalDeps = { terminal: { kill: (connectionId: string, sessionId: string) => void } }

/**
 * PTY lifecycle for any terminal-backed application: idempotent creation keyed by ptyHandle,
 * and a close() that kills the PTY even when creation has not resolved yet.
 *
 * Exported so alternate terminal frontends (see `applications/ghosttyTerminal`) reuse this
 * rather than restating the creation race. Renderers that never populate `cachedTerminal`
 * get a no-op `disposeCachedTerminal()`.
 */
export function makeTerminalOnWorkspaceLoad(
  deps: TerminalDeps,
  startupCommand?: string
): (tab: Tab, workspaceStore: WorkspaceStore) => TerminalAppRef {
  return (tab: Tab, workspaceStore: WorkspaceStore): TerminalAppRef => {
    const ws = workspaceStore.getState()
    const state = tab.state as TerminalState

    // Held so close() can kill a PTY whose creation has not resolved yet — the tab's
    // appState is gone by then, so the resolved ptyId has nowhere to land.
    let creating: Promise<string> | null = null

    if (!state.ptyId) {
      // ptyHandle is the PTY's stable identity, minted once at tab creation. Keying
      // creation by it makes the dispose/re-init churn of reconciliation idempotent.
      const handle = state.ptyHandle ?? crypto.randomUUID()
      creating = ws.ensureTty(handle, ws.workspace.path, undefined, startupCommand)
      void creating.then((ptyId) => {
        workspaceStore.getState().updateTabState<TerminalState>(tab.id, (s) => ({
          ...s,
          ptyId,
          ptyHandle: handle,
          connectionId: ws.connectionId,
        }))
      })
    }

    const ref: TerminalAppRef = {
      cachedTerminal: null,
      disposeCachedTerminal() {
        if (this.cachedTerminal) {
          this.cachedTerminal.mountedHandler = null
          this.cachedTerminal.unsubscribeEvents()
          this.cachedTerminal.engine.dispose()
          this.cachedTerminal = null
        }
      },
      close: () => {
        const current = workspaceStore.getState().workspace.appStates[tab.id]?.state as TerminalState | undefined
        const connectionId = current?.connectionId ?? ws.connectionId
        const ptyId = current?.ptyId ?? state.ptyId
        if (ptyId) {
          deps.terminal.kill(connectionId, ptyId)
          return
        }
        // Creation still in flight. Its .then() will find the tab gone and drop the
        // ptyId, and removeTab drops the ptyHandle memo — so nothing else will ever
        // reference this PTY. Kill it the moment we learn its id.
        if (creating) void creating.then((id) => { deps.terminal.kill(connectionId, id) })
      },
      dispose: () => {
        ref.disposeCachedTerminal()
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
      ptyHandle: crypto.randomUUID(),
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
      ptyHandle: crypto.randomUUID(),
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
