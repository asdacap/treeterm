import { useEffect, useCallback, useRef } from 'react'
import { useSettingsStore } from '../store/settings'

interface UseCapsLockHoldOptions {
  onHoldStart: () => void
  onHoldEnd: () => void
  enabled?: boolean
}

export function useCapsLockHold({
  onHoldStart,
  onHoldEnd,
  enabled = true
}: UseCapsLockHoldOptions): void {
  const isHoldingRef = useRef(false)
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const pollIntervalRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const lastEventRef = useRef<KeyboardEvent | null>(null)
  const { settings } = useSettingsStore()
  const pushToTalkKey = settings.stt?.pushToTalkKey || 'Shift+Space'

  // Parse the key combination (e.g., "Shift+Space" -> { modifier: 'Shift', key: ' ' })
  const parseKeyCombination = (combo: string) => {
    const parts = combo.split('+')
    if (parts.length === 2) {
      const modifier = parts[0].toLowerCase()
      const key = parts[1] === 'Space' ? ' ' : parts[1]
      return { modifier, key }
    }
    return { modifier: 'shift', key: ' ' } // default fallback
  }

  const { modifier, key } = parseKeyCombination(pushToTalkKey)

  const stopRecording = useCallback(() => {
    if (!isHoldingRef.current) return
    isHoldingRef.current = false
    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current)
      timeoutRef.current = null
    }
    if (pollIntervalRef.current) {
      clearInterval(pollIntervalRef.current)
      pollIntervalRef.current = null
    }
    console.log('Push-to-talk: Stopping recording')
    onHoldEnd()
  }, [onHoldEnd])

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (!enabled) return

      // Check for the configured key combination
      const isModifierPressed =
        (modifier === 'ctrl' && e.ctrlKey) ||
        (modifier === 'shift' && e.shiftKey) ||
        (modifier === 'alt' && e.altKey) ||
        (modifier === 'meta' && e.metaKey)

      const isKeyCombination = isModifierPressed && e.key === key

      if (!isKeyCombination) return
      if (isHoldingRef.current) {
        console.log('Push-to-talk: Already holding, ignoring keydown')
        return
      }

      // Prevent default behavior
      e.preventDefault()
      e.stopPropagation()
      e.stopImmediatePropagation()

      isHoldingRef.current = true
      console.log(`🎙️ Push-to-talk: STARTING RECORDING (${pushToTalkKey})`)

      // Safety timeout - auto-stop after 60 seconds
      timeoutRef.current = setTimeout(() => {
        console.log('⏱️ Push-to-talk: Auto-stopping after 60 seconds')
        stopRecording()
      }, 60000)

      onHoldStart()
    },
    [enabled, onHoldStart, stopRecording, modifier, key, pushToTalkKey]
  )

  const handleKeyUp = useCallback(
    (e: KeyboardEvent) => {
      if (!enabled) return

      // Stop recording when either the modifier or the key is released
      const modifierKeyNames = {
        ctrl: 'Control',
        shift: 'Shift',
        alt: 'Alt',
        meta: 'Meta'
      }

      const isModifierOrKey = e.key === modifierKeyNames[modifier as keyof typeof modifierKeyNames] || e.key === key

      if (!isModifierOrKey) return
      if (!isHoldingRef.current) return

      // Prevent default behavior
      e.preventDefault()
      e.stopPropagation()
      e.stopImmediatePropagation()

      console.log('🛑 Push-to-talk: STOPPING RECORDING (released)', e.key)
      stopRecording()
    },
    [enabled, stopRecording, modifier, key]
  )

  useEffect(() => {
    // Use capture phase to intercept before terminal
    window.addEventListener('keydown', handleKeyDown, true)
    window.addEventListener('keyup', handleKeyUp, true)

    // Also listen to ANY key event to update timestamp
    const updateTimestamp = (e: KeyboardEvent) => {
      lastEventRef.current = e
    }
    window.addEventListener('keydown', updateTimestamp, true)
    window.addEventListener('keyup', updateTimestamp, true)

    // Listen to Electron's before-input-event for Caps Lock
    const cleanupCapsLock = window.electron.app.onCapsLockEvent((event) => {
      console.log('🔔 Electron before-input-event:', event)

      if (event.type === 'keyDown') {
        console.log('📥 Creating synthetic keydown event for Caps Lock')
        const syntheticEvent = new KeyboardEvent('keydown', {
          key: 'CapsLock',
          code: 'CapsLock',
          bubbles: true,
          cancelable: true
        })
        // Add deprecated keyCode property using defineProperty for compatibility
        Object.defineProperty(syntheticEvent, 'keyCode', {
          value: 20,
          writable: false,
          configurable: true
        })
        handleKeyDown(syntheticEvent)
      } else if (event.type === 'keyUp') {
        console.log('📤 Creating synthetic keyup event for Caps Lock')
        const syntheticEvent = new KeyboardEvent('keyup', {
          key: 'CapsLock',
          code: 'CapsLock',
          bubbles: true,
          cancelable: true
        })
        // Add deprecated keyCode property using defineProperty for compatibility
        Object.defineProperty(syntheticEvent, 'keyCode', {
          value: 20,
          writable: false,
          configurable: true
        })
        handleKeyUp(syntheticEvent)
      }
    })

    return () => {
      window.removeEventListener('keydown', handleKeyDown, true)
      window.removeEventListener('keyup', handleKeyUp, true)
      window.removeEventListener('keydown', updateTimestamp, true)
      window.removeEventListener('keyup', updateTimestamp, true)
      cleanupCapsLock()

      // Clean up timeouts and intervals if component unmounts
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current)
      }
      if (pollIntervalRef.current) {
        clearInterval(pollIntervalRef.current)
      }
    }
  }, [handleKeyDown, handleKeyUp])
}
