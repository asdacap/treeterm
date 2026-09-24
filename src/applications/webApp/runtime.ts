import type { StoreApi } from 'zustand/vanilla'
import type { TerminalAppRef } from '../../renderer/types'

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

export interface WebAppRef extends TerminalAppRef {
  runtime: StoreApi<WebAppRuntime>
  /** Re-run the startup flow; reuses whatever port, PTY and forward still exist. */
  retry: () => void
  /** Tear down the ssh forward and set it up again (remote only). */
  restartForward: () => Promise<void>
}

export function forwardIdForTab(tabId: string): string {
  return `webapp-${tabId}`
}
