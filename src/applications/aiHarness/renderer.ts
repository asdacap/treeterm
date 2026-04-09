import type { Application, Tab, TerminalAppRef, AiHarnessState, AiHarnessInstance, WorkspaceStore } from '../../renderer/types'
import { isAiHarnessState } from '../../renderer/types'
import AiHarness from '../../renderer/components/AiHarness'
import { createElement } from 'react'
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
      ptyHandle: null,
      keepOnExit: false,
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

      if (state.ptyId) {
        // Restore: PTY already exists, just start analyzer
        // Pre-cache writer so promptHarness doesn't open a second stream
        void ws.getTtyWriter(state.ptyId)
        analyzer.getState().start(state.ptyId)
      } else {
        // New tab: create PTY then start analyzer
        void ws.createTty(ws.workspace.path, state.sandbox, instance.command).then((ptyId) => {
          workspaceStore.getState().updateTabState<AiHarnessState>(tab.id, (s) => ({
            ...s,
            ptyId,
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
            this.cachedTerminal.unsubscribeEvents()
            this.cachedTerminal.terminal.dispose()
            this.cachedTerminal = null
          }
        },
        close: () => {
          // Read current state for up-to-date ptyId (may have been set after onWorkspaceLoad)
          const current = workspaceStore.getState().workspace.appStates[tab.id]?.state as AiHarnessState | undefined
          const ptyId = current?.ptyId ?? state.ptyId
          if (ptyId) {
            deps.terminal.kill(current?.connectionId ?? ws.connectionId, ptyId)
          }
        },
        dispose: () => {
          ref.disposeCachedTerminal()
          analyzer.getState().stop()
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
        stripScrollbackClear: instance.stripScrollbackClear,
      })
    },

    canClose: true,
    showInNewTabMenu: true,
    displayStyle: 'flex',
    isDefault: instance.isDefault
  }
}
