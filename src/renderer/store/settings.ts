import { create } from 'zustand'
import type { Settings, SettingsApi, ReasoningEffort } from '../types'
import { useAppStore } from './app'

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
      backgroundColor: '#1a1a24',
      stripScrollbackClear: true
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
    scrollbackLimit: 10000
  },
  ssh: {
    savedConnections: []
  },
  llm: {
    baseUrl: 'https://openrouter.ai/api/v1',
    apiKey: '',
    model: 'gpt-4o'
  },
  terminalAnalyzer: {
    model: 'openai/gpt-oss-safeguard-20b',
    systemPrompt: 'You are a terminal state analyzer. The current working directory is: {{cwd}}. The safe paths are: {{safe_paths}}. Given the last lines of terminal output, respond with ONLY a JSON object: {"state": "<state>", "reason": "<reason>"} where state is one of: "idle" (shell prompt visible, waiting for command), "working" (process actively running/producing output), "user_input_required" (program asking for user text input), "permission_request" (program asking for y/n or similar confirmation and the action may mutate files OUTSIDE the safe paths), "safe_permission_requested" (program asking for permission but the action only mutates files within one of the safe paths — safe to approve), "completed" (task finished, showing final result). "working" override other state and reason is the reason for the verdict, no more than 10 word. A git operation is considered safe as long as it does not mutate other branch. A plan approval is considered "user_input_required" even if it only mutate safe path.',
    reasoningEffort: 'off' as ReasoningEffort,
    safePaths: ['/tmp'],
    bufferLines: 30
  },
  globalDefaultApplicationId: 'terminal',
  recentDirectories: []
}

interface SettingsState {
  settingsApi: SettingsApi | null
  terminalKill: ((connectionId: string, id: string) => void) | null
  settings: Settings
  isLoaded: boolean
  init: (settingsApi: SettingsApi, terminalKill: (connectionId: string, id: string) => void) => void
  loadSettings: () => Promise<void>
  saveSettings: (settings: Settings) => Promise<void>
  updateSetting: <K extends keyof Settings>(
    category: K,
    key: keyof Settings[K],
    value: Settings[K][keyof Settings[K]]
  ) => void
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  settingsApi: null,
  terminalKill: null,
  settings: defaultSettings,
  isLoaded: false,

  init: (settingsApi: SettingsApi, terminalKill: (connectionId: string, id: string) => void) => {
    set({ settingsApi, terminalKill })
    get().loadSettings()
  },

  loadSettings: async () => {
    try {
      const settings = await get().settingsApi!.load()
      set({ settings, isLoaded: true })
      // Register dynamic terminal variants and update base terminal
      useAppStore.getState().registerTerminalVariants(settings.terminal.instances, settings.terminal)
      // Register dynamic AI Harness variants
      useAppStore.getState().registerAiHarnessVariants(settings.aiHarness.instances)
    } catch (error) {
      console.warn('[settings] Failed to load settings, using defaults:', error)
      set({ isLoaded: true })
    }
  },

  saveSettings: async (settings: Settings) => {
    try {
      await get().settingsApi!.save(settings)
      set({ settings })
      // Re-register terminal variants and update base terminal when settings change
      useAppStore.getState().registerTerminalVariants(settings.terminal.instances, settings.terminal)
      // Re-register AI Harness variants when settings change
      useAppStore.getState().registerAiHarnessVariants(settings.aiHarness.instances)
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
