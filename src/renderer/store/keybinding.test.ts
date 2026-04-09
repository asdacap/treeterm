import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'

const addEventListenerMock = vi.fn()
const removeEventListenerMock = vi.fn()

const mockTarget = {
  addEventListener: addEventListenerMock,
  removeEventListener: removeEventListenerMock,
} as unknown as { addEventListener: Window['addEventListener']; removeEventListener: Window['removeEventListener'] }

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
  parseKeybinding: vi.fn(() => [[['Control'], 'b']])
}))
vi.mock('../utils/keybindingConverter', () => ({
  convertDirectKeybinding: vi.fn(() => '$mod+b')
}))

import { useKeybindingStore, matchesKeybinding, PrefixModeState } from './keybinding'

function createKeyEvent(overrides: Partial<KeyboardEvent> = {}): KeyboardEvent {
  return {
    key: '',
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    defaultPrevented: false,
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
    ...overrides,
  } as unknown as KeyboardEvent
}

/** Call init(), return the captured handleKeyDown listener */
function initAndGetHandler(): (e: KeyboardEvent) => void {
  addEventListenerMock.mockClear()
  useKeybindingStore.getState().init(mockTarget)
  return addEventListenerMock.mock.calls[0]![1] as (e: KeyboardEvent) => void
}

