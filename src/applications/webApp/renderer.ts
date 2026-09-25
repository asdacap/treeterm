import { createElement } from 'react'
import { createStore } from 'zustand/vanilla'
import type { Application, Tab, WebAppInstance, WebAppState, WorkspaceStore, SSHApi } from '../../renderer/types'
import { isWebAppState, WebAppPortStatus } from '../../renderer/types'
import WebAppPane from '../../renderer/components/WebAppPane'
import { useActivityStateStore } from '../../renderer/store/activityState'
import { createHttpProbe, findFreePort, waitForHttp, WaitOutcome } from './ports'
import { ensureForward } from './forward'
import { WebAppPhase, createPageMemory, forwardIdForTab } from './runtime'
import type { WebAppRef, WebAppRuntime } from './runtime'

export type WebAppDeps = {
  terminal: { kill: (connectionId: string, sessionId: string) => void }
  ssh: SSHApi
  openExternal: (url: string) => void
  sleep: (ms: number) => Promise<void>
  random: () => number
}

export function resolveCommand(command: string, port: number): string {
  return command.replaceAll('$PORT', String(port))
}

export function createWebAppVariant(instance: WebAppInstance, deps: WebAppDeps): Application<WebAppState, WebAppRef> {
  return {
    id: `webapp-${instance.id}`,
    name: instance.name,
    icon: instance.icon,

    createInitialState: () => ({
      ptyId: null,
      ptyHandle: crypto.randomUUID(),
      keepOnExit: instance.keepOnExit,
      idleDetectorDisabled: false,
      widthLimitDisabled: false,
      port: { status: WebAppPortStatus.Unassigned },
      localPort: { status: WebAppPortStatus.Unassigned },
      path: '',
    }),

    onWorkspaceLoad: (tab: Tab, workspaceStore: WorkspaceStore): WebAppRef => {
      const ws = workspaceStore.getState()
      const runtime = createStore<WebAppRuntime>(() => ({ phase: WebAppPhase.AllocatingPort }))
      const probe = createHttpProbe(ws.execApi, ws.connectionId)
      const forwardId = forwardIdForTab(tab.id)
      // Held so close() can kill a PTY whose creation has not resolved yet — the tab's
      // appState is gone by then, so the resolved ptyId has nowhere to land.
      let creating: Promise<string> | null = null
      let disposed = false
      const isCancelled = (): boolean => disposed

      const readState = (): WebAppState | undefined =>
        workspaceStore.getState().workspace.appStates[tab.id]?.state as WebAppState | undefined
      const update = (updater: (s: WebAppState) => WebAppState): void => {
        workspaceStore.getState().updateTabState<WebAppState>(tab.id, updater)
      }

      async function ensurePort(state: WebAppState): Promise<number> {
        if (state.port.status === WebAppPortStatus.Assigned) return state.port.port
        const port = await findFreePort(probe, deps.random)
        update((s) => ({ ...s, port: { status: WebAppPortStatus.Assigned, port } }))
        return port
      }

      async function ensurePty(state: WebAppState, port: number): Promise<void> {
        if (state.ptyId) return
        const handle = state.ptyHandle ?? crypto.randomUUID()
        creating = ws.ensureTty(handle, ws.workspace.path, undefined, resolveCommand(instance.command, port))
        const ptyId = await creating
        update((s) => ({ ...s, ptyId, ptyHandle: handle, connectionId: ws.connectionId }))
      }

      async function ensureLocalForward(state: WebAppState, remotePort: number): Promise<number> {
        const localPort = await ensureForward({
          ssh: deps.ssh,
          sleep: deps.sleep,
          random: deps.random,
          connectionId: ws.connectionId,
          forwardId,
          remotePort,
          preferredLocalPort: state.localPort,
        })
        update((s) => ({ ...s, localPort: { status: WebAppPortStatus.Assigned, port: localPort } }))
        return localPort
      }

      async function start(): Promise<void> {
        const initial = readState()
        if (!initial || isCancelled()) return
        runtime.setState({ phase: WebAppPhase.AllocatingPort }, true)
        const port = await ensurePort(initial)
        const afterPort = readState()
        if (!afterPort || isCancelled()) return
        await ensurePty(afterPort, port)
        runtime.setState({ phase: WebAppPhase.WaitingForServer, port }, true)
        if (await waitForHttp(probe, port, isCancelled, deps.sleep) === WaitOutcome.Cancelled) return
        if (!ws.isRemote) {
          runtime.setState({ phase: WebAppPhase.Ready, url: `http://localhost:${String(port)}/` }, true)
          return
        }
        runtime.setState({ phase: WebAppPhase.Forwarding, port }, true)
        const afterServer = readState()
        if (!afterServer || isCancelled()) return
        const localPort = await ensureLocalForward(afterServer, port)
        if (isCancelled()) return
        runtime.setState({ phase: WebAppPhase.Ready, url: `http://localhost:${String(localPort)}/` }, true)
      }

      function run(): void {
        start().catch((err: unknown) => {
          if (isCancelled()) return
          runtime.setState({ phase: WebAppPhase.Error, message: err instanceof Error ? err.message : String(err) }, true)
        })
      }

      run()

      const ref: WebAppRef = {
        runtime,
        ...createPageMemory(workspaceStore, tab.id),
        retry: run,
        restartForward: async () => {
          await deps.ssh.removePortForward(forwardId)
          run()
        },
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
          if (ws.isRemote) void deps.ssh.removePortForward(forwardId)
          const current = readState()
          const connectionId = current?.connectionId ?? ws.connectionId
          const ptyId = current?.ptyId
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
          useActivityStateStore.getState().removeTabState(tab.id)
        },
      }
      return ref
    },

    render: ({ tab, workspace, isVisible }) => {
      if (!isWebAppState(tab.state)) {
        return null
      }
      return createElement(WebAppPane, {
        key: tab.id,
        workspace,
        tabId: tab.id,
        isVisible,
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
