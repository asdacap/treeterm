import { useState, useEffect, useCallback } from 'react'
import { Settings } from '../types'
import { useSettingsStore } from '../store/settings'

interface SettingsDialogProps {
  isOpen: boolean
  onClose: () => void
}

type TabId = 'terminal' | 'sandbox' | 'appearance' | 'keybindings' | 'startup'

const tabs: { id: TabId; label: string }[] = [
  { id: 'terminal', label: 'Terminal' },
  { id: 'sandbox', label: 'Sandbox' },
  { id: 'startup', label: 'Startup' },
  { id: 'appearance', label: 'Appearance' },
  { id: 'keybindings', label: 'Keybindings' }
]

export default function SettingsDialog({ isOpen, onClose }: SettingsDialogProps) {
  const { settings: savedSettings, saveSettings } = useSettingsStore()
  const [localSettings, setLocalSettings] = useState<Settings>(savedSettings)
  const [activeTab, setActiveTab] = useState<TabId>('terminal')
  const [recordingKey, setRecordingKey] = useState<keyof Settings['keybindings'] | null>(null)

  useEffect(() => {
    if (isOpen) {
      setLocalSettings(savedSettings)
    }
  }, [isOpen, savedSettings])

  const handleSave = useCallback(async () => {
    await saveSettings(localSettings)
    onClose()
  }, [localSettings, saveSettings, onClose])

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (!recordingKey) return

      e.preventDefault()
      e.stopPropagation()

      const parts: string[] = []
      if (e.metaKey || e.ctrlKey) parts.push('CommandOrControl')
      if (e.altKey) parts.push('Alt')
      if (e.shiftKey) parts.push('Shift')

      const key = e.key
      if (!['Meta', 'Control', 'Alt', 'Shift'].includes(key)) {
        parts.push(key.length === 1 ? key.toUpperCase() : key)
        const keybinding = parts.join('+')

        setLocalSettings((prev) => ({
          ...prev,
          keybindings: {
            ...prev.keybindings,
            [recordingKey]: keybinding
          }
        }))
        setRecordingKey(null)
      }
    },
    [recordingKey]
  )

  if (!isOpen) return null

  return (
    <div className="dialog-overlay" onClick={onClose}>
      <div
        className="settings-dialog"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        <div className="settings-dialog-header">
          <h2>Settings</h2>
          <button className="dialog-close" onClick={onClose}>
            &times;
          </button>
        </div>

        <div className="settings-dialog-content">
          <div className="settings-tabs">
            {tabs.map((tab) => (
              <button
                key={tab.id}
                className={`settings-tab ${activeTab === tab.id ? 'active' : ''}`}
                onClick={() => setActiveTab(tab.id)}
              >
                {tab.label}
              </button>
            ))}
          </div>

          <div className="settings-panel">
            {activeTab === 'terminal' && (
              <div className="settings-section">
                <div className="settings-group">
                  <label className="settings-label">Font Size</label>
                  <input
                    type="number"
                    className="settings-input"
                    value={localSettings.terminal.fontSize}
                    min={8}
                    max={32}
                    onChange={(e) =>
                      setLocalSettings((prev) => ({
                        ...prev,
                        terminal: { ...prev.terminal, fontSize: parseInt(e.target.value) || 14 }
                      }))
                    }
                  />
                </div>

                <div className="settings-group">
                  <label className="settings-label">Font Family</label>
                  <input
                    type="text"
                    className="settings-input"
                    value={localSettings.terminal.fontFamily}
                    onChange={(e) =>
                      setLocalSettings((prev) => ({
                        ...prev,
                        terminal: { ...prev.terminal, fontFamily: e.target.value }
                      }))
                    }
                  />
                </div>

                <div className="settings-group">
                  <label className="settings-label">Cursor Style</label>
                  <select
                    className="settings-select"
                    value={localSettings.terminal.cursorStyle}
                    onChange={(e) =>
                      setLocalSettings((prev) => ({
                        ...prev,
                        terminal: {
                          ...prev.terminal,
                          cursorStyle: e.target.value as 'block' | 'underline' | 'bar'
                        }
                      }))
                    }
                  >
                    <option value="block">Block</option>
                    <option value="underline">Underline</option>
                    <option value="bar">Bar</option>
                  </select>
                </div>

                <div className="settings-group">
                  <label className="settings-checkbox-label">
                    <input
                      type="checkbox"
                      checked={localSettings.terminal.cursorBlink}
                      onChange={(e) =>
                        setLocalSettings((prev) => ({
                          ...prev,
                          terminal: { ...prev.terminal, cursorBlink: e.target.checked }
                        }))
                      }
                    />
                    Cursor Blink
                  </label>
                </div>
              </div>
            )}

            {activeTab === 'sandbox' && (
              <div className="settings-section">
                <div className="settings-group">
                  <label className="settings-checkbox-label">
                    <input
                      type="checkbox"
                      checked={localSettings.sandbox.enabledByDefault}
                      onChange={(e) =>
                        setLocalSettings((prev) => ({
                          ...prev,
                          sandbox: { ...prev.sandbox, enabledByDefault: e.target.checked }
                        }))
                      }
                    />
                    Enable Sandbox by Default
                  </label>
                  <p className="settings-hint">
                    New workspaces will have sandboxing enabled automatically
                  </p>
                </div>

                <div className="settings-group">
                  <label className="settings-checkbox-label">
                    <input
                      type="checkbox"
                      checked={localSettings.sandbox.allowNetworkByDefault}
                      onChange={(e) =>
                        setLocalSettings((prev) => ({
                          ...prev,
                          sandbox: { ...prev.sandbox, allowNetworkByDefault: e.target.checked }
                        }))
                      }
                    />
                    Allow Network Access by Default
                  </label>
                  <p className="settings-hint">
                    Sandboxed terminals will have network access by default
                  </p>
                </div>
              </div>
            )}

            {activeTab === 'startup' && (
              <div className="settings-section">
                <div className="settings-group">
                  <label className="settings-label">Child Workspace Command</label>
                  <input
                    type="text"
                    className="settings-input"
                    value={localSettings.startup.childWorkspaceCommand}
                    placeholder="e.g., claude"
                    onChange={(e) =>
                      setLocalSettings((prev) => ({
                        ...prev,
                        startup: { ...prev.startup, childWorkspaceCommand: e.target.value }
                      }))
                    }
                  />
                  <p className="settings-hint">
                    Command to run automatically when a child workspace is opened
                  </p>
                </div>
              </div>
            )}

            {activeTab === 'appearance' && (
              <div className="settings-section">
                <div className="settings-group">
                  <label className="settings-label">Theme</label>
                  <select
                    className="settings-select"
                    value={localSettings.appearance.theme}
                    onChange={(e) =>
                      setLocalSettings((prev) => ({
                        ...prev,
                        appearance: {
                          ...prev.appearance,
                          theme: e.target.value as 'dark' | 'light' | 'system'
                        }
                      }))
                    }
                  >
                    <option value="dark">Dark</option>
                    <option value="light">Light</option>
                    <option value="system">System</option>
                  </select>
                </div>
              </div>
            )}

            {activeTab === 'keybindings' && (
              <div className="settings-section">
                {(
                  Object.entries(localSettings.keybindings) as [
                    keyof Settings['keybindings'],
                    string
                  ][]
                ).map(([key, value]) => (
                  <div key={key} className="settings-group">
                    <label className="settings-label">{formatKeybindingLabel(key)}</label>
                    <button
                      className={`settings-keybinding ${recordingKey === key ? 'recording' : ''}`}
                      onClick={() => setRecordingKey(recordingKey === key ? null : key)}
                    >
                      {recordingKey === key ? 'Press keys...' : formatKeybinding(value)}
                    </button>
                  </div>
                ))}
                <p className="settings-hint">Click a keybinding to record a new shortcut</p>
              </div>
            )}
          </div>
        </div>

        <div className="settings-dialog-actions">
          <button className="settings-btn cancel" onClick={onClose}>
            Cancel
          </button>
          <button className="settings-btn save" onClick={handleSave}>
            Save
          </button>
        </div>
      </div>
    </div>
  )
}

function formatKeybindingLabel(key: string): string {
  const labels: Record<string, string> = {
    newTab: 'New Tab',
    closeTab: 'Close Tab',
    nextTab: 'Next Tab',
    prevTab: 'Previous Tab',
    openSettings: 'Open Settings'
  }
  return labels[key] || key
}

function formatKeybinding(keybinding: string): string {
  return keybinding
    .replace('CommandOrControl', process.platform === 'darwin' ? 'Cmd' : 'Ctrl')
    .replace(/\+/g, ' + ')
}
