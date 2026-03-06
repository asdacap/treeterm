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
