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
      useActivityStateStore.getState().setTabState('tab3', 'waiting_for_input')
      
      const states = useActivityStateStore.getState().states
      expect(Object.keys(states)).toHaveLength(3)
      expect(states['tab1']).toBe('working')
      expect(states['tab2']).toBe('idle')
      expect(states['tab3']).toBe('waiting_for_input')
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

    it('returns waiting_for_input when no working but some waiting', () => {
      useActivityStateStore.getState().setTabState('tab1', 'idle')
      useActivityStateStore.getState().setTabState('tab2', 'waiting_for_input')
      
      const state = useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])
      
      expect(state).toBe('waiting_for_input')
    })

    it('returns working over waiting_for_input', () => {
      useActivityStateStore.getState().setTabState('tab1', 'working')
      useActivityStateStore.getState().setTabState('tab2', 'waiting_for_input')
      
      const state = useActivityStateStore.getState().getWorkspaceState(['tab1', 'tab2'])
      
      expect(state).toBe('working')
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
