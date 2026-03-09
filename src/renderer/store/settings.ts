import { create } from 'zustand'
import type { Settings } from '../types'
import { registerTerminalVariants } from '../../applications'

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
    command: 'claude',
    startByDefault: false,
    enableSandbox: false
  },
  appearance: {
    theme: 'dark'
  },
  prefixMode: {
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
  }
}

interface SettingsState {
  settings: Settings
  isLoaded: boolean
  loadSettings: () => Promise<void>
  saveSettings: (settings: Settings) => Promise<void>
  updateSetting: <K extends keyof Settings>(
    category: K,
    key: keyof Settings[K],
    value: Settings[K][keyof Settings[K]]
  ) => void
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  settings: defaultSettings,
  isLoaded: false,

  loadSettings: async () => {
    try {
      const settings = await window.electron.settings.load()
      set({ settings, isLoaded: true })
      // Register dynamic terminal variants and update base terminal
      registerTerminalVariants(settings.terminal.instances, settings.terminal)
    } catch (error) {
      console.error('Failed to load settings:', error)
      set({ isLoaded: true })
    }
  },

  saveSettings: async (settings: Settings) => {
    try {
      await window.electron.settings.save(settings)
      set({ settings })
      // Re-register terminal variants and update base terminal when settings change
      registerTerminalVariants(settings.terminal.instances, settings.terminal)
    } catch (error) {
      console.error('Failed to save settings:', error)
      throw error
    }
  },

  updateSetting: (category, key, value) => {
    const { settings, saveSettings } = get()
    const newSettings = {
      ...settings,
      [category]: {
        ...settings[category],
        [key]: value
      }
    }
    saveSettings(newSettings)
  }
}))
