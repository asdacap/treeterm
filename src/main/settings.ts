import { app } from 'electron'
import { join } from 'path'
import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'fs'

export interface Settings {
  terminal: {
    fontSize: number
    fontFamily: string
    cursorStyle: 'block' | 'underline' | 'bar'
    cursorBlink: boolean
  }
  sandbox: {
    enabledByDefault: boolean
    allowNetworkByDefault: boolean
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
  startup: {
    childWorkspaceCommand: string
  }
}

const defaultSettings: Settings = {
  terminal: {
    fontSize: 14,
    fontFamily: 'Menlo, Monaco, Consolas, monospace',
    cursorStyle: 'block',
    cursorBlink: true
  },
  sandbox: {
    enabledByDefault: false,
    allowNetworkByDefault: true
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
  },
  startup: {
    childWorkspaceCommand: ''
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
  return {
    terminal: {
      ...defaults.terminal,
      ...loaded.terminal
    },
    sandbox: {
      ...defaults.sandbox,
      ...loaded.sandbox
    },
    appearance: {
      ...defaults.appearance,
      ...loaded.appearance
    },
    keybindings: {
      ...defaults.keybindings,
      ...loaded.keybindings
    },
    startup: {
      ...defaults.startup,
      ...loaded.startup
    }
  }
}

export function getDefaultSettings(): Settings {
  return { ...defaultSettings }
}
