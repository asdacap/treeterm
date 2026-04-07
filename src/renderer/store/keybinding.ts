import { create } from 'zustand'
// @ts-expect-error tinykeys has incorrect exports config
import { parseKeybinding } from 'tinykeys'
import { useSettingsStore } from './settings'
import { convertDirectKeybinding } from '../utils/keybindingConverter'
import type { Settings } from '../types'

export type PrefixModeState = 'idle' | 'active' | 'workspace_focus'

export interface KeybindingHandlers {
  newTab?: () => void
  closeTab?: () => void
  nextTab?: () => void
  prevTab?: () => void
  openSettings?: () => void
  workspaceFocus?: () => void
  setActiveWorkspace?: (id: string | null) => void
  switchToTab?: (index: number) => void
}

interface KeybindingStore {
  // Prefix mode state
  prefixState: PrefixModeState
  activatedAt: number | null
  focusedWorkspaceIndex: number
  workspaceIds: string[]

  // Registered action handlers
  handlers: KeybindingHandlers

  // Lifecycle
  init: () => void
  dispose: () => void

  // Handler registration
  setHandlers: (handlers: KeybindingHandlers) => void

  // Prefix mode actions
  activate: () => void
  deactivate: () => void
  enterWorkspaceFocus: (workspaceIds: string[], currentIndex: number) => void
  navigateWorkspace: (direction: 'up' | 'down') => void
  selectFocusedWorkspace: () => string | null
}

// Helper function to match a keybinding event
export function matchesKeybinding(event: KeyboardEvent, modifiers: string[], key: string): boolean {
  const hasControl = modifiers.includes('Control')
  const hasShift = modifiers.includes('Shift')
  const hasAlt = modifiers.includes('Alt')
  const hasMeta = modifiers.includes('Meta')

  const controlMatch = hasControl ? event.ctrlKey : !event.ctrlKey
  const shiftMatch = hasShift ? event.shiftKey : !event.shiftKey
  const altMatch = hasAlt ? event.altKey : !event.altKey
  const metaMatch = hasMeta ? event.metaKey : !event.metaKey

  const eventKey = event.key.toLowerCase()
  const targetKey = key.toLowerCase()

  return controlMatch && shiftMatch && altMatch && metaMatch && eventKey === targetKey
}

// Module-level variables for internal state that shouldn't trigger re-renders
let timeoutId: number | null = null
let cleanupListener: (() => void) | null = null

