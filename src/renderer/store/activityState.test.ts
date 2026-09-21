import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { ActivityTransitionKind, MAX_TRANSITIONS, useActivityStateStore, type ActivityTransitionDetail } from './activityState'
import { ActivityState } from '../types'

const VIEWPORT: ActivityTransitionDetail = { kind: ActivityTransitionKind.ViewportChanged, snapshot: '$ ls' }

describe('ActivityStateStore', () => {
  beforeEach(() => {
    useActivityStateStore.setState({ states: {}, transitions: {} })
  })
  afterEach(() => { vi.useRealTimers() })

  describe('setTabState', () => {
    it('sets state for a tab', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working, VIEWPORT)

      expect(useActivityStateStore.getState().states['tab1']).toBe(ActivityState.Working)
    })

    it('updates state for existing tab', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Idle, VIEWPORT)
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working, VIEWPORT)

      expect(useActivityStateStore.getState().states['tab1']).toBe(ActivityState.Working)
    })

    it('maintains states for multiple tabs', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working, VIEWPORT)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.Idle, VIEWPORT)
      useActivityStateStore.getState().setTabState('tab3', ActivityState.UserInputRequired, VIEWPORT)

      const states = useActivityStateStore.getState().states
      expect(Object.keys(states)).toHaveLength(3)
      expect(states['tab1']).toBe(ActivityState.Working)
      expect(states['tab2']).toBe(ActivityState.Idle)
      expect(states['tab3']).toBe(ActivityState.UserInputRequired)
    })
  })

  describe('transitions', () => {
    it('logs the first transition from idle with the detail and a timestamp', () => {
      vi.useFakeTimers()
      vi.setSystemTime(1_700_000_000_000)
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working, VIEWPORT)

      expect(useActivityStateStore.getState().transitions['tab1']).toEqual([
        { timestamp: 1_700_000_000_000, from: ActivityState.Idle, to: ActivityState.Working, detail: VIEWPORT },
      ])
    })

    it('chains from the previous state and records same-state changes', () => {
      const idle: ActivityTransitionDetail = { kind: ActivityTransitionKind.ViewportIdle, snapshot: 'done', idleTimeoutMs: 2000 }
      const reclassified: ActivityTransitionDetail = { kind: ActivityTransitionKind.Classification, snapshot: 'done', reason: 'prompt' }
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working, VIEWPORT)
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Idle, idle)
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Idle, reclassified)

      const log = useActivityStateStore.getState().transitions['tab1'] ?? []
      expect(log.map((t) => [t.from, t.to])).toEqual([
        [ActivityState.Idle, ActivityState.Working],
        [ActivityState.Working, ActivityState.Idle],
        [ActivityState.Idle, ActivityState.Idle],
      ])
      expect(log[1]?.detail).toEqual(idle)
      expect(log[2]?.detail).toEqual(reclassified)
    })

    it('keeps each tab\'s log separate', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working, VIEWPORT)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.Working, VIEWPORT)

      expect(useActivityStateStore.getState().transitions['tab1']).toHaveLength(1)
      expect(useActivityStateStore.getState().transitions['tab2']).toHaveLength(1)
    })

    it('drops the oldest entries past the cap', () => {
      for (let i = 0; i < MAX_TRANSITIONS + 2; i++) {
        useActivityStateStore.getState().setTabState('tab1', ActivityState.Working, { ...VIEWPORT, snapshot: String(i) })
      }

      const log = useActivityStateStore.getState().transitions['tab1'] ?? []
      expect(log).toHaveLength(MAX_TRANSITIONS)
      expect(log[0]?.detail.snapshot).toBe('2')
      expect(log[log.length - 1]?.detail.snapshot).toBe(String(MAX_TRANSITIONS + 1))
    })

    it('updates the state and the log in one notification', () => {
      const listener = vi.fn()
      const unsubscribe = useActivityStateStore.subscribe(listener)
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working, VIEWPORT)
      unsubscribe()

      expect(listener).toHaveBeenCalledTimes(1)
    })
  })

  describe('removeTabState', () => {
    it('removes state and transitions for a tab', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working, VIEWPORT)
      useActivityStateStore.getState().removeTabState('tab1')

      expect(useActivityStateStore.getState().states['tab1']).toBeUndefined()
      expect(useActivityStateStore.getState().transitions['tab1']).toBeUndefined()
    })

    it('does nothing when removing non-existent tab', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working, VIEWPORT)
      useActivityStateStore.getState().removeTabState('tab2')

      expect(useActivityStateStore.getState().states['tab1']).toBe(ActivityState.Working)
      expect(Object.keys(useActivityStateStore.getState().states)).toHaveLength(1)
    })
  })

  describe('getWorkspaceState', () => {
    it('returns idle when no tabs', () => {
      const state = useActivityStateStore.getState().getWorkspaceState([])

      expect(state).toBe(ActivityState.Idle)
    })

    it('returns idle when all tabs are idle', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Idle, VIEWPORT)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.Idle, VIEWPORT)

      const state = useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])

      expect(state).toBe(ActivityState.Idle)
    })

    it('returns working when any tab is working', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Idle, VIEWPORT)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.Working, VIEWPORT)

      const state = useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])

      expect(state).toBe(ActivityState.Working)
    })

    it('returns user_input_required when no working but some waiting', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Idle, VIEWPORT)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.UserInputRequired, VIEWPORT)

      const state = useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])

      expect(state).toBe(ActivityState.UserInputRequired)
    })

    it('returns working over user_input_required', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working, VIEWPORT)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.UserInputRequired, VIEWPORT)

      const state = useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])

      expect(state).toBe(ActivityState.Working)
    })

    it('returns permission_request over safe_permission_requested', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.SafePermissionRequested, VIEWPORT)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.PermissionRequest, VIEWPORT)

      expect(useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])).toBe(ActivityState.PermissionRequest)
    })

    it('returns safe_permission_requested over user_input_required', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.UserInputRequired, VIEWPORT)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.SafePermissionRequested, VIEWPORT)

      expect(useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])).toBe(ActivityState.SafePermissionRequested)
    })

    it('returns error over completed and idle', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Completed, VIEWPORT)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.Error, VIEWPORT)
      useActivityStateStore.getState().setTabState('tab3', ActivityState.Idle, VIEWPORT)

      expect(useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2', 'tab3'])).toBe(ActivityState.Error)
    })

    it('returns completed over idle', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Idle, VIEWPORT)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.Completed, VIEWPORT)

      expect(useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])).toBe(ActivityState.Completed)
    })

    it('ignores tabs not in the provided list', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working, VIEWPORT)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.Idle, VIEWPORT)
      useActivityStateStore.getState().setTabState('tab3', ActivityState.Working, VIEWPORT)

      const state = useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])

      expect(state).toBe(ActivityState.Working)
    })
  })
})
