import { describe, it, expect, vi, beforeEach } from 'vitest'
import * as fs from 'fs'

vi.mock('fs')
vi.mock('electron', () => ({
  app: { getPath: vi.fn().mockReturnValue('/mock/userData') }
}))

// Import after mocks are set up
import { loadSettings, saveSettings, getDefaultSettings } from './settings'

describe('settings', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('getDefaultSettings', () => {
    it('returns default settings structure', () => {
      const defaults = getDefaultSettings()

      expect(defaults.terminal.fontSize).toBe(14)
      expect(defaults.terminal.fontFamily).toBe('Menlo, Monaco, Consolas, monospace')
      expect(defaults.terminal.cursorStyle).toBe('block')
      expect(defaults.terminal.cursorBlink).toBe(true)
      expect(defaults.appearance.theme).toBe('dark')
      expect(defaults.sandbox.enabledByDefault).toBe(false)
    })
  })

  describe('loadSettings', () => {
    it('returns defaults when settings file does not exist', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false)
      vi.mocked(fs.writeFileSync).mockImplementation(() => {})

      const settings = loadSettings()

      expect(settings.terminal.fontSize).toBe(14)
      expect(fs.existsSync).toHaveBeenCalled()
    })

    it('merges partial settings with defaults', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)
      vi.mocked(fs.readFileSync).mockReturnValue(
        JSON.stringify({
          terminal: { fontSize: 18 }
        })
      )

      const settings = loadSettings()

      expect(settings.terminal.fontSize).toBe(18) // Overridden
      expect(settings.terminal.cursorStyle).toBe('block') // Default preserved
      expect(settings.appearance.theme).toBe('dark') // Default preserved
    })
  })

  describe('mergeSettings migrations (via loadSettings)', () => {
    it('migrates old applications array to terminal.instances', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)
      vi.mocked(fs.readFileSync).mockReturnValue(
        JSON.stringify({
          applications: [
            { id: 'app-1', name: 'My Term', icon: '🖥', command: '/usr/bin/zsh', isDefault: true, isBuiltIn: false }
          ]
        })
      )

      const settings = loadSettings()

      expect(settings.terminal.instances).toHaveLength(1)
      expect(settings.terminal.instances[0]!.id).toBe('app-1')
      expect(settings.terminal.instances[0]!.startupCommand).toBe('/usr/bin/zsh')
    })

    it('skips built-in apps during applications migration', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)
      vi.mocked(fs.readFileSync).mockReturnValue(
        JSON.stringify({
          applications: [
            { id: 'builtin-1', name: 'Built-in', icon: '🖥', command: '/bin/sh', isDefault: true, isBuiltIn: true },
            { id: 'custom-1', name: 'Custom', icon: '🖥', command: '/usr/bin/zsh', isDefault: false, isBuiltIn: false }
          ]
        })
      )

      const settings = loadSettings()

      expect(settings.terminal.instances).toHaveLength(1)
      expect(settings.terminal.instances[0]!.id).toBe('custom-1')
    })

    it('does not migrate applications when terminal.instances already exist', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)
      vi.mocked(fs.readFileSync).mockReturnValue(
        JSON.stringify({
          terminal: { instances: [{ id: 'existing', name: 'Existing', icon: '🖥', startupCommand: '', isDefault: true }] },
          applications: [
            { id: 'app-1', name: 'My Term', icon: '🖥', command: '/usr/bin/zsh', isDefault: true, isBuiltIn: false }
          ]
        })
      )

      const settings = loadSettings()

      expect(settings.terminal.instances).toHaveLength(1)
      expect(settings.terminal.instances[0]!.id).toBe('existing')
    })

    it('migrates old claude config to aiHarness.instances', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)
      vi.mocked(fs.readFileSync).mockReturnValue(
        JSON.stringify({
          claude: { command: 'my-claude', startByDefault: true, enableSandbox: false }
        })
      )

      const settings = loadSettings()

      expect(settings.aiHarness.instances).toHaveLength(1)
      expect(settings.aiHarness.instances[0]!.command).toBe('my-claude')
      expect(settings.aiHarness.instances[0]!.isDefault).toBe(true)
    })

    it('does not migrate claude config when aiHarness.instances already exist', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)
      vi.mocked(fs.readFileSync).mockReturnValue(
        JSON.stringify({
          aiHarness: { instances: [{ id: 'existing-ai', name: 'AI', icon: '✦', command: 'ai', isDefault: false, enableSandbox: false, allowNetwork: true, backgroundColor: '#000' }] },
          claude: { command: 'my-claude' }
        })
      )

      const settings = loadSettings()

      expect(settings.aiHarness.instances).toHaveLength(1)
      expect(settings.aiHarness.instances[0]!.id).toBe('existing-ai')
    })

    it('preserves customRunner.instances from loaded settings', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)
      vi.mocked(fs.readFileSync).mockReturnValue(
        JSON.stringify({
          customRunner: {
            instances: [{ id: 'rider', name: 'Rider', icon: '▶', commandTemplate: 'rider {{workspace_path}}', isDefault: false }]
          }
        })
      )

      const settings = loadSettings()

      expect(settings.customRunner.instances).toHaveLength(1)
      expect(settings.customRunner.instances[0]!.id).toBe('rider')
      expect(settings.customRunner.instances[0]!.commandTemplate).toBe('rider {{workspace_path}}')
    })

    it('defaults customRunner.instances to empty array when not in stored settings', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)
      vi.mocked(fs.readFileSync).mockReturnValue(JSON.stringify({}))

      const settings = loadSettings()

      expect(settings.customRunner.instances).toEqual([])
    })

    it('preserves string keybindings as-is', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)
      vi.mocked(fs.readFileSync).mockReturnValue(
        JSON.stringify({
          keybindings: { newTab: 't' }
        })
      )

      const settings = loadSettings()

      expect(settings.keybindings.newTab).toBe('t')
    })

    it('migrates old object keybinding format using prefixMode field', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)
      vi.mocked(fs.readFileSync).mockReturnValue(
        JSON.stringify({
          keybindings: { newTab: { direct: 'Ctrl+T', prefixMode: 'n' } }
        })
      )

      const settings = loadSettings()

      expect(settings.keybindings.newTab).toBe('n')
    })

    it('uses defaults when keybindings is undefined', () => {
      vi.mocked(fs.existsSync).mockReturnValue(true)
      vi.mocked(fs.readFileSync).mockReturnValue(
        JSON.stringify({ terminal: { fontSize: 16 } })
      )

      const settings = loadSettings()

      expect(settings.keybindings.newTab).toBe('c')
      expect(settings.keybindings.closeTab).toBe('x')
    })
  })

  describe('saveSettings', () => {
    it('creates directory if it does not exist', () => {
      vi.mocked(fs.existsSync).mockReturnValue(false)
      vi.mocked(fs.mkdirSync).mockImplementation(() => undefined)
      vi.mocked(fs.writeFileSync).mockImplementation(() => {})

      const settings = getDefaultSettings()
      saveSettings(settings)

      expect(fs.mkdirSync).toHaveBeenCalledWith('/mock/userData', { recursive: true })
      expect(fs.writeFileSync).toHaveBeenCalled()
    })
  })
})
