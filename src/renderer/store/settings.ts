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
    systemPrompt: 'You are a terminal state analyzer. The current working directory is: {{cwd}}. The safe paths are: {{safe_paths}}. Given the last lines of terminal output, respond with ONLY a JSON object: {"state": "<state>", "reason": "<reason>"} where state is one of the following that best represent the state: \n - "safe_permission_requested" program asking for permission but the action is safe. A safe action are action that either:\n   - Git operation unless it changes other worktree.\n   - Build or test or install dependencies.\n   - Anything that only mutates files within one of the safe paths.\n   - It IS allowed to read outside the safe path, just not mutate them. \n   - But it is not allowed to do a wide ranging search via find or `grep -r` on file outside the safe path.\n - "permission_request" program asking for y/n or similar confirmation but it is not considered safe as "safe_permission_requested".\n - "completed" when previous user request satisfied.\n - "user_input_required" program asking for user text input or confirmation among design choice or a plan confirmation.\n - "idle" for shell prompt visible, waiting for command or user input is incomplete.\nThe reason field should show the reason for the verdict, no more than 10 word. ',
    titleSystemPrompt: 'Given the terminal output below, suggest a short title (max 5 words), a brief description (max 15 words), and a git branch name (lowercase kebab-case, max 4 words). Respond with ONLY a JSON object: {"title": "<title>", "description": "<description>", "branchName": "<branch-name>"}',
    reasoningEffort: 'low' as ReasoningEffort,
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
      useAppStore.getState().registerTerminalVariants(settings.terminal.instances)
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
      useAppStore.getState().registerTerminalVariants(settings.terminal.instances)
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
