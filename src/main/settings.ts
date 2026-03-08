import { app } from 'electron'
import { join } from 'path'
import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'fs'

function getDefaultClaudeCommand(): string {
  return process.platform === 'darwin' ? 'claude' : 'npx @anthropic-ai/claude-code'
}

export interface TerminalInstance {
  id: string
  name: string
  icon: string
  startupCommand: string
  isDefault: boolean
}

export interface Settings {
  terminal: {
    fontSize: number
    fontFamily: string
    cursorStyle: 'block' | 'underline' | 'bar'
    cursorBlink: boolean
    showRawChars: boolean
    instances: TerminalInstance[]
  }
  sandbox: {
    enabledByDefault: boolean
    allowNetworkByDefault: boolean
  }
  claude: {
    command: string
    startByDefault: boolean
    enableSandbox: boolean
  }
  appearance: {
    theme: 'dark' | 'light' | 'system'
  }
  keybindings: {
    newTab: string
    closeTab: string
    nextTab: string
    prevTab: string
    openSettings: string
  }
}

const defaultSettings: Settings = {
  terminal: {
    fontSize: 14,
    fontFamily: 'Menlo, Monaco, Consolas, monospace',
    cursorStyle: 'block',
    cursorBlink: true,
    showRawChars: false,
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
  keybindings: {
    newTab: 'CommandOrControl+T',
    closeTab: 'CommandOrControl+W',
    nextTab: 'CommandOrControl+Shift+]',
    prevTab: 'CommandOrControl+Shift+[',
    openSettings: 'CommandOrControl+,'
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
    keybindings: {
      ...defaults.keybindings,
      ...loaded.keybindings
    }
  }
}

export function getDefaultSettings(): Settings {
  return { ...defaultSettings }
}
