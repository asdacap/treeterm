import type { StoreApi } from 'zustand/vanilla'
import type { AppRef, TerminalAppRef } from '../../renderer/types'

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

/** Ref of a tab that shows a (possibly forwarded) localhost page: WebApp and Browser. */
export interface PageRef extends AppRef {
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
