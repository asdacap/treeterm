import { describe, it, expect, beforeEach } from 'vitest'
import { usePrefixModeStore } from './prefixMode'

describe('PrefixModeStore', () => {
  beforeEach(() => {
    // Reset store to initial state
    usePrefixModeStore.setState({
      state: 'idle',
      activatedAt: null,
      focusedWorkspaceIndex: 0,
      workspaceIds: []
    })
  })

  describe('initial state', () => {
    it('starts in idle state', () => {
      expect(usePrefixModeStore.getState().state).toBe('idle')
      expect(usePrefixModeStore.getState().activatedAt).toBeNull()
      expect(usePrefixModeStore.getState().focusedWorkspaceIndex).toBe(0)
      expect(usePrefixModeStore.getState().workspaceIds).toEqual([])
    })
  })

  describe('activate', () => {
    it('sets state to active', () => {
      usePrefixModeStore.getState().activate()
      
      expect(usePrefixModeStore.getState().state).toBe('active')
      expect(usePrefixModeStore.getState().activatedAt).not.toBeNull()
    })
  })

  describe('deactivate', () => {
    it('resets state to idle', () => {
      usePrefixModeStore.getState().activate()
      usePrefixModeStore.getState().deactivate()
      
      expect(usePrefixModeStore.getState().state).toBe('idle')
      expect(usePrefixModeStore.getState().activatedAt).toBeNull()
      expect(usePrefixModeStore.getState().focusedWorkspaceIndex).toBe(0)
      expect(usePrefixModeStore.getState().workspaceIds).toEqual([])
    })
  })

  describe('enterWorkspaceFocus', () => {
    it('sets state to workspace_focus', () => {
      usePrefixModeStore.getState().enterWorkspaceFocus(['ws1', 'ws2', 'ws3'], 1)
      
      expect(usePrefixModeStore.getState().state).toBe('workspace_focus')
      expect(usePrefixModeStore.getState().workspaceIds).toEqual(['ws1', 'ws2', 'ws3'])
      expect(usePrefixModeStore.getState().focusedWorkspaceIndex).toBe(1)
    })

    it('sets activatedAt timestamp', () => {
      const before = Date.now()
      
      usePrefixModeStore.getState().enterWorkspaceFocus(['ws1'], 0)
      
      const after = Date.now()
      expect(usePrefixModeStore.getState().activatedAt).toBeGreaterThanOrEqual(before)
      expect(usePrefixModeStore.getState().activatedAt).toBeLessThanOrEqual(after)
    })
  })

  describe('navigateWorkspace', () => {
    it('does nothing when no workspaces', () => {
      usePrefixModeStore.getState().activate()
      usePrefixModeStore.getState().navigateWorkspace('down')
      
      expect(usePrefixModeStore.getState().focusedWorkspaceIndex).toBe(0)
    })

    it('moves to next workspace when navigating down', () => {
      usePrefixModeStore.getState().enterWorkspaceFocus(['ws1', 'ws2', 'ws3'], 0)
      usePrefixModeStore.getState().navigateWorkspace('down')
      
      expect(usePrefixModeStore.getState().focusedWorkspaceIndex).toBe(1)
    })

    it('wraps to first workspace when navigating down from last', () => {
      usePrefixModeStore.getState().enterWorkspaceFocus(['ws1', 'ws2', 'ws3'], 2)
      usePrefixModeStore.getState().navigateWorkspace('down')
      
      expect(usePrefixModeStore.getState().focusedWorkspaceIndex).toBe(0)
    })

    it('moves to previous workspace when navigating up', () => {
      usePrefixModeStore.getState().enterWorkspaceFocus(['ws1', 'ws2', 'ws3'], 2)
      usePrefixModeStore.getState().navigateWorkspace('up')
      
      expect(usePrefixModeStore.getState().focusedWorkspaceIndex).toBe(1)
    })

    it('wraps to last workspace when navigating up from first', () => {
      usePrefixModeStore.getState().enterWorkspaceFocus(['ws1', 'ws2', 'ws3'], 0)
      usePrefixModeStore.getState().navigateWorkspace('up')
      
      expect(usePrefixModeStore.getState().focusedWorkspaceIndex).toBe(2)
    })

    it('handles single workspace', () => {
      usePrefixModeStore.getState().enterWorkspaceFocus(['ws1'], 0)
      usePrefixModeStore.getState().navigateWorkspace('down')
      usePrefixModeStore.getState().navigateWorkspace('up')
      
      expect(usePrefixModeStore.getState().focusedWorkspaceIndex).toBe(0)
    })
  })

  describe('selectFocusedWorkspace', () => {
    it('returns null when no workspaces', () => {
      const result = usePrefixModeStore.getState().selectFocusedWorkspace()
      
      expect(result).toBeNull()
    })

    it('returns the focused workspace ID', () => {
      usePrefixModeStore.getState().enterWorkspaceFocus(['ws1', 'ws2', 'ws3'], 1)
      const result = usePrefixModeStore.getState().selectFocusedWorkspace()
      
      expect(result).toBe('ws2')
    })

    it('returns null when index is out of bounds', () => {
      usePrefixModeStore.setState({
        workspaceIds: ['ws1'],
        focusedWorkspaceIndex: 5
      })
      const result = usePrefixModeStore.getState().selectFocusedWorkspace()
      
      expect(result).toBeNull()
    })
  })
})
