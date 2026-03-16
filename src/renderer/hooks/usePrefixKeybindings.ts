import { useEffect, useRef, useCallback } from 'react'
// @ts-expect-error tinykeys has incorrect exports config
import { parseKeybinding } from 'tinykeys'
import { useSettingsStore } from '../store/settings'
import { usePrefixModeStore } from '../store/prefixMode'
import { convertDirectKeybinding } from '../utils/keybindingConverter'
import type { Settings } from '../types'

export interface KeybindingHandlers {
  newTab?: () => void
  closeTab?: () => void
  nextTab?: () => void
  prevTab?: () => void
  openSettings?: () => void
  workspaceFocus?: () => void
  setActiveWorkspace?: (id: string | null) => void
}

export function usePrefixKeybindings(handlers: KeybindingHandlers): void {
  const { settings } = useSettingsStore()
  const {
    state: prefixState,
    activate,
    deactivate,
    navigateWorkspace,
    selectFocusedWorkspace
  } = usePrefixModeStore()
  const timeoutRef = useRef<number | null>(null)

  // Clear timeout on unmount
  useEffect(() => {
    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current)
      }
    }
  }, [])

  // Set up timeout when prefix mode activates
  useEffect(() => {
    if (prefixState === 'active') {
      timeoutRef.current = window.setTimeout(() => {
        deactivate()
      }, settings.prefixMode.timeout)

      return () => {
        if (timeoutRef.current) {
          clearTimeout(timeoutRef.current)
        }
      }
    }
  }, [prefixState, settings.prefixMode.timeout, deactivate])

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      const { prefixMode, keybindings } = settings

      // Action map for all keybindings (only maps to no-argument handlers)
      type NoArgHandlerKey = keyof { [K in keyof KeybindingHandlers as KeybindingHandlers[K] extends (() => void) | undefined ? K : never]: KeybindingHandlers[K] }
      const actionMap: Record<keyof Settings['keybindings'], NoArgHandlerKey> = {
        newTab: 'newTab',
        closeTab: 'closeTab',
        nextTab: 'nextTab',
        prevTab: 'prevTab',
        openSettings: 'openSettings',
        workspaceFocus: 'workspaceFocus'
      }

      // Check for prefix key activation
      const prefixKeybinding = convertDirectKeybinding(prefixMode.prefixKey)
      const prefixParts = parseKeybinding(prefixKeybinding)

      // parseKeybinding returns [[modifiers, key]], get first sequence
      if (prefixParts.length > 0) {
        const [modifiers, key] = prefixParts[0]
        if (matchesKeybinding(e, modifiers, key)) {
          e.preventDefault()
          e.stopPropagation()
          if (prefixState === 'idle') {
            activate()
          } else {
            // Pressing prefix again cancels prefix mode
            deactivate()
          }
          return
        }
      }

      // If in prefix mode, check for action keys
      if (prefixState === 'active') {
        const actionKey = e.key.toLowerCase()

        // Check for Escape to cancel
        if (actionKey === 'escape') {
          e.preventDefault()
          e.stopPropagation()
          deactivate()
          return
        }

        // Check for prefix mode bindings
        for (const [action, handlerKey] of Object.entries(actionMap)) {
          const binding = keybindings[action as keyof Settings['keybindings']]
          if (binding.toLowerCase() === actionKey) {
            e.preventDefault()
            e.stopPropagation()
            deactivate()
            handlers[handlerKey]?.()
            return
          }
        }

        // Unknown key in prefix mode - deactivate
        // But only if it's not a modifier key
        if (!['Meta', 'Control', 'Alt', 'Shift'].includes(e.key)) {
          deactivate()
        }
        return
      }

      // If in workspace focus mode, handle navigation
      if (prefixState === 'workspace_focus') {
        const actionKey = e.key.toLowerCase()

        // Check for Escape to cancel
        if (actionKey === 'escape') {
          e.preventDefault()
          e.stopPropagation()
          deactivate()
          return
        }

        // Check for Enter to select
        if (actionKey === 'enter') {
          e.preventDefault()
          e.stopPropagation()
          const selectedWorkspaceId = selectFocusedWorkspace()
          if (selectedWorkspaceId) {
            handlers.setActiveWorkspace?.(selectedWorkspaceId)
          }
          deactivate()
          return
        }

        // Check for Arrow Up/Down to navigate
        if (actionKey === 'arrowup') {
          e.preventDefault()
          e.stopPropagation()
          navigateWorkspace('up')
          return
        }

        if (actionKey === 'arrowdown') {
          e.preventDefault()
          e.stopPropagation()
          navigateWorkspace('down')
          return
        }

        return
      }
    },
    [
      settings,
      prefixState,
      activate,
      deactivate,
      navigateWorkspace,
      selectFocusedWorkspace,
      handlers
    ]
  )

  useEffect(() => {
    // Use capture phase so the event is caught before xterm.js can stop propagation
    window.addEventListener('keydown', handleKeyDown, true)
    return () => window.removeEventListener('keydown', handleKeyDown, true)
  }, [handleKeyDown])
}

// Helper function to match a keybinding event
// modifiers is an array of modifier keys like ["Meta", "Shift"]
// key is the main key like "t"
function matchesKeybinding(event: KeyboardEvent, modifiers: string[], key: string): boolean {
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
