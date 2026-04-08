import { describe, it, expect, beforeEach } from 'vitest'
import { useActivityStateStore } from './activityState'
import { ActivityState } from '../types'

describe('ActivityStateStore', () => {
  beforeEach(() => {
    // Reset store by setting states to empty object
    useActivityStateStore.setState({ states: {} })
  })

  describe('setTabState', () => {
    it('sets state for a tab', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working)

      expect(useActivityStateStore.getState().states['tab1']).toBe(ActivityState.Working)
    })

    it('updates state for existing tab', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Idle)
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working)

      expect(useActivityStateStore.getState().states['tab1']).toBe(ActivityState.Working)
    })

    it('maintains states for multiple tabs', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.Idle)
      useActivityStateStore.getState().setTabState('tab3', ActivityState.UserInputRequired)

      const states = useActivityStateStore.getState().states
      expect(Object.keys(states)).toHaveLength(3)
      expect(states['tab1']).toBe(ActivityState.Working)
      expect(states['tab2']).toBe(ActivityState.Idle)
      expect(states['tab3']).toBe(ActivityState.UserInputRequired)
    })
  })

  describe('removeTabState', () => {
    it('removes state for a tab', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working)
      useActivityStateStore.getState().removeTabState('tab1')

      expect(useActivityStateStore.getState().states['tab1']).toBeUndefined()
    })

    it('does nothing when removing non-existent tab', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working)
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
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Idle)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.Idle)

      const state = useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])

      expect(state).toBe(ActivityState.Idle)
    })

    it('returns working when any tab is working', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Idle)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.Working)

      const state = useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])

      expect(state).toBe(ActivityState.Working)
    })

    it('returns user_input_required when no working but some waiting', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Idle)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.UserInputRequired)

      const state = useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])

      expect(state).toBe(ActivityState.UserInputRequired)
    })

    it('returns working over user_input_required', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.UserInputRequired)

      const state = useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])

      expect(state).toBe(ActivityState.Working)
    })

    it('returns permission_request over safe_permission_requested', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.SafePermissionRequested)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.PermissionRequest)

      expect(useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])).toBe(ActivityState.PermissionRequest)
    })

    it('returns safe_permission_requested over user_input_required', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.UserInputRequired)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.SafePermissionRequested)

      expect(useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])).toBe(ActivityState.SafePermissionRequested)
    })

    it('returns error over completed and idle', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Completed)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.Error)
      useActivityStateStore.getState().setTabState('tab3', ActivityState.Idle)

      expect(useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2', 'tab3'])).toBe(ActivityState.Error)
    })

    it('returns completed over idle', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Idle)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.Completed)

      expect(useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])).toBe(ActivityState.Completed)
    })

    it('ignores tabs not in the provided list', () => {
      useActivityStateStore.getState().setTabState('tab1', ActivityState.Working)
      useActivityStateStore.getState().setTabState('tab2', ActivityState.Idle)
      useActivityStateStore.getState().setTabState('tab3', ActivityState.Working)

      const state = useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])

      expect(state).toBe(ActivityState.Working)
    })
  })
})
