import { create } from 'zustand'
import type { ActivityState } from '../types'

interface ActivityStateStore {
  // Tab activity states: tabId -> ActivityState
  states: Record<string, ActivityState>

  // Update state for a tab
  setTabState: (tabId: string, state: ActivityState) => void

  // Remove state when tab is closed
  removeTabState: (tabId: string) => void

  // Get consolidated workspace state (working > waiting > idle)
  getWorkspaceState: (tabIds: string[]) => ActivityState
}

export const useActivityStateStore = create<ActivityStateStore>((set, get) => ({
  states: {},

  setTabState: (tabId, state) => {
    set((s) => ({ states: { ...s.states, [tabId]: state } }))
  },

  removeTabState: (tabId) => {
    set((s) => {
      const { [tabId]: _, ...rest } = s.states
      return { states: rest }
    })
  },

  getWorkspaceState: (tabIds) => {
    const states = get().states
    // Priority: working > waiting_for_input > idle
    if (tabIds.some((id) => states[id] === 'working')) return 'working'
    if (tabIds.some((id) => states[id] === 'waiting_for_input')) return 'waiting_for_input'
    return 'idle'
  }
}))
