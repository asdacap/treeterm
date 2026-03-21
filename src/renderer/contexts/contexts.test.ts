import { describe, it, expect, vi, beforeEach } from 'vitest'

const { mockCreateContext, mockUseContext } = vi.hoisted(() => ({
  mockCreateContext: vi.fn(() => ({ _currentValue: null })),
  mockUseContext: vi.fn()
}))

vi.mock('react', () => ({
  createContext: mockCreateContext,
  useContext: mockUseContext
}))

import { useSTTApi } from './STTApiContext'
import { useSessionApi } from './SessionStoreContext'

describe('Context hooks', () => {
  beforeEach(() => {
    vi.clearAllMocks()
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

  describe('useSessionApi', () => {
    it('throws when context is null', () => {
      mockUseContext.mockReturnValue(null)
      expect(() => useSessionApi()).toThrow(
        'useSessionApi must be used within a SessionStoreContext.Provider'
      )
    })

    it('returns value when context is provided', () => {
      const store = { getState: vi.fn() }
      mockUseContext.mockReturnValue(store)
      expect(useSessionApi()).toBe(store)
    })
  })
})
