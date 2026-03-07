import { create } from 'zustand'
import type { Settings, Application } from '../types'

const defaultApplications: Application[] = [
  { id: 'terminal', name: 'Terminal', command: '', icon: '>', isDefault: true, isBuiltIn: true },
  { id: 'claude', name: 'Claude', command: 'claude', icon: '✦', isDefault: false, isBuiltIn: true }
]

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
  },
  applications: defaultApplications
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
    } catch (error) {
      console.error('Failed to load settings:', error)
      set({ isLoaded: true })
    }
  },

  saveSettings: async (settings: Settings) => {
    try {
      await window.electron.settings.save(settings)
      set({ settings })
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
