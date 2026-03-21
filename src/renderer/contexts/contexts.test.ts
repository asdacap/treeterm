import { describe, it, expect, vi, beforeEach } from 'vitest'

const { mockCreateContext, mockUseContext } = vi.hoisted(() => ({
  mockCreateContext: vi.fn(() => ({ _currentValue: null })),
  mockUseContext: vi.fn()
}))

vi.mock('react', () => ({
  createContext: mockCreateContext,
  useContext: mockUseContext
}))

import { useFilesystemApi } from './FilesystemApiContext'
import { useSTTApi } from './STTApiContext'
import { useTerminalApi } from './TerminalApiContext'

describe('Context hooks', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('useFilesystemApi', () => {
    it('throws when context is null', () => {
      mockUseContext.mockReturnValue(null)
      expect(() => useFilesystemApi()).toThrow(
        'useFilesystemApi must be used within a FilesystemApiContext.Provider'
      )
    })

    it('returns value when context is provided', () => {
      const api = { readFile: vi.fn() }
      mockUseContext.mockReturnValue(api)
      expect(useFilesystemApi()).toBe(api)
    })
  })

  describe('useSTTApi', () => {
    it('throws when context is null', () => {
      mockUseContext.mockReturnValue(null)
      expect(() => useSTTApi()).toThrow(
        'useSTTApi must be used within a STTApiContext.Provider'
      )
    })

    it('returns value when context is provided', () => {
      const api = { startRecording: vi.fn() }
      mockUseContext.mockReturnValue(api)
      expect(useSTTApi()).toBe(api)
    })
  })

  describe('useTerminalApi', () => {
    it('throws when context is null', () => {
      mockUseContext.mockReturnValue(null)
      expect(() => useTerminalApi()).toThrow(
        'useTerminalApi must be used within a TerminalApiContext.Provider'
      )
    })

    it('returns value when context is provided', () => {
      const api = { createTerminal: vi.fn() }
      mockUseContext.mockReturnValue(api)
      expect(useTerminalApi()).toBe(api)
    })
  })
})
