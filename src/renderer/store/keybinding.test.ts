import { describe, it, expect, beforeEach, vi } from 'vitest'

// Stub window for node environment (activate uses window.setTimeout)
vi.stubGlobal('window', {
  setTimeout: (fn: () => void, ms: number) => setTimeout(fn, ms),
  clearTimeout: (id: number) => clearTimeout(id),
  addEventListener: vi.fn(),
  removeEventListener: vi.fn(),
})

// Mock dependencies
vi.mock('./settings', () => ({
  useSettingsStore: {
    getState: vi.fn(() => ({
      settings: {
        prefixMode: { enabled: true, prefixKey: 'Control+B', timeout: 1500 },
        keybindings: {
          newTab: 'c', closeTab: 'x', nextTab: 'n',
          prevTab: 'p', openSettings: ',', workspaceFocus: 'w'
        }
      }
    }))
  }
}))
vi.mock('tinykeys', () => ({
  parseKeybinding: vi.fn(() => [])
}))
vi.mock('../utils/keybindingConverter', () => ({
  convertDirectKeybinding: vi.fn(() => '$mod+b')
}))

import { useKeybindingStore, matchesKeybinding } from './keybinding'

describe('useKeybindingStore', () => {
  beforeEach(() => {
    useKeybindingStore.getState().dispose()
    useKeybindingStore.setState({
      prefixState: 'idle',
      activatedAt: null,
      focusedWorkspaceIndex: 0,
      workspaceIds: [],
      handlers: {}
    })
  })

  describe('initial state', () => {
    it('starts in idle state with no handlers', () => {
      const state = useKeybindingStore.getState()
      expect(state.prefixState).toBe('idle')
      expect(state.activatedAt).toBeNull()
      expect(state.focusedWorkspaceIndex).toBe(0)
      expect(state.workspaceIds).toEqual([])
      expect(state.handlers).toEqual({})
    })
  })

  describe('activate/deactivate', () => {
    it('activates and deactivates prefix mode', () => {
      useKeybindingStore.getState().activate()
      expect(useKeybindingStore.getState().prefixState).toBe('active')
      expect(useKeybindingStore.getState().activatedAt).not.toBeNull()

      useKeybindingStore.getState().deactivate()
      expect(useKeybindingStore.getState().prefixState).toBe('idle')
      expect(useKeybindingStore.getState().activatedAt).toBeNull()
      expect(useKeybindingStore.getState().focusedWorkspaceIndex).toBe(0)
      expect(useKeybindingStore.getState().workspaceIds).toEqual([])
    })
  })

  describe('enterWorkspaceFocus', () => {
    it('sets state to workspace_focus with workspace data', () => {
      const before = Date.now()
      useKeybindingStore.getState().enterWorkspaceFocus(['ws1', 'ws2', 'ws3'], 1)
      const after = Date.now()

      const state = useKeybindingStore.getState()
      expect(state.prefixState).toBe('workspace_focus')
      expect(state.workspaceIds).toEqual(['ws1', 'ws2', 'ws3'])
      expect(state.focusedWorkspaceIndex).toBe(1)
      expect(state.activatedAt).toBeGreaterThanOrEqual(before)
      expect(state.activatedAt).toBeLessThanOrEqual(after)
    })
  })

  describe('navigateWorkspace', () => {
    it.each([
      { start: 0, direction: 'down' as const, expected: 1, desc: 'moves down' },
      { start: 2, direction: 'down' as const, expected: 0, desc: 'wraps down from last' },
      { start: 2, direction: 'up' as const, expected: 1, desc: 'moves up' },
      { start: 0, direction: 'up' as const, expected: 2, desc: 'wraps up from first' },
    ])('$desc (start=$start, dir=$direction -> $expected)', ({ start, direction, expected }) => {
      useKeybindingStore.getState().enterWorkspaceFocus(['ws1', 'ws2', 'ws3'], start)
      useKeybindingStore.getState().navigateWorkspace(direction)
      expect(useKeybindingStore.getState().focusedWorkspaceIndex).toBe(expected)
    })

    it('does nothing when no workspaces', () => {
      useKeybindingStore.getState().activate()
      useKeybindingStore.getState().navigateWorkspace('down')
      expect(useKeybindingStore.getState().focusedWorkspaceIndex).toBe(0)
    })

    it('handles single workspace', () => {
      useKeybindingStore.getState().enterWorkspaceFocus(['ws1'], 0)
      useKeybindingStore.getState().navigateWorkspace('down')
      useKeybindingStore.getState().navigateWorkspace('up')
      expect(useKeybindingStore.getState().focusedWorkspaceIndex).toBe(0)
    })
  })

  describe('selectFocusedWorkspace', () => {
    it.each([
      { workspaces: [], index: 0, expected: null, desc: 'null when no workspaces' },
      { workspaces: ['ws1', 'ws2', 'ws3'], index: 1, expected: 'ws2', desc: 'returns focused workspace' },
      { workspaces: ['ws1'], index: 5, expected: null, desc: 'null when out of bounds' },
    ])('returns $desc', ({ workspaces, index, expected }) => {
      if (workspaces.length > 0) {
        useKeybindingStore.getState().enterWorkspaceFocus(workspaces, index)
        // Override index for out-of-bounds test
        if (index >= workspaces.length) {
          useKeybindingStore.setState({ focusedWorkspaceIndex: index })
        }
      }
      expect(useKeybindingStore.getState().selectFocusedWorkspace()).toBe(expected)
    })
  })

  describe('setHandlers', () => {
    it('stores registered handlers', () => {
      const handler = vi.fn()
      useKeybindingStore.getState().setHandlers({ newTab: handler })
      expect(useKeybindingStore.getState().handlers.newTab).toBe(handler)
    })
  })
})

