import { useEffect, useCallback, useRef } from 'react'
import { useSettingsStore } from '../store/settings'
import { usePrefixModeStore } from '../store/prefixMode'
import type { Settings } from '../types'

export interface KeybindingHandlers {
  newTab?: () => void
  closeTab?: () => void
  nextTab?: () => void
  prevTab?: () => void
  openSettings?: () => void
}

function matchesKeybinding(e: KeyboardEvent, keybinding: string): boolean {
  const parts = keybinding.split('+')
  const key = parts[parts.length - 1]
  const hasCmd = parts.includes('CommandOrControl') || parts.includes('Control')
  const hasShift = parts.includes('Shift')
  const hasAlt = parts.includes('Alt')

  const cmdMatch = hasCmd ? e.metaKey || e.ctrlKey : !e.metaKey && !e.ctrlKey
  const shiftMatch = hasShift ? e.shiftKey : !e.shiftKey
  const altMatch = hasAlt ? e.altKey : !e.altKey

  const pressedKey = e.key.length === 1 ? e.key.toUpperCase() : e.key
  const targetKey = key.length === 1 ? key.toUpperCase() : key
  const keyMatch = pressedKey === targetKey || e.key === key

  return cmdMatch && shiftMatch && altMatch && keyMatch
}

export function usePrefixKeybindings(handlers: KeybindingHandlers) {
  const { settings } = useSettingsStore()
  const { state: prefixState, activate, deactivate } = usePrefixModeStore()
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

      // Check for prefix key
      if (prefixMode.enabled && matchesKeybinding(e, prefixMode.prefixKey)) {
        e.preventDefault()
        if (prefixState === 'idle') {
          activate()
        } else {
          // Pressing prefix again cancels prefix mode
          deactivate()
        }
        return
      }

      // If in prefix mode, check for action keys
      if (prefixMode.enabled && prefixState === 'active') {
        const actionKey = e.key.toLowerCase()

        const actionMap: Record<keyof Settings['keybindings'], keyof KeybindingHandlers> = {
          newTab: 'newTab',
          closeTab: 'closeTab',
          nextTab: 'nextTab',
          prevTab: 'prevTab',
          openSettings: 'openSettings'
        }

        for (const [action, handlerKey] of Object.entries(actionMap)) {
          const binding = keybindings[action as keyof Settings['keybindings']]
          if (binding.prefixMode?.toLowerCase() === actionKey) {
            e.preventDefault()
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

      // Check direct keybindings (when prefix mode disabled or in idle state)
      if (!prefixMode.enabled || prefixState === 'idle') {
        const actionMap: Record<keyof Settings['keybindings'], keyof KeybindingHandlers> = {
          newTab: 'newTab',
          closeTab: 'closeTab',
          nextTab: 'nextTab',
          prevTab: 'prevTab',
          openSettings: 'openSettings'
        }

        for (const [action, handlerKey] of Object.entries(actionMap)) {
          const binding = keybindings[action as keyof Settings['keybindings']]
          if (binding.direct && matchesKeybinding(e, binding.direct)) {
            e.preventDefault()
            handlers[handlerKey]?.()
            return
          }
        }
      }
    },
    [settings, prefixState, activate, deactivate, handlers]
  )

  useEffect(() => {
    // Use capture phase so the event is caught before xterm.js can stop propagation
    window.addEventListener('keydown', handleKeyDown, true)
    return () => window.removeEventListener('keydown', handleKeyDown, true)
  }, [handleKeyDown])
}
