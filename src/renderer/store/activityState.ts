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
      // eslint-disable-next-line @typescript-eslint/no-unused-vars
      const { [tabId]: _, ...rest } = s.states
      return { states: rest }
    })
  },

  getWorkspaceState: (tabIds) => {
    const states = get().states
    // Priority: working > permission_request > safe_permission_requested > user_input_required > error > completed > idle
    if (tabIds.some((id) => states[id] === 'working')) return 'working'
    if (tabIds.some((id) => states[id] === 'permission_request')) return 'permission_request'
    if (tabIds.some((id) => states[id] === 'safe_permission_requested')) return 'safe_permission_requested'
    if (tabIds.some((id) => states[id] === 'user_input_required')) return 'user_input_required'
    if (tabIds.some((id) => states[id] === 'error')) return 'error'
    if (tabIds.some((id) => states[id] === 'completed')) return 'completed'
    return 'idle'
  }
}))