describe('useKeybindingStore', () => {
  beforeEach(() => {
    useKeybindingStore.getState().dispose()
    useKeybindingStore.setState({
      prefixState: PrefixModeState.Idle,
      activatedAt: null,
      focusedWorkspaceIndex: 0,
      workspaceIds: [],
      handlers: {}
    })
    addEventListenerMock.mockClear()
    removeEventListenerMock.mockClear()
  })

  describe('initial state', () => {
    it('starts in idle state with no handlers', () => {
      const state = useKeybindingStore.getState()
      expect(state.prefixState).toBe(PrefixModeState.Idle)
      expect(state.activatedAt).toBeNull()
      expect(state.focusedWorkspaceIndex).toBe(0)
      expect(state.workspaceIds).toEqual([])
      expect(state.handlers).toEqual({})
    })
  })

  describe('activate/deactivate', () => {
    it('activates and deactivates prefix mode', () => {
      useKeybindingStore.getState().activate()
      expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.Active)
      expect(useKeybindingStore.getState().activatedAt).not.toBeNull()

      useKeybindingStore.getState().deactivate()
      expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.Idle)
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
      expect(state.prefixState).toBe(PrefixModeState.WorkspaceFocus)
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

  describe('handleKeyDown', () => {
    it('pressing prefix key in idle mode activates', () => {
      const handleKeyDown = initAndGetHandler()

      handleKeyDown(createKeyEvent({ key: 'b', ctrlKey: true }))
      expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.Active)
    })

    it('pressing prefix key in active mode deactivates (toggle)', () => {
      const handleKeyDown = initAndGetHandler()

      handleKeyDown(createKeyEvent({ key: 'b', ctrlKey: true }))
      expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.Active)

      handleKeyDown(createKeyEvent({ key: 'b', ctrlKey: true }))
      expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.Idle)
    })

    it('prefix key preventDefault and stopPropagation', () => {
      const handleKeyDown = initAndGetHandler()
      const event = createKeyEvent({ key: 'b', ctrlKey: true })

      handleKeyDown(event)
      // eslint-disable-next-line @typescript-eslint/unbound-method
      expect(event.preventDefault).toHaveBeenCalled()
      // eslint-disable-next-line @typescript-eslint/unbound-method
      expect(event.stopPropagation).toHaveBeenCalled()
    })

    describe('in active prefix mode', () => {
      let handleKeyDown: (e: KeyboardEvent) => void

      beforeEach(() => {
        handleKeyDown = initAndGetHandler()
        // Activate prefix mode
        handleKeyDown(createKeyEvent({ key: 'b', ctrlKey: true }))
      })

      it.each([
        { key: 'c', handler: 'newTab' },
        { key: 'x', handler: 'closeTab' },
        { key: 'n', handler: 'nextTab' },
        { key: 'p', handler: 'prevTab' },
        { key: ',', handler: 'openSettings' },
        { key: 'w', handler: 'workspaceFocus' },
      ])('pressing "$key" calls $handler handler and deactivates', ({ key, handler }) => {
        const handlerFn = vi.fn()
        useKeybindingStore.getState().setHandlers({ [handler]: handlerFn })

        handleKeyDown(createKeyEvent({ key }))
        expect(handlerFn).toHaveBeenCalledTimes(1)
        expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.Idle)
      })

      it('Escape deactivates without calling handlers', () => {
        const newTab = vi.fn()
        useKeybindingStore.getState().setHandlers({ newTab })

        handleKeyDown(createKeyEvent({ key: 'Escape' }))
        expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.Idle)
        expect(newTab).not.toHaveBeenCalled()
      })

      it('unknown non-modifier key deactivates', () => {
        handleKeyDown(createKeyEvent({ key: 'z' }))
        expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.Idle)
      })

      it('modifier-only key (Shift) does NOT deactivate', () => {
        handleKeyDown(createKeyEvent({ key: 'Shift', shiftKey: true }))
        expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.Active)
      })

      it('modifier-only key (Meta) does NOT deactivate', () => {
        handleKeyDown(createKeyEvent({ key: 'Meta', metaKey: true }))
        expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.Active)
      })
    })

    describe('in workspace_focus mode', () => {
      let handleKeyDown: (e: KeyboardEvent) => void

      beforeEach(() => {
        handleKeyDown = initAndGetHandler()
        useKeybindingStore.getState().enterWorkspaceFocus(['ws1', 'ws2', 'ws3'], 1)
      })

      it('Escape deactivates', () => {
        handleKeyDown(createKeyEvent({ key: 'Escape' }))
        expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.Idle)
      })

      it('Enter selects workspace and calls setActiveWorkspace', () => {
        const setActiveWorkspace = vi.fn()
        useKeybindingStore.getState().setHandlers({ setActiveWorkspace })

        handleKeyDown(createKeyEvent({ key: 'Enter' }))
        expect(setActiveWorkspace).toHaveBeenCalledWith('ws2')
        expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.Idle)
      })

      it('ArrowUp navigates up', () => {
        handleKeyDown(createKeyEvent({ key: 'ArrowUp' }))
        expect(useKeybindingStore.getState().focusedWorkspaceIndex).toBe(0)
      })

      it('ArrowDown navigates down', () => {
        handleKeyDown(createKeyEvent({ key: 'ArrowDown' }))
        expect(useKeybindingStore.getState().focusedWorkspaceIndex).toBe(2)
      })
    })

    describe('in idle mode', () => {
      it('Cmd+1-9 calls switchToTab with index', () => {
        const handleKeyDown = initAndGetHandler()
        const switchToTab = vi.fn()
        useKeybindingStore.getState().setHandlers({ switchToTab })

        handleKeyDown(createKeyEvent({ key: '3', metaKey: true }))
        expect(switchToTab).toHaveBeenCalledWith(2)
      })

      it('Ctrl+1-9 calls switchToTab with index', () => {
        const handleKeyDown = initAndGetHandler()
        const switchToTab = vi.fn()
        useKeybindingStore.getState().setHandlers({ switchToTab })

        handleKeyDown(createKeyEvent({ key: '1', ctrlKey: true }))
        expect(switchToTab).toHaveBeenCalledWith(0)
      })
    })
  })

  describe('timeout', () => {
    beforeEach(() => vi.useFakeTimers())
    afterEach(() => vi.useRealTimers())

    it('activate auto-deactivates after timeout', () => {
      useKeybindingStore.getState().activate()
      expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.Active)

      vi.advanceTimersByTime(1500)
      expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.Idle)
    })

    it('enterWorkspaceFocus auto-deactivates after timeout', () => {
      useKeybindingStore.getState().enterWorkspaceFocus(['ws1'], 0)
      expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.WorkspaceFocus)

      vi.advanceTimersByTime(1500)
      expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.Idle)
    })

    it('dispose clears timeout so it does not fire', () => {
      useKeybindingStore.getState().activate()
      expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.Active)

      useKeybindingStore.getState().dispose()

      // Re-activate — if timeout wasn't cleared, it would fire and deactivate
      useKeybindingStore.getState().activate()
      vi.advanceTimersByTime(1400) // less than timeout
      expect(useKeybindingStore.getState().prefixState).toBe(PrefixModeState.Active)
    })
  })

  describe('init/dispose lifecycle', () => {
    it('init registers keydown listener', () => {
      useKeybindingStore.getState().init(mockTarget)
      expect(addEventListenerMock).toHaveBeenCalledWith('keydown', expect.any(Function), true)
    })

    it('dispose removes keydown listener', () => {
      useKeybindingStore.getState().init(mockTarget)
      useKeybindingStore.getState().dispose()
      expect(removeEventListenerMock).toHaveBeenCalledWith('keydown', expect.any(Function), true)
    })
  })
})

describe('matchesKeybinding', () => {
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
    const e = { ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, ...event } as KeyboardEvent
    expect(matchesKeybinding(e, modifiers, key)).toBe(expected)
  })
})
