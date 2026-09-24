import { ClassifierProvider } from '../../shared/types'
import { defaultAnalyzerSystemPrompt, defaultClassifierCriteria } from '../../shared/classifierSettings'
import { create } from 'zustand'
import type { Settings, SettingsApi, ReasoningEffort } from '../types'
import { useAppStore } from './app'

export const defaultSettings: Settings = {
  terminal: {
    fontSize: 14,
    fontFamily: 'Menlo, Monaco, Consolas, monospace',
    cursorStyle: 'block',
    cursorBlink: true,
    showRawChars: false,
    allowOsc52Clipboard: true,
    maxCols: 160,
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
      command: 'npx @earendil-works/pi-coding-agent',
      isDefault: false,
      enableSandbox: false,
      allowNetwork: true,
      backgroundColor: '#1a1a24',
      disableScrollbar: false,
      keepOnExit: false
    }]
  },
  customRunner: {
    instances: []
  },
  webApp: {
    instances: []
  },
  notifications: { soundEnabled: true },
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
  daemon: {
    mergeThreshold: 50 * 1024,
    compactedLimit: 1024 * 1024,
    scrollbackLines: 10000
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
    provider: ClassifierProvider.ChatCompletions,
    titleModel: 'openai/gpt-oss-safeguard-20b',
    model: 'openai/gpt-oss-safeguard-20b',
    systemPrompt: defaultAnalyzerSystemPrompt,
    criteria: defaultClassifierCriteria,
    titleSystemPrompt: 'Given the terminal output below, suggest a short title (max 5 words), a brief description (max 15 words), and a git branch name (lowercase kebab-case, max 4 words). Respond with ONLY a JSON object: {"title": "<title>", "description": "<description>", "branchName": "<branch-name>"}',
    reasoningEffort: 'low' as ReasoningEffort,
    safePaths: ['/tmp'],
    bufferLines: 30,
    idleDebounceMs: 2000,
    idleDebounceUnreadMs: 15000
  },
  github: {
    pat: '',
    autodetectViaGh: true
  },
  globalDefaultApplicationId: 'terminal',
  recentDirectories: [],
  debug: {
    showBadge: false
  }
}

export enum SoundSaveStatus {
  Idle = 'idle',
  Saving = 'saving',
  Error = 'error'
}

export type SoundSaveState =
  | { status: SoundSaveStatus.Idle }
  | { status: SoundSaveStatus.Saving }
  | { status: SoundSaveStatus.Error; error: string }

interface SettingsState {
  soundSaveState: SoundSaveState
  toggleSound: () => Promise<void>
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
  soundSaveState: { status: SoundSaveStatus.Idle },
  toggleSound: async (): Promise<void> => {
    if (get().soundSaveState.status === SoundSaveStatus.Saving) return
    set({ soundSaveState: { status: SoundSaveStatus.Saving } })
    try {
      const settings = get().settings
      const settingsApi = get().settingsApi
      if (!settingsApi) throw new Error('Settings API not initialized')
      const updated = { ...settings, notifications: { soundEnabled: !settings.notifications.soundEnabled } }
      const result = await settingsApi.save(updated)
      if (!result.success) throw new Error('Settings save was unsuccessful')
      set({ settings: updated, soundSaveState: { status: SoundSaveStatus.Idle } })
    } catch (error) {
      set({ soundSaveState: { status: SoundSaveStatus.Error, error: error instanceof Error ? error.message : String(error) } })
    }
  },
  settingsApi: null,
  terminalKill: null,
  settings: defaultSettings,
  isLoaded: false,

  init: (settingsApi: SettingsApi, terminalKill: (connectionId: string, id: string) => void) => {
    set({ settingsApi, terminalKill })
    void get().loadSettings()
  },

  loadSettings: async () => {
    try {
      const settingsApi = get().settingsApi
      if (!settingsApi) return
      const settings = await settingsApi.load()
      set({ settings, isLoaded: true })
      // Register dynamic terminal variants and update base terminal
      useAppStore.getState().registerTerminalVariants(settings.terminal.instances)
      // Register dynamic AI Harness variants
      useAppStore.getState().registerAiHarnessVariants(settings.aiHarness.instances)
      // Register dynamic custom runner variants
      useAppStore.getState().registerCustomRunnerVariants(settings.customRunner.instances)
      // Register dynamic web app variants
      useAppStore.getState().registerWebAppVariants(settings.webApp.instances)
    } catch (error) {
      console.warn('[settings] Failed to load settings, using defaults:', error)
      set({ isLoaded: true })
    }
  },

  saveSettings: async (settings: Settings) => {
    try {
      const settingsApi = get().settingsApi
      if (!settingsApi) throw new Error('Settings API not initialized')
      await settingsApi.save(settings)
      set({ settings })
      // Re-register terminal variants and update base terminal when settings change
      useAppStore.getState().registerTerminalVariants(settings.terminal.instances)
      // Re-register AI Harness variants when settings change
      useAppStore.getState().registerAiHarnessVariants(settings.aiHarness.instances)
      // Re-register custom runner variants when settings change
      useAppStore.getState().registerCustomRunnerVariants(settings.customRunner.instances)
      // Register dynamic web app variants
      useAppStore.getState().registerWebAppVariants(settings.webApp.instances)
    } catch (error) {
      console.error('Failed to save settings:', error)
      throw error
    }
  },

  updateSetting: (category, key, value) => {
    const { settings, saveSettings } = get()
    const categoryValue = settings[category]
    
    // Handle nested object categories (terminal, sandbox, etc.)
    // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- guard against future changes
    if (typeof categoryValue === 'object' && categoryValue !== null && !Array.isArray(categoryValue)) {
      const newSettings = {
        ...settings,
        [category]: {
          // eslint-disable-next-line @typescript-eslint/no-misused-spread -- categoryValue is always an object here
          ...categoryValue,
          [key]: value
        }
      }
      void saveSettings(newSettings)
    } else {
      // Handle top-level primitive values (for future use)
      const newSettings = {
        ...settings,
        [key]: value
      }
      void saveSettings(newSettings)
    }
  }
}))
