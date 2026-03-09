import { useState, useEffect, useCallback } from 'react'
import type { Settings } from '../types'
import { useSettingsStore } from '../store/settings'

interface SettingsDialogProps {
  isOpen: boolean
  onClose: () => void
}

type TabId = 'terminal' | 'sandbox' | 'claude' | 'appearance' | 'keybindings' | 'terminal-profiles'

const tabs: { id: TabId; label: string }[] = [
  { id: 'terminal', label: 'Terminal' },
  { id: 'terminal-profiles', label: 'Terminal Profiles' },
  { id: 'claude', label: 'Claude' },
  { id: 'sandbox', label: 'Sandbox' },
  { id: 'appearance', label: 'Appearance' },
  { id: 'keybindings', label: 'Keybindings' }
]

// Recording state type - can be for keybinding or prefix key
type RecordingState =
  | { type: 'keybinding'; action: keyof Settings['keybindings'] }
  | { type: 'prefixKey' }
  | null

export default function SettingsDialog({ isOpen, onClose }: SettingsDialogProps) {
  const { settings: savedSettings, saveSettings } = useSettingsStore()
  const [localSettings, setLocalSettings] = useState<Settings>(savedSettings)
  const [activeTab, setActiveTab] = useState<TabId>('terminal')
  const [recording, setRecording] = useState<RecordingState>(null)
  const [sandboxAvailable, setSandboxAvailable] = useState<boolean | null>(null)

  useEffect(() => {
    window.electron.sandbox.isAvailable().then(setSandboxAvailable)
  }, [])

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
      if (!recording) return

      e.preventDefault()
      e.stopPropagation()

      const key = e.key
      // Skip if only modifier keys are pressed
      if (['Meta', 'Control', 'Alt', 'Shift'].includes(key)) return

      if (recording.type === 'keybinding') {
        // For keybindings, we just capture the single key (no modifiers)
        const actionKey = key.length === 1 ? key.toLowerCase() : key
        setLocalSettings((prev) => ({
          ...prev,
          keybindings: {
            ...prev.keybindings,
            [recording.action]: actionKey
          }
        }))
        setRecording(null)
        return
      }

      // For prefix key, capture with modifiers
      const parts: string[] = []
      if (e.metaKey || e.ctrlKey) parts.push('CommandOrControl')
      if (e.altKey) parts.push('Alt')
      if (e.shiftKey) parts.push('Shift')
      parts.push(key.length === 1 ? key.toUpperCase() : key)
      const keybinding = parts.join('+')

      if (recording.type === 'prefixKey') {
        setLocalSettings((prev) => ({
          ...prev,
          prefixMode: {
            ...prev.prefixMode,
            prefixKey: keybinding
          }
        }))
        setRecording(null)
      }
    },
    [recording]
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

                <div className="settings-group">
                  <label className="settings-checkbox-label">
                    <input
                      type="checkbox"
                      checked={localSettings.terminal.showRawChars}
                      onChange={(e) =>
                        setLocalSettings((prev) => ({
                          ...prev,
                          terminal: { ...prev.terminal, showRawChars: e.target.checked }
                        }))
                      }
                    />
                    Show Raw Characters (Debug)
                  </label>
                  <p className="settings-hint">
                    Log the last 50 raw terminal characters to the console (DevTools)
                  </p>
                </div>

                <div className="settings-group">
                  <label className="settings-checkbox-label">
                    <input
                      type="checkbox"
                      checked={localSettings.terminal.startByDefault}
                      onChange={(e) =>
                        setLocalSettings((prev) => ({
                          ...prev,
                          terminal: { ...prev.terminal, startByDefault: e.target.checked }
                        }))
                      }
                    />
                    Start by Default
                  </label>
                  <p className="settings-hint">
                    Automatically open a Terminal tab when creating new workspaces
                  </p>
                </div>
              </div>
            )}

            {activeTab === 'sandbox' && (
              <div className="settings-section">
                {sandboxAvailable === false && (
                  <div className="settings-warning">
                    Sandbox not available on this system.
                    {window.electron.platform === 'linux' && ' Install bubblewrap (bwrap) to enable.'}
                    {window.electron.platform === 'win32' && ' Sandbox is not supported on Windows.'}
                  </div>
                )}
                <div className="settings-group">
                  <label className="settings-checkbox-label">
                    <input
                      type="checkbox"
                      disabled={!sandboxAvailable}
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
                      disabled={!sandboxAvailable}
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

            {activeTab === 'claude' && (
              <div className="settings-section">
                <div className="settings-group">
                  <label className="settings-label">Claude Command</label>
                  <input
                    type="text"
                    className="settings-input"
                    value={localSettings.claude.command}
                    placeholder={window.electron.platform === 'darwin' ? 'claude' : 'npx @anthropic-ai/claude-code'}
                    onChange={(e) =>
                      setLocalSettings((prev) => ({
                        ...prev,
                        claude: { ...prev.claude, command: e.target.value }
                      }))
                    }
                  />
                  <p className="settings-hint">
                    Command to launch Claude Code (e.g., &quot;claude&quot; on macOS, &quot;npx @anthropic-ai/claude-code&quot; on Linux)
                  </p>
                </div>

                <div className="settings-group">
                  <label className="settings-checkbox-label">
                    <input
                      type="checkbox"
                      checked={localSettings.claude.startByDefault}
                      onChange={(e) =>
                        setLocalSettings((prev) => ({
                          ...prev,
                          claude: { ...prev.claude, startByDefault: e.target.checked }
                        }))
                      }
                    />
                    Start by Default
                  </label>
                  <p className="settings-hint">
                    Automatically open a Claude tab when creating new workspaces
                  </p>
                </div>

                <div className="settings-group">
                  <label className="settings-checkbox-label">
                    <input
                      type="checkbox"
                      disabled={!sandboxAvailable}
                      checked={localSettings.claude.enableSandbox}
                      onChange={(e) =>
                        setLocalSettings((prev) => ({
                          ...prev,
                          claude: { ...prev.claude, enableSandbox: e.target.checked }
                        }))
                      }
                    />
                    Enable Sandbox
                  </label>
                  <p className="settings-hint">
                    Run Claude in a sandboxed terminal with restricted file and network access
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
                {/* Prefix Key Configuration */}
                <div className="settings-group">
                  <label className="settings-label">Prefix Key</label>
                  <button
                    className={`settings-keybinding ${recording?.type === 'prefixKey' ? 'recording' : ''}`}
                    onClick={() =>
                      setRecording(
                        recording?.type === 'prefixKey' ? null : { type: 'prefixKey' }
                      )
                    }
                  >
                    {recording?.type === 'prefixKey'
                      ? 'Press keys...'
                      : formatKeybinding(localSettings.prefixMode.prefixKey)}
                  </button>
                  <p className="settings-hint">
                    Press a prefix key first, then the action key. Like tmux or screen.
                  </p>
                </div>

                <div className="settings-group">
                  <label className="settings-label">Timeout (ms)</label>
                  <input
                    type="number"
                    className="settings-input"
                    value={localSettings.prefixMode.timeout}
                    min={500}
                    max={5000}
                    step={100}
                    onChange={(e) =>
                      setLocalSettings((prev) => ({
                        ...prev,
                        prefixMode: {
                          ...prev.prefixMode,
                          timeout: parseInt(e.target.value) || 1500
                        }
                      }))
                    }
                  />
                  <p className="settings-hint">
                    How long to wait for action key after pressing prefix (500-5000ms)
                  </p>
                </div>

                <hr className="settings-divider" />

                {/* Keybinding Actions */}
                {(
                  Object.entries(localSettings.keybindings) as [
                    keyof Settings['keybindings'],
                    string
                  ][]
                ).map(([key, binding]) => (
                  <div key={key} className="settings-group keybinding-row">
                    <label className="settings-label">{formatKeybindingLabel(key)}</label>
                    <button
                      className={`settings-keybinding prefix-key-btn ${recording?.type === 'keybinding' && recording.action === key ? 'recording' : ''}`}
                      onClick={() =>
                        setRecording(
                          recording?.type === 'keybinding' && recording.action === key
                            ? null
                            : { type: 'keybinding', action: key }
                        )
                      }
                    >
                      {recording?.type === 'keybinding' && recording.action === key
                        ? 'Press key...'
                        : binding.toUpperCase()}
                    </button>
                  </div>
                ))}
                <p className="settings-hint">
                  Click a keybinding to record a new key. Press the prefix key first, then the action key shown here.
                </p>
              </div>
            )}

            {activeTab === 'terminal-profiles' && (
              <div className="settings-section">
                <p className="settings-hint">
                  Create custom terminal profiles with startup commands.
                  These appear as separate options in the new tab menu.
                </p>
                <div className="applications-list">
                  {localSettings.terminal.instances.map((inst, index) => (
                    <div key={inst.id} className="application-item">
                      <div className="application-icon">
                        <input
                          type="text"
                          className="settings-input icon-input"
                          value={inst.icon}
                          maxLength={2}
                          onChange={(e) =>
                            setLocalSettings((prev) => ({
                              ...prev,
                              terminal: {
                                ...prev.terminal,
                                instances: prev.terminal.instances.map((a, i) =>
                                  i === index ? { ...a, icon: e.target.value } : a
                                )
                              }
                            }))
                          }
                        />
                      </div>
                      <div className="application-fields">
                        <input
                          type="text"
                          className="settings-input"
                          value={inst.name}
                          placeholder="Name"
                          onChange={(e) =>
                            setLocalSettings((prev) => ({
                              ...prev,
                              terminal: {
                                ...prev.terminal,
                                instances: prev.terminal.instances.map((a, i) =>
                                  i === index ? { ...a, name: e.target.value } : a
                                )
                              }
                            }))
                          }
                        />
                        <input
                          type="text"
                          className="settings-input"
                          value={inst.startupCommand}
                          placeholder="Startup command"
                          onChange={(e) =>
                            setLocalSettings((prev) => ({
                              ...prev,
                              terminal: {
                                ...prev.terminal,
                                instances: prev.terminal.instances.map((a, i) =>
                                  i === index ? { ...a, startupCommand: e.target.value } : a
                                )
                              }
                            }))
                          }
                        />
                      </div>
                      <label className="settings-checkbox-label default-checkbox">
                        <input
                          type="checkbox"
                          checked={inst.isDefault}
                          onChange={(e) =>
                            setLocalSettings((prev) => ({
                              ...prev,
                              terminal: {
                                ...prev.terminal,
                                instances: prev.terminal.instances.map((a, i) =>
                                  i === index ? { ...a, isDefault: e.target.checked } : a
                                )
                              }
                            }))
                          }
                        />
                        Default
                      </label>
                      <button
                        className="application-delete"
                        onClick={() =>
                          setLocalSettings((prev) => ({
                            ...prev,
                            terminal: {
                              ...prev.terminal,
                              instances: prev.terminal.instances.filter((_, i) => i !== index)
                            }
                          }))
                        }
                      >
                        Delete
                      </button>
                    </div>
                  ))}
                </div>
                <button
                  className="settings-btn add-app"
                  onClick={() => {
                    const newId = `term-${Date.now()}`
                    setLocalSettings((prev) => ({
                      ...prev,
                      terminal: {
                        ...prev.terminal,
                        instances: [
                          ...prev.terminal.instances,
                          {
                            id: newId,
                            name: 'New Terminal',
                            icon: '>',
                            startupCommand: '',
                            isDefault: false
                          }
                        ]
                      }
                    }))
                  }}
                >
                  + Add Terminal Profile
                </button>
                <p className="settings-hint">
                  Default profiles open automatically in new workspaces.
                </p>
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
  if (!keybinding) return '(none)'
  return keybinding
    .replace('CommandOrControl', window.electron.platform === 'darwin' ? 'Cmd' : 'Ctrl')
    .replace('Control', 'Ctrl')
    .replace(/\+/g, ' + ')
}
