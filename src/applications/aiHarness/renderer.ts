import type { Application, Tab, TerminalAppRef, AiHarnessState, AiHarnessInstance, WorkspaceStore } from '../../renderer/types'
import { isAiHarnessState } from '../../renderer/types'
import AiHarness from '../../renderer/components/AiHarness'
import { createElement } from 'react'
import { useActivityStateStore } from '../../renderer/store/activityState'
import type { Analyzer } from '../../renderer/store/createAnalyzerStore'

type TerminalDeps = { terminal: { kill: (connectionId: string, sessionId: string) => void } }

export interface AiHarnessRef extends TerminalAppRef {
  analyzer: Analyzer
}

// Factory function to create AI Harness variant applications
export function createAiHarnessVariant(instance: AiHarnessInstance, deps: TerminalDeps): Application<AiHarnessState, AiHarnessRef> {
  return {
    id: `aiharness-${instance.id}`,
    name: instance.name,
    icon: instance.icon,

    createInitialState: () => ({
      ptyId: null,
      ptyHandle: crypto.randomUUID(),
      keepOnExit: instance.keepOnExit,
      sandbox: {
        enabled: instance.enableSandbox,
        allowNetwork: instance.allowNetwork,
        allowedPaths: []
      },
      autoApprove: false,
    }),

    onWorkspaceLoad: (tab: Tab, workspaceStore: WorkspaceStore): AiHarnessRef => {
      const ws = workspaceStore.getState()
      const state = tab.state as AiHarnessState
      const analyzer = ws.initAnalyzer(tab.id)

      // Held so close() can kill a PTY whose creation has not resolved yet — the tab's
      // appState is gone by then, so the resolved ptyId has nowhere to land.
      let creating: Promise<string> | null = null
      // `analyzer.start()` only guards against a *running* analyzer, so a creation that
      // resolves after dispose() would restart the one dispose() just stopped, leaving
      // an orphaned poll loop and TTY stream that nothing holds a handle to.
      let disposed = false

      if (state.ptyId) {
        // Restore: PTY already exists, just start analyzer
        // Pre-cache writer so promptHarness doesn't open a second stream
        void ws.getTtyWriter(state.ptyId)
        analyzer.getState().start(state.ptyId)
      } else {
        // New tab: create PTY (keyed by the stable per-PTY handle so reconciliation
        // churn can't duplicate it) then start analyzer
        const handle = state.ptyHandle ?? crypto.randomUUID()
        creating = ws.ensureTty(handle, ws.workspace.path, state.sandbox, instance.command)
        void creating.then((ptyId) => {
          if (disposed) return
          workspaceStore.getState().updateTabState<AiHarnessState>(tab.id, (s) => ({
            ...s,
            ptyId,
            ptyHandle: handle,
            connectionId: ws.connectionId,
          }))
          analyzer.getState().start(ptyId)
        })
      }

      const ref: AiHarnessRef = {
        analyzer,
        cachedTerminal: null,
        disposeCachedTerminal() {
          if (this.cachedTerminal) {
            this.cachedTerminal.mountedHandler = null
            // Owns the engine and the Tty subscription.
            this.cachedTerminal.owner.dispose()
            this.cachedTerminal = null
          }
        },
        close: () => {
          // Read current state for up-to-date ptyId (may have been set after onWorkspaceLoad)
          const current = workspaceStore.getState().workspace.appStates[tab.id]?.state as AiHarnessState | undefined
          const connectionId = current?.connectionId ?? ws.connectionId
          const ptyId = current?.ptyId ?? state.ptyId
          if (ptyId) {
            deps.terminal.kill(connectionId, ptyId)
            return
          }
          // Creation still in flight — see terminal/renderer.ts.
          if (creating) void creating.then((id) => { deps.terminal.kill(connectionId, id) })
        },
        dispose: () => {
          disposed = true
          ref.disposeCachedTerminal()
          analyzer.getState().stop()
          useActivityStateStore.getState().removeTabState(tab.id)
        },
      }
      return ref
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
      })
    },

    canClose: true,
    showInNewTabMenu: true,
    displayStyle: 'flex',
    isDefault: instance.isDefault
  }
}
