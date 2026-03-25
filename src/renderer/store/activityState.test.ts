import { describe, it, expect, beforeEach } from 'vitest'
import { useActivityStateStore } from './activityState'

describe('ActivityStateStore', () => {
  beforeEach(() => {
    // Reset store by setting states to empty object
    useActivityStateStore.setState({ states: {} })
  })

  describe('setTabState', () => {
    it('sets state for a tab', () => {
      useActivityStateStore.getState().setTabState('tab1', 'working')
      
      expect(useActivityStateStore.getState().states['tab1']).toBe('working')
    })

    it('updates state for existing tab', () => {
      useActivityStateStore.getState().setTabState('tab1', 'idle')
      useActivityStateStore.getState().setTabState('tab1', 'working')
      
      expect(useActivityStateStore.getState().states['tab1']).toBe('working')
    })

    it('maintains states for multiple tabs', () => {
      useActivityStateStore.getState().setTabState('tab1', 'working')
      useActivityStateStore.getState().setTabState('tab2', 'idle')
      useActivityStateStore.getState().setTabState('tab3', 'user_input_required')
      
      const states = useActivityStateStore.getState().states
      expect(Object.keys(states)).toHaveLength(3)
      expect(states['tab1']).toBe('working')
      expect(states['tab2']).toBe('idle')
      expect(states['tab3']).toBe('user_input_required')
    })
  })

  describe('removeTabState', () => {
    it('removes state for a tab', () => {
      useActivityStateStore.getState().setTabState('tab1', 'working')
      useActivityStateStore.getState().removeTabState('tab1')
      
      expect(useActivityStateStore.getState().states['tab1']).toBeUndefined()
    })

    it('does nothing when removing non-existent tab', () => {
      useActivityStateStore.getState().setTabState('tab1', 'working')
      useActivityStateStore.getState().removeTabState('tab2')
      
      expect(useActivityStateStore.getState().states['tab1']).toBe('working')
      expect(Object.keys(useActivityStateStore.getState().states)).toHaveLength(1)
    })
  })

  describe('getWorkspaceState', () => {
    it('returns idle when no tabs', () => {
      const state = useActivityStateStore.getState().getWorkspaceState([])
      
      expect(state).toBe('idle')
    })

    it('returns idle when all tabs are idle', () => {
      useActivityStateStore.getState().setTabState('tab1', 'idle')
      useActivityStateStore.getState().setTabState('tab2', 'idle')
      
      const state = useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])
      
      expect(state).toBe('idle')
    })

    it('returns working when any tab is working', () => {
      useActivityStateStore.getState().setTabState('tab1', 'idle')
      useActivityStateStore.getState().setTabState('tab2', 'working')
      
      const state = useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])
      
      expect(state).toBe('working')
    })

    it('returns user_input_required when no working but some waiting', () => {
      useActivityStateStore.getState().setTabState('tab1', 'idle')
      useActivityStateStore.getState().setTabState('tab2', 'user_input_required')
      
      const state = useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])
      
      expect(state).toBe('user_input_required')
    })

    it('returns working over user_input_required', () => {
      useActivityStateStore.getState().setTabState('tab1', 'working')
      useActivityStateStore.getState().setTabState('tab2', 'user_input_required')

      const state = useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])

      expect(state).toBe('working')
    })

    it('returns permission_request over safe_permission_requested', () => {
      useActivityStateStore.getState().setTabState('tab1', 'safe_permission_requested')
      useActivityStateStore.getState().setTabState('tab2', 'permission_request')

      expect(useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])).toBe('permission_request')
    })

    it('returns safe_permission_requested over user_input_required', () => {
      useActivityStateStore.getState().setTabState('tab1', 'user_input_required')
      useActivityStateStore.getState().setTabState('tab2', 'safe_permission_requested')

      expect(useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])).toBe('safe_permission_requested')
    })

    it('returns error over completed and idle', () => {
      useActivityStateStore.getState().setTabState('tab1', 'completed')
      useActivityStateStore.getState().setTabState('tab2', 'error')
      useActivityStateStore.getState().setTabState('tab3', 'idle')

      expect(useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2', 'tab3'])).toBe('error')
    })

    it('returns completed over idle', () => {
      useActivityStateStore.getState().setTabState('tab1', 'idle')
      useActivityStateStore.getState().setTabState('tab2', 'completed')

      expect(useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])).toBe('completed')
    })

    it('ignores tabs not in the provided list', () => {
      useActivityStateStore.getState().setTabState('tab1', 'working')
      useActivityStateStore.getState().setTabState('tab2', 'idle')
      useActivityStateStore.getState().setTabState('tab3', 'working')
      
      const state = useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])
      
      expect(state).toBe('working')
    })
  })
})
