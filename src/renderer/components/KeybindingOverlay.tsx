import React, { useState, useEffect } from 'react'
import { useKeybindingStore, PrefixModeState } from '../store/keybinding'
import { useSettingsStore } from '../store/settings'
import type { Settings, Platform } from '../types'

interface KeybindingItem {
  key: string
  action: string
  description?: string
}

function formatPrefixKey(key: string, platform: string): string {
  return key
    .replace('CommandOrControl', platform === 'darwin' ? 'Cmd' : 'Ctrl')
    .replace('Control', 'Ctrl')
    .replace(/\+/g, ' + ')
}

function getAvailableKeybindings(settings: Settings): KeybindingItem[] {
  const labels: Record<keyof Settings['keybindings'], string> = {
    newTab: 'New Tab',
    closeTab: 'Close Tab',
    nextTab: 'Next Tab',
    prevTab: 'Previous Tab',
    openSettings: 'Open Settings',
    workspaceFocus: 'Workspace Focus'
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

  const [prevActivatedAt, setPrevActivatedAt] = useState(activatedAt)
  if (activatedAt !== prevActivatedAt) {
    setPrevActivatedAt(activatedAt)
    if (!activatedAt) {
      setRemaining(100)
    }
  }

  useEffect(() => {
    if (!activatedAt) return

    const interval = setInterval(() => {
      const elapsed = Date.now() - activatedAt
      const percent = Math.max(0, 100 - (elapsed / timeout) * 100)
      setRemaining(percent)

      if (percent <= 0) {
        clearInterval(interval)
      }
    }, 16) // ~60fps

    return () => { clearInterval(interval); }
  }, [activatedAt, timeout])

  return (
    <div className="keybinding-overlay-progress">
      <div
        className="keybinding-overlay-progress-bar"
        style={{ width: `${String(remaining)}%` }}
      />
    </div>
  )
}

export default function KeybindingOverlay({ platform }: { platform: Platform }): React.JSX.Element | null {
  const { prefixState, activatedAt } = useKeybindingStore()
  const { settings } = useSettingsStore()

  if (prefixState === PrefixModeState.Idle) {
    return null
  }

  // Workspace focus mode
  if (prefixState === PrefixModeState.WorkspaceFocus) {
    return (
      <div className="keybinding-overlay">
        <div className="keybinding-overlay-header">
          <span className="keybinding-overlay-title">Workspace Focus</span>
          <span className="keybinding-overlay-hint">
            Navigate: ↑/↓ • Select: Enter • Cancel: Esc
          </span>
        </div>
        <div className="keybinding-overlay-list">
          <div className="keybinding-overlay-item">
            <kbd className="keybinding-overlay-key">↑ / ↓</kbd>
            <span className="keybinding-overlay-action">Navigate Workspaces</span>
          </div>
          <div className="keybinding-overlay-item">
            <kbd className="keybinding-overlay-key">Enter</kbd>
            <span className="keybinding-overlay-action">Select Workspace</span>
          </div>
          <div className="keybinding-overlay-item">
            <kbd className="keybinding-overlay-key">Esc</kbd>
            <span className="keybinding-overlay-action">Cancel</span>
          </div>
        </div>
        <TimeoutProgress timeout={settings.prefixMode.timeout} activatedAt={activatedAt} />
      </div>
    )
  }

  // Regular prefix mode
  const keybindings = getAvailableKeybindings(settings)

  return (
    <div className="keybinding-overlay">
      <div className="keybinding-overlay-header">
        <span className="keybinding-overlay-prefix">
          {formatPrefixKey(settings.prefixMode.prefixKey, platform)}
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
