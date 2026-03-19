import type { Tab } from '../types'
import { isAiHarnessState } from '../types'

export interface RunningHarness {
  ptyId: string
  tabId: string
}

/**
 * Finds the first running AI harness tab.
 * Returns the ptyId and tabId, or null if no running harness is found.
 */
export function findRunningHarness(tabs: Tab[]): RunningHarness | null {
  for (const tab of tabs) {
    if (
      tab.applicationId.startsWith('aiharness-') &&
      isAiHarnessState(tab.state) &&
      tab.state.ptyId !== null
    ) {
      return { ptyId: tab.state.ptyId, tabId: tab.id }
    }
  }
  return null
}