describe('matchesKeybinding', () => {
  function createKeyEvent(overrides: Partial<KeyboardEvent> = {}): KeyboardEvent {
    return { key: '', ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, ...overrides } as KeyboardEvent
  }

  it.each([
    { key: 't', modifiers: [] as string[], event: { key: 't' }, expected: true, desc: 'plain key' },
    { key: 't', modifiers: [] as string[], event: { key: 'x' }, expected: false, desc: 'different key' },
    { key: 'b', modifiers: ['Meta'], event: { key: 'b', metaKey: true }, expected: true, desc: 'Meta modifier' },
    { key: 'b', modifiers: ['Meta'], event: { key: 'b', metaKey: false }, expected: false, desc: 'Meta required but not pressed' },
    { key: 'b', modifiers: ['Meta'], event: { key: 'b', metaKey: true, shiftKey: true }, expected: false, desc: 'extra modifier pressed' },
    { key: 'a', modifiers: ['Control'], event: { key: 'a', ctrlKey: true }, expected: true, desc: 'Control modifier' },
    { key: 'k', modifiers: ['Control', 'Shift'], event: { key: 'k', ctrlKey: true, shiftKey: true }, expected: true, desc: 'multiple modifiers' },
    { key: 'k', modifiers: ['Control', 'Shift'], event: { key: 'k', ctrlKey: true, shiftKey: false }, expected: false, desc: 'missing one modifier' },
    { key: 't', modifiers: [] as string[], event: { key: 'T' }, expected: true, desc: 'case insensitive' },
    { key: 'x', modifiers: ['Alt'], event: { key: 'x', altKey: true }, expected: true, desc: 'Alt modifier' },
    { key: 'z', modifiers: ['Control', 'Shift', 'Alt', 'Meta'], event: { key: 'z', ctrlKey: true, shiftKey: true, altKey: true, metaKey: true }, expected: true, desc: 'all four modifiers' },
  ])('$desc -> $expected', ({ key, modifiers, event, expected }) => {
    expect(matchesKeybinding(createKeyEvent(event), modifiers, key)).toBe(expected)
  })
})
