import { create } from 'zustand'
import { ActivityState } from '../types'

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
    if (tabIds.some((id) => states[id] === ActivityState.Working)) return ActivityState.Working
    if (tabIds.some((id) => states[id] === ActivityState.PermissionRequest)) return ActivityState.PermissionRequest
    if (tabIds.some((id) => states[id] === ActivityState.SafePermissionRequested)) return ActivityState.SafePermissionRequested
    if (tabIds.some((id) => states[id] === ActivityState.UserInputRequired)) return ActivityState.UserInputRequired
    if (tabIds.some((id) => states[id] === ActivityState.Error)) return ActivityState.Error
    if (tabIds.some((id) => states[id] === ActivityState.Completed)) return ActivityState.Completed
    return ActivityState.Idle
  }
}))
