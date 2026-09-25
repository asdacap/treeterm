import type { StoreApi } from 'zustand/vanilla'
import type { AppRef, TerminalAppRef, WorkspaceStore } from '../../renderer/types'

export enum WebAppPhase {
  AllocatingPort = 'allocating-port',
  WaitingForServer = 'waiting-for-server',
  Forwarding = 'forwarding',
  Ready = 'ready',
  Error = 'error',
}

export type WebAppRuntime =
  | { phase: WebAppPhase.AllocatingPort }
  | { phase: WebAppPhase.WaitingForServer; port: number }
  | { phase: WebAppPhase.Forwarding; port: number }
  | { phase: WebAppPhase.Ready; url: string }
  | { phase: WebAppPhase.Error; message: string }

/** Remembers the page a tab was on, so remounting the webview (e.g. a workspace switch) goes back to it. */
export interface PageMemory {
  /** The page to open: the remembered path on top of the start page's origin. */
  pageUrl: (homeUrl: string) => string
  /** Records a navigation. Pages on another origin cannot be rebased onto the start page, so they are skipped. */
  rememberPage: (homeUrl: string, url: string) => void
}

/** Ref of a tab that shows a (possibly forwarded) localhost page: WebApp and Browser. */
export interface PageRef extends AppRef, PageMemory {
  runtime: StoreApi<WebAppRuntime>
  /** Re-run the startup flow; reuses whatever port, PTY and forward still exist. */
  retry: () => void
  /** Tear down the ssh forward and set it up again (remote only). */
  restartForward: () => Promise<void>
}

export interface WebAppRef extends PageRef, TerminalAppRef {}

export function forwardIdForTab(tabId: string): string {
  return `webapp-${tabId}`
}

/**
 * Only the path is kept: on remote sessions the origin is the local end of the ssh
 * forward, which may be reassigned between runs.
 */
export function createPageMemory(workspaceStore: WorkspaceStore, tabId: string): PageMemory {
  return {
    pageUrl: (homeUrl) => {
      const state = workspaceStore.getState().workspace.appStates[tabId]?.state as { path: string | undefined } | undefined
      // Tabs persisted before the path was remembered have no field at all.
      return new URL(state?.path ?? '', homeUrl).href
    },
    rememberPage: (homeUrl, url) => {
      const page = new URL(url)
      if (page.origin !== new URL(homeUrl).origin) return
      workspaceStore.getState().updateTabState<{ path: string }>(tabId, (s) => ({
        ...s,
        path: page.pathname + page.search + page.hash,
      }))
    },
  }
}
