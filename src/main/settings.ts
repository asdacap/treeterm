import { app } from 'electron'
import { join } from 'path'
import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'fs'

export interface ApplicationInstance {
  id: string
  applicationId: string
  name: string
  icon: string
  config: Record<string, unknown>
  isDefault: boolean
  isBuiltIn: boolean
}

export interface Settings {
  terminal: {
    fontSize: number
    fontFamily: string
    cursorStyle: 'block' | 'underline' | 'bar'
    cursorBlink: boolean
    showRawChars: boolean
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
  applications: ApplicationInstance[]
}

const defaultApplicationInstances: ApplicationInstance[] = [
  { id: 'files', applicationId: 'filesystem', name: 'Files', icon: '\uD83D\uDCC2', config: {}, isDefault: true, isBuiltIn: true },
  { id: 'default-terminal', applicationId: 'terminal', name: 'Terminal', icon: '>', config: {}, isDefault: true, isBuiltIn: true },
  { id: 'claude', applicationId: 'terminal', name: 'Claude', icon: '\u2726', config: { command: 'claude' }, isDefault: false, isBuiltIn: true }
]

const defaultSettings: Settings = {
  terminal: {
    fontSize: 14,
    fontFamily: 'Menlo, Monaco, Consolas, monospace',
    cursorStyle: 'block',
    cursorBlink: true,
    showRawChars: false
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
  applications: defaultApplicationInstances
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

function mergeApplicationInstances(
  defaults: ApplicationInstance[],
  loaded: ApplicationInstance[]
): ApplicationInstance[] {
  const result: ApplicationInstance[] = []

  // Start with defaults, override with loaded values
  for (const defaultInst of defaults) {
    const loadedInst = loaded.find((a) => a.id === defaultInst.id)
    result.push(loadedInst ? { ...defaultInst, ...loadedInst, isBuiltIn: defaultInst.isBuiltIn } : defaultInst)
  }

  // Add any user-created instances not in defaults
  for (const loadedInst of loaded) {
    if (!defaults.find((a) => a.id === loadedInst.id)) {
      result.push({ ...loadedInst, isBuiltIn: false })
    }
  }

  return result
}

function mergeSettings(defaults: Settings, loaded: Partial<Settings>): Settings {
  // Migrate old application format if needed
  let loadedApps = loaded.applications || []
  if (loadedApps.length > 0 && !('applicationId' in loadedApps[0])) {
    // Old format: { id, name, command, icon, isDefault, isBuiltIn }
    // New format: { id, applicationId, name, icon, config, isDefault, isBuiltIn }
    loadedApps = (loadedApps as Array<{ id: string; name: string; command?: string; icon: string; isDefault: boolean; isBuiltIn: boolean }>).map((old) => ({
      id: old.id,
      applicationId: 'terminal', // Old apps were all terminals
      name: old.name,
      icon: old.icon,
      config: old.command ? { command: old.command } : {},
      isDefault: old.isDefault,
      isBuiltIn: old.isBuiltIn
    }))
  }

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
    applications: mergeApplicationInstances(defaults.applications, loadedApps as ApplicationInstance[])
  }
}

export function getDefaultSettings(): Settings {
  return { ...defaultSettings }
}
