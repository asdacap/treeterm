import { useState, useEffect, useCallback, useMemo } from 'react'
import type { Settings, ReasoningEffort } from '../types'
import { useSettingsStore } from '../store/settings'
import { useAppStore } from '../store/app'
import type { SandboxApi, Platform } from '../types'

interface SettingsDialogProps {
  isOpen: boolean
  onClose: () => void
  sandbox: SandboxApi
  platform: Platform
}

type TabId = 'general' | 'terminal' | 'sandbox' | 'ai-harness' | 'llm' | 'appearance' | 'keybindings' | 'terminal-profiles' | 'speech'

const tabs: { id: TabId; label: string }[] = [
  { id: 'general', label: 'General' },
  { id: 'terminal', label: 'Terminal' },
  { id: 'terminal-profiles', label: 'Terminal Profiles' },
  { id: 'ai-harness', label: 'AI Harness' },
  { id: 'llm', label: 'LLM' },
  { id: 'sandbox', label: 'Sandbox' },
  { id: 'appearance', label: 'Appearance' },
  { id: 'keybindings', label: 'Keybindings' },
  { id: 'speech', label: 'Speech' }
]

// Recording state type - can be for keybinding or prefix key
type RecordingState =
  | { type: 'keybinding'; action: keyof Settings['keybindings'] }
  | { type: 'prefixKey' }
  | { type: 'pttKey' }
  | null

