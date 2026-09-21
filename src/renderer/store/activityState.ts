import { create } from 'zustand'
import { ActivityState } from '../types'

export enum ActivityTransitionKind {
  /** Idle detector saw the viewport change after a quiet period. */
  ViewportChanged = 'viewport_changed',
  /** Idle detector saw no viewport change for the armed timeout. */
  ViewportIdle = 'viewport_idle',
  /** AI harness analyzer classified the buffer (or reset on its own). */
  Classification = 'classification',
  /** System prompt debugger drove the state by hand. */
  Debugger = 'debugger',
}

/** Why a tab changed state. Every kind carries the viewport that was on screen at that moment. */
export type ActivityTransitionDetail =
  | { kind: ActivityTransitionKind.ViewportChanged; snapshot: string }
  | { kind: ActivityTransitionKind.ViewportIdle; snapshot: string; idleTimeoutMs: number }
  | { kind: ActivityTransitionKind.Classification; snapshot: string; reason: string }
  | { kind: ActivityTransitionKind.Debugger; snapshot: string }

export interface ActivityTransition {
  timestamp: number
  from: ActivityState
  to: ActivityState
  detail: ActivityTransitionDetail
}

export const MAX_TRANSITIONS = 1000

/** Stable empty list for selectors, so a tab without history doesn't yield a fresh array per render. */
export const NO_TRANSITIONS: ActivityTransition[] = []

export interface ActivityStateStore {
  // Tab activity states: tabId -> ActivityState
  states: Record<string, ActivityState>

  // Transition log per tab, oldest first, capped at MAX_TRANSITIONS
  transitions: Record<string, ActivityTransition[]>

  // Update state for a tab and log the transition
  setTabState: (tabId: string, state: ActivityState, detail: ActivityTransitionDetail) => void

  // Remove state when tab is closed
  removeTabState: (tabId: string) => void

  // Get consolidated workspace state (working > waiting > idle)
  getWorkspaceState: (tabIds: string[]) => ActivityState
}

export type SetActivityTabState = ActivityStateStore['setTabState']

export const useActivityStateStore = create<ActivityStateStore>((set, get) => ({
  states: {},
  transitions: {},

  setTabState: (tabId, state, detail) => {
    set((s) => {
      const transition: ActivityTransition = {
        timestamp: Date.now(),
        from: s.states[tabId] ?? ActivityState.Idle,
        to: state,
        detail,
      }
      const log = [...(s.transitions[tabId] ?? NO_TRANSITIONS), transition].slice(-MAX_TRANSITIONS)
      return {
        states: { ...s.states, [tabId]: state },
        transitions: { ...s.transitions, [tabId]: log },
      }
    })
  },

  removeTabState: (tabId) => {
    set((s) => {
      // eslint-disable-next-line @typescript-eslint/no-unused-vars
      const { [tabId]: _, ...rest } = s.states
      // eslint-disable-next-line @typescript-eslint/no-unused-vars
      const { [tabId]: __, ...restTransitions } = s.transitions
      return { states: rest, transitions: restTransitions }
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