export const useKeybindingStore = create<KeybindingStore>((set, get) => ({
  prefixState: 'idle',
  activatedAt: null,
  focusedWorkspaceIndex: 0,
  workspaceIds: [],
  handlers: {},

  init: () => {
    // Clean up any previous listener
    cleanupListener?.()

    // Guard for non-browser environments (e.g. Node tests)
    if (typeof window === 'undefined') return

    const handleKeyDown = (e: KeyboardEvent): void => {
      const { settings } = useSettingsStore.getState()
      const { prefixState, handlers } = get()
      const { prefixMode, keybindings } = settings

      // Check for prefix key activation
      const prefixKeybinding = convertDirectKeybinding(prefixMode.prefixKey)
      // eslint-disable-next-line @typescript-eslint/no-unsafe-call
      const prefixParts = parseKeybinding(prefixKeybinding) as [string[], string][]

      if (prefixParts.length > 0) {
        const [modifiers, key] = prefixParts[0]!
        if (matchesKeybinding(e, modifiers, key)) {
          e.preventDefault()
          e.stopPropagation()
          if (prefixState === 'idle') {
            get().activate()
          } else {
            get().deactivate()
          }
          return
        }
      }

      // If in prefix mode, check for action keys
      if (prefixState === 'active') {
        const actionKey = e.key.toLowerCase()

        if (actionKey === 'escape') {
          e.preventDefault()
          e.stopPropagation()
          get().deactivate()
          return
        }

        type NoArgKey = keyof {
          [K in keyof KeybindingHandlers as KeybindingHandlers[K] extends (() => void) | undefined ? K : never]: true
        }
        const actionMap: Record<keyof Settings['keybindings'], NoArgKey> = {
          newTab: 'newTab',
          closeTab: 'closeTab',
          nextTab: 'nextTab',
          prevTab: 'prevTab',
          openSettings: 'openSettings',
          workspaceFocus: 'workspaceFocus'
        }

        for (const [action, handlerKey] of Object.entries(actionMap)) {
          const binding = keybindings[action as keyof Settings['keybindings']]
          if (binding.toLowerCase() === actionKey) {
            e.preventDefault()
            e.stopPropagation()
            get().deactivate()
            handlers[handlerKey]?.()
            return
          }
        }

        // Unknown key in prefix mode - deactivate (but not for modifier-only keys)
        if (!['Meta', 'Control', 'Alt', 'Shift'].includes(e.key)) {
          get().deactivate()
        }
        return
      }

      // If in workspace focus mode, handle navigation
      if (prefixState === 'workspace_focus') {
        const actionKey = e.key.toLowerCase()

        if (actionKey === 'escape') {
          e.preventDefault()
          e.stopPropagation()
          get().deactivate()
          return
        }

        if (actionKey === 'enter') {
          e.preventDefault()
          e.stopPropagation()
          const selectedWorkspaceId = get().selectFocusedWorkspace()
          if (selectedWorkspaceId) {
            handlers.setActiveWorkspace?.(selectedWorkspaceId)
          }
          get().deactivate()
          return
        }

        if (actionKey === 'arrowup') {
          e.preventDefault()
          e.stopPropagation()
          get().navigateWorkspace('up')
          return
        }

        if (actionKey === 'arrowdown') {
          e.preventDefault()
          e.stopPropagation()
          get().navigateWorkspace('down')
          return
        }

        return
      }

      // Cmd/Ctrl+1-9: Switch to tab by number
      if ((e.metaKey || e.ctrlKey) && e.key >= '1' && e.key <= '9') {
        e.preventDefault()
        handlers.switchToTab?.(parseInt(e.key) - 1)
      }

      // Diagnostic: detect if Shift/Enter are being unexpectedly blocked in idle mode
      if ((e.key === 'Shift' || e.key === 'Enter') && e.defaultPrevented) {
        console.error('[KeyDiag] Shift/Enter was preventDefault in idle mode!', {
          key: e.key, shiftKey: e.shiftKey, ctrlKey: e.ctrlKey, metaKey: e.metaKey,
          activeElement: document.activeElement?.tagName,
        })
      }
    }

    window.addEventListener('keydown', handleKeyDown, true)
    cleanupListener = () => { window.removeEventListener('keydown', handleKeyDown, true); }
  },

  dispose: () => {
    cleanupListener?.()
    cleanupListener = null
    if (timeoutId) clearTimeout(timeoutId)
    timeoutId = null
  },

  setHandlers: (handlers: KeybindingHandlers) => { set({ handlers }); },

  activate: () => {
    if (timeoutId) clearTimeout(timeoutId)

    const timeout = useSettingsStore.getState().settings.prefixMode.timeout
    timeoutId = window.setTimeout(() => {
      get().deactivate()
    }, timeout)

    set({
      prefixState: 'active',
      activatedAt: Date.now()
    })
  },

  deactivate: () => {
    if (timeoutId) clearTimeout(timeoutId)
    timeoutId = null

    set({
      prefixState: 'idle',
      activatedAt: null,
      focusedWorkspaceIndex: 0,
      workspaceIds: []
    })
  },

  enterWorkspaceFocus: (workspaceIds: string[], currentIndex: number) => {
    if (timeoutId) clearTimeout(timeoutId)

    const timeout = useSettingsStore.getState().settings.prefixMode.timeout
    timeoutId = window.setTimeout(() => {
      get().deactivate()
    }, timeout)

    set({
      prefixState: 'workspace_focus',
      activatedAt: Date.now(),
      workspaceIds,
      focusedWorkspaceIndex: currentIndex
    })
  },

  navigateWorkspace: (direction: 'up' | 'down') => {
    const { focusedWorkspaceIndex, workspaceIds } = get()
    if (workspaceIds.length === 0) return

    const newIndex = direction === 'up'
      ? (focusedWorkspaceIndex > 0 ? focusedWorkspaceIndex - 1 : workspaceIds.length - 1)
      : (focusedWorkspaceIndex < workspaceIds.length - 1 ? focusedWorkspaceIndex + 1 : 0)

    set({ focusedWorkspaceIndex: newIndex })
  },

  selectFocusedWorkspace: () => {
    const { focusedWorkspaceIndex, workspaceIds } = get()
    if (workspaceIds.length === 0) return null
    return workspaceIds[focusedWorkspaceIndex] || null
  }
}))