export default function SettingsDialog({ isOpen, onClose, sandbox, platform }: SettingsDialogProps) {
  const { settings: savedSettings, saveSettings } = useSettingsStore()
  const applications = useAppStore((s) => s.applications)
  const allApplications = useMemo(() => Object.values(applications), [applications])
  const [localSettings, setLocalSettings] = useState<Settings>(savedSettings)
  const [activeTab, setActiveTab] = useState<TabId>('terminal')
  const [recording, setRecording] = useState<RecordingState>(null)
  const [sandboxAvailable, setSandboxAvailable] = useState<boolean | null>(null)

  useEffect(() => {
    sandbox.isAvailable().then(setSandboxAvailable).catch((error) => {
      console.error('[SettingsDialog] sandbox availability check failed:', error)
      setSandboxAvailable(false)
    })
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

      if (recording.type === 'pttKey') {
        // For push-to-talk key, just capture the single key (no modifiers)
        const pttKey = key.length === 1 ? key : key
        setLocalSettings((prev) => ({
          ...prev,
          stt: {
            ...prev.stt,
            pushToTalkKey: pttKey
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
            {activeTab === 'general' && (
              <div className="settings-section">
                <div className="settings-group">
                  <label className="settings-label">Default Application for New Worktrees</label>
                  <select
                    className="settings-select"
                    value={localSettings.globalDefaultApplicationId}
                    onChange={(e) =>
                      setLocalSettings((prev) => ({
                        ...prev,
                        globalDefaultApplicationId: e.target.value
                      }))
                    }
                  >
                    {allApplications.map((app) => (
                      <option key={app.id} value={app.id}>
                        {app.name}
                      </option>
                    ))}
                  </select>
                  <p className="settings-hint">
                    The default application to open when creating a new worktree. 
                    Child worktrees can inherit or override this setting.
                  </p>
                </div>
              </div>
            )}

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
                    {platform === 'linux' && ' Install bubblewrap (bwrap) to enable.'}
                    {platform === 'win32' && ' Sandbox is not supported on Windows.'}
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

            {activeTab === 'ai-harness' && (
              <div className="settings-section">
                <p className="settings-hint">
                  Configure AI tools like Claude, Cline, OpenCode, etc.
                  These appear as separate options in the new tab menu.
                </p>
                <div className="applications-list">
                  {localSettings.aiHarness.instances.map((inst, index) => (
                    <div key={inst.id} className="application-item ai-harness-item">
                      <div className="application-icon">
                        <input
                          type="text"
                          className="settings-input icon-input"
                          value={inst.icon}
                          maxLength={2}
                          onChange={(e) =>
                            setLocalSettings((prev) => ({
                              ...prev,
                              aiHarness: {
                                ...prev.aiHarness,
                                instances: prev.aiHarness.instances.map((a, i) =>
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
                          placeholder="Name (e.g., Claude)"
                          onChange={(e) =>
                            setLocalSettings((prev) => ({
                              ...prev,
                              aiHarness: {
                                ...prev.aiHarness,
                                instances: prev.aiHarness.instances.map((a, i) =>
                                  i === index ? { ...a, name: e.target.value } : a
                                )
                              }
                            }))
                          }
                        />
                        <input
                          type="text"
                          className="settings-input"
                          value={inst.command}
                          placeholder="Command (e.g., claude, cline, opencode)"
                          onChange={(e) =>
                            setLocalSettings((prev) => ({
                              ...prev,
                              aiHarness: {
                                ...prev.aiHarness,
                                instances: prev.aiHarness.instances.map((a, i) =>
                                  i === index ? { ...a, command: e.target.value } : a
                                )
                              }
                            }))
                          }
                        />
                        <input
                          type="text"
                          className="settings-input"
                          value={inst.backgroundColor}
                          placeholder="Background color (e.g., #1a1a24)"
                          onChange={(e) =>
                            setLocalSettings((prev) => ({
                              ...prev,
                              aiHarness: {
                                ...prev.aiHarness,
                                instances: prev.aiHarness.instances.map((a, i) =>
                                  i === index ? { ...a, backgroundColor: e.target.value } : a
                                )
                              }
                            }))
                          }
                        />
                        <div className="ai-harness-options">
                          <label className="settings-checkbox-label">
                            <input
                              type="checkbox"
                              checked={inst.isDefault}
                              onChange={(e) =>
                                setLocalSettings((prev) => ({
                                  ...prev,
                                  aiHarness: {
                                    ...prev.aiHarness,
                                    instances: prev.aiHarness.instances.map((a, i) =>
                                      i === index ? { ...a, isDefault: e.target.checked } : a
                                    )
                                  }
                                }))
                              }
                            />
                            Start by Default
                          </label>
                          <label className="settings-checkbox-label">
                            <input
                              type="checkbox"
                              disabled={!sandboxAvailable}
                              checked={inst.enableSandbox}
                              onChange={(e) =>
                                setLocalSettings((prev) => ({
                                  ...prev,
                                  aiHarness: {
                                    ...prev.aiHarness,
                                    instances: prev.aiHarness.instances.map((a, i) =>
                                      i === index ? { ...a, enableSandbox: e.target.checked } : a
                                    )
                                  }
                                }))
                              }
                            />
                            Enable Sandbox
                          </label>
                          <label className="settings-checkbox-label">
                            <input
                              type="checkbox"
                              disabled={!sandboxAvailable || !inst.enableSandbox}
                              checked={inst.allowNetwork}
                              onChange={(e) =>
                                setLocalSettings((prev) => ({
                                  ...prev,
                                  aiHarness: {
                                    ...prev.aiHarness,
                                    instances: prev.aiHarness.instances.map((a, i) =>
                                      i === index ? { ...a, allowNetwork: e.target.checked } : a
                                    )
                                  }
                                }))
                              }
                            />
                            Allow Network
                          </label>
                          <label className="settings-checkbox-label">
                            <input
                              type="checkbox"
                              checked={inst.disableScrollbar}
                              onChange={(e) =>
                                setLocalSettings((prev) => ({
                                  ...prev,
                                  aiHarness: {
                                    ...prev.aiHarness,
                                    instances: prev.aiHarness.instances.map((a, i) =>
                                      i === index ? { ...a, disableScrollbar: e.target.checked } : a
                                    )
                                  }
                                }))
                              }
                            />
                            Disable Scrollbar
                          </label>
                          <label className="settings-checkbox-label">
                            <input
                              type="checkbox"
                              checked={inst.stripScrollbackClear ?? false}
                              onChange={(e) =>
                                setLocalSettings((prev) => ({
                                  ...prev,
                                  aiHarness: {
                                    ...prev.aiHarness,
                                    instances: prev.aiHarness.instances.map((a, i) =>
                                      i === index ? { ...a, stripScrollbackClear: e.target.checked } : a
                                    )
                                  }
                                }))
                              }
                            />
                            Strip Scrollback Clear
                          </label>
                        </div>
                      </div>
                      <button
                        className="application-delete"
                        onClick={() =>
                          setLocalSettings((prev) => ({
                            ...prev,
                            aiHarness: {
                              ...prev.aiHarness,
                              instances: prev.aiHarness.instances.filter((_, i) => i !== index)
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
                    const newId = `ai-${Date.now()}`
                    setLocalSettings((prev) => ({
                      ...prev,
                      aiHarness: {
                        ...prev.aiHarness,
                        instances: [
                          ...prev.aiHarness.instances,
                          {
                            id: newId,
                            name: 'New AI Tool',
                            icon: '🤖',
                            command: '',
                            isDefault: false,
                            enableSandbox: false,
                            allowNetwork: true,
                            backgroundColor: '#1a1a1a',
                            disableScrollbar: false,
                            stripScrollbackClear: false
                          }
                        ]
                      }
                    }))
                  }}
                >
                  + Add AI Tool
                </button>
                <p className="settings-hint">
                  Default profiles open automatically in new workspaces.
                  Sandbox settings restrict file and network access.
                </p>
              </div>
            )}

            {activeTab === 'llm' && (
              <div className="settings-section">
                <div className="settings-group">
                  <label className="settings-label">Base URL</label>
                  <input
                    type="text"
                    className="settings-input"
                    value={localSettings.llm.baseUrl}
                    onChange={(e) =>
                      setLocalSettings((prev) => ({
                        ...prev,
                        llm: { ...prev.llm, baseUrl: e.target.value }
                      }))
                    }
                    placeholder="https://openrouter.ai/api/v1"
                  />
                  <p className="settings-hint">
                    OpenAI-compatible API endpoint. Works with OpenAI, Ollama, LM Studio, etc.
                  </p>
                </div>

                <div className="settings-group">
                  <label className="settings-label">API Key</label>
                  <input
                    type="password"
                    className="settings-input"
                    value={localSettings.llm.apiKey}
                    onChange={(e) =>
                      setLocalSettings((prev) => ({
                        ...prev,
                        llm: { ...prev.llm, apiKey: e.target.value }
                      }))
                    }
                    placeholder="sk-..."
                  />
                  <p className="settings-hint">
                    Your API key is stored locally and never sent anywhere except the configured endpoint.
                  </p>
                </div>

                <div className="settings-group">
                  <label className="settings-label">Model</label>
                  <input
                    type="text"
                    className="settings-input"
                    value={localSettings.llm.model}
                    onChange={(e) =>
                      setLocalSettings((prev) => ({
                        ...prev,
                        llm: { ...prev.llm, model: e.target.value }
                      }))
                    }
                    placeholder="gpt-4o"
                  />
                  <p className="settings-hint">
                    Model name to use for chat completions.
                  </p>
                </div>

                <h3 className="settings-section-title" style={{ marginTop: 24 }}>Terminal Analyzer</h3>

                <div className="settings-group">
                  <label className="settings-label">Model</label>
                  <input
                    type="text"
                    className="settings-input"
                    value={localSettings.terminalAnalyzer.model}
                    onChange={(e) =>
                      setLocalSettings((prev) => ({
                        ...prev,
                        terminalAnalyzer: { ...prev.terminalAnalyzer, model: e.target.value }
                      }))
                    }
                    placeholder="openai/gpt-oss-safeguard-20b"
                  />
                  <p className="settings-hint">
                    Model name for terminal state analysis. Uses the Base URL and API Key above.
                  </p>
                </div>

                <div className="settings-group">
                  <label className="settings-label">System Prompt</label>
                  <textarea
                    className="settings-input"
                    rows={4}
                    value={localSettings.terminalAnalyzer.systemPrompt}
                    onChange={(e) =>
                      setLocalSettings((prev) => ({
                        ...prev,
                        terminalAnalyzer: { ...prev.terminalAnalyzer, systemPrompt: e.target.value }
                      }))
                    }
                    style={{ resize: 'vertical', fontFamily: 'inherit' }}
                  />
                  <p className="settings-hint">
                    {'System prompt for the analyzer. Supports {{cwd}} and {{safe_paths}} template variables.'}
                  </p>
                </div>

                <div className="settings-group">
                  <label className="settings-label">Safe Paths</label>
                  <input
                    type="text"
                    className="settings-input"
                    value={localSettings.terminalAnalyzer.safePaths.join(', ')}
                    onChange={(e) =>
                      setLocalSettings((prev) => ({
                        ...prev,
                        terminalAnalyzer: {
                          ...prev.terminalAnalyzer,
                          safePaths: e.target.value.split(',').map((s) => s.trim()).filter(Boolean)
                        }
                      }))
                    }
                    placeholder="/tmp"
                  />
                  <p className="settings-hint">
                    Comma-separated list of paths considered safe. The current working directory is always included automatically.
                  </p>
                </div>

                <div className="settings-group">
                  <label className="settings-label">Buffer Lines</label>
                  <input
                    type="number"
                    className="settings-input"
                    min={1}
                    max={100}
                    value={localSettings.terminalAnalyzer.bufferLines}
                    onChange={(e) =>
                      setLocalSettings((prev) => ({
                        ...prev,
                        terminalAnalyzer: { ...prev.terminalAnalyzer, bufferLines: parseInt(e.target.value) || 10 }
                      }))
                    }
                  />
                  <p className="settings-hint">
                    Number of terminal buffer lines to send to the analyzer.
                  </p>
                </div>

                <div className="settings-group">
                  <label className="settings-label" style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    Reasoning
                    <select
                      value={localSettings.terminalAnalyzer.reasoningEffort}
                      onChange={(e) =>
                        setLocalSettings((prev) => ({
                          ...prev,
                          terminalAnalyzer: { ...prev.terminalAnalyzer, reasoningEffort: e.target.value as ReasoningEffort }
                        }))
                      }
                    >
                      <option value="off">Off</option>
                      <option value="low">Low</option>
                      <option value="medium">Medium</option>
                      <option value="high">High</option>
                    </select>
                  </label>
                  <p className="settings-hint">
                    When checked, reasoning effort is disabled for faster and cheaper analysis.
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
                      : formatKeybinding(localSettings.prefixMode.prefixKey, platform)}
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

            {activeTab === 'speech' && (
              <div className="settings-section">
                <div className="settings-group">
                  <label className="settings-checkbox-label">
                    <input
                      type="checkbox"
                      checked={localSettings.stt.enabled}
                      onChange={(e) =>
                        setLocalSettings((prev) => ({
                          ...prev,
                          stt: { ...prev.stt, enabled: e.target.checked }
                        }))
                      }
                    />
                    Enable Push-to-Talk
                  </label>
                  <p className="settings-hint">
                    Enables speech-to-text in Claude terminal. Hold Caps Lock (or configured key) to
                    speak.
                  </p>
                </div>

                <div className="settings-group">
                  <label className="settings-label">Speech Recognition Provider</label>
                  <select
                    className="settings-select"
                    value={localSettings.stt.provider}
                    onChange={(e) =>
                      setLocalSettings((prev) => ({
                        ...prev,
                        stt: {
                          ...prev.stt,
                          provider: e.target.value as 'openaiWhisper' | 'localWhisper'
                        }
                      }))
                    }
                  >
                    <option value="openaiWhisper">OpenAI Whisper API</option>
                    <option value="localWhisper">Local Whisper (Not Implemented)</option>
                  </select>
                  <p className="settings-hint">
                    OpenAI Whisper provides high-quality speech recognition. Get an API key at{' '}
                    <a
                      href="https://platform.openai.com/api-keys"
                      target="_blank"
                      rel="noopener noreferrer"
                    >
                      platform.openai.com
                    </a>
                  </p>
                </div>

                <div className="settings-group">
                  <label className="settings-label">OpenAI API Key</label>
                  <input
                    type="password"
                    className="settings-input"
                    value={localSettings.stt.openaiApiKey}
                    onChange={(e) =>
                      setLocalSettings((prev) => ({
                        ...prev,
                        stt: { ...prev.stt, openaiApiKey: e.target.value }
                      }))
                    }
                    placeholder="sk-proj-..."
                  />
                  <p className="settings-hint">
                    Required for speech recognition. Your API key is stored locally and only used to
                    transcribe your audio.
                  </p>
                </div>

                <div className="settings-group">
                  <label className="settings-label">Language</label>
                  <select
                    className="settings-select"
                    value={localSettings.stt.language}
                    onChange={(e) =>
                      setLocalSettings((prev) => ({
                        ...prev,
                        stt: { ...prev.stt, language: e.target.value }
                      }))
                    }
                  >
                    <option value="en">English</option>
                    <option value="ms">Malay</option>
                    <option value="zh">Chinese</option>
                    <option value="es">Spanish</option>
                    <option value="fr">French</option>
                    <option value="de">German</option>
                    <option value="ja">Japanese</option>
                    <option value="ko">Korean</option>
                    <option value="pt">Portuguese</option>
                    <option value="ru">Russian</option>
                    <option value="ar">Arabic</option>
                    <option value="hi">Hindi</option>
                    <option value="it">Italian</option>
                    <option value="nl">Dutch</option>
                    <option value="pl">Polish</option>
                    <option value="tr">Turkish</option>
                    <option value="vi">Vietnamese</option>
                    <option value="th">Thai</option>
                    <option value="id">Indonesian</option>
                  </select>
                  <p className="settings-hint">
                    Language of your speech. This helps Whisper transcribe more accurately.
                  </p>
                </div>

                {localSettings.stt.provider === 'localWhisper' && (
                  <div className="settings-group">
                    <label className="settings-label">Whisper Model Path</label>
                    <input
                      type="text"
                      className="settings-input"
                      value={localSettings.stt.localWhisperModelPath}
                      onChange={(e) =>
                        setLocalSettings((prev) => ({
                          ...prev,
                          stt: { ...prev.stt, localWhisperModelPath: e.target.value }
                        }))
                      }
                      placeholder="/path/to/ggml-base.en.bin"
                    />
                    <p className="settings-hint">
                      Path to local Whisper model file. This feature is not yet implemented.
                    </p>
                  </div>
                )}

                <div className="settings-group">
                  <label className="settings-label">Push-to-Talk Key</label>
                  <button
                    className={`settings-keybinding ${recording?.type === 'pttKey' ? 'recording' : ''}`}
                    onClick={() =>
                      setRecording(recording?.type === 'pttKey' ? null : { type: 'pttKey' })
                    }
                  >
                    {recording?.type === 'pttKey' ? 'Press key...' : localSettings.stt.pushToTalkKey}
                  </button>
                  <p className="settings-hint">
                    Hold to record, release to transcribe (default: Ctrl+Space)
                  </p>
                </div>
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

function formatKeybinding(keybinding: string, platform: string): string {
  if (!keybinding) return '(none)'
  return keybinding
    .replace('CommandOrControl', platform === 'darwin' ? 'Cmd' : 'Ctrl')
    .replace('Control', 'Ctrl')
    .replace(/\+/g, ' + ')
}
