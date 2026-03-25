import { describe, it, expect, vi, beforeEach } from 'vitest'

// Mock app store used by settings store
vi.mock('./app', () => ({
  useAppStore: {
    getState: vi.fn().mockReturnValue({
      registerTerminalVariants: vi.fn(),
      registerAiHarnessVariants: vi.fn()
    })
  }
}))

import { useSettingsStore } from './settings'
import type { Settings } from '../types'

// Mock settings API
const mockLoadSettings = vi.fn()
const mockSaveSettings = vi.fn()
const mockSettingsApi = { load: mockLoadSettings, save: mockSaveSettings, onOpen: vi.fn() }
const mockTerminalKill = vi.fn()

describe('SettingsStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    // Reset store to default state
    useSettingsStore.setState({
      settingsApi: mockSettingsApi,
      terminalKill: mockTerminalKill,
      settings: {
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
          mergeThreshold: 50 * 1024,
          compactedLimit: 1024 * 1024,
          scrollbackLines: 10000
        },
        globalDefaultApplicationId: 'terminal',
        recentDirectories: []
      },
      isLoaded: false
    } as unknown as Parameters<typeof useSettingsStore.setState>[0])
  })

  describe('initial state', () => {
    it('has default settings', () => {
      const store = useSettingsStore.getState()
      
      expect(store.settings.terminal.fontSize).toBe(14)
      expect(store.settings.appearance.theme).toBe('dark')
      expect(store.settings.prefixMode.enabled).toBe(true)
      expect(store.isLoaded).toBe(false)
    })
  })

  describe('loadSettings', () => {
    it('loads settings from electron', async () => {
      const mockSettings: Partial<Settings> = {
        terminal: {
          fontSize: 18,
          fontFamily: 'Menlo',
          cursorStyle: 'block',
          cursorBlink: true,
          showRawChars: false,
          instances: []
        },
        appearance: { theme: 'light' },
        aiHarness: { instances: [] },
        sandbox: { enabledByDefault: false, allowNetworkByDefault: true },
        prefixMode: { enabled: true, prefixKey: 'Control+B', timeout: 1500 },
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
          mergeThreshold: 50 * 1024,
          compactedLimit: 1024 * 1024,
          scrollbackLines: 10000
        },
        globalDefaultApplicationId: 'terminal',
        recentDirectories: []
      }
      
      mockLoadSettings.mockResolvedValue(mockSettings)
      
      await useSettingsStore.getState().loadSettings()
      
      expect(useSettingsStore.getState().settings.terminal.fontSize).toBe(18)
      expect(useSettingsStore.getState().settings.appearance.theme).toBe('light')
      expect(useSettingsStore.getState().isLoaded).toBe(true)
      expect(mockLoadSettings).toHaveBeenCalled()
    })

    it('handles load error gracefully', async () => {
      mockLoadSettings.mockRejectedValue(new Error('Failed to load'))
      
      await useSettingsStore.getState().loadSettings()
      
      expect(useSettingsStore.getState().isLoaded).toBe(true)
      // Should keep default settings
      expect(useSettingsStore.getState().settings.terminal.fontSize).toBe(14)
    })
  })

  describe('saveSettings', () => {
    it('saves settings via electron', async () => {
      mockSaveSettings.mockResolvedValue(undefined)
      
      const currentSettings = useSettingsStore.getState().settings
      const newSettings: Settings = {
        ...currentSettings,
        appearance: { theme: 'light' }
      }
      
      await useSettingsStore.getState().saveSettings(newSettings)
      
      expect(mockSaveSettings).toHaveBeenCalledWith(newSettings)
      expect(useSettingsStore.getState().settings.appearance.theme).toBe('light')
    })

    it('throws error when save fails', async () => {
      mockSaveSettings.mockRejectedValue(new Error('Failed to save'))
      
      const currentSettings = useSettingsStore.getState().settings
      
      await expect(useSettingsStore.getState().saveSettings(currentSettings)).rejects.toThrow('Failed to save')
    })
  })

  describe('init', () => {
    it('sets settingsApi and terminalKill then loads settings', async () => {
      mockLoadSettings.mockResolvedValue(useSettingsStore.getState().settings)
      useSettingsStore.setState({ settingsApi: null, terminalKill: null })

      useSettingsStore.getState().init(mockSettingsApi as any, mockTerminalKill)

      expect(useSettingsStore.getState().settingsApi).toBe(mockSettingsApi)
      expect(useSettingsStore.getState().terminalKill).toBe(mockTerminalKill)
      await new Promise(resolve => setTimeout(resolve, 10))
      expect(mockLoadSettings).toHaveBeenCalled()
    })
  })

  describe('updateSetting', () => {
    it('updates nested setting and saves', async () => {
      mockSaveSettings.mockResolvedValue(undefined)
      
      useSettingsStore.getState().updateSetting('terminal', 'fontSize', 20)
      
      // Wait for async save
      await new Promise(resolve => setTimeout(resolve, 10))
      
      expect(mockSaveSettings).toHaveBeenCalled()
      const savedSettings = mockSaveSettings.mock.calls[0][0]
      expect(savedSettings.terminal.fontSize).toBe(20)
    })

    it('preserves other values when updating a category', async () => {
      mockSaveSettings.mockResolvedValue(undefined)
      
      // Ensure we have default state
      useSettingsStore.setState({
        settings: {
          ...useSettingsStore.getState().settings,
          terminal: {
            ...useSettingsStore.getState().settings.terminal,
            fontFamily: 'Menlo, Monaco, Consolas, monospace'
          }
        }
      })
      
      useSettingsStore.getState().updateSetting('terminal', 'fontSize', 20)
      
      // Wait for async save
      await new Promise(resolve => setTimeout(resolve, 10))
      
      const savedSettings = mockSaveSettings.mock.calls[0][0]
      expect(savedSettings.terminal.fontFamily).toBe('Menlo, Monaco, Consolas, monospace')
      expect(savedSettings.terminal.fontSize).toBe(20)
    })
  })
})
