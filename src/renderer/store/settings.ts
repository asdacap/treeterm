import { create } from 'zustand'
import type { Settings } from '../types'
import { registerTerminalVariants, registerAiHarnessVariants } from '../../applications'

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
  aiHarness: {
    instances: [{
      id: 'claude',
      name: 'Claude',
      icon: '✦',
      command: 'claude',
      isDefault: false,
      enableSandbox: false,
      allowNetwork: true,
      backgroundColor: '#1a1a24'
    }]
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
    enabled: false,
    orphanTimeout: 30000,
    scrollbackLimit: 10000,
    killOnQuit: true
  },
  globalDefaultApplicationId: 'terminal'
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
      // Register dynamic AI Harness variants
      registerAiHarnessVariants(settings.aiHarness.instances)
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
      // Re-register AI Harness variants when settings change
      registerAiHarnessVariants(settings.aiHarness.instances)
    } catch (error) {
      console.error('Failed to save settings:', error)
      throw error
    }
  },

  updateSetting: (category, key, value) => {
    const { settings, saveSettings } = get()
    const categoryValue = settings[category]
    
    // Handle nested object categories (terminal, sandbox, etc.)
    if (typeof categoryValue === 'object' && categoryValue !== null && !Array.isArray(categoryValue)) {
      const newSettings = {
        ...settings,
        [category]: {
          ...categoryValue,
          [key]: value
        }
      }
      saveSettings(newSettings)
    } else {
      // Handle top-level primitive values (for future use)
      const newSettings = {
        ...settings,
        [key]: value
      }
      saveSettings(newSettings)
    }
  }
}))
