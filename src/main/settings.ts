import { app } from 'electron'
import { join } from 'path'
import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'fs'
import type { Settings, TerminalInstance, PrefixModeConfig, STTProvider } from '../shared/types'

// Re-export for backward compatibility
export type { Settings, TerminalInstance, PrefixModeConfig, STTProvider }

function getDefaultClaudeCommand(): string {
  return process.platform === 'darwin' ? 'claude' : 'npx @anthropic-ai/claude-code'
}

const defaultSettings: Settings = {
  terminal: {
    fontSize: 14,
    fontFamily: 'Menlo, Monaco, Consolas, monospace',
    cursorStyle: 'block',
    cursorBlink: true,
    showRawChars: false,
    startByDefault: true,
    instances: []
  },
  sandbox: {
    enabledByDefault: false,
    allowNetworkByDefault: true
  },
  claude: {
    command: getDefaultClaudeCommand(),
    startByDefault: false,
    enableSandbox: false
  },
  appearance: {
    theme: 'dark'
  },
  prefixMode: {
    enabled: true,
    prefixKey: 'Control+B',
    timeout: 1500
  },
  keybindings: {
    newTab: 'c',
    closeTab: 'x',
    nextTab: 'n',
    prevTab: 'p',
    openSettings: ',',
    workspaceFocus: 'w'
  },
  stt: {
    enabled: true,
    provider: 'openaiWhisper',
    openaiApiKey: '',
    localWhisperModelPath: '',
    pushToTalkKey: 'Shift+Space',
    language: 'en'
  },
  daemon: {
    enabled: true,
    orphanTimeout: 0,
    scrollbackLimit: 50000,
    killOnQuit: false
  }
}

function getSettingsDir(): string {
  return app.getPath('userData')
}

function getSettingsPath(): string {
  return join(getSettingsDir(), 'settings.json')
}

export function loadSettings(): Settings {
  const settingsPath = getSettingsPath()

  try {
    if (existsSync(settingsPath)) {
      const data = readFileSync(settingsPath, 'utf-8')
      const loaded = JSON.parse(data)
      // Deep merge with defaults to handle missing fields
      return mergeSettings(defaultSettings, loaded)
    }
  } catch (error) {
    console.error('Failed to load settings:', error)
  }

  // Return defaults and save them
  saveSettings(defaultSettings)
  return defaultSettings
}

export function saveSettings(settings: Settings): void {
  const settingsDir = getSettingsDir()
  const settingsPath = getSettingsPath()

  try {
    if (!existsSync(settingsDir)) {
      mkdirSync(settingsDir, { recursive: true })
    }
    writeFileSync(settingsPath, JSON.stringify(settings, null, 2), 'utf-8')
  } catch (error) {
    console.error('Failed to save settings:', error)
    throw error
  }
}

function migrateKeybindings(
  defaults: Settings['keybindings'],
  loaded: Record<string, string | { direct?: string; prefixMode?: string }> | undefined
): Settings['keybindings'] {
  if (!loaded) return defaults

  const result = { ...defaults }
  const keys = ['newTab', 'closeTab', 'nextTab', 'prevTab', 'openSettings'] as const

  for (const key of keys) {
    const value = loaded[key]
    if (value) {
      if (typeof value === 'string') {
        // Already a string, keep it
        result[key] = value
      } else if (value.prefixMode) {
        // Migrate from old object format - use prefixMode field
        result[key] = value.prefixMode
      }
    }
  }

  return result
}

function mergeSettings(defaults: Settings, loaded: Partial<Settings>): Settings {
  // Migrate terminal instances from various old formats
  let terminalInstances: TerminalInstance[] = loaded.terminal?.instances || []

  // Migrate from old applications array format
  const oldApplications = (loaded as { applications?: Array<{
    id: string
    applicationId?: string
    name: string
    icon: string
    config?: { command?: string }
    command?: string
    isDefault: boolean
    isBuiltIn: boolean
  }> }).applications

  if (oldApplications && terminalInstances.length === 0) {
    // Extract custom terminal instances from old applications format
    terminalInstances = oldApplications
      .filter(a => {
        // Only migrate non-built-in terminals with custom commands
        const hasCommand = a.config?.command || a.command
        const isTerminal = a.applicationId === 'terminal' || !a.applicationId
        return !a.isBuiltIn && isTerminal && hasCommand
      })
      .map(a => ({
        id: a.id,
        name: a.name,
        icon: a.icon,
        startupCommand: a.config?.command || a.command || '',
        isDefault: a.isDefault
      }))
  }

  return {
    terminal: {
      ...defaults.terminal,
      ...loaded.terminal,
      instances: terminalInstances
    },
    sandbox: {
      ...defaults.sandbox,
      ...loaded.sandbox
    },
    claude: {
      ...defaults.claude,
      ...loaded.claude
    },
    appearance: {
      ...defaults.appearance,
      ...loaded.appearance
    },
    prefixMode: {
      ...defaults.prefixMode,
      ...loaded.prefixMode
    },
    keybindings: migrateKeybindings(
      defaults.keybindings,
      loaded.keybindings as Record<string, string | { direct?: string; prefixMode?: string }> | undefined
    ),
    stt: {
      ...defaults.stt,
      ...loaded.stt
    },
    daemon: {
      ...defaults.daemon,
      ...loaded.daemon
    }
  }
}

export function getDefaultSettings(): Settings {
  return { ...defaultSettings }
}
