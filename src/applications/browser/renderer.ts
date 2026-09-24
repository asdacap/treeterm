import { createElement } from 'react'
import { createStore } from 'zustand/vanilla'
import type { Application, Tab, BrowserInstance, BrowserState, WorkspaceStore, SSHApi } from '../../renderer/types'
import { isBrowserState, WebAppPortStatus } from '../../renderer/types'
import BrowserPane from '../../renderer/components/BrowserPane'
import { ensureForward } from '../webApp/forward'
import { WebAppPhase, forwardIdForTab } from '../webApp/runtime'
import type { PageRef, WebAppRuntime } from '../webApp/runtime'
import { forwardedUrl, parseLocalUrl, targetPort } from './url'

export type BrowserDeps = {
  ssh: SSHApi
  openExternal: (url: string) => void
  sleep: (ms: number) => Promise<void>
  random: () => number
}

export function createBrowserVariant(instance: BrowserInstance, deps: BrowserDeps): Application<BrowserState, PageRef> {
  return {
    id: `browser-${instance.id}`,
    name: instance.name,
    icon: instance.icon,

    createInitialState: () => ({
      localPort: { status: WebAppPortStatus.Unassigned },
    }),

    onWorkspaceLoad: (tab: Tab, workspaceStore: WorkspaceStore): PageRef => {
      const ws = workspaceStore.getState()
      // Placeholder only: run() below replaces it synchronously (Ready, Forwarding or Error).
      const runtime = createStore<WebAppRuntime>(() => ({ phase: WebAppPhase.Forwarding, port: 0 }))
      const forwardId = forwardIdForTab(tab.id)
      let disposed = false
      const isCancelled = (): boolean => disposed

      async function start(): Promise<void> {
        const url = parseLocalUrl(instance.url)
        if (!ws.isRemote) {
          runtime.setState({ phase: WebAppPhase.Ready, url: url.href }, true)
          return
        }
        const state = workspaceStore.getState().workspace.appStates[tab.id]?.state as BrowserState | undefined
        if (!state) return
        const remotePort = targetPort(url)
        runtime.setState({ phase: WebAppPhase.Forwarding, port: remotePort }, true)
        const localPort = await ensureForward({
          ssh: deps.ssh,
          sleep: deps.sleep,
          random: deps.random,
          connectionId: ws.connectionId,
          forwardId,
          remotePort,
          preferredLocalPort: state.localPort,
        })
        if (isCancelled()) return
        workspaceStore.getState().updateTabState<BrowserState>(tab.id, (s) => ({
          ...s,
          localPort: { status: WebAppPortStatus.Assigned, port: localPort },
        }))
        runtime.setState({ phase: WebAppPhase.Ready, url: forwardedUrl(url, localPort) }, true)
      }

      function run(): void {
        start().catch((err: unknown) => {
          if (isCancelled()) return
          runtime.setState({ phase: WebAppPhase.Error, message: err instanceof Error ? err.message : String(err) }, true)
        })
      }

      run()

      return {
        runtime,
        retry: run,
        restartForward: async () => {
          await deps.ssh.removePortForward(forwardId)
          run()
        },
        close: () => {
          if (ws.isRemote) void deps.ssh.removePortForward(forwardId)
        },
        dispose: () => {
          disposed = true
        },
      }
    },

    render: ({ tab, workspace }) => {
      if (!isBrowserState(tab.state)) {
        return null
      }
      return createElement(BrowserPane, {
        key: tab.id,
        workspace,
        tabId: tab.id,
        ssh: deps.ssh,
        openExternal: deps.openExternal,
      })
    },

    canClose: true,
    showInNewTabMenu: true,
    displayStyle: 'flex',
    isDefault: instance.isDefault
  }
}
