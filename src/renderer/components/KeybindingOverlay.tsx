import { useState, useEffect } from 'react'
import { usePrefixModeStore } from '../store/prefixMode'
import { useSettingsStore } from '../store/settings'
import type { Settings } from '../types'

interface KeybindingItem {
  key: string
  action: string
  description?: string
}

function formatPrefixKey(key: string): string {
  return key
    .replace('CommandOrControl', window.electron.platform === 'darwin' ? 'Cmd' : 'Ctrl')
    .replace('Control', 'Ctrl')
    .replace(/\+/g, ' + ')
}

function getAvailableKeybindings(settings: Settings): KeybindingItem[] {
  const labels: Record<keyof Settings['keybindings'], string> = {
    newTab: 'New Tab',
    closeTab: 'Close Tab',
    nextTab: 'Next Tab',
    prevTab: 'Previous Tab',
    openSettings: 'Open Settings'
  }

  return Object.entries(settings.keybindings)
    .map(([action, binding]) => ({
      key: binding.toUpperCase(),
      action: labels[action as keyof Settings['keybindings']] || action
    }))
    .sort((a, b) => a.key.localeCompare(b.key))
}

function TimeoutProgress({ timeout, activatedAt }: { timeout: number; activatedAt: number | null }) {
  const [remaining, setRemaining] = useState(100)

  useEffect(() => {
    if (!activatedAt) {
      setRemaining(100)
      return
    }

    const interval = setInterval(() => {
      const elapsed = Date.now() - activatedAt
      const percent = Math.max(0, 100 - (elapsed / timeout) * 100)
      setRemaining(percent)

      if (percent <= 0) {
        clearInterval(interval)
      }
    }, 16) // ~60fps

    return () => clearInterval(interval)
  }, [activatedAt, timeout])

  return (
    <div className="keybinding-overlay-progress">
      <div
        className="keybinding-overlay-progress-bar"
        style={{ width: `${remaining}%` }}
      />
    </div>
  )
}

export default function KeybindingOverlay() {
  const { state, activatedAt } = usePrefixModeStore()
  const { settings } = useSettingsStore()

  if (state !== 'active') {
    return null
  }

  const keybindings = getAvailableKeybindings(settings)

  return (
    <div className="keybinding-overlay">
      <div className="keybinding-overlay-header">
        <span className="keybinding-overlay-prefix">
          {formatPrefixKey(settings.prefixMode.prefixKey)}
        </span>
        <span className="keybinding-overlay-title">Prefix Mode</span>
        <span className="keybinding-overlay-hint">Press a key or Esc to cancel</span>
      </div>
      <div className="keybinding-overlay-list">
        {keybindings.map((binding) => (
          <div key={binding.key} className="keybinding-overlay-item">
            <kbd className="keybinding-overlay-key">{binding.key}</kbd>
            <span className="keybinding-overlay-action">{binding.action}</span>
          </div>
        ))}
      </div>
      <TimeoutProgress timeout={settings.prefixMode.timeout} activatedAt={activatedAt} />
    </div>
  )
}
